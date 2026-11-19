//! Scan newly closed ledgers for failed invocations involving a target.

use stellar_xdr::{ContractId, HostFunction, LedgerKey, ScAddress, ScVal, SorobanCredentials};

use crate::discover::{Candidate, failures_matching};
use crate::rpc::RpcClient;
use crate::{Result, RewindError, strkey, tx::DecodedTx};

/// The account or contract a watcher follows.
#[derive(Debug)]
pub enum Target {
    /// A contract ID.
    Contract(ContractId),
    /// A canonical G-address.
    Account(String),
}

impl Target {
    /// Parse a checked address for the selected target kind.
    pub fn parse(contract: Option<&str>, account: Option<&str>) -> Result<Self> {
        let target = if let Some(contract) = contract {
            strkey::parse_contract(contract).map(Self::Contract)
        } else {
            account.and_then(|address| {
                stellar_strkey::ed25519::PublicKey::from_string(address)
                    .ok()
                    .map(|key| Self::Account(strkey::account(&key.0)))
            })
        };
        target.ok_or_else(|| RewindError::InvalidOption {
            name: "watch target",
            reason: "supply a valid C-address with --contract or G-address with --account".into(),
        })
    }

    /// Canonical address stored in checkpoints.
    pub fn address(&self) -> String {
        match self {
            Self::Contract(id) => strkey::contract(&(id.0).0),
            Self::Account(address) => address.clone(),
        }
    }

    fn matches(&self, tx: &DecodedTx) -> bool {
        let address = |candidate: &ScAddress| match (self, candidate) {
            (Self::Contract(id), ScAddress::Contract(other)) => other == id,
            (Self::Contract(id), ScAddress::MuxedContract(other)) => &other.contract_id == id,
            (Self::Account(account), ScAddress::Account(other)) => {
                strkey::account_id(other) == *account
            }
            (Self::Account(account), ScAddress::MuxedAccount(other)) => {
                strkey::account(&other.ed25519.0) == *account
            }
            _ => false,
        };
        let source = matches!(self, Self::Account(account) if strkey::account_id(&tx.source_account) == *account);
        let invocation = matches!(&tx.host_function, HostFunction::InvokeContract(invoke) if
            address(&invoke.contract_address)
                || invoke.args.iter().any(|value| matches!(value, ScVal::Address(candidate) if address(candidate))));
        let authorization = tx.auth.iter().any(|auth| match &auth.credentials {
            SorobanCredentials::Address(credentials)
            | SorobanCredentials::AddressV2(credentials) => address(&credentials.address),
            SorobanCredentials::AddressWithDelegates(credentials) => {
                address(&credentials.address_credentials.address)
            }
            SorobanCredentials::SourceAccount => false,
        });
        let footprint = tx.footprint_keys().iter().any(|key| match key {
            LedgerKey::ContractData(data) => address(&data.contract),
            LedgerKey::Account(entry) => matches!(self, Self::Account(account) if strkey::account_id(&entry.account_id) == *account),
            LedgerKey::Trustline(entry) => matches!(self, Self::Account(account) if strkey::account_id(&entry.account_id) == *account),
            _ => false,
        });
        let events = tx.onchain_diagnostic_events.iter().any(|event| {
            if let Self::Contract(id) = self {
                event.event.contract_id.as_ref() == Some(id)
                    || matches!(crate::display::event_parts(&event.event).0,
                        [ScVal::Symbol(kind), ScVal::Bytes(bytes), ..]
                            if kind.0.as_slice() == b"fn_call" && bytes.0.as_slice() == (id.0).0)
            } else {
                false
            }
        });
        source || invocation || authorization || footprint || events
    }
}

/// What the RPC knows about ledgers after the checkpoint.
pub struct Batch {
    /// The last ledger fully scanned, or the initial watermark.
    pub through: u32,
    /// The number of older ledgers the RPC had already pruned.
    pub pruned: u32,
    /// Matching failed transactions in scan order.
    pub candidates: Vec<Candidate>,
}

/// Poll one bounded span of newly closed ledgers. A fresh watch starts at
/// the current tip and waits for the next close.
pub async fn poll(rpc: &RpcClient, target: &Target, after: Option<u32>) -> Result<Batch> {
    let health = rpc.health().await?;
    let latest =
        health
            .latest_ledger
            .filter(|&ledger| ledger > 0)
            .ok_or(RewindError::MissingField {
                method: "getHealth",
                field: "latestLedger",
            })?;
    let Some(after) = after else {
        return Ok(Batch {
            through: latest,
            pruned: 0,
            candidates: Vec::new(),
        });
    };
    if latest <= after {
        return Ok(Batch {
            through: after,
            pruned: 0,
            candidates: Vec::new(),
        });
    }
    let start = (after + 1).max(health.oldest_ledger.unwrap_or(1));
    let through = start.saturating_add(49).min(latest);
    let candidates = failures_matching(rpc, start, through, |tx| target.matches(tx)).await?;
    Ok(Batch {
        through,
        pruned: start - (after + 1),
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec;
    use crate::test_support::{InvokeSpec, MemoryTransport, invoke_envelope, result_xdr};
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};
    use stellar_xdr::InvokeHostFunctionResult;

    #[tokio::test]
    async fn first_poll_starts_at_tip_then_only_scans_new_ledgers_and_matching_failures() {
        let ledger = AtomicU32::new(100);
        let rpc = RpcClient::new(MemoryTransport::new().on_fn("getHealth", move |_| {
            json!({"result":{"oldestLedger":80, "latestLedger":ledger.fetch_add(1, Ordering::Relaxed)}})
        }).on_fn("getTransactions", |params| {
            let start = params["startLedger"].as_u64().unwrap();
            let tx = |byte: &str, ledger, status| {
                let envelope = invoke_envelope(&InvokeSpec::default());
                json!({"status":status, "txHash":byte.repeat(32), "ledger":ledger,
                    "envelopeXdr":codec::b64_encode(&codec::encode(&envelope, "envelope").unwrap()),
                    "resultXdr":result_xdr(InvokeHostFunctionResult::Trapped)})
            };
            json!({"result":{"transactions":[
                tx("aa", start - 1, "FAILED"),
                tx("bb", start, "SUCCESS"),
                tx("cc", start, "FAILED"),
                tx("dd", start + 1, "FAILED")
            ]}})
        }));
        let target = Target::parse(Some(&strkey::contract(&[0xaa; 32])), None).unwrap();
        let initial = poll(&rpc, &target, None).await.unwrap();
        assert_eq!(initial.through, 100);
        assert!(initial.candidates.is_empty());
        let next = poll(&rpc, &target, Some(initial.through)).await.unwrap();
        assert_eq!(next.through, 101);
        assert_eq!(next.candidates.len(), 1);
        assert_eq!(next.candidates[0].hash.as_str(), "cc".repeat(32));

        let other = Target::parse(None, Some(&strkey::account(&[0x53; 32]))).unwrap();
        let next = poll(&rpc, &other, Some(next.through)).await.unwrap();
        assert_eq!(next.through, 102);
        assert!(next.candidates.is_empty());
    }
}

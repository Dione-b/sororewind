//! the pipeline: fetch → decode → reconstruct → replay → narrate → report.
//!
//! The order is the product: every stage exists to feed the next one exactly
//! what it needs, and the fidelity check at the end is only meaningful if
//! nothing upstream quietly substituted a guess for a fact. [`run`] keeps that
//! order on one screen; each stage is a function with a typed result.

use std::path::PathBuf;

use crate::culprit;
use crate::display;
use crate::error::{Result, Retention, RewindError};
use crate::findings;
use crate::hash::TxHash;
use crate::network::{Network, NetworkId};
use crate::progress::{Progress, Silent, Step};
use crate::replay::{self, Fidelity, ReplayOutcome};
use crate::report::{
    Diagnostics, EntryReason, EntryStatus, EntryStatusKind, FeeReport, FootprintReport, Report,
    ResultPair, SCHEMA_VERSION, TtlDriftEntry,
};
use crate::rpc::{RawTransaction, RpcClient, TxStatus};
use crate::state::{self, LedgerContext, Reconstructed};
use crate::story::Story;
use crate::trust;
use crate::tx::{self, DecodedTx};
use crate::{host, spec, strkey};
use stellar_xdr::{ConfigSettingId, InvokeHostFunctionResult};

/// What to replay.
#[derive(Debug, Clone)]
pub struct Request {
    /// The transaction to replay.
    pub hash: TxHash,
    /// The network the caller believes the RPC serves. When set, a different
    /// passphrase is an error instead of a silently mislabelled report.
    pub network: Option<Network>,
    /// Read the failing contract's error names from this WASM instead of the
    /// ledger.
    pub wasm_debug: Option<PathBuf>,
}

/// Replay `request.hash` against `rpc` and assemble the [`Report`]: fetch,
/// decode, reconstruct the ledger state, replay, compare with what the
/// network recorded, and narrate the outcome.
///
/// ```
/// use std::path::Path;
/// use sororewind::{FixtureTransport, Outcome, Request, RpcClient, run};
///
/// # #[tokio::main]
/// # async fn main() -> sororewind::Result<()> {
/// let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/contract-error");
/// # if !path.is_dir() { return Ok(()); } // crates.io packages omit the recordings
/// let fixture = FixtureTransport::open(path).await?;
/// let request = Request {
///     hash: fixture.hash_for(None)?,
///     network: None,
///     wasm_debug: None,
/// };
/// let report = run(&RpcClient::new(fixture), &request).await?;
/// assert_eq!(report.outcome(), Outcome::Matched);
/// assert!(report.story.failure.is_some());
/// # Ok(())
/// # }
/// ```
pub async fn run(rpc: &RpcClient, request: &Request) -> Result<Report> {
    run_with_progress(rpc, request, &Silent).await
}

/// [`run`], telling `progress` each stage as the run enters it.
pub async fn run_with_progress(
    rpc: &RpcClient,
    request: &Request,
    progress: &dyn Progress,
) -> Result<Report> {
    let (fetched, tx) = fetch_and_decode(rpc, request, progress).await?;
    let (ledger, state, host) = reconstruct(rpc, &fetched, &tx, progress, true).await?;
    let outcome = run_replay(host, &tx, &state, &ledger, progress)?;
    let fidelity = replay::compare(&tx, &outcome);
    let story = narrate(rpc, request, &state, &outcome).await?;

    Ok(assemble(Replayed {
        rpc,
        network: fetched.network,
        tx,
        ledger,
        state,
        outcome,
        fidelity,
        story,
    }))
}

async fn fetch_and_decode(
    rpc: &RpcClient,
    request: &Request,
    progress: &dyn Progress,
) -> Result<(Fetched, DecodedTx)> {
    progress.step(Step::FetchingTransaction);
    let fetched = fetch(rpc, request).await?;
    let tx = tx::decode(&request.hash, &fetched.raw)?;
    Ok((fetched, tx))
}

async fn reconstruct(
    rpc: &RpcClient,
    fetched: &Fetched,
    tx: &DecodedTx,
    progress: &dyn Progress,
    check_host: bool,
) -> Result<(LedgerContext, Reconstructed, Option<&'static host::Host>)> {
    progress.step(Step::FetchingEntries {
        count: tx.footprint_keys().len(),
    });
    let (ledger, entries) = tokio::try_join!(
        state::read_ledger(rpc, tx.ledger, &fetched.passphrase),
        state::fetch(rpc, tx),
    )?;
    let host = check_host
        .then(|| embedded_host(tx, &ledger, &fetched.network))
        .transpose()?;
    progress.step(Step::RebuildingHistory);
    let state = state::reconstruct(rpc, tx, &ledger, entries).await?;
    Ok((ledger, state, host))
}

fn run_replay(
    host: Option<&host::Host>,
    tx: &DecodedTx,
    state: &Reconstructed,
    ledger: &LedgerContext,
    progress: &dyn Progress,
) -> Result<ReplayOutcome> {
    let host = host.ok_or_else(|| RewindError::Replay("no host was selected".into()))?;
    progress.step(Step::Replaying);
    replay::replay(host, tx, state, ledger)
}

/// A pipeline stage to inspect without running later stages.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum InspectStage {
    /// Fetch and decode the transaction and its footprint.
    Decode,
    /// Reconstruct the ledger context and footprint entries.
    State,
    /// Run the local host and stop before fidelity comparison and narration.
    Replay,
}

/// Execute only as far as the requested stage and return its inspection document.
#[doc(hidden)]
pub async fn inspect(
    rpc: &RpcClient,
    request: &Request,
    progress: &dyn Progress,
    stage: InspectStage,
) -> Result<serde_json::Value> {
    use crate::codec::b64_encode;
    use serde_json::json;

    let (fetched, tx) = fetch_and_decode(rpc, request, progress).await?;
    if stage == InspectStage::Decode {
        let label = |key: &stellar_xdr::LedgerKey| -> Result<serde_json::Value> {
            Ok(json!({
                "key": display::ledger_key(key),
                "keyXdr": b64_encode(&crate::codec::encode(key, "footprint key")?),
            }))
        };
        let footprint = &tx.resources.footprint;
        let invocation = match &tx.host_function {
            stellar_xdr::HostFunction::InvokeContract(call) => Some(json!({
                "contract": strkey::address(&call.contract_address),
                "function": call.function_name.to_string(),
                "arguments": call.args.iter().map(display::scval).collect::<Vec<_>>(),
            })),
            _ => None,
        };
        return Ok(json!({
            "stage": "decode", "hash": tx.hash, "ledger": tx.ledger, "feeBump": tx.fee_bump,
            "applicationOrder": tx.application_order, "envelopeSize": tx.envelope_size,
            "network": fetched.network.to_string(), "resourceFee": tx.resource_fee,
            "onchainResult": crate::report::ResultSummary::from(&tx.onchain_result),
            "sourceAccountXdr": b64_encode(&tx.encoded_source_account()?),
            "hostFunction": tx.host_function.name(),
            "invocation": invocation,
            "sourceAccount": strkey::account_id(&tx.source_account),
            "hostFunctionXdr": b64_encode(&tx.encoded_host_function()?),
            "resourcesXdr": b64_encode(&tx.encoded_resources()?),
            "authXdr": tx.encoded_auth()?.iter().map(|entry| b64_encode(entry)).collect::<Vec<_>>(),
            "restoredReadWriteIndices": tx.restored_rw_indices,
            "readOnly": footprint.read_only.iter().map(label).collect::<Result<Vec<_>>>()?,
            "readWrite": footprint.read_write.iter().map(label).collect::<Result<Vec<_>>>()?,
        }));
    }

    let (ledger, state, host) =
        reconstruct(rpc, &fetched, &tx, progress, stage == InspectStage::Replay).await?;
    if stage == InspectStage::State {
        let entries: Vec<_> = state
            .slots
            .iter()
            .map(|slot| {
                let provenance = if slot.rebuilt {
                    "rebuilt"
                } else if slot.value_drifted(ledger.sequence) {
                    "drifted"
                } else if slot.drift_unknown() {
                    "unknown"
                } else {
                    "current"
                };
                let live_until = match slot.liveness {
                    state::Liveness::Live { live_until }
                    | state::Liveness::Restored { live_until } => Some(live_until),
                    _ => None,
                };
                json!({
                    "key": display::ledger_key(&slot.key),
                    "keyXdr": b64_encode(&slot.key_xdr),
                    "entryXdr": slot.entry_xdr.as_deref().map(b64_encode),
                    "lastModifiedLedger": slot.last_modified,
                    "liveUntilLedger": live_until,
                    "archived": slot.archived(),
                    "missing": slot.missing(),
                    "provenance": provenance,
                })
            })
            .collect();
        return Ok(json!({
            "stage": "state", "hash": tx.hash, "ledger": ledger.sequence, "protocol": ledger.protocol,
            "closeTime": ledger.close_time, "baseReserve": ledger.base_reserve,
            "readOnly": state.read_only, "entries": entries,
            "footprint": footprint_report(&state, ledger.sequence, false),
        }));
    }

    let outcome = run_replay(host, &tx, &state, &ledger, progress)?;
    Ok(json!({
        "stage": "replay", "hash": tx.hash, "ledger": ledger.sequence, "protocol": ledger.protocol,
        "result": crate::report::ResultSummary::from(&outcome.result),
        "resultXdr": b64_encode(&crate::codec::encode(&outcome.result, "the local result")?),
        "errorXdr": outcome.error.as_ref()
            .map(|error| crate::codec::encode(error, "the local error")).transpose()?.as_deref().map(b64_encode),
        "diagnostics": lines(&outcome.diagnostics), "diagnosticsXdr": raw(&outcome.diagnostics),
        "budget": outcome.budget, "prngSeed": outcome.prng_seed,
        "nonRefundableFee": outcome.non_refundable_fee,
    }))
}

/// Measure one stage of a replay for the repository's offline benchmarks.
#[doc(hidden)]
pub async fn benchmark_stage(
    rpc: &RpcClient,
    request: &Request,
    stage: InspectStage,
) -> Result<std::time::Duration> {
    use std::time::Instant;

    let fetched = fetch(rpc, request).await?;
    let start = Instant::now();
    let tx = tx::decode(&request.hash, &fetched.raw)?;
    if stage == InspectStage::Decode {
        return Ok(start.elapsed());
    }
    let start = Instant::now();
    let (ledger, state, host) = reconstruct(rpc, &fetched, &tx, &Silent, true).await?;
    if stage == InspectStage::State {
        return Ok(start.elapsed());
    }
    let start = Instant::now();
    std::hint::black_box(run_replay(host, &tx, &state, &ledger, &Silent)?);
    Ok(start.elapsed())
}

/// A failed transaction and the network it was found on.
struct Fetched {
    raw: RawTransaction,
    passphrase: String,
    network: NetworkId,
}

async fn fetch(rpc: &RpcClient, request: &Request) -> Result<Fetched> {
    let (raw, info) = tokio::try_join!(rpc.get_transaction(&request.hash), rpc.network())?;

    // The passphrase is the network's identity; `--network` is only a claim.
    let network = NetworkId::from_passphrase(&info.passphrase);
    if let Some(expected) = request.network
        && network != NetworkId::Known(expected)
    {
        return Err(RewindError::NetworkMismatch {
            expected,
            actual: network,
            origin: rpc.origin(),
        });
    }

    match raw.status {
        TxStatus::Failed => Ok(Fetched {
            raw,
            passphrase: info.passphrase,
            network,
        }),
        TxStatus::Success => Err(RewindError::NotAFailure {
            hash: request.hash.clone(),
        }),
        TxStatus::NotFound => Err(rpc.history_error(RewindError::NotFound {
            hash: request.hash.clone(),
            network,
            // Only used to word the error, so a failed probe is not an error.
            retention: match rpc.health().await.map(|health| {
                (
                    health.ledger_retention_window.filter(|window| *window > 0),
                    health.oldest_ledger,
                )
            }) {
                Ok((Some(window), oldest)) => Retention::Reported { window, oldest },
                _ => Retention::Unknown,
            },
        })),
        TxStatus::Unknown => Err(RewindError::Rpc {
            origin: rpc.origin(),
            method: "getTransaction".to_string(),
            code: None,
            detail: "the transaction has a status this build of sororewind does not know \
                     (neither SUCCESS, FAILED nor NOT_FOUND)"
                .to_string(),
        }),
    }
}

/// The embedded host for the transaction's ledger. Each host hard-rejects
/// any ledger whose protocol differs from its own, so checking here turns a
/// cryptic host error into an actionable sentence.
fn embedded_host(
    tx: &DecodedTx,
    ledger: &LedgerContext,
    network: &NetworkId,
) -> Result<&'static host::Host> {
    host::for_protocol(ledger.protocol).ok_or_else(|| RewindError::ProtocolMismatch {
        hash: tx.hash.clone(),
        tx_protocol: ledger.protocol,
        supported: host::describe(),
        network: network.clone(),
    })
}

/// Build the call tree and give the contract error behind the failure, the
/// fatal one or the one a host error wraps, its name.
async fn narrate(
    rpc: &RpcClient,
    request: &Request,
    state: &Reconstructed,
    outcome: &ReplayOutcome,
) -> Result<Story> {
    let mut story = Story::build(&outcome.diagnostics, outcome.error.as_ref());
    let Some(failure) = &story.failure else {
        return Ok(story);
    };
    // The contract the error was raised in, falling back to the outermost
    // call when the failure happened before any frame opened.
    let (code, contract) = match (&failure.cause, failure.contract_code) {
        (Some(cause), _) => (cause.contract_code, cause.contract.clone()),
        (None, Some(code)) => (code, failure.contract.clone()),
        (None, None) => return Ok(story),
    };
    let contract = contract.or_else(|| story.calls.first().and_then(|call| call.contract.clone()));

    let Some(found) = error_name(rpc, request, state, contract.as_deref(), code).await? else {
        return Ok(story);
    };
    if let Some(failure) = &mut story.failure {
        match &mut failure.cause {
            Some(cause) => {
                cause.name = Some(found.name);
                cause.name_origin = Some(found.origin);
            }
            None => {
                failure.name = Some(found.name);
                failure.name_origin = Some(found.origin);
            }
        }
    }
    Ok(story)
}

/// The name `contract` declares for error `code`, and where it was read from.
///
/// The contract's code is usually already in the footprint; when it is not, it
/// costs two extra `getLedgerEntries` calls, which is still cheap next to the
/// value of printing `InsufficientBalance` instead of `#3`.
async fn error_name(
    rpc: &RpcClient,
    request: &Request,
    state: &Reconstructed,
    contract: Option<&str>,
    code: u32,
) -> Result<Option<spec::ErrorName>> {
    let (names, source) = match &request.wasm_debug {
        Some(path) => {
            let wasm = tokio::fs::read(path)
                .await
                .map_err(|source| RewindError::WasmFile {
                    path: path.clone(),
                    source,
                })?;
            (
                spec::from_wasm(&wasm),
                format!(", from --wasm-debug {}", path.display()),
            )
        }
        None => {
            let Some(contract_id) = contract.and_then(strkey::parse_contract) else {
                return Ok(None);
            };
            match spec::contract_wasm(rpc, state, &contract_id).await {
                Ok(spec::ContractCode::StellarAsset) => {
                    (Ok(spec::stellar_asset_contract()), String::new())
                }
                // The name is read from the code the contract has now.
                Ok(spec::ContractCode::Wasm(contract)) if contract.changed_since => (
                    spec::from_wasm(&contract.wasm),
                    ", read from code that changed after the transaction: the name may not be \
                     the one this error had"
                        .to_string(),
                ),
                Ok(spec::ContractCode::Wasm(contract)) => (
                    spec::from_wasm(&contract.wasm),
                    ", from the contract's code on the ledger".to_string(),
                ),
                Err(err) => {
                    log::debug!(target: "spec", "{err}");
                    return Ok(None);
                }
            }
        }
    };
    // A contract without a readable spec still has a number; not an error.
    match names {
        Ok(names) => Ok(names.lookup(code).map(|found| spec::ErrorName {
            origin: format!("{}{source}", found.origin),
            ..found
        })),
        Err(err) => {
            log::debug!(target: "spec", "{err}");
            Ok(None)
        }
    }
}

/// Everything the stages produced, ready to be summarised.
struct Replayed<'a> {
    rpc: &'a RpcClient,
    network: NetworkId,
    tx: DecodedTx,
    ledger: LedgerContext,
    state: Reconstructed,
    outcome: ReplayOutcome,
    fidelity: Fidelity,
    story: Story,
}

fn assemble(run: Replayed<'_>) -> Report {
    let footprint = footprint_report(
        &run.state,
        run.ledger.sequence,
        [&run.tx.onchain_result, &run.outcome.result]
            .contains(&&InvokeHostFunctionResult::InsufficientRefundableFee),
    );
    let trust = trust::assess(&footprint, run.outcome.prng_seed, &run.tx);
    let probable_causes = if run.fidelity.matches {
        Vec::new()
    } else {
        culprit::find(
            &footprint,
            run.outcome.prng_seed,
            &run.tx,
            &run.outcome,
            &run.story,
        )
    };
    let result = ResultPair {
        onchain: (&run.tx.onchain_result).into(),
        local: (&run.outcome.result).into(),
    };
    let fee = FeeReport {
        onchain_non_refundable: run.tx.onchain_fee.map(|fee| fee.non_refundable),
        local_non_refundable: run.outcome.non_refundable_fee,
        onchain_refundable: run.tx.onchain_fee.map(|fee| fee.refundable),
        onchain_rent: run.tx.onchain_fee.map(|fee| fee.rent),
    };
    let findings = findings::derive(&findings::Inputs {
        fidelity: &run.fidelity,
        result: &result,
        failure: run.story.failure.as_ref(),
        fee: &fee,
        probable_causes: &probable_causes,
        trust: &trust,
    });
    Report {
        schema_version: SCHEMA_VERSION,
        hash: run.tx.hash.clone(),
        fee_bump: run.tx.fee_bump,
        network: match &run.network {
            NetworkId::Known(network) => network.name().to_string(),
            NetworkId::Custom(passphrase) => passphrase.clone(),
        },
        rpc: run.rpc.origin(),
        ledger: run.ledger.sequence,
        protocol: run.ledger.protocol,
        close_time: run.ledger.close_time,
        status: "failed",
        findings,
        fidelity: run.fidelity,
        result,
        story: run.story,
        budget: run.outcome.budget,
        fee,
        footprint,
        trust,
        probable_causes,
        prng_seed: run.outcome.prng_seed,
        diagnostics: Diagnostics {
            onchain: lines(&run.tx.onchain_diagnostic_events),
            local: lines(&run.outcome.diagnostics),
            onchain_xdr: raw(&run.tx.onchain_diagnostic_events),
            local_xdr: raw(&run.outcome.diagnostics),
        },
    }
}

fn lines(events: &[stellar_xdr::DiagnosticEvent]) -> Vec<String> {
    events.iter().map(display::diagnostic).collect()
}

fn raw(events: &[stellar_xdr::DiagnosticEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| crate::codec::encode(event, "a diagnostic event").ok())
        .map(|bytes| crate::codec::b64_encode(&bytes))
        .collect()
}

fn footprint_report(state: &Reconstructed, tx_ledger: u32, fee_decided: bool) -> FootprintReport {
    let labels = |keep: &dyn Fn(&state::FootprintSlot) -> bool| -> Vec<String> {
        state
            .slots
            .iter()
            .filter(|slot| keep(slot))
            .map(|slot| display::ledger_key(&slot.key))
            .collect()
    };
    let mut config_drifted: Vec<String> = state
        .config
        .drifted
        .iter()
        .map(|setting| setting.to_string())
        .collect();
    // The state-size window only prices rent, so it is only worth a mention
    // when the refundable fee decided a result.
    if fee_decided && state.config.state_size_window_drifted {
        config_drifted.push(
            ConfigSettingId::LiveSorobanStateSizeWindow
                .name()
                .to_string(),
        );
    }
    FootprintReport {
        entries: state.slots.len(),
        missing: state.slots.iter().filter(|slot| slot.missing()).count(),
        missing_entries: labels(&|slot| slot.missing()),
        entry_status: entry_status(state),
        drifted: labels(&|slot| slot.value_drifted(tx_ledger)),
        ttl_drifted: state
            .slots
            .iter()
            .filter_map(|slot| {
                Some(TtlDriftEntry {
                    entry: display::ledger_key(&slot.key),
                    reason: slot.ttl_drift?.into(),
                })
            })
            .collect(),
        archived: labels(&|slot| slot.archived()),
        archived_inferred: labels(&|slot| {
            matches!(slot.liveness, state::Liveness::Archived { inferred: true })
        }),
        rebuilt: labels(&|slot| slot.rebuilt),
        rebuild_note: state.rebuild_note.clone(),
        drift_unknown: labels(&|slot| slot.drift_unknown()),
        ext_missing: labels(&|slot| slot.ext_missing),
        config_drifted,
        config_drift_unknown: state
            .config
            .drift_unknown
            .iter()
            .map(|setting| setting.to_string())
            .collect(),
    }
}

/// Every slot that is not as plain as it looks, with why.
fn entry_status(state: &Reconstructed) -> Vec<EntryStatus> {
    let mut statuses = Vec::new();
    for slot in &state.slots {
        let entry = display::ledger_key(&slot.key);
        let mut push = |status, reason, detail: &str| {
            statuses.push(EntryStatus {
                entry: entry.clone(),
                status,
                reason,
                detail: detail.to_string(),
            });
        };
        if let Some(reason) = slot.unresolved {
            push(
                EntryStatusKind::Unresolved,
                reason.into(),
                &format!("not rebuilt: {}", reason.describe()),
            );
        }
        match slot.liveness {
            state::Liveness::Absent => push(
                EntryStatusKind::Missing,
                EntryReason::Absent,
                "the RPC has no such entry, so the host was handed none",
            ),
            state::Liveness::Expired => push(
                EntryStatusKind::Missing,
                EntryReason::Expired,
                "a temporary entry past its TTL, which to the host does not exist",
            ),
            _ => {}
        }
        if slot.drift_unknown() {
            push(
                EntryStatusKind::DriftUnknown,
                EntryReason::NoModificationLedger,
                "the RPC did not say when it last changed",
            );
        }
        if slot.ext_missing {
            push(
                EntryStatusKind::ExtMissing,
                EntryReason::NoExtension,
                "the RPC sent it without its extension, so it was replayed with an empty one",
            );
        }
    }
    statuses
}

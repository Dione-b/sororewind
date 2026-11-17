//! End-to-end replays of situations no recording covers.
//!
//! Each test takes a recorded fixture and rewrites some of its RPC responses
//! (see `common::Overlay`), then runs the whole pipeline offline. The
//! rewrites stay consistent with what the network would have recorded, so a
//! correct replay must still match.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use serde_json::Value;
use sororewind::codec;
use sororewind::culprit::CulpritKind;
use sororewind::replay::Divergence;
use sororewind::report::Report;
use sororewind::{Network, Outcome, RewindError};
use stellar_xdr::{
    ConfigSettingEntry, ConfigSettingId, LedgerEntryData, LedgerKeyConfigSetting, TransactionExt,
};
use stellar_xdr::{
    ContractDataDurability, FeeBumpTransaction, FeeBumpTransactionEnvelope, FeeBumpTransactionExt,
    FeeBumpTransactionInnerTx, Hash, InnerTransactionResult, InnerTransactionResultExt,
    InnerTransactionResultPair, InnerTransactionResultResult, InvokeHostFunctionResult, LedgerKey,
    Limits, MuxedAccount, OperationResult, OperationResultTr, TransactionEnvelope,
    TransactionResult, TransactionResultExt, TransactionResultResult, Uint256, WriteXdr,
};

use common::Overlay;

/// A fixture whose failure is an ordinary contract error.
const FIXTURE: &str = "contract-error";

async fn replay(overlay: Overlay) -> Report {
    let request = common::request(FIXTURE).await;
    sororewind::run(&overlay.client(), &request)
        .await
        .expect("the rewritten fixture could not be replayed")
}

fn b64<T: WriteXdr>(value: &T) -> String {
    codec::b64_encode(&value.to_xdr(Limits::none()).unwrap())
}

/// The operation results of a plain `TransactionResult`.
fn operation_results(result: &TransactionResult) -> Vec<OperationResult> {
    match &result.result {
        TransactionResultResult::TxFailed(ops) | TransactionResultResult::TxSuccess(ops) => {
            ops.to_vec()
        }
        other => panic!("the fixture's result is not a plain one: {other:?}"),
    }
}

/// Rewrite `getTransaction` as if the transaction had been sponsored by a
/// fee-bump: the envelope is wrapped, and the result moves one level down.
fn as_fee_bump(result: &mut Value) {
    let inner: TransactionEnvelope =
        codec::decode(result["envelopeXdr"].as_str().unwrap(), "envelopeXdr").unwrap();
    let TransactionEnvelope::Tx(inner) = inner else {
        panic!("the fixture's envelope is not a v1 transaction");
    };
    let envelope = TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
        tx: FeeBumpTransaction {
            fee_source: MuxedAccount::Ed25519(Uint256([7; 32])),
            fee: inner.tx.fee as i64 * 2,
            inner_tx: FeeBumpTransactionInnerTx::Tx(inner),
            ext: FeeBumpTransactionExt::V0,
        },
        signatures: Vec::new().try_into().unwrap(),
    });

    let plain: TransactionResult =
        codec::decode(result["resultXdr"].as_str().unwrap(), "resultXdr").unwrap();
    let bumped = TransactionResult {
        fee_charged: plain.fee_charged,
        result: TransactionResultResult::TxFeeBumpInnerFailed(InnerTransactionResultPair {
            transaction_hash: Hash([1; 32]),
            result: InnerTransactionResult {
                fee_charged: plain.fee_charged,
                result: InnerTransactionResultResult::TxFailed(
                    operation_results(&plain).try_into().unwrap(),
                ),
                ext: InnerTransactionResultExt::V0,
            },
        }),
        ext: TransactionResultExt::V0,
    };

    result["envelopeXdr"] = b64(&envelope).into();
    result["resultXdr"] = b64(&bumped).into();
    result["feeBump"] = true.into();
}

#[tokio::test]
async fn a_fee_bumped_transaction_replays_like_a_plain_one() {
    let plain = replay(Overlay::on(FIXTURE).await).await;
    let bumped = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", as_fee_bump),
    )
    .await;

    assert_eq!(bumped.outcome(), Outcome::Matched);
    assert!(bumped.fee_bump);
    assert!(!plain.fee_bump);
    // Only the fee payer changed: the host ran the same inner transaction.
    assert_eq!(bumped.result.local.code, plain.result.local.code);
    assert_eq!(bumped.story.calls.len(), plain.story.calls.len());
    let (bumped_failure, plain_failure) = (
        bumped.story.failure.as_ref().unwrap(),
        plain.story.failure.as_ref().unwrap(),
    );
    assert_eq!(bumped_failure.description, plain_failure.description);
    assert_eq!(bumped_failure.function, plain_failure.function);
}

/// The first persistent contract-data entry in the fixture's footprint.
fn persistent_entry_key() -> String {
    let path = common::fixture_dir(FIXTURE).join("ledgerEntries.json");
    let store: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    store["entries"]
        .as_object()
        .unwrap()
        .keys()
        .find(|key| {
            let key: LedgerKey = codec::decode(key, "a ledger key").unwrap();
            matches!(
                key,
                LedgerKey::ContractData(data) if data.durability == ContractDataDurability::Persistent
            )
        })
        .expect("the fixture's footprint has no persistent contract data")
        .clone()
}

/// Rewrite `getLedgerEntries` as if `key` had been archived since.
fn archive(key: String) -> impl Fn(&mut Value) + Send + Sync + 'static {
    move |result| {
        for entry in result["entries"].as_array_mut().unwrap() {
            if entry["key"] == key.as_str() {
                entry["liveUntilLedgerSeq"] = 0.into();
            }
        }
    }
}

/// Rewrite `getTransaction` as if core had failed the operation with `code`
/// before the host ran, so there are no diagnostics.
fn failed_before_the_host(result: &mut Value, code: InvokeHostFunctionResult) {
    let plain: TransactionResult =
        codec::decode(result["resultXdr"].as_str().unwrap(), "resultXdr").unwrap();
    let failed = TransactionResult {
        fee_charged: plain.fee_charged,
        result: TransactionResultResult::TxFailed(
            vec![OperationResult::OpInner(
                OperationResultTr::InvokeHostFunction(code),
            )]
            .try_into()
            .unwrap(),
        ),
        ext: TransactionResultExt::V0,
    };
    result["resultXdr"] = b64(&failed).into();
    // The diagnostics live in the transaction meta.
    result.as_object_mut().unwrap().remove("resultMetaXdr");
}

/// Rewrite `getTransaction` as if core had refused an archived entry.
fn failed_on_archived_entry(result: &mut Value) {
    failed_before_the_host(result, InvokeHostFunctionResult::EntryArchived);
}

/// Rewrite `getTransaction` as if the transaction had declared no bytes to
/// read from disk, and core had stopped at its first classic entry.
fn declared_no_disk_reads(result: &mut Value) {
    let envelope: TransactionEnvelope =
        codec::decode(result["envelopeXdr"].as_str().unwrap(), "envelopeXdr").unwrap();
    let TransactionEnvelope::Tx(mut envelope) = envelope else {
        panic!("the fixture's envelope is not a v1 transaction");
    };
    let TransactionExt::V1(data) = &mut envelope.tx.ext else {
        panic!("the fixture's transaction carries no Soroban data");
    };
    data.resources.disk_read_bytes = 0;
    result["envelopeXdr"] = b64(&TransactionEnvelope::Tx(envelope)).into();
    failed_before_the_host(result, InvokeHostFunctionResult::ResourceLimitExceeded);
}

#[tokio::test]
async fn classic_entries_count_against_the_declared_disk_reads() {
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", declared_no_disk_reads),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Matched);
    assert_eq!(report.result.local.code, "resource_limit_exceeded");
    let failure = report.story.failure.as_ref().unwrap();
    assert!(
        failure
            .message
            .contains("operation byte-read resources exceeds amount specified"),
        "{failure:?}"
    );
}

/// Rewrite `getLedgerEntries` as if the network took contract data entries of
/// one byte at most.
fn tiny_contract_data_limit(result: &mut Value) {
    let key = b64(&LedgerKey::ConfigSetting(LedgerKeyConfigSetting {
        config_setting_id: ConfigSettingId::ContractDataEntrySizeBytes,
    }));
    for entry in result["entries"].as_array_mut().unwrap() {
        if entry["key"] == key.as_str() {
            entry["xdr"] = b64(&LedgerEntryData::ConfigSetting(
                ConfigSettingEntry::ContractDataEntrySizeBytes(1),
            ))
            .into();
        }
    }
}

#[tokio::test]
async fn an_entry_over_the_network_size_limit_fails_like_core() {
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getLedgerEntries", tiny_contract_data_limit)
            .rewrite_result("getTransaction", |result| {
                failed_before_the_host(result, InvokeHostFunctionResult::ResourceLimitExceeded);
            }),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Matched);
    assert_eq!(report.result.local.code, "resource_limit_exceeded");
    let failure = report.story.failure.as_ref().unwrap();
    assert!(
        failure
            .message
            .contains("ContractData size exceeds network config maximum size"),
        "{failure:?}"
    );
}

#[tokio::test]
async fn the_refundable_fee_check_is_mirrored() {
    // The host succeeds; core then finds the rent beyond the refundable fee.
    let report = common::replay("refundable-fee").await.unwrap();
    assert_eq!(report.outcome(), Outcome::Matched);
    assert_eq!(report.result.local.code, "insufficient_refundable_fee");
    let failure = report.story.failure.as_ref().unwrap();
    assert!(
        failure
            .message
            .contains("refundable resource fee was not sufficient"),
        "{failure:?}"
    );
    // Rent is priced from a window that keeps moving; the report says so.
    assert!(
        report
            .footprint
            .config_drifted
            .iter()
            .any(|setting| setting == "LiveSorobanStateSizeWindow")
    );
}

#[tokio::test]
async fn a_drifted_entry_is_rebuilt_from_ledger_metadata() {
    let report = common::replay("rebuilt-entry").await.unwrap();
    assert_eq!(report.outcome(), Outcome::Matched);
    assert!(!report.footprint.rebuilt.is_empty());
    assert!(report.footprint.drifted.is_empty());
    assert_eq!(report.footprint.rebuild_note, None);
}

#[tokio::test]
async fn an_archived_entry_fails_like_core() {
    let key = persistent_entry_key();
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getLedgerEntries", archive(key))
            .rewrite_result("getTransaction", failed_on_archived_entry),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Matched);
    assert_eq!(report.result.local.code, "entry_archived");
    assert_eq!(report.footprint.archived.len(), 1);
    let failure = report.story.failure.as_ref().unwrap();
    assert!(failure.message.contains("archived"), "{failure:?}");
}

#[tokio::test]
async fn an_entry_archived_since_the_transaction_is_ttl_drift() {
    // The network ran the host, so the entry was live back then: it is
    // replayed as live and reported as TTL drift, not as a failure.
    let key = persistent_entry_key();
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getLedgerEntries", archive(key)),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Matched);
    assert!(report.footprint.archived.is_empty());
    assert_eq!(report.footprint.ttl_drifted.len(), 1);
}

#[tokio::test]
async fn a_network_the_rpc_does_not_serve_is_an_error() {
    // The fixture is a testnet recording; asking for mainnet must not produce
    // a report labelled "mainnet".
    let mut request = common::request(FIXTURE).await;
    request.network = Some(Network::Mainnet);
    let result = sororewind::run(&Overlay::on(FIXTURE).await.client(), &request).await;
    assert!(
        matches!(result, Err(RewindError::NetworkMismatch { .. })),
        "{result:?}"
    );

    request.network = Some(Network::Testnet);
    let report = sororewind::run(&Overlay::on(FIXTURE).await.client(), &request)
        .await
        .unwrap();
    assert_eq!(report.network, "testnet");
}

#[tokio::test]
async fn each_embedded_host_replays_its_own_protocol() {
    // One recording per embedded host; both must match the network.
    for (fixture, protocol) in [("mainnet-auth-error", 29), ("contract-error", 30)] {
        let report = common::replay(fixture).await.unwrap();
        assert_eq!(report.protocol, protocol, "{fixture}");
        assert!(sororewind::host::for_protocol(protocol).is_some());
        assert_eq!(report.outcome(), Outcome::Matched, "{fixture}");
    }
}

/// A WASM whose `contractspecv0` section declares one error enum.
fn wasm_declaring(enum_name: &str, cases: &[(&str, u32)]) -> Vec<u8> {
    use stellar_xdr::{ScSpecEntry, ScSpecUdtErrorEnumCaseV0, ScSpecUdtErrorEnumV0};
    let spec = ScSpecEntry::UdtErrorEnumV0(ScSpecUdtErrorEnumV0 {
        doc: Default::default(),
        lib: Default::default(),
        name: enum_name.try_into().unwrap(),
        cases: cases
            .iter()
            .map(|(name, value)| ScSpecUdtErrorEnumCaseV0 {
                doc: Default::default(),
                name: (*name).try_into().unwrap(),
                value: *value,
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
    })
    .to_xdr(Limits::none())
    .unwrap();
    let payload = [&[14u8][..], b"contractspecv0", &spec].concat();
    assert!(payload.len() < 128, "one LEB128 byte of length");
    [
        b"\0asm\x01\0\0\0",
        &[0u8, payload.len() as u8][..],
        &payload,
    ]
    .concat()
}

/// A custom account's `__check_auth` rejected the signature.
const CUSTOM_ACCOUNT: &str = "auth-check-auth";

#[tokio::test]
async fn a_contract_error_is_named_from_the_contract_on_the_ledger() {
    // The whole lookup, end to end: the failing contract's instance names its
    // code, the code's spec names the error.
    let report = common::replay(CUSTOM_ACCOUNT).await.unwrap();
    assert_eq!(report.outcome(), Outcome::Matched);
    let failure = report.story.failure.as_ref().unwrap();
    // The host reports an auth error; the contract error behind it is the
    // cause, in the account contract's own frame.
    assert_eq!(failure.error_type, "auth");
    assert_eq!(failure.name, None);
    let cause = failure.cause.as_ref().unwrap();
    assert_eq!(cause.function.as_deref(), Some("__check_auth"));
    assert!(cause.name.is_some(), "{cause:?}");
    let origin = cause.name_origin.as_deref().unwrap();
    assert!(
        origin.starts_with("spec enum ")
            && origin.ends_with("from the contract's code on the ledger"),
        "{origin}"
    );
    let frame = &report.story.calls[failure.call_index.unwrap()];
    assert!(frame.is_failure && frame.function == "__check_auth");
}

#[tokio::test]
async fn wasm_debug_names_the_error_from_a_local_file() {
    let ledger = common::replay(CUSTOM_ACCOUNT).await.unwrap();
    let code = ledger
        .story
        .failure
        .as_ref()
        .unwrap()
        .cause
        .as_ref()
        .unwrap()
        .contract_code;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("account.wasm");
    std::fs::write(
        &path,
        wasm_declaring("LocalError", &[("FromTheFile", code)]),
    )
    .unwrap();
    let mut request = common::request(CUSTOM_ACCOUNT).await;
    request.wasm_debug = Some(path.clone());
    let rpc = Overlay::on(CUSTOM_ACCOUNT).await.client();
    let report = sororewind::run(&rpc, &request).await.unwrap();

    let cause = report
        .story
        .failure
        .as_ref()
        .unwrap()
        .cause
        .as_ref()
        .unwrap();
    assert_eq!(cause.name.as_deref(), Some("FromTheFile"));
    assert_eq!(
        cause.name_origin.as_deref(),
        Some(format!("spec enum LocalError, from --wasm-debug {}", path.display()).as_str())
    );

    // A file that does not declare the code names nothing, and claims nothing.
    std::fs::write(&path, wasm_declaring("LocalError", &[("Other", code + 1)])).unwrap();
    let rpc = Overlay::on(CUSTOM_ACCOUNT).await.client();
    let report = sororewind::run(&rpc, &request).await.unwrap();
    let cause = report
        .story
        .failure
        .as_ref()
        .unwrap()
        .cause
        .as_ref()
        .unwrap();
    assert_eq!(
        (cause.name.as_deref(), cause.name_origin.as_deref()),
        (None, None)
    );
}

#[tokio::test]
async fn the_stellar_asset_contract_error_is_named() {
    let report = common::replay(FIXTURE).await.unwrap();
    let failure = report.story.failure.as_ref().unwrap();
    assert_eq!(failure.name.as_deref(), Some("BalanceError"));
    // The message keeps the values the contract reported.
    assert!(failure.message.contains('['), "{}", failure.message);
}

/// Rewrite the recorded `resultMetaXdr` of `getTransaction`.
fn edit_meta(result: &mut Value, edit: impl FnOnce(&mut stellar_xdr::TransactionMetaV4)) {
    let mut meta: stellar_xdr::TransactionMeta =
        codec::decode(result["resultMetaXdr"].as_str().unwrap(), "resultMetaXdr").unwrap();
    let stellar_xdr::TransactionMeta::V4(v4) = &mut meta else {
        panic!("the fixture's meta is not a v4 one");
    };
    edit(v4);
    result["resultMetaXdr"] = b64(&meta).into();
}

#[tokio::test]
async fn a_fee_the_network_charged_differently_is_a_divergence() {
    // Same result, same error, but the non-refundable fee the network
    // recorded is not the one the declared resources cost.
    let plain = replay(Overlay::on(FIXTURE).await).await;
    assert_eq!(plain.outcome(), Outcome::Matched);
    assert_eq!(
        plain.fee.onchain_non_refundable,
        Some(plain.fee.local_non_refundable)
    );

    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", |result| {
                edit_meta(result, |meta| {
                    let Some(soroban) = &mut meta.soroban_meta else {
                        panic!("the fixture's meta has no Soroban part");
                    };
                    let stellar_xdr::SorobanTransactionMetaExt::V1(fees) = &mut soroban.ext else {
                        panic!("the fixture's meta has no fee breakdown");
                    };
                    fees.total_non_refundable_resource_fee_charged += 1;
                });
            }),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Diverged);
    assert_eq!(report.fidelity.divergence, Some(Divergence::Fee));
    assert_eq!(
        report.fee.onchain_non_refundable,
        Some(plain.fee.local_non_refundable + 1)
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["fidelity"]["divergence"], "fee");
    assert_eq!(
        json["fee"]["localNonRefundable"],
        plain.fee.local_non_refundable
    );
    let text = sororewind::render::human(&report, false, sororewind::render::Detail::Summary);
    assert!(
        text.contains("same result, different resource fee"),
        "{text}"
    );
}

#[tokio::test]
async fn a_meta_without_a_fee_breakdown_compares_no_fee() {
    // Older meta carries no breakdown: nothing to compare, and not a verdict.
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", |result| {
                edit_meta(result, |meta| {
                    if let Some(soroban) = &mut meta.soroban_meta {
                        soroban.ext = stellar_xdr::SorobanTransactionMetaExt::V0;
                    }
                });
            }),
    )
    .await;
    assert_eq!(report.outcome(), Outcome::Matched);
    assert_eq!(report.fee.onchain_non_refundable, None);
}

#[tokio::test]
async fn events_the_network_never_recorded_are_a_divergence() {
    // The network's call trace departs from the replay's at its first event.
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", |result| {
                edit_meta(result, |meta| {
                    let mut events = meta.diagnostic_events.to_vec();
                    events[0].in_successful_contract_call = !events[0].in_successful_contract_call;
                    meta.diagnostic_events = events.try_into().unwrap();
                });
            }),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Diverged);
    assert_eq!(report.fidelity.divergence, Some(Divergence::Events));
    // Nothing the run knows of changed, and the report says so rather than
    // guessing.
    assert!(report.probable_causes.is_empty());
    let text = sororewind::render::human(&report, false, sororewind::render::Detail::Summary);
    assert!(text.contains("same result, different events"), "{text}");
    assert!(
        text.contains("nothing in this run explains the divergence"),
        "{text}"
    );
}

#[tokio::test]
async fn a_replay_that_stopped_early_is_not_an_events_divergence() {
    // The replay's own events are the start of the network's, which keeps
    // core's trailing diagnostics: every recorded fixture relies on this.
    let report = replay(Overlay::on(FIXTURE).await).await;
    assert!(report.diagnostics.local.len() < report.diagnostics.onchain.len());
    assert_eq!(report.outcome(), Outcome::Matched);
}

/// Rewrite `getLedgerEntries` as if `key` had last been written at `ledger`.
fn modified_at(key: String, ledger: u32) -> impl Fn(&mut Value) + Send + Sync + 'static {
    move |result| {
        for entry in result["entries"].as_array_mut().unwrap() {
            if entry["key"] == key.as_str() {
                entry["lastModifiedLedgerSeq"] = ledger.into();
            }
        }
    }
}

#[tokio::test]
async fn a_divergence_names_the_entry_that_drifted() {
    let plain = replay(Overlay::on(FIXTURE).await).await;
    let key = persistent_entry_key();
    let label = sororewind::display::ledger_key(&codec::decode(&key, "a ledger key").unwrap());

    // The entry was written after the transaction, and the network's
    // recorded result is one the replay no longer reproduces.
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getLedgerEntries", modified_at(key, plain.ledger + 1))
            .rewrite_result("getTransaction", |result| {
                failed_before_the_host(result, InvokeHostFunctionResult::ResourceLimitExceeded);
            }),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Diverged);
    let first = report
        .probable_causes
        .first()
        .expect("a drifted entry is a candidate");
    assert_eq!(first.kind, CulpritKind::EntryDrifted);
    assert_eq!(first.subject, label);

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["probableCauses"][0]["kind"], "entry_drifted");
    assert_eq!(json["probableCauses"][0]["subject"], label.as_str());
    let text = sororewind::render::human(&report, false, sororewind::render::Detail::Summary);
    assert!(text.contains("probably caused by:"), "{text}");
    assert!(text.contains(&format!("1. {label}")), "{text}");
}

#[tokio::test]
async fn a_divergence_with_no_evidence_says_so() {
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", |result| {
                failed_before_the_host(result, InvokeHostFunctionResult::ResourceLimitExceeded);
            }),
    )
    .await;

    assert_eq!(report.outcome(), Outcome::Diverged);
    assert!(report.probable_causes.is_empty());
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["probableCauses"], serde_json::json!([]));
    let text = sororewind::render::human(&report, false, sororewind::render::Detail::Summary);
    assert!(
        text.contains("nothing in this run explains the divergence"),
        "{text}"
    );
}

#[tokio::test]
async fn a_match_names_no_cause() {
    let report = replay(Overlay::on(FIXTURE).await).await;
    assert_eq!(report.outcome(), Outcome::Matched);
    assert!(report.probable_causes.is_empty());
}

// Failure kinds no testnet transaction currently exercises. Each rewrites the
// transaction's arguments or the network's settings so that the real host
// fails the way a contract would, and the recorded result is rewritten to
// the code core would have recorded.

/// Replace the host function's arguments, drop the recorded meta (its
/// diagnostics belong to the original arguments) and record `code` as the
/// network's result.
fn with_args(
    result: &mut Value,
    code: InvokeHostFunctionResult,
    edit: impl FnOnce(&mut Vec<stellar_xdr::ScVal>),
) {
    let envelope: TransactionEnvelope =
        codec::decode(result["envelopeXdr"].as_str().unwrap(), "envelopeXdr").unwrap();
    let TransactionEnvelope::Tx(mut envelope) = envelope else {
        panic!("the fixture's envelope is not a v1 transaction");
    };
    let mut operations = envelope.tx.operations.to_vec();
    let stellar_xdr::OperationBody::InvokeHostFunction(op) = &mut operations[0].body else {
        panic!("the fixture's operation is not a host function");
    };
    let stellar_xdr::HostFunction::InvokeContract(call) = &mut op.host_function else {
        panic!("the fixture does not invoke a contract");
    };
    let mut args = call.args.to_vec();
    edit(&mut args);
    call.args = args.try_into().unwrap();
    envelope.tx.operations = operations.try_into().unwrap();
    result["envelopeXdr"] = b64(&TransactionEnvelope::Tx(envelope)).into();
    failed_before_the_host(result, code);
}

/// Replay `fixture` with `overlay` and check it still matches the network.
async fn replay_matching(fixture: &str, overlay: Overlay) -> Report {
    let request = common::request(fixture).await;
    let report = sororewind::run(&overlay.client(), &request)
        .await
        .expect("the rewritten fixture could not be replayed");
    assert_eq!(report.outcome(), Outcome::Matched, "{fixture}");
    report
}

#[tokio::test]
async fn a_wasm_trap_is_replayed_and_narrated() {
    // A contract compiled from the SDK traps when its argument does not
    // convert: the host reports a WasmVm error.
    let report = replay_matching(
        "nested-contract-error",
        Overlay::on("nested-contract-error")
            .await
            .rewrite_result("getTransaction", |result| {
                with_args(result, InvokeHostFunctionResult::Trapped, |args| {
                    args[0] = stellar_xdr::ScVal::U32(1);
                });
            }),
    )
    .await;
    assert_eq!(report.result.local.code, "trapped");
    let failure = report.story.failure.as_ref().unwrap();
    assert_eq!(failure.error_type, "wasm_vm");
    assert!(failure.message.contains("VM call trapped"), "{failure:?}");
    assert_eq!(failure.function.as_deref(), Some("execute"));
}

#[tokio::test]
async fn a_value_error_is_replayed_and_narrated() {
    // The Stellar Asset Contract cannot read an amount from a symbol.
    let report = replay_matching(
        FIXTURE,
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getTransaction", |result| {
                with_args(result, InvokeHostFunctionResult::Trapped, |args| {
                    args[2] = stellar_xdr::ScVal::Symbol("x".try_into().unwrap());
                });
            }),
    )
    .await;
    let failure = report.story.failure.as_ref().unwrap();
    assert_eq!(failure.error_type, "value");
    assert_eq!(failure.error_code, Some("UnexpectedType"));
}

#[tokio::test]
async fn a_crypto_error_is_replayed_and_narrated() {
    // The custom account's `__check_auth` verifies an ed25519 signature that
    // does not verify, so `verify_sig_ed25519` fails with a crypto error,
    // which the host reports as the authentication failure it caused. The
    // recorded run ran out of budget inside that very call; raising the
    // declared instructions lets it get to the verdict.
    let report = replay_matching(
        "budget-error",
        Overlay::on("budget-error")
            .await
            .rewrite_result("getTransaction", |result| {
                let envelope: TransactionEnvelope =
                    codec::decode(result["envelopeXdr"].as_str().unwrap(), "envelopeXdr").unwrap();
                let TransactionEnvelope::Tx(mut envelope) = envelope else {
                    panic!("the fixture's envelope is not a v1 transaction");
                };
                let TransactionExt::V1(data) = &mut envelope.tx.ext else {
                    panic!("the fixture's transaction carries no Soroban data");
                };
                data.resources.instructions = 400_000_000;
                result["envelopeXdr"] = b64(&TransactionEnvelope::Tx(envelope)).into();
                failed_before_the_host(result, InvokeHostFunctionResult::Trapped);
            }),
    )
    .await;
    let failure = report.story.failure.as_ref().unwrap();
    assert_eq!(failure.error_type, "auth");
    assert!(
        failure.message.contains("crypto error InvalidInput"),
        "{failure:?}"
    );
}

#[tokio::test]
async fn events_over_the_network_limit_fail_like_core() {
    // The host succeeds on `refundable-fee`; core then finds the events larger
    // than the network allows. Rewrite the limit down to one byte.
    let fixture = "refundable-fee";
    let report = replay_matching(
        fixture,
        Overlay::on(fixture)
            .await
            .rewrite_result("getLedgerEntries", |result| {
                let key = b64(&LedgerKey::ConfigSetting(LedgerKeyConfigSetting {
                    config_setting_id: ConfigSettingId::ContractEventsV0,
                }));
                for entry in result["entries"].as_array_mut().unwrap() {
                    if entry["key"] != key.as_str() {
                        continue;
                    }
                    let LedgerEntryData::ConfigSetting(ConfigSettingEntry::ContractEventsV0(
                        mut events,
                    )) = codec::decode::<LedgerEntryData>(
                        entry["xdr"].as_str().unwrap(),
                        "a config setting",
                    )
                    .unwrap()
                    else {
                        panic!("not the events setting");
                    };
                    events.tx_max_contract_events_size_bytes = 1;
                    entry["xdr"] = b64(&LedgerEntryData::ConfigSetting(
                        ConfigSettingEntry::ContractEventsV0(events),
                    ))
                    .into();
                }
            })
            .rewrite_result("getTransaction", |result| {
                failed_before_the_host(result, InvokeHostFunctionResult::ResourceLimitExceeded);
            }),
    )
    .await;
    assert_eq!(report.result.local.code, "resource_limit_exceeded");
    let failure = report.story.failure.as_ref().unwrap();
    assert!(failure.message.contains("events size"), "{failure:?}");
}

use sororewind::report::{EntryReason, EntryStatus, EntryStatusKind};

fn statuses(report: &Report, kind: EntryStatusKind) -> Vec<&EntryStatus> {
    report
        .footprint
        .entry_status
        .iter()
        .filter(|status| status.status == kind)
        .collect()
}

#[tokio::test]
async fn a_missing_entry_is_listed_with_its_reason() {
    // The fixture's footprint holds one key the RPC has no entry for.
    let report = replay(Overlay::on(FIXTURE).await).await;
    assert_eq!(report.footprint.missing, 1);
    assert_eq!(report.footprint.missing_entries.len(), 1);
    let [missing] = statuses(&report, EntryStatusKind::Missing)[..] else {
        panic!("{:?}", report.footprint.entry_status);
    };
    assert_eq!(missing.entry, report.footprint.missing_entries[0]);
    assert_eq!(missing.reason, EntryReason::Absent);

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["footprint"]["missing"], 1);
    assert_eq!(
        json["footprint"]["missingEntries"][0],
        missing.entry.as_str()
    );
    let status = json["footprint"]["entryStatus"]
        .as_array()
        .unwrap()
        .iter()
        .find(|status| status["status"] == "missing")
        .unwrap();
    assert_eq!(status["reason"], "absent");
}

/// The account entry of the fixture's footprint, as a response key.
fn classic_entry_key() -> String {
    let path = common::fixture_dir(FIXTURE).join("ledgerEntries.json");
    let store: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    store["entries"]
        .as_object()
        .unwrap()
        .keys()
        .find(|key| {
            matches!(
                codec::decode::<LedgerKey>(key, "a ledger key").unwrap(),
                LedgerKey::Account(_)
            )
        })
        .expect("the fixture's footprint has no account")
        .clone()
}

#[tokio::test]
async fn an_entry_the_rpc_left_vague_says_what_is_unknown() {
    let key = classic_entry_key();
    let label = sororewind::display::ledger_key(&codec::decode(&key, "a ledger key").unwrap());
    let report = replay(Overlay::on(FIXTURE).await.rewrite_result(
        "getLedgerEntries",
        move |result| {
            for entry in result["entries"].as_array_mut().unwrap() {
                if entry["key"] == key.as_str() {
                    let entry = entry.as_object_mut().unwrap();
                    entry.remove("lastModifiedLedgerSeq");
                    entry.remove("extXdr");
                }
            }
        },
    ))
    .await;

    let [unknown] = statuses(&report, EntryStatusKind::DriftUnknown)[..] else {
        panic!("{:?}", report.footprint.entry_status);
    };
    assert_eq!(unknown.entry, label);
    assert_eq!(unknown.reason, EntryReason::NoModificationLedger);
    let [ext] = statuses(&report, EntryStatusKind::ExtMissing)[..] else {
        panic!("{:?}", report.footprint.entry_status);
    };
    assert_eq!(
        (ext.entry.as_str(), ext.reason),
        (label.as_str(), EntryReason::NoExtension)
    );
}

#[tokio::test]
async fn an_entry_that_could_not_be_rebuilt_says_why() {
    let plain = replay(Overlay::on(FIXTURE).await).await;
    let key = persistent_entry_key();
    let label = sororewind::display::ledger_key(&codec::decode(&key, "a ledger key").unwrap());

    // The entry changed after the transaction, but the recorded ledger
    // metadata holds no change to it: the metadata contradicts the entry.
    let report = replay(Overlay::on(FIXTURE).await.rewrite_result(
        "getLedgerEntries",
        modified_at(key.clone(), plain.ledger + 1),
    ))
    .await;
    let [unresolved] = statuses(&report, EntryStatusKind::Unresolved)[..] else {
        panic!("{:?}", report.footprint.entry_status);
    };
    assert_eq!(unresolved.entry, label);
    assert_eq!(unresolved.reason, EntryReason::MetadataUnreadable);
    assert!(report.footprint.rebuild_note.is_some(), "the note stays");

    // Without the transaction's position in its ledger, changes made after it
    // in that ledger cannot be told from earlier ones.
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getLedgerEntries", modified_at(key, plain.ledger + 1))
            .rewrite_result("getTransaction", |result| {
                result.as_object_mut().unwrap().remove("applicationOrder");
            }),
    )
    .await;
    // Every drifted entry is in the same position, the one the overlay
    // drifted among them.
    let unresolved = statuses(&report, EntryStatusKind::Unresolved);
    assert!(unresolved.iter().any(|status| status.entry == label));
    assert!(
        unresolved
            .iter()
            .all(|status| status.reason == EntryReason::NoApplyOrder)
    );
    let json = serde_json::to_value(&report).unwrap();
    assert!(
        json["footprint"]["entryStatus"]
            .as_array()
            .unwrap()
            .iter()
            .any(|status| status["status"] == "unresolved" && status["reason"] == "no_apply_order")
    );
}

#[tokio::test]
async fn a_match_over_unverified_state_says_so() {
    // Plain fixture: nothing unverified, nothing said.
    let plain = replay(Overlay::on(FIXTURE).await).await;
    assert!(plain.trust.verified && plain.trust.caveats.is_empty());
    let text = sororewind::render::human(&plain, false, sororewind::render::Detail::Summary);
    assert!(!text.contains("could not be verified"), "{text}");

    // The RPC leaves out when an entry last changed, and the apply order.
    let key = classic_entry_key();
    let label = sororewind::display::ledger_key(&codec::decode(&key, "a ledger key").unwrap());
    let report = replay(
        Overlay::on(FIXTURE)
            .await
            .rewrite_result("getLedgerEntries", move |result| {
                for entry in result["entries"].as_array_mut().unwrap() {
                    if entry["key"] == key.as_str() {
                        entry
                            .as_object_mut()
                            .unwrap()
                            .remove("lastModifiedLedgerSeq");
                    }
                }
            })
            .rewrite_result("getTransaction", |result| {
                result.as_object_mut().unwrap().remove("applicationOrder");
            }),
    )
    .await;

    // Still a match: the contract does not draw random numbers.
    assert_eq!(report.outcome(), Outcome::Matched);
    assert!(!report.trust.verified);
    let kinds: Vec<_> = report.trust.caveats.iter().map(|c| c.kind).collect();
    assert!(kinds.contains(&sororewind::trust::CaveatKind::EntryDriftUnknown));
    assert!(kinds.contains(&sororewind::trust::CaveatKind::PrngSeedFixed));

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["trust"]["verified"], false);
    let caveat = json["trust"]["caveats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|caveat| caveat["kind"] == "entry_drift_unknown")
        .unwrap();
    assert_eq!(caveat["subject"], label.as_str());

    let text = sororewind::render::human(&report, false, sororewind::render::Detail::Summary);
    assert!(
        text.contains("✓ replay matches network")
            && text.contains("⚠ the match rests on state that could not be verified:"),
        "{text}"
    );
    assert!(text.contains(&label), "{text}");
}

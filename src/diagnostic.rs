//! diagnostic: the run's primary failure, as a compiler would report it.
//!
//! A [`Diagnostic`] names the one failure a reader should look at first, with
//! a stable [`Code`] tools can key on, where it happened and what to check.
//! Like the findings, it is derived from the report's data and never adds
//! facts to it: a field the evidence does not support is left out, never
//! guessed.
//!
//! Classification follows fixed rules over structured values — the result
//! code the network recorded, then the host error's category and code. The
//! one value read out of text is the ledger key of a footprint violation,
//! which the host only reports in its diagnostic message. After a divergence
//! the replay's error may not be the network's, so only the on-chain result
//! code is used, as in the catalog of known failures.

use serde::Serialize;

use crate::findings::Inputs;
use crate::replay::Divergence;
use crate::story::Failure;

/// The phrase the host's diagnostic carries when a key is outside the
/// footprint; `storage/ExceededLimit` alone does not say so.
const OUTSIDE_FOOTPRINT: &str = "outside of the footprint";

/// A stable identifier for a kind of failure. The string never changes
/// meaning; a new kind of failure gets a new code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Code {
    /// A ledger key was accessed outside the transaction's footprint.
    #[serde(rename = "SRW1001")]
    FootprintViolation,
    /// The host's authorization checks failed.
    #[serde(rename = "SRW1002")]
    Authorization,
    /// A CPU, memory or other resource limit ran out.
    #[serde(rename = "SRW1003")]
    BudgetExceeded,
    /// The contract returned one of its own errors.
    #[serde(rename = "SRW1004")]
    ContractError,
    /// A ledger entry the invocation needed did not exist.
    #[serde(rename = "SRW1005")]
    MissingStorage,
    /// A persistent entry the invocation needed was archived.
    #[serde(rename = "SRW1006")]
    EntryArchived,
    /// The refundable fee did not cover rent or events.
    #[serde(rename = "SRW1007")]
    RefundableFee,
    /// Any other failure; the host error is kept as reported.
    #[serde(rename = "SRW1999")]
    Unclassified,
}

impl Code {
    /// Every code, in numeric order.
    pub const ALL: [Code; 8] = [
        Code::FootprintViolation,
        Code::Authorization,
        Code::BudgetExceeded,
        Code::ContractError,
        Code::MissingStorage,
        Code::EntryArchived,
        Code::RefundableFee,
        Code::Unclassified,
    ];

    /// `SRW1001`, as the JSON document and the terminal show it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::FootprintViolation => "SRW1001",
            Code::Authorization => "SRW1002",
            Code::BudgetExceeded => "SRW1003",
            Code::ContractError => "SRW1004",
            Code::MissingStorage => "SRW1005",
            Code::EntryArchived => "SRW1006",
            Code::RefundableFee => "SRW1007",
            Code::Unclassified => "SRW1999",
        }
    }

    /// The machine-readable category, lower snake_case.
    pub const fn kind(self) -> &'static str {
        match self {
            Code::FootprintViolation => "footprint_violation",
            Code::Authorization => "authorization",
            Code::BudgetExceeded => "budget_exceeded",
            Code::ContractError => "contract_error",
            Code::MissingStorage => "missing_storage",
            Code::EntryArchived => "entry_archived",
            Code::RefundableFee => "refundable_fee",
            Code::Unclassified => "unclassified",
        }
    }

    /// The failure in a few words.
    pub const fn message(self) -> &'static str {
        match self {
            Code::FootprintViolation => "Undeclared storage access",
            Code::Authorization => "Authorization failed",
            Code::BudgetExceeded => "Resource limit exceeded",
            Code::ContractError => "Contract returned an error",
            Code::MissingStorage => "Missing ledger entry",
            Code::EntryArchived => "Archived ledger entry",
            Code::RefundableFee => "Insufficient refundable fee",
            Code::Unclassified => "Unclassified failure",
        }
    }
}

/// How serious a diagnostic is. sororewind only replays failed transactions,
/// so the primary diagnostic is always an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// The transaction failed.
    Error,
}

/// The run's primary failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// Stable identifier, `SRW1001`.
    pub code: Code,
    /// Always `error`.
    pub severity: Level,
    /// Category matching `code`: `footprint_violation`, `authorization`, …
    pub kind: &'static str,
    /// The failure in a few words, fixed per code.
    pub message: &'static str,
    /// The contract and function whose frame raised the error, as far as
    /// they are known.
    pub location: Location,
    /// The values that identify this failure; only those relevant to its
    /// kind and supported by the evidence are present.
    pub details: Details,
    /// What this kind of failure typically means, from the catalog. Not
    /// verified for this transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<&'static str>,
    /// One thing to check next, from the catalog. A suggestion, never a
    /// verified fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<&'static str>,
    /// Where the diagnostic comes from and what was verified.
    pub evidence: Evidence,
}

/// Where the failure happened. Both fields are absent when no frame was open
/// or the replay diverged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Full strkey of the contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// The function, without arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

/// The values a diagnostic rests on. Absent fields are unknown or do not
/// apply to the diagnostic's kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    /// The result code the network recorded: `trapped`,
    /// `resource_limit_exceeded`, …
    pub result: &'static str,
    /// The host error as `category/Code` (`storage/ExceededLimit`), or the
    /// category alone for contract errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_error: Option<String>,
    /// The ledger key outside the footprint, as the host's diagnostic
    /// printed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The contract that owns `key`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_contract: Option<String>,
    /// The contract's own error number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_code: Option<u32>,
    /// The contract's name for that error, when its spec has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_name: Option<String>,
    /// The contract error a host error only wraps, such as a custom
    /// account's `__check_auth` rejecting authorization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caused_by: Option<CausedBy>,
    /// The message the host attached to the error, for kinds whose details
    /// say nothing else about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_message: Option<String>,
}

/// A contract error behind a host error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CausedBy {
    /// The contract's own error number.
    pub contract_code: u32,
    /// The contract's name for it, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The function that returned it, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

/// What a diagnostic is based on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// `host_diagnostic` when the host error from a matching replay was
    /// used, `result_code` when only the network's result code was.
    pub source: EvidenceSource,
    /// The replay reproduced the network's outcome. It does not mean every
    /// detail above was compared with the network's.
    pub outcome_reproduced: bool,
    /// Whether the key was checked against the transaction's footprint;
    /// only for footprint violations, and always `false` for now: the host
    /// reported it, sororewind did not check it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footprint_verified: Option<bool>,
    /// Whether `cause` and `help` were verified for this transaction: always
    /// `false`, they are what this kind of failure usually means.
    pub help_verified: bool,
}

/// Where a diagnostic's facts come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// The host error of a replay that matched the network.
    HostDiagnostic,
    /// The result code the network recorded.
    ResultCode,
}

/// Whether the replay reproduced the network's outcome, in a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replay {
    /// The verdict, phrased no stronger than what was compared.
    pub status: ReplayStatus,
    /// The result code the network recorded.
    pub on_chain: &'static str,
    /// The result code the replay ended in.
    pub local: &'static str,
    /// The two result codes are the same. The replay can still diverge in
    /// its error, fee or events: see `status` and `fidelity`.
    pub outcome_matches: bool,
    /// How the replay diverged, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergence: Option<Divergence>,
}

/// The replay's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayStatus {
    /// Both runs trapped, and the replay matched the network.
    TrapReproduced,
    /// Both runs ended in the same failure other than a trap (a resource
    /// limit, an archived entry, …), and the replay matched the network.
    FailureReproduced,
    /// The replay succeeded where the network failed.
    ExecutionSucceeded,
    /// The replay did not reproduce the network's outcome.
    ReplayMismatch,
}

impl ReplayStatus {
    /// The verdict in words.
    pub const fn label(self) -> &'static str {
        match self {
            ReplayStatus::TrapReproduced => "trap reproduced",
            ReplayStatus::FailureReproduced => "failure reproduced",
            ReplayStatus::ExecutionSucceeded => "execution succeeded",
            ReplayStatus::ReplayMismatch => "replay mismatch",
        }
    }
}

/// The primary diagnostic of a run.
pub fn derive(inputs: &Inputs<'_>) -> Diagnostic {
    let result = inputs.result.onchain.code;
    // As in the catalog: after a divergence the replay's error need not be
    // the one the network saw.
    let failure = inputs.failure.filter(|_| inputs.fidelity.matches);
    let code = classify(result, failure);
    let mut details = Details {
        result,
        ..Details::default()
    };
    let mut location = Location::default();
    if let Some(failure) = failure {
        location = Location {
            contract: failure.contract.clone(),
            function: failure.function.clone(),
        };
        fill(&mut details, code, failure);
    }
    let guidance = crate::catalog::lookup(inputs).map(|(guidance, _)| guidance);
    Diagnostic {
        code,
        severity: Level::Error,
        kind: code.kind(),
        message: code.message(),
        location,
        details,
        cause: guidance.as_ref().map(|guidance| guidance.explanation),
        help: guidance.and_then(|guidance| guidance.checks.first().copied()),
        evidence: Evidence {
            source: if failure.is_some() {
                EvidenceSource::HostDiagnostic
            } else {
                EvidenceSource::ResultCode
            },
            outcome_reproduced: inputs.fidelity.matches,
            footprint_verified: (code == Code::FootprintViolation).then_some(false),
            help_verified: false,
        },
    }
}

/// The replay's verdict, from the result codes and the fidelity check.
pub fn replay(inputs: &Inputs<'_>) -> Replay {
    let on_chain = inputs.result.onchain.code;
    let local = inputs.result.local.code;
    let status = if inputs.fidelity.matches {
        if local == "trapped" {
            ReplayStatus::TrapReproduced
        } else {
            ReplayStatus::FailureReproduced
        }
    } else if local == "success" {
        ReplayStatus::ExecutionSucceeded
    } else {
        ReplayStatus::ReplayMismatch
    };
    Replay {
        status,
        on_chain,
        local,
        outcome_matches: on_chain == local,
        divergence: inputs.fidelity.divergence,
    }
}

/// The code for a failure: the result code first, since it is what the
/// network decided on, then the host error that led to a trap.
fn classify(result: &str, failure: Option<&Failure>) -> Code {
    match result {
        "entry_archived" => return Code::EntryArchived,
        "insufficient_refundable_fee" => return Code::RefundableFee,
        "resource_limit_exceeded" => return Code::BudgetExceeded,
        _ => {}
    }
    let Some(failure) = failure else {
        return Code::Unclassified;
    };
    match (failure.error_type, failure.error_code) {
        ("storage", Some("ExceededLimit")) if failure.message.contains(OUTSIDE_FOOTPRINT) => {
            Code::FootprintViolation
        }
        ("storage", Some("MissingValue")) => Code::MissingStorage,
        ("auth", _) => Code::Authorization,
        ("budget", Some("ExceededLimit")) => Code::BudgetExceeded,
        ("contract", _) => Code::ContractError,
        _ => Code::Unclassified,
    }
}

/// The details of `failure` that matter to `code`.
fn fill(details: &mut Details, code: Code, failure: &Failure) {
    details.host_error = Some(match failure.error_code {
        Some(error_code) => format!("{}/{error_code}", failure.error_type),
        None => failure.error_type.to_string(),
    });
    match code {
        Code::FootprintViolation => {
            if let Some((contract, key)) = footprint_key(&failure.message) {
                details.key_contract = Some(contract);
                details.key = Some(key);
            }
        }
        Code::Authorization => {
            details.caused_by = failure.cause.as_ref().map(|cause| CausedBy {
                contract_code: cause.contract_code,
                name: cause.name.clone(),
                function: cause.function.clone(),
            });
            if details.caused_by.is_none() {
                details.host_message = Some(failure.message.clone());
            }
        }
        Code::ContractError => {
            details.contract_code = failure.contract_code;
            details.error_name = failure.name.clone();
        }
        Code::BudgetExceeded
        | Code::MissingStorage
        | Code::EntryArchived
        | Code::RefundableFee
        | Code::Unclassified => {
            details.host_message = Some(failure.message.clone());
        }
    }
}

/// The contract and key a footprint violation names: the host's message is
/// `… outside of the footprint [<contract>, <key>]`, its arguments as the
/// story prints them. `None` when it is not in that shape.
fn footprint_key(message: &str) -> Option<(String, String)> {
    let (_, rest) = message.split_once(OUTSIDE_FOOTPRINT)?;
    let inner = rest.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (contract, key) = inner.split_once(", ")?;
    let contract = contract.trim();
    // A contract strkey has no separators of its own; anything else means
    // the arguments are not the ones expected.
    if !contract.starts_with('C') || contract.contains([' ', '[', ']', ',']) {
        return None;
    }
    let key = key.trim();
    (!key.is_empty()).then(|| (contract.to_string(), key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::Fidelity;
    use crate::report::{FeeReport, ResultPair, ResultSummary};
    use crate::story::Cause;
    use crate::trust::Trust;
    use stellar_xdr::{ScError, ScErrorCode};

    const CONTRACT: &str = "CBLLSKUH6N6HGFV5MSAMMJWJFYNUFEH7EGH2T4HJKOVMIZNJ23BZSMCM";

    fn failure(
        error_type: &'static str,
        error_code: Option<&'static str>,
        message: &str,
    ) -> Failure {
        Failure {
            error: ScError::Storage(ScErrorCode::ExceededLimit),
            error_type,
            error_code,
            contract_code: None,
            description: format!("{error_type} error"),
            name: None,
            name_origin: None,
            message: message.to_string(),
            contract: Some(CONTRACT.to_string()),
            function: Some("withdraw_position".to_string()),
            depth: Some(0),
            trail: Vec::new(),
            cause: None,
            call_index: None,
        }
    }

    fn summary(code: &'static str) -> ResultSummary {
        ResultSummary {
            code,
            result_hash: None,
        }
    }

    /// The diagnostic and replay verdict for a run.
    fn run(
        onchain: &'static str,
        local: &'static str,
        fidelity: Fidelity,
        failure: Option<&Failure>,
    ) -> (Diagnostic, Replay) {
        let result = ResultPair {
            onchain: summary(onchain),
            local: summary(local),
        };
        let trust = Trust {
            verified: true,
            caveats: Vec::new(),
        };
        let inputs = Inputs {
            fidelity: &fidelity,
            result: &result,
            failure,
            fee: &FeeReport::default(),
            probable_causes: &[],
            trust: &trust,
        };
        (derive(&inputs), replay(&inputs))
    }

    fn matched(failure: &Failure) -> Diagnostic {
        run("trapped", "trapped", Fidelity::MATCH, Some(failure)).0
    }

    const FOOTPRINT: &str = "trying to access contract data key outside of the footprint \
        [CBLLSKUH6N6HGFV5MSAMMJWJFYNUFEH7EGH2T4HJKOVMIZNJ23BZSMCM, [UserRewardState, GAEG]]";

    #[test]
    fn codes_are_stable() {
        // Codes are part of the public interface: changing this table is a
        // breaking change, adding to it is not.
        let table: Vec<_> = Code::ALL
            .iter()
            .map(|code| {
                let json = serde_json::to_value(code).unwrap();
                assert_eq!(json, code.as_str(), "{code:?}");
                (code.as_str(), code.kind())
            })
            .collect();
        assert_eq!(
            table,
            [
                ("SRW1001", "footprint_violation"),
                ("SRW1002", "authorization"),
                ("SRW1003", "budget_exceeded"),
                ("SRW1004", "contract_error"),
                ("SRW1005", "missing_storage"),
                ("SRW1006", "entry_archived"),
                ("SRW1007", "refundable_fee"),
                ("SRW1999", "unclassified"),
            ]
        );
    }

    #[test]
    fn a_footprint_violation_names_its_key() {
        let diagnostic = matched(&failure("storage", Some("ExceededLimit"), FOOTPRINT));
        assert_eq!(diagnostic.code, Code::FootprintViolation);
        assert_eq!(diagnostic.message, "Undeclared storage access");
        assert_eq!(
            diagnostic.details.key.as_deref(),
            Some("[UserRewardState, GAEG]")
        );
        assert_eq!(diagnostic.details.key_contract.as_deref(), Some(CONTRACT));
        assert_eq!(
            diagnostic.details.host_error.as_deref(),
            Some("storage/ExceededLimit")
        );
        assert_eq!(
            diagnostic.location.function.as_deref(),
            Some("withdraw_position")
        );
        assert_eq!(diagnostic.evidence.source, EvidenceSource::HostDiagnostic);
        assert_eq!(diagnostic.evidence.footprint_verified, Some(false));
        assert!(!diagnostic.evidence.help_verified);
        assert!(diagnostic.cause.is_some() && diagnostic.help.is_some());
    }

    #[test]
    fn exceeded_limit_without_footprint_evidence_is_not_a_footprint_violation() {
        let diagnostic = matched(&failure(
            "storage",
            Some("ExceededLimit"),
            "key size exceeds the limit",
        ));
        assert_eq!(diagnostic.code, Code::Unclassified);
        assert_eq!(diagnostic.details.key, None);
        // The original host error survives.
        assert_eq!(
            diagnostic.details.host_error.as_deref(),
            Some("storage/ExceededLimit")
        );
        assert_eq!(
            diagnostic.details.host_message.as_deref(),
            Some("key size exceeds the limit")
        );
        // No catalog entry matches, so nothing is suggested.
        assert_eq!((diagnostic.cause, diagnostic.help), (None, None));
    }

    #[test]
    fn an_unknown_host_error_keeps_what_the_host_said() {
        let diagnostic = matched(&failure("wasm_vm", Some("InvalidAction"), "unreachable"));
        assert_eq!(diagnostic.code, Code::Unclassified);
        assert_eq!(diagnostic.kind, "unclassified");
        assert_eq!(
            diagnostic.details.host_error.as_deref(),
            Some("wasm_vm/InvalidAction")
        );
        assert_eq!(
            diagnostic.details.host_message.as_deref(),
            Some("unreachable")
        );
    }

    #[test]
    fn host_errors_map_to_their_codes() {
        for (error_type, error_code, code) in [
            ("auth", Some("InvalidAction"), Code::Authorization),
            ("auth", Some("ExistingValue"), Code::Authorization),
            ("budget", Some("ExceededLimit"), Code::BudgetExceeded),
            ("storage", Some("MissingValue"), Code::MissingStorage),
            ("contract", None, Code::ContractError),
        ] {
            assert_eq!(
                matched(&failure(error_type, error_code, "msg")).code,
                code,
                "{error_type}"
            );
        }
    }

    #[test]
    fn the_result_code_decides_first() {
        let fee = failure("budget", Some("ExceededLimit"), "refundable fee");
        for (result, code) in [
            ("insufficient_refundable_fee", Code::RefundableFee),
            ("entry_archived", Code::EntryArchived),
            ("resource_limit_exceeded", Code::BudgetExceeded),
        ] {
            let (diagnostic, replay) = run(result, result, Fidelity::MATCH, Some(&fee));
            assert_eq!(diagnostic.code, code, "{result}");
            assert_eq!(replay.status, ReplayStatus::FailureReproduced, "{result}");
        }
    }

    #[test]
    fn a_contract_error_and_a_wrapped_one_name_the_contract_error() {
        let mut contract = failure("contract", None, "failing");
        contract.contract_code = Some(10);
        contract.name = Some("BalanceError".to_string());
        let details = matched(&contract).details;
        assert_eq!(details.contract_code, Some(10));
        assert_eq!(details.error_name.as_deref(), Some("BalanceError"));

        let mut auth = failure(
            "auth",
            Some("InvalidAction"),
            "failed account authentication",
        );
        auth.cause = Some(Cause {
            contract_code: 3,
            description: "contract error #3".to_string(),
            name: Some("SpendingLimitExceeded".to_string()),
            name_origin: None,
            message: String::new(),
            contract: None,
            function: Some("__check_auth".to_string()),
            depth: Some(1),
        });
        let details = matched(&auth).details;
        assert_eq!(
            details.caused_by,
            Some(CausedBy {
                contract_code: 3,
                name: Some("SpendingLimitExceeded".to_string()),
                function: Some("__check_auth".to_string()),
            })
        );
        assert_eq!(details.host_message, None);
    }

    #[test]
    fn missing_metadata_is_left_out() {
        let mut bare = failure("storage", Some("ExceededLimit"), "outside of the footprint");
        bare.contract = None;
        bare.function = None;
        let diagnostic = matched(&bare);
        assert_eq!(diagnostic.code, Code::FootprintViolation);
        assert_eq!(diagnostic.location, Location::default());
        assert_eq!(
            (&diagnostic.details.key, &diagnostic.details.key_contract),
            (&None, &None)
        );
        let json = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(json["location"], serde_json::json!({}));
        assert!(json["details"].get("key").is_none(), "{json}");

        // No failure at all: the result code is all there is.
        let (diagnostic, _) = run("trapped", "trapped", Fidelity::MATCH, None);
        assert_eq!(diagnostic.code, Code::Unclassified);
        assert_eq!(diagnostic.details.host_error, None);
        assert_eq!(diagnostic.evidence.source, EvidenceSource::ResultCode);
    }

    #[test]
    fn the_replay_verdict_claims_no_more_than_was_compared() {
        let storage = failure("storage", Some("ExceededLimit"), FOOTPRINT);
        let (diagnostic, replay) = run("trapped", "trapped", Fidelity::MATCH, Some(&storage));
        assert_eq!(replay.status, ReplayStatus::TrapReproduced);
        assert!(replay.outcome_matches && diagnostic.evidence.outcome_reproduced);

        // Same result, different events: the outcome matches, the replay
        // does not.
        let (diagnostic, replay) = run(
            "trapped",
            "trapped",
            Fidelity::diverged(Divergence::Events),
            Some(&storage),
        );
        assert_eq!(replay.status, ReplayStatus::ReplayMismatch);
        assert!(replay.outcome_matches);
        assert_eq!(replay.divergence, Some(Divergence::Events));
        // The replay's error is not trusted after a divergence.
        assert_eq!(diagnostic.code, Code::Unclassified);
        assert_eq!(diagnostic.location, Location::default());
        assert_eq!(diagnostic.evidence.source, EvidenceSource::ResultCode);
        assert!(!diagnostic.evidence.outcome_reproduced);

        let (_, replay) = run(
            "trapped",
            "success",
            Fidelity::diverged(Divergence::Result),
            None,
        );
        assert_eq!(replay.status, ReplayStatus::ExecutionSucceeded);
        assert!(!replay.outcome_matches);
    }

    #[test]
    fn footprint_keys_are_only_read_from_the_expected_shape() {
        assert_eq!(
            footprint_key(FOOTPRINT),
            Some((CONTRACT.to_string(), "[UserRewardState, GAEG]".to_string()))
        );
        assert_eq!(
            footprint_key("key outside of the footprint [CABC, \"BTC\"]"),
            Some(("CABC".to_string(), "\"BTC\"".to_string()))
        );
        for odd in [
            "outside of the footprint",
            "outside of the footprint []",
            "outside of the footprint [CABC]",
            "outside of the footprint [[a, b], key]",
            "outside of the footprint CABC, key",
        ] {
            assert_eq!(footprint_key(odd), None, "{odd}");
        }
    }
}

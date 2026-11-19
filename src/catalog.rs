//! Evidence-driven explanations for common Soroban failures.

use crate::findings::{Guidance, Inputs};
use crate::story::Failure;

enum Pattern {
    Result(&'static str),
    Failure {
        kind: &'static str,
        code: Option<&'static str>,
        named: Option<&'static str>,
        diagnostic: Option<&'static str>,
        wrapped: bool,
    },
}

impl Pattern {
    fn matches(&self, result: &str, failure: Option<&Failure>) -> bool {
        match self {
            Self::Result(code) => result == *code,
            Self::Failure {
                kind,
                code,
                named,
                diagnostic,
                wrapped,
            } => failure.is_some_and(|failure| {
                failure.error_type == *kind
                    && code.is_none_or(|code| failure.error_code == Some(code))
                    && named.is_none_or(|name| failure.name.as_deref() == Some(name))
                    && diagnostic.is_none_or(|phrase| failure.message.contains(phrase))
                    && (!wrapped || failure.cause.is_some())
            }),
        }
    }
}

struct Entry {
    pattern: Pattern,
    guidance: Guidance,
}

const fn rule(kind: &'static str, code: Option<&'static str>) -> Pattern {
    Pattern::Failure {
        kind,
        code,
        named: None,
        diagnostic: None,
        wrapped: false,
    }
}

const CATALOG: &[Entry] = &[
    Entry {
        pattern: Pattern::Result("entry_archived"),
        guidance: Guidance::unverified(
            "entry-archived",
            "A persistent ledger entry needed by the invocation was archived.",
            &[
                "Check the archived keys and TTL evidence in the footprint.",
                "Check whether fresh simulation includes restoration before submitting again.",
            ],
        ),
    },
    Entry {
        pattern: Pattern::Result("insufficient_refundable_fee"),
        guidance: Guidance::unverified(
            "refundable-fee",
            "The refundable fee could not cover the rent or events charged for this invocation.",
            &[
                "Compare the simulated refundable fee with the transaction budget and storage changes.",
            ],
        ),
    },
    Entry {
        pattern: Pattern::Failure {
            kind: "auth",
            code: Some("InvalidAction"),
            named: None,
            diagnostic: None,
            wrapped: true,
        },
        guidance: Guidance::unverified(
            "custom-account-auth",
            "A custom account rejected authorization and the host surfaced its contract error.",
            &[
                "Inspect the named cause and its __check_auth call.",
                "Check the account's authorization rules and the signed invocation tree.",
            ],
        ),
    },
    Entry {
        pattern: rule("auth", Some("InvalidAction")),
        guidance: Guidance::unverified(
            "authorization",
            "The invocation did not satisfy the host's authorization checks.",
            &[
                "Compare the recorded authorization entries with the signer, arguments and invocation tree.",
            ],
        ),
    },
    Entry {
        pattern: rule("auth", Some("ExistingValue")),
        guidance: Guidance::unverified(
            "authorization-nonce",
            "The authorization check encountered an existing value, such as a consumed nonce.",
            &[
                "Check the diagnostic message and the account's nonce, then compare with fresh simulation.",
            ],
        ),
    },
    Entry {
        pattern: Pattern::Failure {
            kind: "storage",
            code: Some("ExceededLimit"),
            named: None,
            diagnostic: Some("outside of the footprint"),
            wrapped: false,
        },
        guidance: Guidance::unverified(
            "footprint",
            "The contract accessed a ledger key outside the declared transaction footprint.",
            &[
                "Check the key in the diagnostic against the simulated read-only and read-write footprint.",
            ],
        ),
    },
    Entry {
        pattern: rule("storage", Some("MissingValue")),
        guidance: Guidance::unverified(
            "missing-storage",
            "A required ledger entry could not be loaded.",
            &["Check the key, storage durability, and TTL or archive status in the footprint."],
        ),
    },
    Entry {
        pattern: rule("budget", Some("ExceededLimit")),
        guidance: Guidance::unverified(
            "resource-budget",
            "The invocation exceeded a resource limit.",
            &["Check the budget section and diagnostic message for the resource that ran out."],
        ),
    },
    Entry {
        pattern: Pattern::Result("resource_limit_exceeded"),
        guidance: Guidance::unverified(
            "resource-budget",
            "The network recorded a resource limit failure.",
            &["Check the declared limits against the CPU, memory and ledger-access diagnostics."],
        ),
    },
    Entry {
        pattern: Pattern::Failure {
            kind: "contract",
            code: None,
            named: Some("BalanceError"),
            diagnostic: Some("resulting balance"),
            wrapped: false,
        },
        guidance: Guidance::unverified(
            "token-balance",
            "The transfer would put an account balance outside the token's permitted range.",
            &["Compare the attempted amount with the available balance and any account limits."],
        ),
    },
    Entry {
        pattern: rule("contract", None),
        guidance: Guidance::unverified(
            "contract-error",
            "The contract returned an application-defined error.",
            &[
                "Check the named error in the contract spec and the innermost failing call's inputs.",
            ],
        ),
    },
];

pub(crate) fn lookup(inputs: &Inputs<'_>) -> Option<(Guidance, &'static str)> {
    // A divergence can still support advice about the recorded result code,
    // but its local exception may not be the exception the network saw.
    let failure = inputs.failure.filter(|_| inputs.fidelity.matches);
    CATALOG
        .iter()
        .find(|entry| entry.pattern.matches(inputs.result.onchain.code, failure))
        .map(|entry| {
            let evidence = match entry.pattern {
                Pattern::Result(_) => "result.onchain",
                Pattern::Failure { .. } => "failure",
            };
            (entry.guidance.clone(), evidence)
        })
}

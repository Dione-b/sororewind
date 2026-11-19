//! findings: the report's evidence, ranked and in one list.
//!
//! The report already holds everything a reader needs, spread over several
//! sections: the failure, the fidelity verdict, the probable causes, the
//! caveats. A finding is one line of that, with a severity and pointers to the
//! fields it rests on, so the reader can tell what matters without
//! interpreting each section.
//!
//! The list keeps two things apart: the transaction's own failure
//! ([`Severity::Error`]) and everything about the replay itself
//! ([`Severity::Divergence`], [`Severity::Warning`], [`Severity::Limitation`]).
//! Findings are derived from the report's data and never add facts to it.

use serde::Serialize;

use crate::culprit::Culprit;
use crate::replay::Divergence;
use crate::replay::Fidelity;
use crate::report::{FeeReport, ResultPair};
use crate::story::Failure;
use crate::trust::{Caveat, CaveatKind, Trust};

/// How many subjects a finding names before it says "and N more".
const SUBJECTS_NAMED: usize = 3;

/// How much a finding matters, most severe first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The transaction's own failure, as the network recorded it.
    Error,
    /// The replay did not reproduce the network's outcome.
    Divergence,
    /// An input of the replay may differ from what the transaction saw.
    Warning,
    /// Something the replay could not compare or reproduce.
    Limitation,
}

/// One conclusion drawn from the report, and what it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// How much it matters.
    pub severity: Severity,
    /// The conclusion in a sentence: the probable cause, when there is one.
    pub summary: String,
    /// The report fields it is based on, as paths into the JSON document
    /// (`failure`, `probableCauses[0]`, `trust.caveats[2]`).
    pub evidence: Vec<String>,
    /// A matching catalog explanation and checks to investigate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<Guidance>,
}

/// Catalog guidance associated with replay evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Guidance {
    /// Stable identifier for this catalog entry.
    pub id: &'static str,
    /// What the observed failure typically means.
    pub explanation: &'static str,
    /// Checks a developer can make for their transaction.
    pub checks: &'static [&'static str],
    /// Whether the suggested checks were verified for this transaction.
    pub verified: bool,
}

impl Guidance {
    pub(crate) const fn unverified(
        id: &'static str,
        explanation: &'static str,
        checks: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            explanation,
            checks,
            verified: false,
        }
    }
}

/// What the findings are derived from.
pub struct Inputs<'a> {
    /// Whether the replay matched, and how it diverged.
    pub fidelity: &'a Fidelity,
    /// The network's result and the replay's.
    pub result: &'a ResultPair,
    /// The error the replay ended on.
    pub failure: Option<&'a Failure>,
    /// The resource fee, for a fee divergence.
    pub fee: &'a FeeReport,
    /// What probably caused a divergence.
    pub probable_causes: &'a [Culprit],
    /// What could not be verified.
    pub trust: &'a Trust,
}

/// The findings of a run, most severe first; within a severity, in the order
/// the report lists their evidence.
pub fn derive(inputs: &Inputs<'_>) -> Vec<Finding> {
    let mut first = on_chain_failure(inputs);
    if let Some((guidance, evidence)) = crate::catalog::lookup(inputs) {
        first.guidance = Some(guidance);
        if !first.evidence.iter().any(|path| path == evidence) {
            first.evidence.push(evidence.into());
        }
    }
    let mut findings = vec![first];
    findings.extend(divergence(inputs));
    findings.extend(caveats(&inputs.trust.caveats));
    // Stable: keeps the evidence order inside a severity.
    findings.sort_by_key(|finding| finding.severity);
    findings
}

/// The transaction failed: always true, since only failures are replayed.
fn on_chain_failure(inputs: &Inputs<'_>) -> Finding {
    // The replay's failure is the network's only when they matched; after a
    // divergence the network's own error is known by its result code alone.
    match inputs.failure.filter(|_| inputs.fidelity.matches) {
        Some(failure) => {
            let place = failure
                .function
                .as_ref()
                .map_or(String::new(), |function| format!(" in {function}"));
            let mut evidence = vec!["failure".to_string()];
            if failure.cause.is_some() {
                evidence.push("failure.cause".to_string());
            }
            Finding {
                severity: Severity::Error,
                summary: format!("the transaction failed: {}{place}", failure.description),
                guidance: None,
                evidence,
            }
        }
        None => Finding {
            severity: Severity::Error,
            summary: format!(
                "the transaction failed on-chain: {}",
                inputs.result.onchain.label()
            ),
            guidance: None,
            evidence: vec!["result.onchain".to_string()],
        },
    }
}

fn divergence(inputs: &Inputs<'_>) -> Option<Finding> {
    let kind = inputs.fidelity.divergence?;
    let (what, field) = match kind {
        Divergence::Result => (
            format!(
                "the replay ended in {} where the network recorded {}",
                inputs.result.local.label(),
                inputs.result.onchain.label()
            ),
            "result",
        ),
        Divergence::Error => (
            "the replay failed through an error the network never recorded".to_string(),
            "failure",
        ),
        Divergence::Fee => (
            format!(
                "the non-refundable fee differs: {} stroops on-chain, {} locally",
                inputs
                    .fee
                    .onchain_non_refundable
                    .map_or_else(|| "unknown".to_string(), |fee| fee.to_string()),
                inputs.fee.local_non_refundable
            ),
            "fee",
        ),
        Divergence::Events => (
            "the replay's diagnostic events are not the start of the network's".to_string(),
            "diagnostics",
        ),
    };
    let summary = match inputs.probable_causes.first() {
        Some(cause) => format!("{what}; probably caused by {}", cause.subject),
        None => format!("{what}; nothing in this run explains it"),
    };
    let mut evidence = vec!["fidelity".to_string(), field.to_string()];
    evidence.extend((0..inputs.probable_causes.len()).map(|i| format!("probableCauses[{i}]")));
    Some(Finding {
        severity: Severity::Divergence,
        summary,
        guidance: None,
        evidence,
    })
}

/// One finding per kind of caveat, naming the entries or settings it covers.
fn caveats(all: &[Caveat]) -> Vec<Finding> {
    let mut kinds: Vec<CaveatKind> = Vec::new();
    for caveat in all {
        if !kinds.contains(&caveat.kind) {
            kinds.push(caveat.kind);
        }
    }
    kinds
        .into_iter()
        .map(|kind| {
            let members: Vec<(usize, &Caveat)> = all
                .iter()
                .enumerate()
                .filter(|(_, caveat)| caveat.kind == kind)
                .collect();
            Finding {
                severity: severity_of(kind),
                summary: caveat_summary(kind, &members),
                guidance: None,
                evidence: members
                    .iter()
                    .map(|(i, _)| format!("trust.caveats[{i}]"))
                    .collect(),
            }
        })
        .collect()
}

fn severity_of(kind: CaveatKind) -> Severity {
    match kind {
        CaveatKind::EntryUnresolved
        | CaveatKind::SettingDrifted
        | CaveatKind::SettingDriftUnknown
        | CaveatKind::EntryDriftUnknown
        | CaveatKind::EntryExtMissing
        | CaveatKind::PrngSeedFixed => Severity::Warning,
        CaveatKind::EntryArchivedInferred
        | CaveatKind::NoOnchainDiagnostics
        | CaveatKind::NoOnchainFee => Severity::Limitation,
    }
}

fn caveat_summary(kind: CaveatKind, members: &[(usize, &Caveat)]) -> String {
    let what = match kind {
        CaveatKind::EntryUnresolved => "changed after the transaction and could not be rebuilt",
        CaveatKind::SettingDrifted => "network setting changed after the transaction",
        CaveatKind::SettingDriftUnknown => "network setting the RPC does not date",
        CaveatKind::EntryDriftUnknown => "entry the RPC does not date",
        CaveatKind::EntryArchivedInferred => "archived state taken from the network's result",
        CaveatKind::EntryExtMissing => "classic entry replayed without its extension",
        // These are about the run, not an entry: the caveat says it in full.
        CaveatKind::PrngSeedFixed | CaveatKind::NoOnchainDiagnostics | CaveatKind::NoOnchainFee => {
            return members
                .first()
                .map_or_else(String::new, |(_, caveat)| caveat.detail.clone());
        }
    };
    let subjects: Vec<&str> = members
        .iter()
        .map(|(_, caveat)| caveat.subject.as_str())
        .collect();
    let named = subjects
        .iter()
        .take(SUBJECTS_NAMED)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    let more = match subjects.len().saturating_sub(SUBJECTS_NAMED) {
        0 => String::new(),
        n => format!(" and {n} more"),
    };
    format!("{what}: {named}{more}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::culprit::CulpritKind;
    use crate::report::ResultSummary;

    fn result(onchain: &'static str, local: &'static str) -> ResultPair {
        let summary = |code| ResultSummary {
            code,
            result_hash: None,
        };
        ResultPair {
            onchain: summary(onchain),
            local: summary(local),
        }
    }

    fn caveat(kind: CaveatKind, subject: &str) -> Caveat {
        Caveat {
            kind,
            subject: subject.to_string(),
            detail: format!("{subject} detail"),
        }
    }

    fn derive_with(fidelity: &Fidelity, causes: &[Culprit], caveats: Vec<Caveat>) -> Vec<Finding> {
        let trust = Trust {
            verified: caveats.is_empty(),
            caveats,
        };
        derive(&Inputs {
            fidelity,
            result: &result("trapped", "resource_limit_exceeded"),
            failure: None,
            fee: &FeeReport::default(),
            probable_causes: causes,
            trust: &trust,
        })
    }

    #[test]
    fn a_clean_match_has_only_the_on_chain_error() {
        let findings = derive_with(&Fidelity::MATCH, &[], Vec::new());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].evidence, ["result.onchain"]);
    }

    #[test]
    fn a_divergence_names_its_probable_cause_and_points_at_the_evidence() {
        let causes = [Culprit {
            kind: CulpritKind::EntryDrifted,
            subject: "account A".to_string(),
            detail: "changed".to_string(),
        }];
        let findings = derive_with(&Fidelity::diverged(Divergence::Result), &causes, Vec::new());
        let divergence = &findings[1];
        assert_eq!(divergence.severity, Severity::Divergence);
        assert!(divergence.summary.contains("probably caused by account A"));
        assert_eq!(
            divergence.evidence,
            ["fidelity", "result", "probableCauses[0]"]
        );
    }

    #[test]
    fn a_divergence_with_no_evidence_says_so() {
        let findings = derive_with(&Fidelity::diverged(Divergence::Events), &[], Vec::new());
        assert!(findings[1].summary.contains("nothing in this run explains"));
    }

    #[test]
    fn caveats_group_by_kind_and_rank_below_the_divergence() {
        let findings = derive_with(
            &Fidelity::diverged(Divergence::Result),
            &[],
            vec![
                caveat(CaveatKind::NoOnchainFee, ""),
                caveat(CaveatKind::EntryDriftUnknown, "a"),
                caveat(CaveatKind::EntryDriftUnknown, "b"),
                caveat(CaveatKind::EntryDriftUnknown, "c"),
                caveat(CaveatKind::EntryDriftUnknown, "d"),
            ],
        );
        let severities: Vec<_> = findings.iter().map(|f| f.severity).collect();
        assert_eq!(
            severities,
            [
                Severity::Error,
                Severity::Divergence,
                Severity::Warning,
                Severity::Limitation
            ]
        );
        assert_eq!(
            findings[2].summary,
            "entry the RPC does not date: a, b, c and 1 more"
        );
        assert_eq!(
            findings[2].evidence,
            [
                "trust.caveats[1]",
                "trust.caveats[2]",
                "trust.caveats[3]",
                "trust.caveats[4]"
            ]
        );
    }
}

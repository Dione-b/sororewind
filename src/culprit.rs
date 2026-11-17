//! culprit: tie a divergence to the evidence that probably caused it.
//!
//! A replay that disagrees with the network is almost always the ledger
//! having moved. The run already knows *what* moved: entries it could not put
//! back as they were, settings changed since, and where the two call trees
//! stop agreeing. This module lines that evidence up, strongest first, so the
//! report can say "probably caused by: …" instead of a generic hint.
//!
//! The ranking is a judgement about how directly each kind of evidence can
//! change an outcome, not a probability:
//!
//! 1. an entry the transaction read whose value changed and could not be
//!    rebuilt: the host saw different data;
//! 2. a network setting changed since: it prices and limits everything;
//! 3. the first call where the on-chain and local call trees differ: where
//!    the runs parted, whatever the reason;
//! 4. TTL drift and archived entries: liveness, not value;
//! 5. things that are unknown rather than known to differ (entries or
//!    settings the RPC gave no modification ledger for, classic entries
//!    returned without their extension, an apply order that was missing and
//!    left the random seed fixed).
//!
//! Entries that were *rebuilt* are not candidates: rebuilding them is the
//! replay working as intended.

use serde::Serialize;

use crate::replay::{PrngSeed, ReplayOutcome};
use crate::report::FootprintReport;
use crate::story::{Call, Story};
use crate::tx::DecodedTx;

/// One piece of evidence that may explain a divergence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Culprit {
    /// What kind of evidence this is.
    pub kind: CulpritKind,
    /// The entry, setting or call it is about, rendered for reading.
    pub subject: String,
    /// Why it is a candidate, in a sentence.
    pub detail: String,
}

/// The kinds of evidence a [`Culprit`] can be, in rank order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CulpritKind {
    /// An entry written at or after the transaction's ledger that was not
    /// rebuilt: the replay used its current value.
    EntryDrifted,
    /// A network setting modified at or after the transaction's ledger.
    SettingDrifted,
    /// The first call at which the on-chain and local call trees differ.
    CallDivergence,
    /// An entry whose TTL changed since the transaction ran.
    EntryTtlDrifted,
    /// An entry that was archived when the transaction ran.
    EntryArchived,
    /// An entry the RPC returned without saying when it last changed.
    EntryDriftUnknown,
    /// A network setting the RPC returned without saying when it last changed.
    SettingDriftUnknown,
    /// A classic entry the RPC returned without its extension.
    EntryExtMissing,
    /// The RPC reported no apply order, so the random seed is a stand-in.
    PrngSeedFixed,
}

/// The candidates for a divergence, strongest first. Empty means the run
/// holds no evidence that points anywhere.
pub fn find(
    footprint: &FootprintReport,
    prng_seed: PrngSeed,
    tx: &DecodedTx,
    outcome: &ReplayOutcome,
    local: &Story,
) -> Vec<Culprit> {
    let mut found = Vec::new();
    let mut push = |kind, subject: &str, detail: &str| {
        found.push(Culprit {
            kind,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    };

    for entry in &footprint.drifted {
        push(
            CulpritKind::EntryDrifted,
            entry,
            "changed after the transaction ran and could not be rebuilt, so the replay used \
             its current value",
        );
    }
    for setting in &footprint.config_drifted {
        push(
            CulpritKind::SettingDrifted,
            setting,
            "network setting changed since the transaction ran; the replay used its current value",
        );
    }
    if let Some(call) = call_divergence(tx, outcome, local) {
        push(CulpritKind::CallDivergence, &call.0, &call.1);
    }
    for drifted in &footprint.ttl_drifted {
        push(
            CulpritKind::EntryTtlDrifted,
            &drifted.entry,
            drifted.reason.describe(),
        );
    }
    for entry in &footprint.archived {
        push(
            CulpritKind::EntryArchived,
            entry,
            "archived when the transaction ran",
        );
    }
    for entry in &footprint.drift_unknown {
        push(
            CulpritKind::EntryDriftUnknown,
            entry,
            "the RPC does not say when it last changed, so it may have changed since",
        );
    }
    for setting in &footprint.config_drift_unknown {
        push(
            CulpritKind::SettingDriftUnknown,
            setting,
            "the RPC does not say when it last changed, so it may have changed since",
        );
    }
    for entry in &footprint.ext_missing {
        push(
            CulpritKind::EntryExtMissing,
            entry,
            "returned without its extension and replayed with an empty one, so it may be \
             smaller than it was",
        );
    }
    if prng_seed == PrngSeed::Fixed {
        push(
            CulpritKind::PrngSeedFixed,
            "prng seed",
            "the RPC did not report the transaction's apply order, so a contract that draws \
             random numbers saw other ones",
        );
    }
    found
}

/// Where the on-chain and local call trees first differ: a short label for
/// the frame, and what each run did there. `None` when the network kept no
/// diagnostics, or when the trees agree as far as the replay got.
fn call_divergence(
    tx: &DecodedTx,
    outcome: &ReplayOutcome,
    local: &Story,
) -> Option<(String, String)> {
    if tx.onchain_diagnostic_events.is_empty() {
        return None;
    }
    let onchain = Story::build(&tx.onchain_diagnostic_events, None);
    let index = (0..local.calls.len().max(onchain.calls.len())).find(|&index| {
        match (onchain.calls.get(index), local.calls.get(index)) {
            (Some(a), Some(b)) => !same_call(a, b),
            _ => true,
        }
    })?;
    // A local run that stopped early (a footprint check, a host failure
    // before the network's did) differs only by being shorter: not a place
    // where the runs parted.
    if local.calls.get(index).is_none() && outcome.error.is_some() {
        return None;
    }
    let describe = |call: Option<&Call>| match call {
        Some(call) => match &call.contract {
            Some(contract) => format!("{contract}::{}", call.function),
            None => call.function.clone(),
        },
        None => "nothing".to_string(),
    };
    let (onchain, local) = (
        describe(onchain.calls.get(index)),
        describe(local.calls.get(index)),
    );
    let subject = if local == "nothing" { &onchain } else { &local };
    Some((
        format!("call #{}: {subject}", index + 1),
        format!("the network called {onchain} here and the replay called {local}"),
    ))
}

/// Two frames are the same call when they name the same function on the same
/// contract with the same arguments.
fn same_call(a: &Call, b: &Call) -> bool {
    (&a.contract, &a.function, &a.args_xdr) == (&b.contract, &b.function, &b.args_xdr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::BudgetUse;
    use crate::report::{TtlDriftEntry, TtlDriftReason};
    use stellar_xdr::InvokeHostFunctionResult;

    fn outcome() -> ReplayOutcome {
        ReplayOutcome {
            result: InvokeHostFunctionResult::Trapped,
            error: None,
            diagnostics: Vec::new(),
            budget: BudgetUse {
                cpu_insns: 0,
                cpu_limit: 0,
                mem_bytes: 0,
                mem_limit: 0,
                network_cpu_insns: None,
                network_mem_bytes: None,
            },
            prng_seed: PrngSeed::Derived,
            non_refundable_fee: 0,
        }
    }

    fn kinds(found: &[Culprit]) -> Vec<CulpritKind> {
        found.iter().map(|culprit| culprit.kind).collect()
    }

    #[test]
    fn no_evidence_names_no_candidate() {
        let tx = crate::test_support::decoded_tx();
        let found = find(
            &FootprintReport::default(),
            PrngSeed::Derived,
            &tx,
            &outcome(),
            &Story::default(),
        );
        assert!(found.is_empty());
    }

    #[test]
    fn candidates_come_strongest_first() {
        let footprint = FootprintReport {
            drifted: vec!["contract data A".to_string()],
            ttl_drifted: vec![TtlDriftEntry {
                entry: "contract data B".to_string(),
                reason: TtlDriftReason::ExtendedSince,
            }],
            archived: vec!["contract code C".to_string()],
            drift_unknown: vec!["account D".to_string()],
            ext_missing: vec!["trustline E".to_string()],
            config_drifted: vec!["StateArchival".to_string()],
            config_drift_unknown: vec!["ContractBandwidth".to_string()],
            ..FootprintReport::default()
        };
        let tx = crate::test_support::decoded_tx();
        let found = find(
            &footprint,
            PrngSeed::Fixed,
            &tx,
            &outcome(),
            &Story::default(),
        );
        assert_eq!(
            kinds(&found),
            [
                CulpritKind::EntryDrifted,
                CulpritKind::SettingDrifted,
                CulpritKind::EntryTtlDrifted,
                CulpritKind::EntryArchived,
                CulpritKind::EntryDriftUnknown,
                CulpritKind::SettingDriftUnknown,
                CulpritKind::EntryExtMissing,
                CulpritKind::PrngSeedFixed,
            ]
        );
        assert_eq!(found[0].subject, "contract data A");
    }
}

//! trust: say when a result rests on state that could not be verified.
//!
//! A match means the replay reproduced the network's outcome *with the
//! inputs it had*. Some of those inputs are guesses: an entry whose change
//! time the RPC did not report, a setting that has changed since, a random
//! seed made up because the apply order was missing. A match over such inputs
//! is weaker than one over verified ones, and the report says so instead of
//! leaving it to be found in the footprint section.
//!
//! The same list applies to a divergence, where it doubles as the places to
//! look; the text report only prints it for a match, because a divergence
//! already lists its probable causes.

use serde::Serialize;

use crate::replay::PrngSeed;
use crate::report::{EntryStatusKind, FootprintReport};
use crate::tx::DecodedTx;

/// How far the result can be trusted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Trust {
    /// Every input the replay used was verified, and everything the network
    /// recorded was compared. `false` when there is any caveat.
    pub verified: bool,
    /// What could not be verified, in the order the report lists it.
    pub caveats: Vec<Caveat>,
}

/// One thing the result depends on that could not be verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Caveat {
    /// What kind of thing it is.
    pub kind: CaveatKind,
    /// The entry or setting it is about; empty when it is about the run.
    pub subject: String,
    /// What is unverified, in a sentence.
    pub detail: String,
}

/// The kinds of caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaveatKind {
    /// An entry changed since the transaction and could not be rebuilt.
    EntryUnresolved,
    /// A network setting changed since the transaction; the current value
    /// was used.
    SettingDrifted,
    /// The RPC did not say when a network setting last changed.
    SettingDriftUnknown,
    /// The RPC did not say when an entry last changed.
    EntryDriftUnknown,
    /// An entry's archived state is taken from the network's result, not
    /// reproduced.
    EntryArchivedInferred,
    /// A classic entry was replayed without its extension.
    EntryExtMissing,
    /// The random seed is a stand-in.
    PrngSeedFixed,
    /// The network kept no diagnostic events, so the replay's error and call
    /// trace were not checked against them.
    NoOnchainDiagnostics,
    /// The network's transaction meta carried no fee breakdown, so the fee
    /// was not compared.
    NoOnchainFee,
}

/// What the run could not verify.
pub fn assess(footprint: &FootprintReport, prng_seed: PrngSeed, tx: &DecodedTx) -> Trust {
    let mut caveats = Vec::new();
    let mut push = |kind, subject: &str, detail: &str| {
        caveats.push(Caveat {
            kind,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    };

    for entry in &footprint.drifted {
        let detail = footprint
            .entry_status
            .iter()
            .find(|status| status.entry == *entry && status.status == EntryStatusKind::Unresolved)
            .map_or(
                "changed after the transaction and could not be rebuilt; the replay used its \
                 current value"
                    .to_string(),
                |status| {
                    format!(
                        "changed after the transaction; the replay used its current value \
                         ({})",
                        status.detail
                    )
                },
            );
        push(CaveatKind::EntryUnresolved, entry, &detail);
    }
    for setting in &footprint.config_drifted {
        push(
            CaveatKind::SettingDrifted,
            setting,
            "network setting changed after the transaction; the replay used its current value",
        );
    }
    for setting in &footprint.config_drift_unknown {
        push(
            CaveatKind::SettingDriftUnknown,
            setting,
            "the RPC does not say when it last changed, so it may differ from what the \
             transaction saw",
        );
    }
    for entry in &footprint.drift_unknown {
        push(
            CaveatKind::EntryDriftUnknown,
            entry,
            "the RPC does not say when it last changed, so it may differ from what the \
             transaction saw",
        );
    }
    for entry in &footprint.archived_inferred {
        push(
            CaveatKind::EntryArchivedInferred,
            entry,
            "archived according to the network's result; its TTL then is lost, so this was \
             assumed rather than reproduced",
        );
    }
    for entry in &footprint.ext_missing {
        push(
            CaveatKind::EntryExtMissing,
            entry,
            "sent without its extension and replayed with an empty one, so it may be smaller \
             than it was",
        );
    }
    if prng_seed == PrngSeed::Fixed {
        push(
            CaveatKind::PrngSeedFixed,
            "",
            "the RPC did not report the transaction's apply order, so the random seed is a \
             stand-in: a contract that draws random numbers may behave differently",
        );
    }
    if tx.onchain_diagnostic_events.is_empty() {
        push(
            CaveatKind::NoOnchainDiagnostics,
            "",
            "the network kept no diagnostic events, so the replay's error and call trace could \
             not be checked against them",
        );
    }
    if tx.onchain_fee.is_none() {
        push(
            CaveatKind::NoOnchainFee,
            "",
            "the transaction meta has no fee breakdown, so the resource fee was not compared",
        );
    }
    Trust {
        verified: caveats.is_empty(),
        caveats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{EntryReason, EntryStatus};
    use crate::tx::OnchainFee;

    fn tx() -> DecodedTx {
        let mut tx = crate::test_support::decoded_tx();
        tx.onchain_diagnostic_events = vec![crate::test_support::error_event(
            None,
            stellar_xdr::ScError::Contract(1),
            "boom",
        )];
        tx.onchain_fee = Some(OnchainFee {
            non_refundable: 1,
            refundable: 0,
            rent: 0,
        });
        tx
    }

    #[test]
    fn verified_inputs_carry_no_caveat() {
        let trust = assess(&FootprintReport::default(), PrngSeed::Derived, &tx());
        assert_eq!(
            trust,
            Trust {
                verified: true,
                caveats: Vec::new()
            }
        );
    }

    fn kinds(trust: &Trust) -> Vec<CaveatKind> {
        trust.caveats.iter().map(|caveat| caveat.kind).collect()
    }

    #[test]
    fn each_unverified_input_is_a_caveat() {
        let footprint = FootprintReport {
            drifted: vec!["account A".to_string()],
            entry_status: vec![EntryStatus {
                entry: "account A".to_string(),
                status: EntryStatusKind::Unresolved,
                reason: EntryReason::NoApplyOrder,
                detail: "not rebuilt: no apply order".to_string(),
            }],
            config_drifted: vec!["StateArchival".to_string()],
            config_drift_unknown: vec!["ContractBandwidth".to_string()],
            drift_unknown: vec!["account B".to_string()],
            archived_inferred: vec!["contract code C".to_string()],
            ext_missing: vec!["trustline D".to_string()],
            ..FootprintReport::default()
        };
        let trust = assess(
            &footprint,
            PrngSeed::Fixed,
            &crate::test_support::decoded_tx(),
        );
        assert!(!trust.verified);
        assert_eq!(
            kinds(&trust),
            [
                CaveatKind::EntryUnresolved,
                CaveatKind::SettingDrifted,
                CaveatKind::SettingDriftUnknown,
                CaveatKind::EntryDriftUnknown,
                CaveatKind::EntryArchivedInferred,
                CaveatKind::EntryExtMissing,
                CaveatKind::PrngSeedFixed,
                CaveatKind::NoOnchainDiagnostics,
                CaveatKind::NoOnchainFee,
            ]
        );
        assert!(trust.caveats[0].detail.contains("no apply order"));
    }
}

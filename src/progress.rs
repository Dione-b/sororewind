//! progress: tell the caller which stage a live run is in.
//!
//! A live run fetches the transaction, the ledger entries and ledger metadata
//! before it replays, and on a slow RPC that is many silent seconds. The
//! library does not print; it reports each stage to a [`Progress`] and the
//! binary decides whether and how to show it.

use std::fmt;

/// A stage a run has just entered, in the order a run goes through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Asking the RPC for the transaction and its network.
    FetchingTransaction,
    /// Asking the RPC for the entries the transaction's footprint names.
    FetchingEntries {
        /// How many footprint keys are being fetched.
        count: usize,
    },
    /// Rebuilding the state those entries had when the transaction ran,
    /// which may take ledger metadata from the RPC.
    RebuildingHistory,
    /// Running the transaction on the embedded host.
    Replaying,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::FetchingTransaction => f.write_str("fetching the transaction"),
            Step::FetchingEntries { count: 1 } => f.write_str("fetching 1 ledger entry"),
            Step::FetchingEntries { count } => write!(f, "fetching {count} ledger entries"),
            Step::RebuildingHistory => f.write_str("rebuilding the state it ran against"),
            Step::Replaying => f.write_str("replaying"),
        }
    }
}

/// Receives the stages of a run as it enters them.
pub trait Progress: Send + Sync {
    /// The run has entered `step`.
    fn step(&self, step: Step);
}

/// Reports nothing: what [`crate::run`] uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct Silent;

impl Progress for Silent {
    fn step(&self, _step: Step) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_as_a_phrase() {
        assert_eq!(
            Step::FetchingEntries { count: 1 }.to_string(),
            "fetching 1 ledger entry"
        );
        assert_eq!(
            Step::FetchingEntries { count: 7 }.to_string(),
            "fetching 7 ledger entries"
        );
        assert_eq!(Step::Replaying.to_string(), "replaying");
    }
}

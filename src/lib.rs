//! sororewind — replay a failed Soroban transaction and narrate the failure.
//!
//! Start a replay with [`run`] and a [`Request`]. Use [`HttpTransport`]
//! for live RPC requests or [`FixtureTransport`] for an offline recording.
//! [`run_with_progress`] also reports stage transitions. The returned
//! [`Report`] gives callers the outcome, diagnostics, call tree and evidence
//! about the state used by the replay.
//!
//! The supported library interface consists of the items re-exported here,
//! the [`render`] functions and their public types, and the public fields
//! of the report and its component types. See the changelog for the
//! compatibility policy.

// Tests assert with `unwrap`/`expect`; production code may not.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod cache;
mod catalog;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod codec;
mod config;
#[doc(hidden)]
pub mod culprit;
#[doc(hidden)]
pub mod discover;
#[doc(hidden)]
pub mod display;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod export;
mod fallback;
#[doc(hidden)]
pub mod findings;
#[doc(hidden)]
pub mod hash;
#[doc(hidden)]
pub mod history;
#[doc(hidden)]
pub mod host;
#[doc(hidden)]
pub mod network;
#[doc(hidden)]
pub mod pipeline;
#[doc(hidden)]
pub mod progress;
pub mod render;
#[doc(hidden)]
pub mod replay;
#[doc(hidden)]
pub mod report;
#[doc(hidden)]
pub mod rpc;
#[doc(hidden)]
pub mod spec;
#[doc(hidden)]
pub mod state;
#[doc(hidden)]
pub mod story;
#[doc(hidden)]
pub mod strkey;
#[doc(hidden)]
pub mod transport;
#[doc(hidden)]
pub mod trust;
#[doc(hidden)]
pub mod tx;

#[doc(hidden)]
pub mod watch;

#[cfg(test)]
mod test_support;

pub use culprit::{Culprit, CulpritKind};
pub use error::{Result, Retention, RewindError, Stage};
pub use findings::{Finding, Guidance, Severity};
pub use hash::TxHash;
pub use network::{Network, NetworkId};
pub use pipeline::{Request, run, run_with_progress};
pub use progress::{Progress, Silent, Step};
pub use replay::{BudgetUse, Divergence, Fidelity, PrngSeed};
pub use report::{
    Diagnostics, EntryReason, EntryStatus, EntryStatusKind, FeeReport, FootprintReport, Outcome,
    Report, ResultPair, ResultSummary, SCHEMA_VERSION, TtlDriftEntry, TtlDriftReason, WhatIf,
    WhatIfOutcome,
};
pub use rpc::RpcClient;
pub use story::{Call, Cause, Failure, Story};
pub use transport::{BoxFuture, FixtureTransport, HttpTransport, Transport, TransportConfig};
pub use trust::{Caveat, CaveatKind, Trust};

/// The replay ran and reproduced the network's recorded result.
pub const EXIT_OK: u8 = 0;
/// The tool itself failed: bad input, unreachable RPC, un-replayable state.
pub const EXIT_ERROR: u8 = 1;
/// The replay ran but disagreed with the network.
pub const EXIT_DIVERGED: u8 = 2;

/// The process exit code for a finished run.
pub fn exit_code(result: &Result<Report>) -> u8 {
    match result.as_ref().map(Report::outcome) {
        Ok(Outcome::Matched) => EXIT_OK,
        Ok(Outcome::Diverged) => EXIT_DIVERGED,
        Err(_) => EXIT_ERROR,
    }
}

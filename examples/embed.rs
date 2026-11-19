//! Replay a fixture from another Rust program and inspect structured results.
//!
//! cargo run --example embed -- fixtures/contract-error

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;

use sororewind::{FixtureTransport, Request, RpcClient, run};

#[tokio::main]
async fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: embed <fixture-directory>");
        return ExitCode::FAILURE;
    };
    match replay(path).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", error.kind());
            ExitCode::FAILURE
        }
    }
}

async fn replay(path: PathBuf) -> sororewind::Result<()> {
    let fixture = FixtureTransport::open(path).await?;
    let request = Request {
        hash: fixture.hash_for(None)?,
        network: None,
        wasm_debug: None,
    };
    let report = run(&RpcClient::new(fixture), &request).await?;
    println!(
        "{} on {}: {:?}",
        report.hash,
        report.network,
        report.outcome()
    );
    if let Some(failure) = &report.story.failure {
        println!("Failure: {failure:?}");
    }
    Ok(())
}

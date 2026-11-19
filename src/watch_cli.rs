//! CLI and persistent checkpoint for watch mode.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sororewind::cli::{Cli, WatchArgs};
use sororewind::render::{self, Detail, Format};
use sororewind::watch::{Target, poll};
use sororewind::{NetworkId, Request, Result, RewindError};

#[derive(Debug, Deserialize, Serialize)]
struct Checkpoint {
    network: String,
    target: String,
    ledger: u32,
}

pub async fn run(cli: &mut Cli, args: WatchArgs) -> Result<()> {
    tokio::select! {
        stopped = tokio::signal::ctrl_c() => stopped.map_err(|error| RewindError::Output(error.to_string())),
        watched = watch(cli, args) => watched,
    }
}

async fn watch(cli: &mut Cli, args: WatchArgs) -> Result<()> {
    let target = Target::parse(args.contract.as_deref(), args.account.as_deref())?;
    let address = target.address();
    cli.network = args.network;
    cli.rpc_url = args.rpc_url;
    cli.format = args.format;
    cli.verbose = args.verbose;
    super::init_logging(cli.verbose);
    cli.load_config()?;

    let initial = cli.watch_rpc()?;
    let passphrase = initial.network().await?.passphrase;
    let actual = NetworkId::from_passphrase(&passphrase);
    if let Some(expected) = cli.network
        && actual != NetworkId::Known(expected)
    {
        return Err(RewindError::NetworkMismatch {
            expected,
            actual,
            origin: initial.origin(),
        });
    }
    let network = hex::encode(Sha256::digest(passphrase.as_bytes()));
    let path = args
        .checkpoint
        .map_or_else(|| default_checkpoint(&address, &network), Ok)?;
    let _lock = lock_checkpoint(&path)?;
    let mut checkpoint = read_checkpoint(&path, &address, &network)?;
    if let Some(dir) = &args.output_dir {
        std::fs::create_dir_all(dir).map_err(|error| output_error(dir, error))?;
    }

    loop {
        let result = tick(
            cli,
            &target,
            &passphrase,
            checkpoint,
            args.output_dir.as_deref(),
        )
        .await;
        match result {
            Ok(through) if checkpoint != Some(through) => {
                let saved = Checkpoint {
                    network: network.clone(),
                    target: address.clone(),
                    ledger: through,
                };
                let bytes =
                    serde_json::to_vec(&saved).map_err(|error| output_error(&path, error))?;
                write_file(&path, &bytes)?;
                checkpoint = Some(through);
            }
            Ok(_) => {}
            Err(error) if error.stage() == sororewind::Stage::Output => return Err(error),
            Err(error) => eprintln!("sororewind watch: {error}"),
        }
        tokio::time::sleep(Duration::from_secs(args.poll_interval)).await;
    }
}

async fn tick(
    cli: &Cli,
    target: &Target,
    passphrase: &str,
    after: Option<u32>,
    output: Option<&Path>,
) -> Result<u32> {
    let rpc = cli.watch_rpc()?;
    if rpc.network().await?.passphrase != passphrase {
        return Err(RewindError::Rpc {
            origin: rpc.origin(),
            method: "getNetwork".into(),
            code: None,
            detail: "the RPC's network passphrase changed after watch started".into(),
        });
    }
    let batch = poll(&rpc, target, after).await?;
    if batch.pruned > 0 {
        eprintln!(
            "sororewind watch: RPC retention has already pruned {} unprocessed ledgers",
            batch.pruned
        );
    }
    let format = cli.output_format();
    for tx in batch.candidates {
        let request = Request {
            hash: tx.hash.clone(),
            network: cli.network,
            wasm_debug: None,
        };
        let result = sororewind::run(&rpc, &request).await;
        let document = match result {
            Ok(report) if format == Format::Json && output.is_none() => {
                serde_json::to_string(&report)
                    .map_err(|error| RewindError::Output(error.to_string()))?
            }
            Ok(report) => {
                render::report(&report, format, false, Detail::from_verbosity(cli.verbose))
                    .map_err(|error| RewindError::Output(error.to_string()))?
            }
            Err(error) => {
                eprintln!("sororewind watch: {}: {error}", tx.hash);
                // A transient transport failure should be retried on a fresh poll.
                if error.is_transient_rpc() {
                    return Err(error);
                }
                render::error(&error, format).unwrap_or_else(|| format!("{}: {error}", tx.hash))
            }
        };
        if let Some(dir) = output {
            let extension = match format {
                Format::Text => "txt",
                Format::Json => "json",
                Format::Markdown => "md",
                Format::Html => "html",
            };
            write_file(
                &dir.join(format!("{}.{extension}", tx.hash)),
                document.as_bytes(),
            )?;
        } else if format == Format::Json {
            let value: serde_json::Value = serde_json::from_str(&document)
                .map_err(|error| RewindError::Output(error.to_string()))?;
            super::write_stdout(&format!("{value}\n"))?;
        } else {
            super::write_stdout(&format!("{}\n\n", document.trim_end()))?;
        }
    }
    Ok(batch.through)
}

fn default_checkpoint(address: &str, network: &str) -> Result<PathBuf> {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let directory = absolute("XDG_STATE_HOME")
        .or_else(|| absolute("LOCALAPPDATA"))
        .or_else(|| absolute("HOME").map(|home| home.join(".local/state")))
        .ok_or_else(|| RewindError::InvalidOption {
            name: "checkpoint",
            reason: "pass --checkpoint when no state directory can be found".into(),
        })?;
    Ok(directory
        .join("sororewind/watch")
        .join(format!("{network}-{address}.json")))
}

fn lock_checkpoint(path: &Path) -> Result<std::fs::File> {
    let mut lock = path.as_os_str().to_owned();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    std::fs::create_dir_all(parent(&lock)).map_err(|error| output_error(&lock, error))?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(|error| output_error(&lock, error))?;
    file.try_lock()
        .map_err(|error| output_error(&lock, error))?;
    Ok(file)
}

fn read_checkpoint(path: &Path, target: &str, network: &str) -> Result<Option<u32>> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(output_error(path, error)),
    };
    let saved: Checkpoint =
        serde_json::from_slice(&data).map_err(|error| output_error(path, error))?;
    if saved.target != target || saved.network != network {
        return Err(output_error(
            path,
            "checkpoint belongs to a different network or target",
        ));
    }
    Ok(Some(saved.ledger))
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    let dir = parent(path);
    std::fs::create_dir_all(dir).map_err(|error| output_error(dir, error))?;
    let mut file =
        tempfile::NamedTempFile::new_in(dir).map_err(|error| output_error(path, error))?;
    file.write_all(data)
        .map_err(|error| output_error(path, error))?;
    file.persist(path)
        .map_err(|error| output_error(path, error.error))?;
    Ok(())
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn output_error(path: &Path, error: impl std::fmt::Display) -> RewindError {
    RewindError::Output(format!("{}: {error}", path.display()))
}

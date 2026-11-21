//! Command-line surface.

use std::path::PathBuf;
use std::sync::LazyLock;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

use crate::error::{Result, RewindError};
use crate::hash::TxHash;
use crate::network::Network;
use crate::pipeline::Request;
use crate::render::{ColorChoice, Format};
use crate::rpc::RpcClient;
use crate::transport::{
    FixtureTransport, HttpTransport, Recording, RecordingHandle, Transport, TransportConfig,
};

/// Replay a failed Soroban transaction locally and narrate the call tree.
#[derive(Debug, Parser)]
#[command(
    name = "sororewind",
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true,
    about = "Explain a failed Soroban transaction by replaying it locally",
    long_about = "sororewind fetches a failed Soroban transaction from an RPC, rebuilds the ledger \
state it ran against, re-executes it with the same Soroban host, and reports the failure the way \
a compiler reports an error: a stable code, where it happened and what to check. --trace prints \
the narrated call tree up to the point of failure.",
    after_help = "EXAMPLES:\n  \
        sororewind 3f9a...c21\n  \
        sororewind 3f9a...c21 --network mainnet\n  \
        sororewind 3f9a...c21 --trace\n  \
        sororewind 3f9a...c21 --json\n  \
        sororewind 3f9a...c21 --wasm-debug ./vault.wasm -v\n\n\
        EXIT CODES:\n  \
        0  the replay matched the network's recorded result\n  \
        1  sororewind could not replay the transaction (bad input, RPC error, ...)\n  \
        2  the replay ran but diverged from the network"
)]
pub struct Cli {
    /// Hash of the failed transaction (hex, with or without 0x). Optional with
    /// --fixture, which records the hash it was made for
    #[arg(required_unless_present_any = ["fixture", "completions", "man"])]
    pub tx_hash: Option<TxHash>,

    /// Network the transaction lives on; selects the default RPC endpoint and
    /// is checked against the RPC's passphrase. (The help text is completed
    /// with the default endpoints in `Cli::command`.)
    #[arg(long, short = 'n', value_enum)]
    pub network: Option<Network>,

    /// Use this RPC endpoint instead of the default for --network. Repeat for
    /// fallback endpoints in order. Prefer the
    /// SOROREWIND_RPC_URL environment variable for a URL that holds an API
    /// key: arguments end up in shell history and process listings
    #[arg(long, value_name = "URL", conflicts_with = "fixture", value_parser = parse_rpc_url)]
    pub rpc_url: Vec<String>,

    /// Read and write immutable RPC responses in this directory
    #[arg(long, value_name = "DIR", conflicts_with_all = ["no_cache", "fixture", "record"])]
    pub cache_dir: Option<PathBuf>,

    /// Disable the disk cache for live RPC responses
    #[arg(long)]
    pub no_cache: bool,

    /// Seconds one RPC request may take before it is abandoned and retried
    /// [default: 30; env SOROREWIND_TIMEOUT]
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub timeout: Option<u64>,

    /// How many times a failed RPC call is retried (connection errors,
    /// timeouts, HTTP 429/502/503/504, rate-limit errors); 0 never retries
    /// [default: 3; env SOROREWIND_RETRIES]
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(0..=10))]
    pub retries: Option<u32>,

    /// Seconds the whole run may spend on the RPC, retries and waits
    /// included; a call still going then fails with rpc_unreachable
    /// [default: none; env SOROREWIND_DEADLINE]
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..=86_400))]
    pub deadline: Option<u64>,

    /// How to write the report: text (the narrative, default), json,
    /// markdown (for a GitHub issue or PR) or html (one self-contained file)
    #[arg(long, value_enum, value_name = "FORMAT", conflicts_with = "json")]
    pub format: Option<Format>,

    /// Stop after decode, state reconstruction, or replay and print that stage
    #[arg(long, value_enum, conflicts_with = "record")]
    pub stage: Option<crate::pipeline::InspectStage>,

    /// Also compare with this embedded host using the recovered ledger inputs
    #[arg(long, value_name = "N", conflicts_with_all = ["stage", "record"])]
    pub what_if_protocol: Option<u32>,

    /// Same as --format json
    #[arg(long)]
    pub json: bool,

    /// Print the full narrative (findings, fidelity, where it broke, the
    /// call tree) instead of the compact diagnostic; text output only
    #[arg(long)]
    pub trace: bool,

    /// Read the contract's error names from this WASM file instead of the
    /// ledger (debugging aid)
    #[arg(long, value_name = "PATH", value_parser = parse_wasm_path)]
    pub wasm_debug: Option<PathBuf>,

    /// When to colour the narrative: auto colours a terminal, and honours
    /// NO_COLOR, CLICOLOR_FORCE and TERM=dumb
    #[arg(long, value_enum, value_name = "WHEN")]
    pub color: Option<ColorChoice>,

    /// Same as --color never
    #[arg(long, conflicts_with = "color")]
    pub no_color: bool,

    /// Repeat for more detail: -v prints the --trace narrative with budget
    /// and footprint detail and full identifiers, -vv adds raw diagnostics
    /// and RPC tracing on stderr
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Do not show which stage a live run is in on stderr (it is only shown
    /// on a terminal, and never with --json, -v or --fixture)
    #[arg(long)]
    pub no_progress: bool,

    /// Replay from a recorded RPC fixture directory instead of the network
    #[arg(long, value_name = "DIR")]
    pub fixture: Option<PathBuf>,

    /// Record the run's RPC responses as a fixture in this directory. It is
    /// written in one step once the replay has run; a run that fails leaves
    /// the directory as it was
    #[arg(long, value_name = "DIR", hide = true, conflicts_with = "fixture")]
    pub record: Option<PathBuf>,

    /// Print a completion script for SHELL and exit, e.g. `sororewind
    /// --completions fish > ~/.config/fish/completions/sororewind.fish`
    #[arg(long, value_name = "SHELL", value_enum, exclusive = true)]
    pub completions: Option<clap_complete::Shell>,

    /// Print a man page (roff) and exit, e.g. `sororewind --man > sororewind.1`
    #[arg(long, exclusive = true)]
    pub man: bool,

    /// Compare saved reports or monitor newly closed ledgers.
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(skip)]
    settings: crate::config::Settings,
}

/// Operations on saved replay reports.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Compare two JSON reports from replay runs.
    Diff {
        /// The first JSON report.
        before: PathBuf,
        /// The second JSON report.
        after: PathBuf,
    },
    /// Replay new failed Soroban transactions involving a contract or account.
    Watch(WatchArgs),
}

/// Options for monitoring newly closed ledgers.
#[derive(Debug, clap::Args)]
#[command(group(clap::ArgGroup::new("watch-target").required(true).args(["contract", "account"])))]
pub struct WatchArgs {
    /// Monitor this C-prefixed contract.
    #[arg(long, group = "watch-target")]
    pub contract: Option<String>,
    /// Monitor this G-prefixed account.
    #[arg(long, group = "watch-target")]
    pub account: Option<String>,
    /// Network to monitor.
    #[arg(long, short = 'n', value_enum)]
    pub network: Option<Network>,
    /// RPC endpoint. Repeat to configure ordered fallback.
    #[arg(long, value_parser = parse_rpc_url)]
    pub rpc_url: Vec<String>,
    /// Seconds between polls.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub poll_interval: u64,
    /// Store the last processed ledger at this path.
    #[arg(long)]
    pub checkpoint: Option<PathBuf>,
    /// Write one report file per transaction in this directory.
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    /// Report format. JSON on stdout is newline-delimited.
    #[arg(long, value_enum)]
    pub format: Option<Format>,
    /// Log progress details.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

/// `0.1.0 (protocols 29 (soroban-env-host 29.0.2), 30 (...); commit
/// 1b1ad7efb040, x86_64-unknown-linux-gnu)`: which transactions this build
/// can replay, and the exact code and platform it was built from, are part of
/// its version. The commit is `unknown` outside a git checkout (`build.rs`).
static VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{} (protocols {}; commit {}, {})",
        env!("CARGO_PKG_VERSION"),
        crate::host::describe(),
        env!("SOROREWIND_COMMIT"),
        env!("SOROREWIND_TARGET"),
    )
});

/// Where the responses come from, what to replay, and the recording being
/// made of it, if any.
pub struct Connection {
    /// The client over the live RPC, the recorder wrapping it, or the fixture.
    pub rpc: RpcClient,
    /// The transaction to replay and the options that shape the replay.
    pub request: Request,
    /// With `--record`: the staged recording, to commit once the run is over.
    pub recording: Option<RecordingHandle>,
}

/// The environment variable that names the RPC endpoint when `--rpc-url` is
/// not given.
pub const RPC_URL_ENV: &str = "SOROREWIND_RPC_URL";

/// The endpoint an environment value names; an empty value names none.
fn rpc_url_from(value: Option<String>) -> Option<String> {
    value.filter(|url| !url.trim().is_empty())
}

/// The environment variables behind `--timeout`, `--retries` and
/// `--deadline`.
pub const TIMEOUT_ENV: &str = "SOROREWIND_TIMEOUT";
/// See [`TIMEOUT_ENV`].
pub const RETRIES_ENV: &str = "SOROREWIND_RETRIES";
/// See [`TIMEOUT_ENV`].
pub const DEADLINE_ENV: &str = "SOROREWIND_DEADLINE";

/// A number the flag gave, or else the one the environment variable holds,
/// within `range`. An empty variable names none.
fn number_setting(
    flag: Option<u64>,
    name: &'static str,
    value: Option<String>,
    range: std::ops::RangeInclusive<u64>,
) -> Result<Option<u64>> {
    if flag.is_some() {
        return Ok(flag);
    }
    let Some(text) = value.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let invalid = |reason: String| RewindError::InvalidOption { name, reason };
    let number: u64 = text
        .trim()
        .parse()
        .map_err(|_| invalid(format!("`{}` is not a whole number", text.trim())))?;
    if !range.contains(&number) {
        return Err(invalid(format!(
            "{number} is outside {} to {}",
            range.start(),
            range.end()
        )));
    }
    Ok(Some(number))
}

/// An RPC endpoint has to be an absolute http(s) URL: `localhost:8000` parses
/// as a URL with the scheme `localhost`, and would only fail once sent.
fn parse_rpc_url(text: &str) -> std::result::Result<String, String> {
    let text = text.trim();
    let url =
        reqwest::Url::parse(text).map_err(|e| format!("{e} (expected http://… or https://…)"))?;
    if !matches!(url.scheme(), "http" | "https") {
        // The hint shows only scheme, host and port: the rest of the URL may
        // be an API key, and this message reaches stderr and `--json`.
        return Err(format!(
            "the scheme is `{}`, expected http or https (did you mean {}?)",
            url.scheme(),
            crate::transport::redact(&format!("http://{text}"))
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("it has no host".to_string());
    }
    Ok(text.to_string())
}

/// `--wasm-debug` is only read when the failure turns out to be a contract
/// error; a path that does not exist should not wait until then to be told.
fn parse_wasm_path(text: &str) -> std::result::Result<PathBuf, String> {
    let path = PathBuf::from(text);
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => Ok(path),
        Ok(_) => Err("it is not a file".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// `testnet https://…, mainnet https://… (third-party), …`, from the
/// endpoints the code actually uses.
fn default_rpcs() -> String {
    Network::ALL
        .into_iter()
        .map(|network| {
            format!(
                "{network} {}{}",
                network.default_rpc_url(),
                if network == Network::Mainnet {
                    " (third-party)"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl Cli {
    /// The clap command, with the version and `--network` help that list the
    /// embedded protocols and default RPCs.
    pub fn command() -> clap::Command {
        <Cli as CommandFactory>::command()
            .version(VERSION.as_str())
            .mut_arg("network", |arg| {
                arg.help(format!(
                    "Network the transaction lives on; selects the default RPC endpoint and is \
                     checked against the RPC's passphrase [default RPCs: {}]",
                    default_rpcs()
                ))
            })
    }

    /// The completion script for `shell`, as `--completions` prints it.
    pub fn completion_script(shell: clap_complete::Shell) -> String {
        let mut script = Vec::new();
        clap_complete::generate(shell, &mut Cli::command(), "sororewind", &mut script);
        String::from_utf8_lossy(&script).into_owned()
    }

    /// The man page, in roff, as `--man` prints it.
    pub fn man_page() -> Result<String> {
        let mut page = Vec::new();
        clap_mangen::Man::new(Cli::command())
            .render(&mut page)
            .map_err(|e| RewindError::Output(e.to_string()))?;
        Ok(String::from_utf8_lossy(&page).into_owned())
    }

    /// Parse arguments; clap's own error carries help, version and usage
    /// problems alike.
    pub fn try_parse_args<I, T>(args: I) -> std::result::Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let matches = Cli::command().try_get_matches_from(args)?;
        Cli::from_arg_matches(&matches)
    }

    /// Load project and user defaults, applying environment and command-line overrides.
    pub fn load_config(&mut self) -> Result<()> {
        self.apply_settings(crate::config::Settings::discover()?, |name| {
            std::env::var(name).ok()
        })
    }

    fn apply_settings(
        &mut self,
        settings: crate::config::Settings,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<()> {
        fn choose<T: ValueEnum>(
            flag: Option<T>,
            name: &'static str,
            env: Option<String>,
            setting: Option<String>,
        ) -> Result<Option<T>> {
            if flag.is_some() {
                return Ok(flag);
            }
            env.filter(|text| !text.trim().is_empty())
                .or(setting)
                .map(|text| {
                    T::from_str(text.trim(), false).map_err(|_| RewindError::InvalidOption {
                        name,
                        reason: format!(
                            "expected one of {}",
                            T::value_variants()
                                .iter()
                                .filter_map(ValueEnum::to_possible_value)
                                .map(|v| v.get_name().to_owned())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    })
                })
                .transpose()
        }
        self.network = choose(
            self.network,
            "network",
            env("SOROREWIND_NETWORK"),
            settings.network.clone(),
        )?;
        if !self.json {
            self.format = choose(
                self.format,
                "format",
                env("SOROREWIND_FORMAT"),
                settings.format.clone(),
            )?;
        }
        if !self.no_color {
            self.color = choose(
                self.color,
                "color",
                env("SOROREWIND_COLOR"),
                settings.color.clone(),
            )?;
        }
        self.settings = settings;
        Ok(())
    }

    /// The output format the flags amount to.
    pub fn output_format(&self) -> Format {
        if self.json {
            Format::Json
        } else {
            self.format.unwrap_or_default()
        }
    }

    /// The colour choice the flags amount to.
    pub fn color_choice(&self) -> ColorChoice {
        if self.no_color {
            ColorChoice::Never
        } else {
            self.color.unwrap_or_default()
        }
    }

    /// Whether to print the stage a live run is in: only when stderr is a
    /// terminal, and not when the output is JSON, the logs already narrate
    /// (`-v`), a fixture answers instantly, or `--no-progress` says no.
    pub fn show_progress(&self, stderr_is_terminal: bool) -> bool {
        stderr_is_terminal
            && !self.no_progress
            && self.output_format() != Format::Json
            && self.verbose == 0
            && self.fixture.is_none()
    }

    /// The timeouts, retries and deadline the flags and the environment ask
    /// for; a flag wins over its variable.
    pub fn transport_config(&self) -> Result<TransportConfig> {
        self.transport_config_with(|name| std::env::var(name).ok())
    }

    fn transport_config_with(
        &self,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<TransportConfig> {
        let value = |name, setting: Option<u64>| {
            env(name)
                .filter(|text| !text.trim().is_empty())
                .or_else(|| setting.map(|number| number.to_string()))
        };
        let defaults = TransportConfig::default();
        let timeout = number_setting(
            self.timeout,
            TIMEOUT_ENV,
            value(TIMEOUT_ENV, self.settings.timeout),
            1..=3600,
        )?;
        let retries = number_setting(
            self.retries.map(u64::from),
            RETRIES_ENV,
            value(RETRIES_ENV, self.settings.retries),
            0..=10,
        )?;
        let deadline = number_setting(
            self.deadline,
            DEADLINE_ENV,
            value(DEADLINE_ENV, self.settings.deadline),
            1..=86_400,
        )?;
        Ok(TransportConfig {
            timeout: timeout.map_or(defaults.timeout, std::time::Duration::from_secs),
            retries: retries.map_or(defaults.retries, |n| u32::try_from(n).unwrap_or(10)),
            deadline: deadline.map(std::time::Duration::from_secs),
            ..defaults
        })
    }

    fn rpc_urls(&self, env: Option<String>) -> Result<Vec<String>> {
        if !self.rpc_url.is_empty() {
            return Ok(self.rpc_url.clone());
        }
        let (source, candidates) = match rpc_url_from(env) {
            Some(value) => (
                RPC_URL_ENV,
                value.split_whitespace().map(str::to_owned).collect(),
            ),
            None => (
                "rpc-urls",
                self.settings.rpc_urls.clone().unwrap_or_default(),
            ),
        };
        if candidates.is_empty() {
            return Ok(vec![
                self.network
                    .unwrap_or(Network::Testnet)
                    .default_rpc_url()
                    .into(),
            ]);
        }
        candidates
            .iter()
            .map(|url| {
                parse_rpc_url(url).map_err(|reason| RewindError::InvalidRpcUrl {
                    reason: format!("{source}: {reason}"),
                })
            })
            .collect()
    }

    fn live_transport(&self) -> Result<Box<dyn Transport>> {
        let urls = self.rpc_urls(std::env::var(RPC_URL_ENV).ok())?;
        let config = self.transport_config()?;
        let mut transports = urls
            .into_iter()
            .map(|url| HttpTransport::with_config(url, config))
            .collect::<Result<Vec<_>>>()?;
        let http: Box<dyn Transport> = if transports.len() == 1 {
            Box::new(transports.remove(0))
        } else {
            Box::new(crate::fallback::Fallback::new(
                transports,
                self.network.map(|network| network.passphrase().to_owned()),
            ))
        };
        Ok(http)
    }

    fn wrap_live_transport(&self, http: Box<dyn Transport>) -> RpcClient {
        match self.cache_dir.clone().or_else(crate::cache::default_dir) {
            Some(dir) if !self.no_cache => RpcClient::new(crate::cache::Cache::new(http, dir)),
            _ => RpcClient::new(http),
        }
    }

    /// Build a new live client so a configured deadline applies to this poll.
    pub fn watch_rpc(&self) -> Result<RpcClient> {
        Ok(self.wrap_live_transport(self.live_transport()?))
    }

    /// Open the source of RPC responses the arguments ask for, and work out
    /// which transaction to replay.
    pub async fn connect(&self) -> Result<Connection> {
        let mut recording = None;
        let (rpc, hash) = match &self.fixture {
            Some(dir) => {
                let fixture = FixtureTransport::open(dir).await?;
                // A fixture holds one transaction; a different hash would
                // only fail later, on a response file it does not have.
                let hash = fixture.hash_for(self.tx_hash.as_ref())?;
                (RpcClient::new(fixture), hash)
            }
            None => {
                // clap requires a hash whenever there is no fixture; a `Cli`
                // built by hand without one is the caller's invalid input.
                let hash = self.tx_hash.clone().ok_or(RewindError::MissingHash)?;
                let http = self.live_transport()?;
                let rpc = match &self.record {
                    Some(dir) => {
                        let recorder = Recording::create(http, dir, &hash).await?;
                        recording = Some(recorder.handle());
                        RpcClient::new(recorder)
                    }
                    None => self.wrap_live_transport(http),
                };
                (rpc, hash)
            }
        };
        Ok(Connection {
            rpc,
            request: Request {
                hash,
                network: self.network,
                wasm_debug: self.wasm_debug.clone(),
            },
            recording,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    const HASH: &str = "015b092ded2eec4838dee20a5d172c428faa863a74e3706cd3aeb6581df0fd16";

    fn parse(args: &[&str]) -> std::result::Result<Cli, clap::Error> {
        Cli::try_parse_args(std::iter::once("sororewind").chain(args.iter().copied()))
    }

    #[test]
    fn progress_is_only_for_a_terminal_and_a_live_run() {
        let shown = |args: &[&str], terminal| parse(args).unwrap().show_progress(terminal);
        assert!(shown(&[HASH], true));
        assert!(!shown(&[HASH], false));
        for off in ["--no-progress", "--json", "--format=json", "-v"] {
            assert!(!shown(&[HASH, off], true), "{off}");
        }
        assert!(!shown(&["--fixture", "dir"], true));
        // A document to share is written to a file, so the stages still show.
        assert!(shown(&[HASH, "--format", "markdown"], true));
    }

    #[test]
    fn the_compact_diagnostic_is_the_default_text() {
        let detail = |args: &[&str]| {
            let cli = parse(args).unwrap();
            crate::render::Detail::from_flags(cli.verbose, cli.trace)
        };
        use crate::render::Detail;
        assert_eq!(detail(&[HASH]), Detail::Compact);
        assert_eq!(detail(&[HASH, "--trace"]), Detail::Summary);
        assert_eq!(detail(&[HASH, "-v"]), Detail::Budget);
        assert_eq!(detail(&[HASH, "--trace", "-vv"]), Detail::Diagnostics);
    }

    #[test]
    fn json_is_an_alias_of_the_json_format() {
        let format = |args: &[&str]| parse(args).unwrap().output_format();
        assert_eq!(format(&[HASH]), Format::Text);
        assert_eq!(format(&[HASH, "--json"]), Format::Json);
        assert_eq!(format(&[HASH, "--format", "html"]), Format::Html);
        assert_eq!(
            parse(&[HASH, "--json", "--format", "html"])
                .unwrap_err()
                .kind(),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn rpc_timeouts_are_validated_at_parse_time() {
        let cli = parse(&[HASH, "--timeout", "5", "--retries", "0", "--deadline", "60"]).unwrap();
        assert_eq!(
            (cli.timeout, cli.retries, cli.deadline),
            (Some(5), Some(0), Some(60))
        );
        for bad in [
            &["--timeout", "0"][..],
            &["--timeout", "soon"][..],
            &["--retries", "11"][..],
            &["--deadline", "0"][..],
        ] {
            let args: Vec<&str> = std::iter::once(HASH).chain(bad.iter().copied()).collect();
            assert_eq!(
                parse(&args).unwrap_err().kind(),
                ErrorKind::ValueValidation,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn settings_come_from_the_flag_then_the_environment() {
        let var = |text: &str| Some(text.to_string());
        // The flag wins, even over an invalid variable.
        assert_eq!(
            number_setting(Some(5), TIMEOUT_ENV, var("x"), 1..=10).unwrap(),
            Some(5)
        );
        assert_eq!(
            number_setting(None, TIMEOUT_ENV, var(" 7 "), 1..=10).unwrap(),
            Some(7)
        );
        // Unset and empty name nothing.
        assert_eq!(
            number_setting(None, TIMEOUT_ENV, None, 1..=10).unwrap(),
            None
        );
        assert_eq!(
            number_setting(None, TIMEOUT_ENV, var("  "), 1..=10).unwrap(),
            None
        );
        for bad in ["soon", "-3", "0", "11", "1.5"] {
            let err = number_setting(None, RETRIES_ENV, var(bad), 1..=10).unwrap_err();
            assert_eq!(err.kind(), "invalid_option", "{bad}");
            assert!(err.to_string().contains(RETRIES_ENV), "{err}");
        }
    }

    #[test]
    fn command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn hash_is_validated_at_parse_time() {
        let cli = parse(&[&format!("0x{}", HASH.to_uppercase())]).unwrap();
        assert_eq!(cli.tx_hash.unwrap().as_str(), HASH);
        assert_eq!(cli.network, None);

        let err = parse(&["not-a-hash"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);
        assert!(
            err.to_string().contains("64 hexadecimal characters"),
            "{err}"
        );
    }

    #[test]
    fn hash_is_required_without_a_fixture() {
        assert_eq!(
            parse(&[]).unwrap_err().kind(),
            ErrorKind::MissingRequiredArgument
        );
        let cli = parse(&["--fixture", "fixtures/x"]).unwrap();
        assert!(cli.tx_hash.is_none());
    }

    #[test]
    fn fixture_conflicts_with_live_sources() {
        for flag in ["--rpc-url", "--record"] {
            let err = parse(&[HASH, "--fixture", "dir", flag, "http://x"]).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{flag}");
        }
        // Recording a live run against a custom endpoint is fine.
        assert!(parse(&[HASH, "--rpc-url", "http://x", "--record", "dir"]).is_ok());
    }

    #[test]
    fn networks_and_verbosity() {
        for (name, network) in [
            ("testnet", Network::Testnet),
            ("mainnet", Network::Mainnet),
            ("futurenet", Network::Futurenet),
            ("local", Network::Local),
        ] {
            assert_eq!(parse(&[HASH, "-n", name]).unwrap().network, Some(network));
        }
        assert!(parse(&[HASH, "--network", "moon"]).is_err());
        assert_eq!(parse(&[HASH, "-vv"]).unwrap().verbose, 2);
    }

    #[test]
    fn rpc_urls_must_be_http() {
        for url in [
            "http://localhost:8000/rpc",
            "https://rpc.example/v1/key?x=1",
        ] {
            assert_eq!(
                parse(&[HASH, "--rpc-url", url])
                    .unwrap()
                    .rpc_url
                    .first()
                    .map(String::as_str),
                Some(url)
            );
        }
        let message = |url: &str| parse(&[HASH, "--rpc-url", url]).unwrap_err().to_string();
        // Parses as a URL whose scheme is `localhost`.
        assert!(
            message("localhost:8000").contains("did you mean http://localhost:8000?"),
            "{}",
            message("localhost:8000")
        );
        // The hint never repeats a path, which may be an API key.
        let reason = parse_rpc_url("rpc.provider.io:443/v1/SECRETKEY").unwrap_err();
        assert!(
            reason.contains("did you mean http://rpc.provider.io:443/…?"),
            "{reason}"
        );
        assert!(!reason.contains("SECRETKEY"), "{reason}");
        assert!(message("ftp://rpc.example").contains("expected http or https"));
        assert!(message("rpc.example").contains("expected http://"));
    }

    #[test]
    fn a_wasm_debug_path_must_be_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("contract.wasm");
        std::fs::write(&file, b"\0asm").unwrap();
        let file = file.display().to_string();
        assert!(parse(&[HASH, "--wasm-debug", &file]).is_ok());

        let missing = dir.path().join("nope.wasm").display().to_string();
        let err = parse(&[HASH, "--wasm-debug", &missing]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);
        let directory = dir.path().display().to_string();
        let err = parse(&[HASH, "--wasm-debug", &directory]).unwrap_err();
        assert!(err.to_string().contains("it is not a file"), "{err}");
    }

    #[test]
    fn help_lists_the_default_rpcs_the_code_uses() {
        let help = Cli::command().render_long_help().to_string();
        for network in Network::ALL {
            assert!(
                help.contains(network.default_rpc_url()),
                "{network}: {help}"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_hash_is_named_as_such() {
        let cli = Cli {
            tx_hash: None,
            ..parse(&[HASH]).unwrap()
        };
        let err = cli.connect().await.err().unwrap();
        assert!(matches!(err, RewindError::MissingHash), "{err}");
        assert!(!err.to_string().contains("``"));
    }

    #[test]
    fn the_environment_can_name_the_rpc() {
        assert_eq!(
            rpc_url_from(Some("https://rpc.example/key".to_string())).as_deref(),
            Some("https://rpc.example/key")
        );
        assert_eq!(rpc_url_from(Some("  ".to_string())), None);
        assert_eq!(rpc_url_from(None), None);
    }

    #[test]
    fn transport_defaults_respect_flags_then_env_then_config() {
        let config = || crate::config::Settings {
            rpc_urls: Some(vec!["https://project.example/rpc".into()]),
            timeout: Some(12),
            retries: Some(1),
            ..Default::default()
        };
        let mut cli = parse(&[
            HASH,
            "--timeout",
            "7",
            "--rpc-url",
            "https://one.example",
            "--rpc-url",
            "https://two.example",
        ])
        .unwrap();
        cli.apply_settings(config(), |_| None).unwrap();
        assert_eq!(
            cli.rpc_urls(Some("https://env.example".into())).unwrap(),
            ["https://one.example", "https://two.example"]
        );
        let transport = cli
            .transport_config_with(|name| match name {
                TIMEOUT_ENV => Some("9".into()),
                RETRIES_ENV => Some("2".into()),
                _ => None,
            })
            .unwrap();
        assert_eq!(transport.timeout.as_secs(), 7);
        assert_eq!(transport.retries, 2);

        let mut cli = parse(&[HASH]).unwrap();
        cli.apply_settings(config(), |_| None).unwrap();
        assert_eq!(
            cli.rpc_urls(Some("https://one.example https://two.example".into()))
                .unwrap(),
            ["https://one.example", "https://two.example"]
        );
        assert_eq!(cli.rpc_urls(None).unwrap(), ["https://project.example/rpc"]);
        let transport = cli.transport_config_with(|_| None).unwrap();
        assert_eq!(transport.timeout.as_secs(), 12);
        assert_eq!(transport.retries, 1);
    }

    #[test]
    fn colour_flags() {
        assert_eq!(parse(&[HASH]).unwrap().color_choice(), ColorChoice::Auto);
        for (flags, choice) in [
            (&["--color", "always"][..], ColorChoice::Always),
            (&["--color=never"][..], ColorChoice::Never),
            (&["--color", "auto"][..], ColorChoice::Auto),
            (&["--no-color"][..], ColorChoice::Never),
        ] {
            let args = [&[HASH][..], flags].concat();
            assert_eq!(parse(&args).unwrap().color_choice(), choice, "{flags:?}");
        }
        assert!(parse(&[HASH, "--color", "sometimes"]).is_err());
        let err = parse(&[HASH, "--no-color", "--color", "always"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn version_names_the_supported_protocol() {
        let err = parse(&["--version"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
        let text = err.to_string();
        let expected = format!(
            "sororewind {} (protocols {}; commit {}, {})\n",
            env!("CARGO_PKG_VERSION"),
            crate::host::describe(),
            env!("SOROREWIND_COMMIT"),
            env!("SOROREWIND_TARGET"),
        );
        assert_eq!(text, expected);
        assert!(text.contains("29 (soroban-env-host 29."), "{text}");
        assert!(text.contains("30 (soroban-env-host 30."), "{text}");
    }
}

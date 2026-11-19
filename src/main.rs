//! The `sororewind` binary: parse arguments, run the pipeline, print the
//! report, exit with the outcome. Everything else lives in the library.

// The library is linted against printing (Cargo.toml); this is where its
// output reaches the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::error::ErrorKind;
use sororewind::cli::{Cli, Command};
use sororewind::progress::{Progress, Silent, Step};
use sororewind::render::{self, Detail};
use sororewind::{Report, Result};

mod diff;

/// Diagnostics go to stderr, tagged with where they came from: `[rpc] → …`.
/// This is the only place in the program that writes to stderr.
struct StderrLogger;

/// The tags sororewind logs under. Dependencies log too; they are not ours to
/// print.
const TARGETS: [&str; 6] = ["rpc", "fixture", "record", "spec", "replay", "config"];

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        TARGETS.contains(&metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

fn init_logging(verbose: u8) {
    static LOGGER: StderrLogger = StderrLogger;
    // Installing a logger only fails if one is already set; there is not.
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(match verbose {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    });
}

/// Shows each stage of a live run on stderr, one line each.
struct StderrProgress;

impl Progress for StderrProgress {
    fn step(&self, step: Step) {
        eprintln!("sororewind: {step}…");
    }
}

async fn run(cli: &Cli) -> Result<Report> {
    let connection = cli.connect().await?;
    let progress: &dyn Progress = if cli.show_progress(std::io::stderr().is_terminal()) {
        &StderrProgress
    } else {
        &Silent
    };
    let result =
        sororewind::run_with_progress(&connection.rpc, &connection.request, progress).await;
    if let Some(recording) = &connection.recording {
        // Only a run that got as far as a report is a fixture worth having:
        // anything else would replay as a different error, or as another
        // transaction's leftovers.
        match &result {
            Ok(_) => recording.commit().await?,
            Err(_) => {
                recording.discard().await;
                log::warn!(
                    target: "record",
                    "the run failed, so nothing was recorded; {} is unchanged",
                    recording.target().display()
                );
            }
        }
    }
    result
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut cli = match Cli::try_parse_args(std::env::args_os()) {
        Ok(cli) => cli,
        Err(err) => {
            // `--help` and `--version` are not failures. Everything else is a
            // tool error: clap's default exit code for usage errors is 2,
            // which here means "the replay diverged".
            let _ = err.print();
            return match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::SUCCESS,
                _ => ExitCode::from(sororewind::EXIT_ERROR),
            };
        }
    };
    if let Some(Command::Diff { before, after }) = &cli.command {
        return match diff::compare(before, after) {
            Ok((output, different)) => match finish(write_stdout(&output)) {
                ExitCode::SUCCESS if different => ExitCode::from(sororewind::EXIT_DIVERGED),
                code => code,
            },
            Err(error) => finish(Err(error)),
        };
    }
    // `--completions` and `--man` stand alone (clap rejects anything next
    // to them), so there is nothing to replay.
    if let Some(shell) = cli.completions {
        return finish(write_stdout(&Cli::completion_script(shell)));
    }
    if cli.man {
        return finish(Cli::man_page().and_then(|page| write_stdout(&page)));
    }
    init_logging(cli.verbose);

    let config = cli.load_config();
    if let Some(stage) = cli.stage {
        let format = cli.output_format();
        let printed = match config {
            Ok(()) => inspect(&cli, stage).await,
            Err(error) => Err(error),
        };
        return print_result(printed, format, sororewind::EXIT_OK);
    }
    let result = match config {
        Ok(()) => run(&cli).await,
        Err(error) => Err(error),
    };
    let code = sororewind::exit_code(&result);
    let format = cli.output_format();
    let printed = result.and_then(|report| {
        let detail = Detail::from_verbosity(cli.verbose);
        let color = render::use_color(cli.color_choice(), &render::ColorEnv::current());
        let text = render::report(&report, format, color, detail)
            .map_err(|e| sororewind::RewindError::Output(e.to_string()))?;
        write_stdout(&text)
    });

    print_result(printed, format, code)
}

async fn inspect(cli: &Cli, stage: sororewind::pipeline::InspectStage) -> Result<()> {
    use sororewind::render::Format;
    use std::fmt::Write as _;

    let format = cli.output_format();
    if !matches!(format, Format::Text | Format::Json) {
        return Err(sororewind::RewindError::InvalidOption {
            name: "format",
            reason: "--stage supports text and json".into(),
        });
    }
    let connection = cli.connect().await?;
    let progress: &dyn Progress = if cli.show_progress(std::io::stderr().is_terminal()) {
        &StderrProgress
    } else {
        &Silent
    };
    let output =
        sororewind::pipeline::inspect(&connection.rpc, &connection.request, progress, stage)
            .await?;
    let mut text = String::new();
    match format {
        Format::Json => {
            text = serde_json::to_string_pretty(&output)
                .map_err(|error| sororewind::RewindError::Output(error.to_string()))?;
            text.push('\n');
        }
        _ => {
            let stage = output["stage"].as_str().unwrap_or("inspection");
            let hash = output["hash"].as_str().unwrap_or_default();
            let _ = writeln!(text, "{stage} · {hash}");
            if let Some(fields) = output.as_object() {
                for (name, value) in fields {
                    if !matches!(name.as_str(), "stage" | "hash") {
                        let _ = writeln!(text, "{name}  {value}");
                    }
                }
            }
        }
    }
    write_stdout(&text)
}

fn print_result(printed: Result<()>, format: render::Format, code: u8) -> ExitCode {
    match printed {
        Ok(()) => ExitCode::from(code),
        Err(err) => {
            // A script reading `--json`, or a file written as Markdown or
            // HTML, gets a document either way.
            if let Some(document) = render::error(&err, format) {
                let _ = write_stdout(&document);
            }
            eprintln!("sororewind: [{}] {err}", err.stage().name());
            ExitCode::from(sororewind::EXIT_ERROR)
        }
    }
}

/// The exit code of a run that only prints, reporting a failure to print.
fn finish(printed: Result<()>) -> ExitCode {
    match printed {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sororewind: [{}] {err}", err.stage().name());
            ExitCode::from(sororewind::EXIT_ERROR)
        }
    }
}

/// Write to stdout without panicking when the reader has gone away:
/// `sororewind … | head -1` closes the pipe early, and that is not a failure.
fn write_stdout(text: &str) -> Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(sororewind::RewindError::Output(e.to_string())),
    }
}

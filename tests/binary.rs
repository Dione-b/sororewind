//! The `sororewind` binary itself, run as a user runs it.
//!
//! The other integration tests drive the library, which is the whole tool but
//! for `src/main.rs`: argument errors, exit codes, what goes to stdout and
//! what to stderr, colour, and a reader that closes the pipe. Everything here
//! replays recorded fixtures; nothing touches the network.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const FIXTURE: &str = "contract-error";

fn command(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sororewind"));
    command
        .args(args)
        // Whatever the developer's shell exports must not leak in.
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("SOROREWIND_RPC_URL")
        .env_remove("SOROREWIND_NETWORK")
        .env_remove("SOROREWIND_FORMAT")
        .env_remove("SOROREWIND_COLOR")
        .env(
            "XDG_CONFIG_HOME",
            common::fixture_dir(FIXTURE).join("config"),
        );
    command
}

fn sororewind(args: &[&str]) -> Output {
    command(args).output().unwrap()
}

fn fixture() -> String {
    common::fixture_dir(FIXTURE).display().to_string()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_match_exits_0_with_the_diagnostic_on_stdout() {
    let out = sororewind(&["--fixture", &fixture()]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(text.starts_with("error[SRW1004]: "), "{text}");
    assert!(text.contains("✓ replay: trap reproduced"), "{text}");
    // The compact view leaves the narrative to --trace.
    assert!(!text.lines().any(|line| line == "call tree"), "{text}");
    assert!(text.lines().count() <= 10, "{text}");
    assert!(out.stderr.is_empty(), "{}", stderr(&out));

    let out = sororewind(&["--fixture", &fixture(), "--trace"]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(text.contains("replay matches network"), "{text}");
    assert!(text.contains("call tree"), "{text}");
    // -v is the trace with more detail.
    let verbose = stdout(&sororewind(&["--fixture", &fixture(), "-v"]));
    assert!(verbose.contains("call tree"), "{verbose}");

    let out = sororewind(&["--fixture", &fixture(), "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["fidelity"]["match"], true);
    assert!(out.stderr.is_empty(), "{}", stderr(&out));
}

/// A copy of the fixture whose recorded result is one the replay will not
/// reproduce.
fn diverging_fixture(dir: &Path) {
    use sororewind::codec;
    use stellar_xdr::{
        InvokeHostFunctionResult, OperationResult, OperationResultTr, TransactionResult,
        TransactionResultResult,
    };
    for entry in std::fs::read_dir(common::fixture_dir(FIXTURE)).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
    }
    let path = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("getTransaction"))
        })
        .unwrap();
    let mut response: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let result = &mut response["result"];
    let mut recorded: TransactionResult =
        codec::decode(result["resultXdr"].as_str().unwrap(), "resultXdr").unwrap();
    recorded.result = TransactionResultResult::TxFailed(
        vec![OperationResult::OpInner(
            OperationResultTr::InvokeHostFunction(InvokeHostFunctionResult::ResourceLimitExceeded),
        )]
        .try_into()
        .unwrap(),
    );
    result["resultXdr"] = codec::b64_encode(&codec::encode(&recorded, "result").unwrap()).into();
    std::fs::write(&path, response.to_string()).unwrap();
}

#[test]
fn a_divergence_exits_2_and_still_prints_the_report() {
    let dir = tempfile::tempdir().unwrap();
    diverging_fixture(dir.path());
    let path = dir.path().display().to_string();

    let out = sororewind(&["--fixture", &path]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("✗ replay: replay mismatch (result)"),
        "{text}"
    );
    insta::assert_snapshot!(
        "diverged-compact",
        format!("--- exit code: 2\n{text}").replace(&path, "<fixture>")
    );

    let out = sororewind(&["--fixture", &path, "--trace"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("replay diverged from the network"), "{text}");
    // A divergence shows the footprint without being asked.
    assert!(text.contains("footprint "), "{text}");
    // The whole narrative and its exit code, pinned. Nothing here drifted,
    // so the report says that nothing explains the divergence.
    insta::assert_snapshot!(
        "diverged-text",
        format!("--- exit code: 2\n{text}").replace(&path, "<fixture>")
    );

    let out = sororewind(&["--fixture", &path, "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["fidelity"]["match"], false);
    assert_eq!(json["fidelity"]["divergence"], "result");
    assert_eq!(json["probableCauses"], serde_json::json!([]));
    // The replay's error is not the network's: only the result code is used.
    assert_eq!(json["diagnostic"]["code"], "SRW1003");
    assert_eq!(json["diagnostic"]["evidence"]["source"], "result_code");
    assert_eq!(json["replay"]["status"], "replay_mismatch");
    assert_eq!(json["replay"]["outcomeMatches"], false);
    let mut pinned = json.clone();
    // `rpc` names the temporary fixture directory.
    pinned["rpc"] = "<fixture>".into();
    insta::assert_snapshot!(
        "diverged-json",
        format!(
            "--- exit code: 2\n{}",
            serde_json::to_string_pretty(&pinned).unwrap()
        )
    );
}

#[test]
fn json_is_one_ansi_free_document_with_the_diagnostic() {
    // Colour forced on: it applies to the text output only.
    let out = sororewind(&["--fixture", &fixture(), "--json", "--color", "always"]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(!text.contains('\u{1b}'), "{text}");
    // Exactly one document: a stream of JSON values has nothing after it.
    let mut documents = serde_json::Deserializer::from_str(&text).into_iter::<Value>();
    let json = documents.next().unwrap().unwrap();
    assert!(documents.next().is_none(), "{text}");

    let diagnostic = &json["diagnostic"];
    for field in [
        "code", "severity", "kind", "message", "location", "details", "evidence",
    ] {
        assert!(
            !diagnostic[field].is_null(),
            "diagnostic.{field}: {diagnostic}"
        );
    }
    assert_eq!(diagnostic["code"], "SRW1004");
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["kind"], "contract_error");
    // Identifiers are never abbreviated in JSON.
    let contract = diagnostic["location"]["contract"].as_str().unwrap();
    assert_eq!(contract.len(), 56, "{contract}");
    assert_eq!(diagnostic["location"]["function"], "transfer");
    assert_eq!(diagnostic["evidence"]["source"], "host_diagnostic");
    assert_eq!(diagnostic["evidence"]["helpVerified"], false);
    assert_eq!(json["replay"]["status"], "trap_reproduced");
    assert_eq!(json["replay"]["onChain"], "trapped");
    assert_eq!(json["replay"]["local"], "trapped");
    assert_eq!(json["replay"]["outcomeMatches"], true);
}

#[test]
fn argument_errors_exit_1_on_stderr() {
    for args in [
        &["not-a-hash"][..],
        &[][..],
        &["--no-such-flag"][..],
        &["--fixture", "dir", "--rpc-url", "http://x"][..],
        &["--color", "sometimes"][..],
    ] {
        let out = sororewind(args);
        // clap's own code for these is 2, which here means "diverged".
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}: {}", stdout(&out));
        assert!(!out.stderr.is_empty(), "{args:?}");
    }
    let out = sororewind(&["not-a-hash"]);
    assert!(
        stderr(&out).contains("expected 64 hexadecimal characters"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn run_errors_name_their_stage_on_stderr() {
    let out = sororewind(&["--fixture", "/nonexistent/fixture"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).starts_with("sororewind: [fetch] "),
        "{}",
        stderr(&out)
    );
}

#[test]
fn help_and_version_exit_0_on_stdout() {
    let out = sororewind(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let help = stdout(&out);
    assert!(
        help.contains("EXIT CODES") && help.contains("--fixture"),
        "{help}"
    );
    assert!(out.stderr.is_empty());

    let out = sororewind(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("sororewind "), "{}", stdout(&out));
    assert!(stdout(&out).contains("protocols"), "{}", stdout(&out));
    assert!(stdout(&out).contains("; commit "), "{}", stdout(&out));
}

#[test]
fn completions_and_the_man_page_print_and_exit_0() {
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let out = sororewind(&["--completions", shell]);
        assert_eq!(out.status.code(), Some(0), "{shell}");
        assert!(stdout(&out).contains("sororewind"), "{shell}");
        assert!(out.stderr.is_empty(), "{shell}");
    }

    let out = sororewind(&["--man"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains(".TH sororewind 1"),
        "{}",
        stdout(&out)
    );

    // They stand alone: nothing is replayed next to them.
    let hash = "ab".repeat(32);
    let out = sororewind(&["--man", &hash]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let out = sororewind(&["--completions", "tcsh"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_run_that_fails_exits_1_with_the_reason_on_stderr() {
    let other = "ab".repeat(32);
    let out = sororewind(&["--fixture", &fixture(), &other]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let text = stderr(&out);
    assert!(text.starts_with("sororewind: "), "{text}");
    assert!(
        text.contains("was recorded for transaction") && text.contains(&other),
        "{text}"
    );

    let out = sororewind(&["--fixture", "/no/such/fixture"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stderr(&out);
    assert!(text.contains("there is no such directory"), "{text}");
    assert!(!text.contains("different transaction"), "{text}");
}

#[test]
fn json_mode_reports_errors_as_json() {
    let missing = common::fixtures_root().join("no-such-fixture");
    let missing = missing.display().to_string();
    let out = sororewind(&["--fixture", &missing, "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["error"]["kind"], "fixture_invalid");
    let message = json["error"]["message"].as_str().unwrap();
    // The same sentence a person gets on stderr.
    assert!(stderr(&out).contains(message));
}

#[test]
fn colour_is_off_in_a_pipe_unless_asked_for() {
    const ESC: u8 = 0x1b;
    let args = ["--fixture", &fixture()];
    assert!(!sororewind(&args).stdout.contains(&ESC));

    let forced = sororewind(&["--fixture", &fixture(), "--color", "always"]);
    assert!(forced.stdout.contains(&ESC));
    let forced = command(&args).env("CLICOLOR_FORCE", "1").output().unwrap();
    assert!(forced.stdout.contains(&ESC));

    // NO_COLOR beats CLICOLOR_FORCE, but only when it is not empty...
    let quiet = command(&args)
        .env("CLICOLOR_FORCE", "1")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!quiet.stdout.contains(&ESC));
    let empty = command(&args)
        .env("CLICOLOR_FORCE", "1")
        .env("NO_COLOR", "")
        .output()
        .unwrap();
    assert!(empty.stdout.contains(&ESC));
    // ...and the flags beat the environment.
    for flag in [&["--no-color"][..], &["--color", "never"][..]] {
        let out = command(&[&args[..], flag].concat())
            .env("CLICOLOR_FORCE", "1")
            .output()
            .unwrap();
        assert!(!out.stdout.contains(&ESC), "{flag:?}");
    }
    // JSON is never coloured.
    let json = sororewind(&["--fixture", &fixture(), "--json", "--color", "always"]);
    assert!(!json.stdout.contains(&ESC));
}

#[test]
fn verbosity_adds_detail_and_logs_go_to_stderr() {
    let quiet = sororewind(&["--fixture", &fixture()]);
    let verbose = sororewind(&["--fixture", &fixture(), "-v"]);
    let trace = sororewind(&["--fixture", &fixture(), "-vv"]);
    assert!(!stdout(&quiet).contains("budget "));
    assert!(stdout(&verbose).contains("budget "));
    assert!(stdout(&trace).contains("local diagnostics"));
    // Tracing is on stderr, tagged; the narrative stays clean.
    assert!(stderr(&trace).contains("[fixture] "), "{}", stderr(&trace));
    assert!(!stdout(&trace).contains("[fixture] "));
    for out in [&quiet, &verbose, &trace] {
        assert_eq!(out.status.code(), Some(0));
    }
}

/// `sororewind … | head -1`: the reader going away is not a failure, and
/// certainly not a panic (exit code 101).
#[test]
fn a_closed_pipe_is_not_an_error() {
    let mut child = command(&["--fixture", &fixture(), "-vv"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Close our end before the child has anything to write.
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

#[test]
fn an_rpc_url_from_the_environment_is_validated() {
    let hash = "ab".repeat(32);
    let out = command(&[&hash])
        .env("SOROREWIND_RPC_URL", "localhost:8000")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = stderr(&out);
    assert!(
        text.contains("SOROREWIND_RPC_URL") && text.contains("did you mean http://localhost:8000?"),
        "{text}"
    );
}

#[test]
fn stage_inspection_keeps_text_and_json_snapshots() {
    for stage in ["decode", "state", "replay"] {
        for format in ["text", "json"] {
            let out = sororewind(&[
                "--fixture",
                &fixture(),
                "--stage",
                stage,
                "--format",
                format,
            ]);
            assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
            assert!(out.stderr.is_empty(), "{}", stderr(&out));
            if format == "json" {
                let document: Value = serde_json::from_slice(&out.stdout).unwrap();
                assert_eq!(document["stage"], stage);
            }
            insta::assert_snapshot!(format!("stage-{stage}-{format}"), stdout(&out));
        }
    }
}

#[test]
fn diff_compares_reports_from_fixtures() {
    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before.json");
    let after = dir.path().join("after.json");
    let write_report = |name: &str, path: &Path| {
        let fixture = common::fixture_dir(name);
        let out = sororewind(&["--fixture", fixture.to_str().unwrap(), "--json"]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        std::fs::write(path, out.stdout).unwrap();
    };
    write_report(FIXTURE, &before);
    write_report("budget-error", &after);
    let before = before.to_str().unwrap();
    let after = after.to_str().unwrap();

    let same = sororewind(&["diff", before, before]);
    assert_eq!(same.status.code(), Some(0), "{}", stderr(&same));
    assert!(stdout(&same).contains("equivalent"));

    let different = sororewind(&["diff", before, after]);
    assert_eq!(different.status.code(), Some(2), "{}", stderr(&different));
    for section in [
        "Outcome",
        "Failure",
        "Call tree",
        "Events",
        "Budget",
        "Footprint",
    ] {
        assert!(stdout(&different).contains(section), "{section}");
    }

    let mut incompatible: Value = serde_json::from_slice(&std::fs::read(after).unwrap()).unwrap();
    incompatible["schemaVersion"] = 2.into();
    std::fs::write(after, incompatible.to_string()).unwrap();
    let wrong_version = sororewind(&["diff", before, after]);
    assert_eq!(wrong_version.status.code(), Some(1));
    assert!(stderr(&wrong_version).contains("schemaVersion differs"));
}

#[test]
fn config_precedence_reaches_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let user_dir = dir.path().join("user");
    let project_dir = dir.path().join("project");
    std::fs::create_dir_all(user_dir.join("sororewind")).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        user_dir.join("sororewind/sororewind.toml"),
        "format = 'markdown'",
    )
    .unwrap();
    let config = project_dir.join("sororewind.toml");
    std::fs::write(&config, "format = 'json'").unwrap();

    let run = |format: Option<&str>, flags: &[&str]| {
        let mut command = command(&["--fixture", &fixture(), "-v"]);
        command
            .current_dir(&project_dir)
            .env("XDG_CONFIG_HOME", &user_dir)
            .args(flags);
        if let Some(format) = format {
            command.env("SOROREWIND_FORMAT", format);
        }
        command.output().unwrap()
    };
    let project = run(None, &[]);
    assert_eq!(project.status.code(), Some(0), "{}", stderr(&project));
    assert!(stdout(&project).starts_with('{'));
    assert!(stderr(&project).contains("[config] loaded"));

    let env = run(Some("html"), &[]);
    assert_eq!(env.status.code(), Some(0), "{}", stderr(&env));
    assert!(stdout(&env).contains("<!DOCTYPE html>") || stdout(&env).contains("<!doctype html>"));

    let flag = run(Some("html"), &["--format", "text"]);
    assert_eq!(flag.status.code(), Some(0), "{}", stderr(&flag));
    assert!(stdout(&flag).contains("replay matches network"));

    std::fs::remove_file(&config).unwrap();
    let user = run(None, &[]);
    assert_eq!(user.status.code(), Some(0), "{}", stderr(&user));
    assert!(stdout(&user).starts_with('#'));

    std::fs::write(config, "unknown = 'value'").unwrap();
    let unknown = run(None, &[]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(stderr(&unknown).contains("unknown key"));
}

#[test]
fn what_if_p30_preserves_faithful_p29_report_and_exit_status() {
    let fixture = common::fixture_dir("mainnet-auth-error");
    let fixture = fixture.to_str().unwrap();
    let faithful = sororewind(&["--fixture", fixture, "--json"]);
    let alternate = sororewind(&["--fixture", fixture, "--json", "--what-if-protocol", "30"]);
    assert_eq!(faithful.status.code(), Some(0), "{}", stderr(&faithful));
    assert_eq!(
        alternate.status.code(),
        faithful.status.code(),
        "{}",
        stderr(&alternate)
    );
    let faithful: Value = serde_json::from_slice(&faithful.stdout).unwrap();
    let mut alternate: Value = serde_json::from_slice(&alternate.stdout).unwrap();
    let what_if = alternate.as_object_mut().unwrap().remove("whatIf").unwrap();
    assert_eq!(alternate, faithful);
    assert_eq!(what_if["protocol"], 30);
    assert_eq!(what_if["faithful"], false);
    assert_eq!(what_if["status"], "compared");
    assert!(what_if["budget"]["cpuInsns"].is_number());
    assert!(what_if["diagnostics"].is_array());
    let narrative = sororewind(&["--fixture", fixture, "--what-if-protocol", "30"]);
    assert!(stdout(&narrative).contains("what-if protocol 30 (non-faithful)"));

    let unavailable = sororewind(&["--fixture", fixture, "--json", "--what-if-protocol", "999"]);
    assert_eq!(unavailable.status.code(), Some(0));
    let unavailable: Value = serde_json::from_slice(&unavailable.stdout).unwrap();
    assert_eq!(unavailable["whatIf"]["status"], "unavailable");
}

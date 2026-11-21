# sororewind

[![CI](https://github.com/Dione-b/sororewind/actions/workflows/ci.yml/badge.svg)](https://github.com/Dione-b/sororewind/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Dione-b/sororewind?include_prereleases)](https://github.com/Dione-b/sororewind/releases)
[![docs.rs](https://docs.rs/sororewind/badge.svg)](https://docs.rs/sororewind)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

**Rewind a failed Soroban transaction, run it again on your machine, and see
exactly where it broke.**

```
$ sororewind --fixture fixtures/contract-error
error[SRW1004]: Contract returned an error
 --> CDLZF…GCYSC::transfer()

  error  BalanceError (#10)
  cause  The transfer would put an account balance outside the token's permitted range.
  help   Compare the attempted amount with the available balance and any account limits.

✓ replay: trap reproduced · testnet / ledger 5672381
```

`--trace` prints the whole story: the findings, the verdict, where it broke
and the decoded call tree.

**Contents:** [Install](#install) · [Quick start](#quick-start) ·
[Reading the output](#reading-the-output) · [Options](#options) ·
[JSON output](#json-output) · [Limitations](#limitations) ·
[Why a replay can diverge](#why-a-replay-can-diverge) ·
[Contributing](#contributing)

## Why

Your transaction failed. The network hands you a result code, a wall of
diagnostic events, and good luck.

sororewind gives you a story instead. Hand it a hash and it:

1. **Fetches** the transaction from a Soroban RPC.
2. **Rebuilds** the world it ran in: footprint entries, TTLs, network cost
   settings and the ledger header, as they were *at that moment*, not as they
   are now.
3. **Replays** it with the same `soroban-env-host` the network ran.
4. **Tells you** what failed, the way a compiler reports an error: a stable
   code, the contract and function, the values that matter and one thing to
   check. `--trace` adds which call failed and how it got there.

And it doesn't ask you to take its word for it. Every replay is checked
against the result the network recorded. A match means the call tree you are
reading is the one that really happened. A mismatch is reported loudly, with
what changed, instead of being quietly smoothed over.

## Install

Grab a prebuilt binary with the installer script (macOS, Linux):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Dione-b/sororewind/releases/latest/download/sororewind-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Dione-b/sororewind/releases/latest/download/sororewind-installer.ps1 | iex"
```

Binaries are published on the
[Releases page](https://github.com/Dione-b/sororewind/releases) for macOS
(Apple silicon and Intel), Linux (x86_64 and arm64, plus a static musl build
that runs on any distribution) and Windows (x86_64).

Prefer building it yourself? You need Rust 1.89 or newer. It is on
[crates.io](https://crates.io/crates/sororewind):

```bash
cargo install --locked sororewind
```

Don't drop `--locked`. The embedded hosts are tested against exact dependency
versions, and a different version can change what a host charges for a call,
which is enough to turn a match into a divergence.

### Which transactions it can replay

Each build embeds two Soroban hosts: right now protocols **29** and **30**.
That covers testnet and mainnet through an upgrade, when testnet runs the new
protocol weeks before mainnet does. `sororewind --version` tells you what your
build supports.

See the [compatibility matrix](docs/compatibility.md) for supported host
versions, networks, default RPCs, historical data notes, and the minimum Rust
version.

## Quick start

Paste a hash (64 hex characters, `0x` optional). Testnet is the default:

```bash
sororewind <TX_HASH>
```

Mainnet, or your own RPC:

```bash
sororewind <TX_HASH> --network mainnet
```

```bash
sororewind <TX_HASH> --rpc-url <RPC_URL>
```

No network handy? The recorded fixtures live in the repository (the crate
doesn't ship them). Clone it and replay one with the sororewind you installed:

```bash
git clone https://github.com/Dione-b/sororewind && cd sororewind
```

```bash
sororewind --fixture fixtures/contract-error
```

The [offline example gallery](https://github.com/Dione-b/sororewind/blob/master/docs/examples.md)
shows the command and expected output for every recorded fixture.

A few things worth knowing:

- **It only replays failures.** The transaction must have failed inside the
  Soroban host.
- **RPCs forget.** They typically keep about a week of history. When a hash is
  too old, the error tells you the window the RPC still covers.
- **API keys stay private.** If your RPC URL carries a key, pass it through
  `SOROREWIND_RPC_URL` so it stays out of your shell history and process list.
  Either way, sororewind only ever prints the scheme and host
  (`https://rpc.provider.io/…`).
- **Flaky RPCs are expected.** Connection errors, timeouts, HTTP 429, 502, 503
  and 504, and rate-limit errors (even inside an HTTP 200) are retried a few
  times before giving up, waiting as long as a `Retry-After` header asks (a
  number of seconds or an HTTP date, up to 10 seconds). `--timeout`,
  `--retries` and `--deadline` tune this ([Options](#options)). Responses are
  requested gzip-compressed, which keeps the ledger metadata it downloads small.

## Reading the output

By default sororewind prints one diagnostic, compiler-style, and the replay's
verdict:

```text
error[SRW1001]: Undeclared storage access
 --> CBLLS…ZSMCM::withdraw_position()

  key    [UserRewardState, GAEGW…JD4BM]
  cause  The contract accessed a ledger key outside the declared transaction footprint.
  help   Check the key in the diagnostic against the simulated read-only and read-write footprint.

✓ replay: trap reproduced · testnet / ledger 5672305
```

- **`error[CODE]: message`** names the failure. The code is stable, so a
  script, a CI job or an agent can key on it (table below).
- **`-->`** is the contract and function whose frame raised the error, when
  one was open. Identifiers are shortened here; `--trace`, `-v` and `--json`
  show them whole.
- **The labelled lines** are only those that matter for this kind of failure
  and that the evidence supports: the key outside the footprint, the
  contract's error name, the account contract that rejected authorization, the
  host error and its message. Anything unknown is left out, never guessed.
  `cause` and `help` come from a catalog of known failures: what this kind of
  failure usually means and one thing to check. They are suggestions, not a
  verified diagnosis.
- **`✓ replay: trap reproduced`** means the local run passed the fidelity
  check: the same result code, error and resource fee, and diagnostic events
  that are the start of the network's (see
  [Why a replay can diverge](#why-a-replay-can-diverge)). It claims nothing
  that check did not compare. `failure reproduced`
  says the same for a failure other than a trap. `✗ replay: replay mismatch`
  says what differed and names the strongest probable cause; `execution
  succeeded` means the replay succeeded where the network failed. After a
  divergence the diagnostic rests on the network's result code alone, since
  the replay's error may not be the network's. A replay that rests on inputs
  it could not verify says so on the next line.

| Code | Kind | When |
|------|------|------|
| `SRW1001` | `footprint_violation` | A storage error whose host diagnostic says the key is outside the footprint. A storage `ExceededLimit` without that evidence is `SRW1999`. |
| `SRW1002` | `authorization` | An `auth` host error, including a custom account's `__check_auth` rejecting it. |
| `SRW1003` | `budget_exceeded` | A `budget` `ExceededLimit` error, or the `resource_limit_exceeded` result. |
| `SRW1004` | `contract_error` | The contract returned one of its own errors. |
| `SRW1005` | `missing_storage` | A storage `MissingValue` error. |
| `SRW1006` | `entry_archived` | The `entry_archived` result. |
| `SRW1007` | `refundable_fee` | The `insufficient_refundable_fee` result. |
| `SRW1999` | `unclassified` | Anything else; the host error is kept as it was reported. |

A code never changes meaning; a new kind of failure gets a new code.

`--trace` prints the narrative instead. It has a header, the findings, a
verdict and a call tree. The rest shows up when it has something to say, or
when you ask for more.

| Section | What it tells you |
|---------|-------------------|
| **Header** | The hash, the network, and the ledger it ran in, with protocol and close time. |
| **Verdict** | `✓ replay matches network`: trust everything below. `⚠ replay diverged`: the local run disagrees, and the line says how (another result code, or the same code with another error, resource fee or events). On a divergence, `probably caused by:` lists the changed entries and settings, and the first call where the two runs differ, strongest first; when there are none it says so. A separate warning appears if the network's settings changed since. A match that rests on state that could not be verified says so right under it (see [Limitations](#limitations)). |
| **Where it broke** | The error that ended it: its type (contract, auth, storage, budget, …), the contract's own error name when it declares one, the logged message, and the function and call depth that raised it. |
| **Call tree** | Every contract call, in order, with decoded arguments. Nested calls hang off `├─` / `└─`, `✗` marks the failing call, and `·` lines are the contract's logs. `(recovered by try_call)` marks a failure the caller caught and moved past; `(no return)` marks a call the transaction died inside. |
| **Footprint** | How many entries were touched, which changed since (value, TTL, archived), how many were missing, and whether the random seed was derived or fixed. Shown on a divergence, and always with `-v`. |

Want more?

- **`-v`** prints the `--trace` narrative and adds return values, the CPU
  and memory budget (local against the limits, and what the network used)
  and the full footprint. Debug lines go to
  stderr, tagged by source: `[spec]`, `[replay]`, `[rpc]`, `[record]`.
- **`-vv`** also dumps the raw on-chain and local diagnostic events and traces
  every RPC request and response (`[rpc]`, or `[fixture]` offline).

The report always goes to stdout, the debug lines to stderr. To keep
everything: `sororewind <TX_HASH> -vv 2>&1 | tee run.log`.

## Sharing a report

To put a diagnosis in an issue, a PR or a chat, ask for a document instead of
pasting terminal text:

```bash
sororewind <hash> --format markdown | gh issue create --title "…" --body-file -
sororewind <hash> --format html > diagnosis.html
```

Both are built from the same report as the JSON, not from the terminal
narrative, and hold the same content: the verdict, the findings, the probable
causes, where it broke, the call tree, the budget and footprint, and the
diagnostic events. The Markdown puts the call tree and the rest of the detail
in collapsible sections. The HTML is a single file with its style inline, no
scripts and no external requests; it follows the reader's light or dark
setting. Text that came from a contract or an RPC is escaped, so it cannot
turn into markup.

When the run fails (exit `1`) the same format is used for the error:
Markdown and HTML get a short document with the stage, the kind and the
message, `json` its error document, and `text` only the message on stderr.

## Feedback and bug reports

If you use sororewind against production Soroban transactions, please share
what worked, where a replay diverged, and which output was useful using the
[production feedback form](https://github.com/Dione-b/sororewind/issues/new?template=production_feedback.md).
For a specific mismatch, use the [report a divergence form](https://github.com/Dione-b/sororewind/issues/new?template=report_divergence.md).
Remove credentials and private information from RPC URLs and logs before
submitting them.

## Replay failures in GitHub Actions

Use the composite Action on a Linux x64 or arm64 runner. Pin the Action ref
to a release or commit containing the Action and set `version` to the exact
release tag of the CLI. The job uploads one JSON report per replay and
writes a verdict table to the job summary. A divergence or CLI error fails
the step after all supplied hashes have been processed.

```yaml
name: Diagnose a Soroban failure
on:
  workflow_dispatch:
    inputs:
      transaction_hash:
        description: Failed transaction hash
        required: true
        type: string
permissions:
  contents: read
jobs:
  diagnose:
    runs-on: ubuntu-latest
    steps:
      - uses: Dione-b/sororewind/.github/actions/replay@master
        with:
          version: v0.2.0
          network: testnet
          hashes: ${{ inputs.transaction_hash }}
```

Pass a private RPC endpoint through the `rpc-url` input using a repository
secret. For offline CI, check out a recording and pass its workspace-relative
path with `fixture` in place of `hashes`.

## Watching new failures

```bash
sororewind watch --contract <C_ADDRESS> --network mainnet --output-dir reports
sororewind watch --account <G_ADDRESS> --format json --poll-interval 15
```

The watcher starts at the current tip and polls newly closed ledgers.
Matches come from the invocation, authorization entries, footprint, source
account and available contract diagnostics. It uses the configured RPC
retries and waits 10 seconds by default between polls. JSON reports on
stdout use one line per transaction. With `--output-dir`, reports are
written under their transaction hashes.

The last fully processed ledger is saved by network and target under
`$XDG_STATE_HOME/sororewind/watch`, `~/.local/state/sororewind/watch`, or
the corresponding `%LOCALAPPDATA%` directory on Windows. Use
`--checkpoint <PATH>` to select another file. Ctrl-C stops the watcher.
An interrupted batch resumes from the saved checkpoint and can repeat
reports for transactions already emitted. If an RPC has pruned unprocessed
ledgers, the watcher reports the gap and continues from its oldest ledger.

## Comparing embedded protocols

```bash
sororewind --fixture fixtures/mainnet-auth-error --what-if-protocol 30
```

The primary replay runs on the transaction's own protocol. The optional
what-if comparison runs the other embedded host using the same reconstructed
state, fee settings, cost tables and transaction inputs. It compares the
result, CPU and memory use, and diagnostic events with the primary replay.
This does not model a network upgrade or predict a different network's
state. Text labels it non-faithful and JSON puts it under `whatIf` with
`faithful: false`. The primary report and exit code remain authoritative
even when the alternate host cannot run the transaction.

## Inspecting a replay stage

Use `--stage decode` to see the transaction and its declared footprint,
`--stage state` for the reconstructed entries and their provenance, or
`--stage replay` for the raw local result, events and resource use. The
command stops at the selected stage. Each stage supports text and JSON.

```bash
sororewind --fixture fixtures/contract-error --stage state --json
```

A completed inspection exits `0`, including a replay whose raw result is a
contract failure. An inspection error exits `1`. The full replay verdict and
its divergence exit code apply only when running the full pipeline.

## Comparing saved reports

```bash
sororewind diff before.json after.json
```

Both files must be replay reports with the same `schemaVersion`. The
comparison shows changes in transaction identity, outcome, failure, call
tree, events, budget and footprint. The RPC source label is ignored so the
same replay can be compared across providers. The exit status is `0` for
equivalent reports, `2` for differences, or `1` for an input or output error.

## Exit codes

| Code | Meaning |
|------|---------|
| `0`  | Replayed, and **matched** the network. |
| `1`  | Couldn't replay: a bad hash or flag, an unreachable RPC, the wrong network, a transaction that succeeded or isn't Soroban, an unsupported protocol, missing state. |
| `2`  | Replayed, but **diverged** from the network. |

`2` is an answer, not a crash: you still get the full report, just read it
with care ([why a replay can diverge](#why-a-replay-can-diverge)). Invalid
flags exit `1`, not the usual `2`, so scripts can treat `2` as "diverged" and
nothing else.

## Options

| Option | Description |
|--------|-------------|
| `-n, --network <NETWORK>` | `testnet` (default), `mainnet`, `futurenet`, or `local` (a quickstart or standalone node). Picks the default RPC. |
| `--rpc-url <URL>` | Use an `http://` or `https://` RPC. Repeat the option for ordered fallback, or set `SOROREWIND_RPC_URL` to a whitespace-separated list. |
| `--cache-dir <DIR>` | Put the live RPC cache in this directory. By default it uses `$XDG_CACHE_HOME/sororewind`, `%LOCALAPPDATA%/sororewind` on Windows, or `~/.cache/sororewind`. |
| `--no-cache` | Fetch all RPC responses from the endpoint for this run. |
| `--timeout <SECONDS>` | How long one RPC request may take before it is abandoned and retried (1 to 3600, default 30). `SOROREWIND_TIMEOUT` sets it from the environment. |
| `--retries <N>` | How many times a failed RPC call is retried (0 to 10, default 3; `0` never retries). `SOROREWIND_RETRIES` sets it. |
| `--deadline <SECONDS>` | How long the whole run may spend on the RPC, retries and waits included (1 to 86400; default none). A call still going then fails with `rpc_unreachable`. `SOROREWIND_DEADLINE` sets it. |
| `--format <text\|json\|markdown\|html>` | How the report is written: the terminal diagnostic (`text`, default; `--trace` for the narrative), the JSON document, Markdown for a GitHub issue or PR, or one self-contained HTML file. See [Sharing a report](#sharing-a-report). |
| `--what-if-protocol <N>` | Also replay with the other embedded host and compare its result, budget and events with the primary replay. |
| `--stage <decode\|state\|replay>` | Stop after the selected stage and print its output in text or JSON. |
| `--json` | Same as `--format json`. |
| `--trace` | Print the narrative (findings, verdict, where it broke, the call tree) instead of the compact diagnostic. Text output only; `-v` implies it. |
| `--wasm-debug <PATH>` | For a contract error, read the error names from a local WASM file instead of the ledger. |
| `--color <auto\|always\|never>` | `auto` (default) colours a terminal and honours a non-empty `NO_COLOR`, `CLICOLOR_FORCE` and `TERM=dumb`. Use `always` in a pager: `sororewind … --color always \| less -R`. |
| `--no-color` | Same as `--color never`. |
| `--no-progress` | A live run prints the stage it is in on stderr (`sororewind: fetching the transaction…`) when stderr is a terminal. This turns it off; it is also off with `--json`, `-v` and `--fixture`. |
| `-v, --verbose` | The `--trace` narrative with more detail; repeat for even more (`-vv`). |
| `--fixture <DIR>` | Replay offline from a recorded fixture. The hash is optional (the fixture knows it); a different one is an error. Not combinable with `--rpc-url`. |
| `--completions <SHELL>` | Print a completion script for `bash`, `zsh`, `fish`, `elvish` or `powershell`, e.g. `sororewind --completions fish > ~/.config/fish/completions/sororewind.fish`. |
| `--man` | Print a man page, e.g. `sororewind --man > ~/.local/share/man/man1/sororewind.1`. |
| `-h, --help` | Show usage, options and examples. |
| `-V, --version` | Version, supported protocols, and the commit and target it was built from. Paste it into bug reports. |

The live cache reuses final transactions and complete pages of closed ledger
metadata and verified, hash-addressed contract code from the same network.
Current ledger entries are fetched
on every run. Fixture playback and fixture recording bypass the disk cache.

With multiple RPC endpoints, requests fall back in order on an unreachable
endpoint, a rate limit after retries, or missing transaction or ledger history.
Every endpoint must report the same network passphrase. `-v` shows which
endpoint served a request, with URLs redacted to their scheme and host.

Default RPCs: `https://soroban-testnet.stellar.org` (testnet),
`https://mainnet.sorobanrpc.com` (mainnet, third-party),
`https://rpc-futurenet.stellar.org` (futurenet) and
`http://localhost:8000/rpc` (local).

Pass `--network` explicitly and sororewind checks it against the passphrase
the RPC reports. If they disagree, it stops rather than print a report with
the wrong label on it.

## Configuration

Set project defaults in `sororewind.toml` in the working directory. User
defaults come from `$XDG_CONFIG_HOME/sororewind/sororewind.toml`, falling
back to `~/.config/sororewind/sororewind.toml` or the corresponding
`%APPDATA%` directory on Windows.

Settings are resolved per key. Command-line flags take precedence over
environment variables, followed by the project file and then the user file.
For network, format and colour, use `SOROREWIND_NETWORK`,
`SOROREWIND_FORMAT` and `SOROREWIND_COLOR`. The RPC and transport variables
are in [Options](#options). Unknown keys produce an error. Use `-v` to see
the paths of the files that were loaded.

```toml
network = "testnet"
rpc-urls = ["https://soroban-testnet.stellar.org", "https://archive.example/rpc"]
timeout = 20
retries = 2
deadline = 60
format = "text"
color = "auto"
```

## JSON output

`--json` prints the whole report as one document, at any verbosity. Pipe it
into `jq`, feed it to CI, build tooling on top of it. The main fields:

| Field | Content |
|-------|---------|
| `schemaVersion` | The document's shape, currently `1`. Bumped only when a field is removed, renamed or changes meaning; new fields don't bump it. |
| `hash`, `network`, `ledger`, `protocol`, `closeTime` | Which transaction ran, and where. |
| `feeBump` | Whether it was wrapped in a fee-bump; `hash` is then whichever of the outer and inner hashes you asked for. |
| `rpc` | Where the responses came from: scheme and host only (a path or query may hold an API key), or `fixture:<dir>`. |
| `status` | Always `failed`: sororewind only replays failures. |
| `findings` | What matters, most severe first, as `{ severity, summary, evidence }`, with optional catalog `guidance` (explanation, checks and a verification flag). `severity` is `error` (the transaction's own failure), `divergence` (the replay did not reproduce it; the summary names the probable cause), `warning` (an input may differ from what the transaction saw) or `limitation` (something could not be compared or reproduced). `evidence` lists the report fields it rests on, as paths into this document (`failure`, `probableCauses[0]`, `trust.caveats[2]`). The text report prints them at the top; `-v` adds the evidence. |
| `diagnostic` | The primary failure, as the text output prints it but with identifiers whole: `{ code, severity, kind, message, location, details, cause, help, evidence }`. `code` and `kind` are from the table in [Reading the output](#reading-the-output); `severity` is `error`. `location` has the `contract` and `function` when known. `details` holds only the values relevant to the kind: always `result`, then `hostError` (`storage/ExceededLimit`), `key` and `keyContract`, `contractCode` and `errorName`, `causedBy` (`{ contractCode, name, function }`) or `hostMessage`. `cause` and `help` are catalog suggestions. `evidence.source` is `host_diagnostic` (the host error of a matching replay) or `result_code` (the network's result code alone); `outcomeReproduced` says whether the replay matched; `helpVerified` is always `false`; `footprintVerified` (footprint violations only) is `false`, since the key comes from the host's report and is not checked against the footprint. |
| `replay` | The verdict in a line: `{ status, onChain, local, outcomeMatches, divergence }`. `status` is `trap_reproduced`, `failure_reproduced`, `execution_succeeded` or `replay_mismatch`. `outcomeMatches` compares the result codes only; the run can still diverge in its error, fee or events, which `status` and `divergence` report. |
| `fidelity.match`, `fidelity.divergence` | Whether the replay matched, and if not, how it diverged: `result`, `error`, `fee` or `events`. |
| `probableCauses` | On a divergence, the evidence that probably caused it, strongest first: `{ kind, subject, detail }` with `kind` one of `entry_drifted`, `setting_drifted`, `call_divergence`, `entry_ttl_drifted`, `entry_archived`, `entry_drift_unknown`, `setting_drift_unknown`, `entry_ext_missing`, `prng_seed_fixed`. Empty on a match, and when nothing points anywhere. |
| `fee` | The non-refundable resource fee the network recorded and the one the replay computes, in stroops (`onchainNonRefundable`, `localNonRefundable`), plus the refundable fee and rent the network kept. |
| `result.onchain`, `result.local` | Each side's result, as `{ code, resultHash }`. |
| `failure` | The error that ended it, with its contract, function and depth; `null` when there is none. `failure.cause` is the contract error a host error answered, when there is one. |
| `calls` | The call tree, one entry per call, with arguments, return value and logs. `args` and `returned` are shortened for reading; `argsXdr` and `returnedXdr` hold them whole, as base64 `ScVal` XDR. |
| `trust` | `verified` is `true` when every input was verified and everything the network recorded was compared. Otherwise `caveats` lists what was not, as `{ kind, subject, detail }`: see [Limitations](#limitations). |
| `budget`, `footprint`, `prngSeed` | Resource use, the footprint and its drift, and how the random seed was obtained. |
| `footprint.missingEntries`, `footprint.entryStatus` | The keys of the entries the host was handed none for (`footprint.missing` is their count), and every entry that could not be rebuilt, is missing, or has unknown drift or size, each with a stable `reason`: `{ entry, status, reason, detail }`. `status` is `unresolved`, `missing`, `drift_unknown` or `ext_missing`; `reason` is `no_apply_order`, `metadata_unavailable`, `too_many_changes`, `metadata_unreadable`, `absent`, `expired`, `no_modification_ledger` or `no_extension`. `-v` prints them. |
| `diagnostics` | On-chain and local diagnostic events, one line each (`onchain`, `local`) and as base64 `DiagnosticEvent` XDR (`onchainXdr`, `localXdr`). |

Enum-like values are lower `snake_case` everywhere (`"status": "failed"`,
`"errorType": "wasm_vm"`, `"reason": "extended_since"`). Exit codes are the
same as for the text output, so a script can branch on the code and dig into the
document for details.

Even when the replay can't run (exit `1`), `--json` still prints a document,
next to the message on stderr:

```json
{
  "schemaVersion": 1,
  "error": { "kind": "not_found", "stage": "fetch", "message": "transaction … not found on testnet. …" }
}
```

`stage` says which part of the run failed: `input`, `fetch`, `decode`,
`reconstruct`, `replay` or `output`; on stderr the message carries it as a
prefix (`sororewind: [fetch] …`). `kind` is stable: `not_found`, `not_a_failure`, `not_soroban`,
`never_reached_host`, `protocol_mismatch`, `network_mismatch`,
`ledger_unavailable`, `rpc_unreachable` (also when `--deadline` passes),
`rpc_error`, `invalid_option` (a bad `SOROREWIND_*` variable), plus a few for
malformed responses and fixtures. Invalid arguments are reported by the argument parser,
as text.

## Why a replay can diverge

A divergence (exit `2`) means the local run didn't reproduce what the network
recorded. Here is what can cause it.

**What is compared, and what is not.** The verdict is not only the result
code. Each signal the network records is either compared (a mismatch is a
divergence) or shown for information:

| Signal | Treatment |
|--------|-----------|
| Result and error | Compared (`result`, `error`). |
| Non-refundable resource fee | Compared (`fee`). It depends only on the declared resources, the transaction's size and the network's fee settings, which the replay has, and is priced as stellar-core does for the ledger's protocol. |
| Diagnostic events | Compared (`events`). The replay's events must be the start of the network's: core appends its own after the host's, so the replay's list may be shorter, never different. |
| Refundable fee and rent | Shown, not compared. A failed transaction is refunded all of it, and rent is priced from a moving window (below). |
| CPU and memory | Shown, not compared. The network's metrics include work core does around the host, so a faithful replay reports slightly different figures (up to about 1.5 % in the recorded fixtures). |
| Ledger changes and contract events | Not compared. A failed transaction commits neither, so there is nothing on the network to compare with. A replay that succeeds where the network failed is already a result divergence. |

**The state moved on and couldn't be rebuilt.** An RPC only serves the
*current* value of a ledger entry, and busy contracts (price oracles, say)
rewrite theirs every few ledgers. So sororewind digs through the ledger
metadata the RPC keeps to recover what each entry looked like when your
transaction ran. Rebuilt entries are listed with `-v` (`footprint … N
rebuilt`; `footprint.rebuilt` in JSON), and they are why such contracts still
replay as a match.

It has limits. An entry stays `drifted`, with the reason, when:

- the metadata is past the RPC's retention window (about a week), or the RPC
  keeps none;
- the entry's history is too long to follow (more than 24 earlier writes
  back);
- the change was a TTL extension or a deletion more than 16 ledgers after
  the transaction. Those 16 ledgers are fetched together, in about one round
  trip;
- it is a network setting: those are not rebuilt.

Metadata is only downloaded when current state can't prove nothing changed,
so a classic-only footprint costs nothing extra.

**Rent priced from a moving window.** The checks stellar-core runs around the
host are mirrored: archived entries, per-entry size limits, disk-read and
write bytes, events size, and the refundable fee (`insufficient refundable
fee`). The rent part of that fee depends on the network's state-size window,
which is resampled every few minutes, and the replay uses the current one. It
only matters within a few stroops of the limit; when it does, the report lists
`LiveSorobanStateSizeWindow` among the changed settings.

**Unknown apply order.** The host's random seed is derived the way the network
derives it, from the ledger's transaction set and the transaction's position
in it. If the RPC doesn't report that position, a fixed seed is used (`prng
seed fixed (apply order unknown)`; `"prngSeed": "fixed"` in JSON), and a
contract that draws random numbers may diverge.

### When it can't replay at all

Some transactions are out of reach. sororewind exits `1` and says why:

- **Too old.** The RPC no longer has the transaction, or the ledger it ran in.
- **Another protocol.** A Soroban host only executes ledgers from its own
  protocol, and a build carries the previous and current ones
  (`sororewind --version`). Anything else is refused, and the error names the
  host version you'd need.
- **Failed before the host.** If stellar-core rejected it during validation
  or fee processing, no contract code ran, so there is nothing to replay.

sororewind replays and explains. It doesn't suggest fixes or profile
contracts: that part is up to you.

## Limitations

What a replay can and cannot tell you, in one place. The details are in the
sections linked.

**What a match guarantees.** `✓ replay matches network` means the same
Soroban host, fed the state sororewind rebuilt, ended with the same result
and error the network recorded, charged the same non-refundable resource fee
and left the same start of diagnostic events. It does not mean the state was
the one the transaction saw: entries are rebuilt from ledger metadata where
possible, and otherwise replayed as they are now.

**When a match rests on weak ground.** The report says so, under the verdict
(`⚠ the match rests on state that could not be verified`) and in `trust` in
JSON, when the result depends on:

- an entry that changed since the transaction and could not be rebuilt, or
  whose last change the RPC did not report;
- a network setting that changed since, or whose change time is unknown;
- an archived entry whose archived state is taken from the network's result;
- a classic entry the RPC returned without its extension;
- a random seed made up because the apply order was unknown;
- a network that kept no diagnostics or fee breakdown, so the error, the call
  trace or the fee were not compared.

**What is not compared.** CPU, memory, rent and ledger changes are not part
of the verdict ([why](#why-a-replay-can-diverge)).

**Which transactions it can replay.** Only failures that reached the host, on
a protocol one of the embedded hosts executes, still inside the RPC's
retention window ([which protocols](#which-transactions-it-can-replay),
[when it can't replay at all](#when-it-cant-replay-at-all)).

**Why a replay can diverge.** The state moved on and could not be rebuilt,
rent is priced from a moving window, or the apply order is unknown: see
[Why a replay can diverge](#why-a-replay-can-diverge). A divergence lists its
probable causes.

sororewind replays and explains. It doesn't suggest fixes or profile
contracts.

## Contributing

Setup, the checks a pull request must pass, and the protocol-upgrade checklist
live in [CONTRIBUTING.md](CONTRIBUTING.md). With [`just`](https://github.com/casey/just)
installed, `just check` runs everything CI enforces. By hand:

```bash
cargo build
```

```bash
cargo test
```

```bash
cargo test --release --locked --test live -- --ignored --nocapture
```

`cargo test` is fully offline and takes seconds: unit tests plus a snapshot of
the narrative for every fixture. The last command is the live test CI runs
nightly: it finds recent failed testnet transactions and checks that they
replay quickly and that at least one matches.

CI also runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
the tests on Linux, macOS and Windows, a snapshot check that rejects pending
or unreferenced snapshots, `cargo doc` with warnings as errors, a `cargo
check` on the minimum supported Rust (1.89), and `cargo deny check`.

See the [diagnostic integrations RFC](docs/diagnostic-integrations-rfc.md)
for proposals around evidence-grounded explanations, editor integration
and a diagnostic API.

### Embedding and benchmarking

Run the offline Rust embedding example with
`cargo run --example embed -- fixtures/contract-error`. The
[API documentation](https://docs.rs/sororewind) covers the supported
library interface.

Once Cargo has downloaded the locked dependencies, `just bench` measures
decode, reconstruction and replay using the recorded fixtures and prints
the number of RPC calls for each full replay. In CI, Criterion compares the
pull request against the base revision on the same runner and reports
regressions above its 15% noise threshold without blocking the PR.

### How the code is organised

`src/pipeline.rs` runs the stages in order; `src/lib.rs` maps the outcome to
an exit code.

```
fetch ─► decode ─► reconstruct ─► replay ─► narrate ─► report
```

| Stage | Module | What it does |
|-------|--------|--------------|
| Fetch | `rpc.rs`, `transport.rs` | Talk to the RPC, or answer from a fixture. |
| Decode | `tx.rs` | Read the envelope, footprint and on-chain result. |
| Reconstruct | `state.rs`, `history.rs` | Rebuild ledger entries, TTLs, network settings and the ledger header; recover the value an entry had at the transaction from later ledger metadata. |
| Replay | `replay.rs`, `host/` | Run the host for the ledger's protocol and compare its result with the network's. |
| Narrate | `story.rs`, `spec.rs` | Turn diagnostic events into a call tree and a failure; read error names from the contract's spec. |
| Diagnose | `findings.rs`, `catalog.rs`, `diagnostic.rs` | Rank the report's evidence, match known failures, and classify the primary failure under a stable `SRW` code. |
| Report | `report.rs`, `render.rs` | Hold the outcome as data (the JSON schema), and print the compact diagnostic, the narrative or the document. |

`src/host/` embeds one `soroban-env-host` per supported protocol (`p29.rs`,
`p30.rs`). The host-facing code is written once, in `host/proto.rs`, and
compiled against each host; only XDR bytes and plain structs cross that
boundary, and the rest of the crate uses the `stellar-xdr` of the newest host.
Adding or dropping a protocol follows "Protocol upgrades" in
[CONTRIBUTING.md](CONTRIBUTING.md).

`discover.rs` finds recent failed transactions on a live network, for the
fixture tooling and the live test.

### Fixtures

A fixture is a complete offline recording of every RPC response one replay
needs. Each directory under `fixtures/` holds one failed transaction, from
testnet unless its name says otherwise (`mainnet-auth-error`).
`tests/narrative.rs` pins its narrative and exit code and checks the main JSON
fields. The layout is documented at the top of `src/transport.rs`.

Fixtures must replay as a match, so record them soon after the transaction
ran. First, list recent failures; the first column is the category, which is
also the usual fixture name:

```bash
cargo run --example find_failed
```

Then record one. An existing name refreshes that fixture, a new name adds one:

```bash
cargo run --example record_fixture -- contract-error <TX_HASH>
```

With `just`, the same two steps are `just find` and `just record
contract-error <TX_HASH>`.

`record_fixture` keeps a recording only if it replays as a match
(`--allow-diverged` keeps a divergence on purpose), and never replaces a
working fixture with a broken one. Without arguments it re-records every
fixture from the hash and network it was made for, which only works while the
RPC still has those transactions.

A new fixture also needs a line in the `fixtures!` list in
`tests/narrative.rs` and an accepted snapshot: run `cargo insta review` (`just
snap`), after `cargo install cargo-insta`.
Add its command, original transaction and snapshot output to the
[offline example gallery](docs/examples.md).

Each fixture comes from a `find_failed` category:

| Fixture | How to find one |
|---------|-----------------|
| `auth-error`, `budget-error`, `contract-error`, `storage-error` | A transaction of that category. For `budget-error`, pick one whose result is `resource limit exceeded`. |
| `refundable-fee` | A `budget-error` whose result is `insufficient refundable fee`. |
| `nested-contract-error` | A `contract-error` raised by a WASM contract (the report names it: `contract error #17 · Limit`) whose call tree spans several contracts and levels. |
| `rebuilt-entry` | A failure of a contract that rewrites its entries every few ledgers (an oracle's `write_prices`, for example) whose replay reports `N rebuilt`. |
| `auth-check-auth` | An `auth-error` whose call tree, from `sororewind <TX_HASH>`, shows a `__check_auth` call: a custom account contract rejected the signature. |
| `mainnet-*` | A mainnet transaction, recorded with `--network mainnet`, that keeps the older embedded host covered while mainnet runs the previous protocol. |

Recorded fixtures work offline forever. Re-record them when:

- a protocol is dropped from the embedded hosts, since its fixtures no longer
  replay (when a host is added, record fixtures for the new protocol and keep
  the others until their host goes);
- the fixture layout changes, which bumps `FIXTURE_VERSION` in
  `src/transport.rs`, and old fixtures fail with "re-record it";
- testnet is reset.

In every case the old hashes are gone from the RPC, so record fresh
transactions found with `find_failed` rather than running `record_fixture`
without arguments.

Error paths no recording covers are tested by rewriting a fixture's RPC
responses in memory (`common::Overlay`): see `tests/errors.rs` for every error
a user can hit, and `tests/scenarios.rs` for fee-bumps, archived entries and
network mismatches.

## License

[Apache-2.0](LICENSE)

# sororewind

[![CI](https://github.com/Dione-b/sororewind/actions/workflows/ci.yml/badge.svg)](https://github.com/Dione-b/sororewind/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Dione-b/sororewind?include_prereleases)](https://github.com/Dione-b/sororewind/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

**Rewind a failed Soroban transaction, run it again on your machine, and see
exactly where it broke.**

```
$ sororewind --fixture fixtures/contract-error
sororewind  dd1f9b…53db  testnet
ledger 5672381 · protocol 30 · 2026-11-11 08:25:48 UTC

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  contract error #10 · BalanceError
  "resulting balance is not within the allowed range [10000000, -15000000100, 9223372036854775807]"
  in CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer (depth 0)

call tree
CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer(GB2YUAICRUQNAHAEAYDA7SS4SGQTECKKCU6PQ7BX32ZRX67VBH7MSCWC, GBZH7YHAQYRG3MQFKFZDOGIKEEYGZ6LJNCEHYOA7K7BDLJGLQI6R3RAG, 20000000000)  ✗
```

## Why

Your transaction failed. The network hands you a result code, a wall of
diagnostic events, and good luck.

sororewind gives you a story instead. Hand it a hash and it:

1. **Fetches** the transaction from a Soroban RPC.
2. **Rebuilds** the world it ran in: footprint entries, TTLs, network cost
   settings and the ledger header, as they were *at that moment*, not as they
   are now.
3. **Replays** it with the same `soroban-env-host` the network ran.
4. **Tells you** which call failed, with what error, and how it got there.

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

Prefer building it yourself? You need Rust 1.89 or newer (sororewind is not on
crates.io yet):

```bash
cargo install --locked --git https://github.com/Dione-b/sororewind
```

Don't drop `--locked`. The embedded hosts are tested against exact dependency
versions, and a different version can change what a host charges for a call,
which is enough to turn a match into a divergence.

### Which transactions it can replay

Each build embeds two Soroban hosts: right now protocols **29** and **30**.
That covers testnet and mainnet through an upgrade, when testnet runs the new
protocol weeks before mainnet does. `sororewind --version` tells you what your
build supports.

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

No network handy? Clone the repo and replay one of the recorded fixtures:

```bash
git clone https://github.com/Dione-b/sororewind && cd sororewind
```

```bash
cargo run -- --fixture fixtures/contract-error
```

A few things worth knowing:

- **It only replays failures.** The transaction must have failed inside the
  Soroban host.
- **RPCs forget.** They typically keep about a week of history. When a hash is
  too old, the error tells you the window the RPC still covers.
- **API keys stay private.** If your RPC URL carries a key, pass it through
  `SOROREWIND_RPC_URL` so it stays out of your shell history and process list.
  Either way, sororewind only ever prints the scheme and host
  (`https://rpc.provider.io/…`).
- **Flaky RPCs are expected.** Connection errors, timeouts and HTTP 429, 502,
  503 and 504 are retried a few times before giving up. Responses are
  requested gzip-compressed, which keeps the ledger metadata it downloads small.

## Reading the output

Every report has a header, a verdict and a call tree. The rest shows up when
it has something to say, or when you ask for more.

| Section | What it tells you |
|---------|-------------------|
| **Header** | The hash, the network, and the ledger it ran in, with protocol and close time. |
| **Verdict** | `✓ replay matches network`: trust everything below. `⚠ replay diverged`: the local run disagrees, and the line says how (another result code, or the same code with another error). A separate warning appears if the network's settings changed since. |
| **Where it broke** | The error that ended it: its type (contract, auth, storage, budget, …), the contract's own error name when it declares one, the logged message, and the function and call depth that raised it. |
| **Call tree** | Every contract call, in order, with decoded arguments. Nested calls hang off `├─` / `└─`, `✗` marks the failing call, and `·` lines are the contract's logs. `(recovered by try_call)` marks a failure the caller caught and moved past; `(no return)` marks a call the transaction died inside. |
| **Footprint** | How many entries were touched, which changed since (value, TTL, archived), how many were missing, and whether the random seed was derived or fixed. Shown on a divergence, and always with `-v`. |

Want more?

- **`-v`** adds return values, the CPU and memory budget (local against the
  limits, and what the network used) and the full footprint. Debug lines go to
  stderr, tagged by source: `[spec]`, `[replay]`, `[rpc]`, `[record]`.
- **`-vv`** also dumps the raw on-chain and local diagnostic events and traces
  every RPC request and response (`[rpc]`, or `[fixture]` offline).

The report always goes to stdout, the debug lines to stderr. To keep
everything: `sororewind <TX_HASH> -vv 2>&1 | tee run.log`.

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
| `--rpc-url <URL>` | Use this RPC instead. Must be `http://` or `https://`; `SOROREWIND_RPC_URL` sets it from the environment. |
| `--json` | Print a JSON document instead of the narrative. |
| `--wasm-debug <PATH>` | For a contract error, read the error names from a local WASM file instead of the ledger. |
| `--color <auto\|always\|never>` | `auto` (default) colours a terminal and honours a non-empty `NO_COLOR`, `CLICOLOR_FORCE` and `TERM=dumb`. Use `always` in a pager: `sororewind … --color always \| less -R`. |
| `--no-color` | Same as `--color never`. |
| `-v, --verbose` | More detail; repeat for even more (`-vv`). |
| `--fixture <DIR>` | Replay offline from a recorded fixture. The hash is optional (the fixture knows it); a different one is an error. Not combinable with `--rpc-url`. |
| `--completions <SHELL>` | Print a completion script for `bash`, `zsh`, `fish`, `elvish` or `powershell`, e.g. `sororewind --completions fish > ~/.config/fish/completions/sororewind.fish`. |
| `--man` | Print a man page, e.g. `sororewind --man > ~/.local/share/man/man1/sororewind.1`. |
| `-V, --version` | Version, supported protocols, and the commit and target it was built from. Paste it into bug reports. |

Default RPCs: `https://soroban-testnet.stellar.org` (testnet),
`https://mainnet.sorobanrpc.com` (mainnet, third-party),
`https://rpc-futurenet.stellar.org` (futurenet) and
`http://localhost:8000/rpc` (local).

Pass `--network` explicitly and sororewind checks it against the passphrase
the RPC reports. If they disagree, it stops rather than print a report with
the wrong label on it.

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
| `fidelity.match`, `fidelity.divergence` | Whether the replay matched, and if not, how it diverged. |
| `result.onchain`, `result.local` | Each side's result, as `{ code, resultHash }`. |
| `failure` | The error that ended it, with its contract, function and depth; `null` when there is none. `failure.cause` is the contract error a host error answered, when there is one. |
| `calls` | The call tree, one entry per call, with arguments, return value and logs. `args` and `returned` are shortened for reading; `argsXdr` and `returnedXdr` hold them whole, as base64 `ScVal` XDR. |
| `budget`, `footprint`, `prngSeed` | Resource use, the footprint and its drift, and how the random seed was obtained. |
| `diagnostics` | On-chain and local diagnostic events, one line each (`onchain`, `local`) and as base64 `DiagnosticEvent` XDR (`onchainXdr`, `localXdr`). |

Enum-like values are lower `snake_case` everywhere (`"status": "failed"`,
`"errorType": "wasm_vm"`, `"reason": "extended_since"`). Exit codes are the
same as for the narrative, so a script can branch on the code and dig into the
document for details.

Even when the replay can't run (exit `1`), `--json` still prints a document,
next to the message on stderr:

```json
{
  "schemaVersion": 1,
  "error": { "kind": "not_found", "message": "transaction … not found on testnet. …" }
}
```

`kind` is stable: `not_found`, `not_a_failure`, `not_soroban`,
`never_reached_host`, `protocol_mismatch`, `network_mismatch`,
`ledger_unavailable`, `rpc_unreachable`, `rpc_error`, plus a few for malformed
responses and fixtures. Invalid arguments are reported by the argument parser,
as text.

## Why a replay can diverge

A divergence (exit `2`) means the local run didn't reproduce what the network
recorded. Here is what can cause it.

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
| Report | `report.rs`, `render.rs` | Hold the outcome as data (the JSON schema), and print the narrative or the document. |

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

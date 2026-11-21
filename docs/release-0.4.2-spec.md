# Release preparation spec: 0.4.2

## Objective

Prepare the merged sororewind changes since `0.4.1` for a `0.4.2` release.
The release fixes false divergences on some `ENTRY_ARCHIVED` transactions
(PR #51). An entry the transaction listed for restoration that is live now,
with a TTL at least what a restoration in the transaction's ledger would set,
was assumed archived back then, although an extension before the
transaction explains the same TTL. It is now replayed as live and reported
as unproven. This is a bug fix whose only interface change is an additive
JSON enum value, so for this pre-1.0 crate `0.4.2` is the next patch
version.

## Scope

- Set the package version in `Cargo.toml` and the root package entry in
  `Cargo.lock` to `0.4.2`.
- Move the accumulated `Unreleased` changes to a dated `0.4.2` changelog
  section and leave an empty `Unreleased` heading for later work.
- Keep the existing GitHub release workflow as the binary release mechanism;
  it creates a GitHub Release from a version tag after required checks pass.
- Document this release decision and the publication gates in this spec.

## Compatibility notes

- `footprint.ttlDrifted[].reason` gains the value
  `restored_or_extended_since`. `restored_since` is no longer reported but
  remains a valid value, so `schemaVersion` stays `1`.
- A transaction affected by the fix now exits `0` (matched) where `0.4.1`
  exited `2` (diverged); its replay may report one more unverified input.
- The default text output, Markdown, HTML, `watch` output and the exit-code
  meanings are otherwise unchanged.

## Acceptance criteria

- Manifest, lockfile, and changelog identify version `0.4.2` consistently.
- `cargo package --locked` succeeds, verifying package contents and build.
- CI passes for the release preparation commit on the default branch before
  the release tag is created.
- The version tag is created only after explicit approval of the prepared
  release. The release workflow does not publish the crate to crates.io;
  `0.4.1` is published there, so `cargo publish --locked` is run by hand
  from the tagged commit after the GitHub Release succeeds.

## Current execution

- The fix was merged to `master` in PR #51 and checked there with
  `just check`, a new scenario test, `just live`, and live replays of the
  two testnet transactions that exposed it.
- Version and changelog are prepared on the `prepare-release-0.4.2` branch,
  based on `master`; the release tag has not been created.
- CI must pass on the merged release-preparation commit before a tag is
  created.

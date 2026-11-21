# Release preparation spec: 0.4.1

## Objective

Prepare the merged sororewind changes since `0.4.0` for a `0.4.1` release.
The release fixes `--what-if-protocol 29` on protocol-30 transactions, which
reported `unavailable` with a budget `InternalError` because the protocol-29
host rejected the network's longer cost-parameter list (PR #49). It is a bug
fix with no interface change, so for this pre-1.0 crate `0.4.1` is the next
patch version.

## Scope

- Set the package version in `Cargo.toml` and the root package entry in
  `Cargo.lock` to `0.4.1`.
- Move the accumulated `Unreleased` changes to a dated `0.4.1` changelog
  section and leave an empty `Unreleased` heading for later work.
- Keep the existing GitHub release workflow as the binary release mechanism;
  it creates a GitHub Release from a version tag after required checks pass.
- Document this release decision and the publication gates in this spec.

## Compatibility notes

- The faithful replay, the default text output, the JSON document
  (`schemaVersion` stays `1`), Markdown, HTML, `watch` output and the exit
  codes are unchanged.
- Only the `whatIf` block of a report changes: for a protocol-30
  transaction with `--what-if-protocol 29` it is now `compared` instead of
  `unavailable`.

## Acceptance criteria

- Manifest, lockfile, and changelog identify version `0.4.1` consistently.
- `cargo package --locked` succeeds, verifying package contents and build.
- CI passes for the release preparation commit on the default branch before
  the release tag is created.
- The version tag is created only after explicit approval of the prepared
  release. This repository workflow does not publish the crate to crates.io;
  crates.io publication, if intended, is a separate manual action.

## Current execution

- The fix was merged to `master` in PR #49 and checked there with
  `just check`, a CLI run of every fixture and mode, and `just live`.
- Version and changelog are prepared on the `prepare-release-0.4.1` branch,
  based on `master`; the release tag has not been created.
- CI must pass on the merged release-preparation commit before a tag is
  created.

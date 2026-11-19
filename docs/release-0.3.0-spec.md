# Release preparation spec: 0.3.0

## Objective

Prepare the merged sororewind changes since `0.2.0` for a `0.3.0` release.
The release contains additive capabilities delivered in P2/P3 and the P4
compatibility and feedback documentation. For this pre-1.0 crate, `0.3.0` is
the next minor version for the accumulated feature additions.

## Scope

- Set the package version in `Cargo.toml` and the root package entry in
  `Cargo.lock` to `0.3.0`.
- Move the accumulated `Unreleased` additions to a dated `0.3.0` changelog
  section and leave an empty `Unreleased` heading for later work.
- Keep the existing GitHub release workflow as the binary release mechanism;
  it creates a GitHub Release from a version tag after required checks pass.
- Document this release decision and the publication gates in this spec.

## Acceptance criteria

- Manifest, lockfile, and changelog identify version `0.3.0` consistently.
- `cargo package --locked` succeeds, verifying package contents and build.
- CI passes for the release preparation commit on the default branch before
  the release tag is created.
- The version tag is created only after explicit approval of the prepared
  release. This repository workflow does not publish the crate to crates.io;
  crates.io publication, if intended, is a separate manual action.

## Current execution

- Version and changelog are prepared on the `prepare-release-0.3.0` branch; the
  release tag has not been created.
- `cargo package --locked --allow-dirty` passed and packaged 63 files.
- CI must pass on the merged release-preparation commit before a tag is
  created.

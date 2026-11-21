# Release preparation spec: 0.4.0

## Objective

Prepare the merged sororewind changes since `0.3.0` for a `0.4.0` release.
The release makes the default text output a compact, compiler-style
diagnostic with stable `SRW` codes, moves the previous narrative behind
`--trace`, and adds `diagnostic` and `replay` to the JSON document. The
default output changes and `render::Detail` gains a variant, so for this
pre-1.0 crate `0.4.0` is the next minor version.

## Scope

- Set the package version in `Cargo.toml` and the root package entry in
  `Cargo.lock` to `0.4.0`, and mention the compiler-style diagnostics in the
  package description.
- Move the accumulated `Unreleased` changes to a dated `0.4.0` changelog
  section and leave an empty `Unreleased` heading for later work.
- Keep the existing GitHub release workflow as the binary release mechanism;
  it creates a GitHub Release from a version tag after required checks pass.
- Document this release decision and the publication gates in this spec.

## Compatibility notes

- Scripts that parse the default text output must switch to `--json` or
  `--trace`; the `--trace` narrative is the previous default unchanged.
- The JSON changes are additive: `schemaVersion` stays `1`.
- Exit codes, Markdown, HTML and `watch` output are unchanged.

## Acceptance criteria

- Manifest, lockfile, and changelog identify version `0.4.0` consistently.
- `cargo package --locked` succeeds, verifying package contents and build.
- The compiler-style diagnostics change (PR #46) is merged before this
  preparation lands on the default branch.
- CI passes for the release preparation commit on the default branch before
  the release tag is created.
- The version tag is created only after explicit approval of the prepared
  release. This repository workflow does not publish the crate to crates.io;
  crates.io publication, if intended, is a separate manual action.

## Current execution

- Version and changelog are prepared on the `prepare-release-0.4.0` branch,
  based on `compiler-style-diagnostics`; the release tag has not been
  created.
- CI must pass on the merged release-preparation commit before a tag is
  created.

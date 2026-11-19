#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

[[ "$REPLAY_VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]] ||
  fail "version must be an exact sororewind release tag"
[[ "$RUNNER_OS" == Linux ]] || fail "the sororewind Action requires a Linux runner"
case "$RUNNER_ARCH" in
  X64) target=x86_64-unknown-linux-gnu ;;
  ARM64) target=aarch64-unknown-linux-gnu ;;
  *) fail "the sororewind Action supports Linux X64 and ARM64" ;;
esac

# Validate before downloading, and use the checked spelling only for file names.
hashes=()
while IFS= read -r line; do
  read -ra words <<< "$line"
  hashes+=("${words[@]}")
done <<< "$REPLAY_HASHES"
if [[ -n "$REPLAY_FIXTURE" ]]; then
  [[ "${#hashes[@]}" == 0 ]] || fail "use either hashes or a fixture"
  fixture=$(realpath -- "$GITHUB_WORKSPACE/$REPLAY_FIXTURE")
  [[ "$fixture" == "$GITHUB_WORKSPACE/"* && -f "$fixture/fixture.json" ]] ||
    fail "fixture must point to an offline recording inside the workspace"
else
  [[ "${#hashes[@]}" != 0 ]] || fail "supply at least one transaction hash"
  for hash in "${hashes[@]}"; do
    [[ "$hash" =~ ^(0x)?[a-fA-F0-9]{64}$ ]] || fail "invalid transaction hash"
  done
fi

work=$(mktemp -d "$RUNNER_TEMP/sororewind.XXXXXXXX")
cd "$work"
archive="sororewind-$target.tar.xz"
url="https://github.com/Dione-b/sororewind/releases/download/$REPLAY_VERSION"
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 -O "$url/$archive" -O "$url/$archive.sha256"
read -r checksum _ < "$archive.sha256"
[[ "$checksum" =~ ^[a-fA-F0-9]{64}$ ]] || fail "invalid release checksum"
printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
mkdir bin reports
tar -xJf "$archive" -C bin --strip-components=1
binary="$work/bin/sororewind"
read -r tool version _ <<< "$("$binary" --version)"
[[ "$tool" == sororewind && "$version" == "${REPLAY_VERSION#v}" ]] ||
  fail "the downloaded binary does not match the requested release"
printf 'reports=%s/reports\n' "$work" >> "$GITHUB_OUTPUT"

# Keep step summaries bounded. Full structured reports are in the artifact.
{
  printf '## Soroban replay\n\n'
  printf '| Transaction | Replay |\n| --- | --- |\n'
} >> "$GITHUB_STEP_SUMMARY"

exit_status=0
run_replay() {
  local label=$1 code=0 verdict
  shift
  "$binary" "$@" --json > "reports/$label.json" || code=$?
  case "$code" in
    0) verdict=matched ;;
    2) verdict=diverged ;;
    *) verdict=error; code=1 ;;
  esac
  if [[ ! -s "reports/$label.json" ]]; then
    printf '{"error":{"kind":"no_report","message":"The CLI emitted no JSON report."}}\n' > "reports/$label.json"
  fi
  printf '| `%s` | %s |\n' "$label" "$verdict" >> "$GITHUB_STEP_SUMMARY"
  if [[ "$code" == 1 || "$exit_status" == 0 ]]; then
    exit_status=$code
  fi
}

export XDG_CONFIG_HOME="$work/config"
export XDG_CACHE_HOME="$work/cache"
unset SOROREWIND_NETWORK SOROREWIND_FORMAT SOROREWIND_COLOR
if [[ -n "$REPLAY_FIXTURE" ]]; then
  run_replay fixture --fixture "$fixture"
else
  for hash in "${hashes[@]}"; do
    run_replay "${hash#0x}" "$hash" --network "$REPLAY_NETWORK"
  done
fi
exit "$exit_status"

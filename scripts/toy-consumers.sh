#!/usr/bin/env bash
# Build and run the toy-consumer probe crates (bd:JMAP-698j.3, decision 51).
#
# Each probe under docs/toy-consumers/<name>/ is a throwaway binary crate
# that uses only the public API of the workspace crates: no #[doc(hidden)]
# items, no `test-helpers` features, no `*_for_test` functions. Its
# Cargo.toml names workspace crates as
#   jmap-foo = { path = "@WORKSPACE@/crate-jmap-foo" }
# and this script scaffolds a fresh crate outside the workspace (cargo new),
# copies the probe's Cargo.toml (with @WORKSPACE@ replaced) and src/ over
# it, seeds Cargo.lock from the workspace's, then builds it, runs it and
# runs clippy on it. A probe that runs offline asserts what it exercised
# and exits non-zero on failure.
#
# Usage: scripts/toy-consumers.sh [-o OUT_DIR] [PROBE...]
#   OUT_DIR  where the scaffolded crates go (default: $TOY_CONSUMERS_DIR,
#            else a fresh mktemp dir); it must be outside the workspace.
#   PROBE    probe names (directory names under docs/toy-consumers/);
#            default: all of them.
# The probes share one target dir, $TOY_CONSUMERS_TARGET_DIR (default:
# OUT_DIR/target).
#
# See AGENTS.md "End of each feature wave: toy-consumer check".
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/docs/toy-consumers"

out="${TOY_CONSUMERS_DIR:-}"
while getopts "o:" opt; do
  case $opt in
    o) out=$OPTARG ;;
    *) echo "usage: $0 [-o OUT_DIR] [PROBE...]" >&2; exit 2 ;;
  esac
done
shift $((OPTIND - 1))
if [ -z "$out" ]; then
  out=$(mktemp -d "${TMPDIR:-/tmp}/toy-consumers.XXXXXX")
fi
mkdir -p "$out"
out=$(cd "$out" && pwd)
case "$out/" in
  "$root"/*) echo "toy-consumers.sh: OUT_DIR must be outside $root" >&2; exit 2 ;;
esac
export CARGO_TARGET_DIR="${TOY_CONSUMERS_TARGET_DIR:-$out/target}"

if [ $# -eq 0 ]; then
  set -- $(cd "$src" && ls -d */ | tr -d /)
fi

failed=()
for name in "$@"; do
  probe="$src/$name"
  if [ ! -f "$probe/Cargo.toml" ]; then
    echo "toy-consumers.sh: no probe $probe" >&2
    exit 2
  fi
  dest="$out/$name"
  rm -rf "$dest"
  cargo new --quiet --vcs none --bin --name "$name" "$dest"
  rm -rf "$dest/src"
  cp -rf "$probe/src" "$dest/src"
  sed "s|@WORKSPACE@|$root|g" "$probe/Cargo.toml" > "$dest/Cargo.toml"
  cp -f "$root/Cargo.lock" "$dest/Cargo.lock"
  echo "== $name ($dest)"
  if (cd "$dest" && cargo build --quiet && cargo run --quiet \
      && cargo clippy --quiet -- -D warnings); then
    echo "== $name: ok"
  else
    echo "== $name: FAILED"
    failed+=("$name")
  fi
done

if [ ${#failed[@]} -ne 0 ]; then
  echo "toy-consumers.sh: failed: ${failed[*]}" >&2
  exit 1
fi
echo "toy-consumers.sh: all probes built and ran ($out)"

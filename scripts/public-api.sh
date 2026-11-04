#!/usr/bin/env bash
# Regenerate public-api/<crate>.txt for every publishable workspace crate
# (bd:JMAP-698j.1). CI runs this and fails on any diff; see AGENTS.md
# "Public API snapshots" for how to bless an intended change.
#
# Needs cargo-public-api 0.52.0 and the nightly below; rustdoc JSON is
# nightly-only. Keep both pins in step with .github/workflows/ci.yml
# (job `public-api`) and regenerate the snapshots when bumping them.
#
# Builds in a private target dir (override with PUBLIC_API_TARGET_DIR) so a
# shared global target-dir held by another cargo cannot break the rustdoc
# JSON step, and swaps the snapshots in only after every crate succeeded, so
# a failed run leaves public-api/ untouched (bd:JMAP-o0bo).
set -euo pipefail
toolchain=nightly-2026-09-13
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${PUBLIC_API_TARGET_DIR:-$PWD/target/public-api}"
crates=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.publish != []) | .name' \
  | sort)
if [ -z "$crates" ]; then
  echo "public-api.sh: no publishable crates found" >&2
  exit 1
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for crate in $crates; do
  cargo +"$toolchain" public-api -p "$crate" --all-features -ss > "$tmp/$crate.txt"
done
mkdir -p public-api
rm -f public-api/*.txt
mv "$tmp"/*.txt public-api/

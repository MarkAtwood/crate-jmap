#!/usr/bin/env bash
# Regenerate public-api/<crate>.txt for every publishable workspace crate
# (bd:JMAP-698j.1). CI runs this and fails on any diff; see AGENTS.md
# "Public API snapshots" for how to bless an intended change.
#
# Needs cargo-public-api 0.52.0 and the nightly below; rustdoc JSON is
# nightly-only. Keep both pins in step with .github/workflows/ci.yml
# (job `public-api`) and regenerate the snapshots when bumping them.
set -euo pipefail
toolchain=nightly-2026-09-13
cd "$(dirname "$0")/.."
rm -f public-api/*.txt
mkdir -p public-api
cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.publish != []) | .name' \
  | sort \
  | while read -r crate; do
      cargo +"$toolchain" public-api -p "$crate" --all-features -ss > "public-api/$crate.txt"
    done

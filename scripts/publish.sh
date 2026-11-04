#!/usr/bin/env bash
# Publish every publishable workspace crate at <version> to crates.io,
# retrying through the crates.io rate limit (bd:JMAP-l7sr). Run from a
# clean worktree at the release tag; see RELEASING.md "Publish procedure".
#
# Each attempt checks the crates.io sparse index and runs one
# `cargo publish -p ...` for the crates not yet there (cargo orders them).
# A 429 Too Many Requests sleeps 70s and retries; any other failure stops.
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 <version>" >&2; exit 2; }
version=$1
cd "$(dirname "$0")/.."
log=$(mktemp)
idx=$(mktemp)
trap 'rm -f "$log" "$idx"' EXIT

crates=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.publish != []) | .name' | sort)

for attempt in $(seq 1 60); do
  remaining=()
  for c in $crates; do
    code=$(curl -sS -o "$idx" -w '%{http_code}' \
      "https://index.crates.io/${c:0:2}/${c:2:2}/$c")
    case $code in
      200) grep -qF "\"vers\":\"$version\"" "$idx" || remaining+=("$c") ;;
      404) remaining+=("$c") ;;
      *) echo "index lookup for $c: HTTP $code" >&2; exit 1 ;;
    esac
  done
  echo "attempt $attempt: ${#remaining[@]} remaining $(date -u +%T)"
  [ ${#remaining[@]} -eq 0 ] && { echo "all crates published at $version"; exit 0; }

  args=()
  for c in "${remaining[@]}"; do args+=(-p "$c"); done
  if cargo publish "${args[@]}" 2>&1 | tee "$log"; then continue; fi
  grep -q "429 Too Many Requests" "$log" || { echo "cargo publish failed" >&2; exit 1; }
  sleep 70
done
echo "gave up after $attempt attempts" >&2
exit 1

#!/usr/bin/env bash
# Run the jmap-interop tests against a fresh Stalwart container and keep a
# full log, so a failure can always be traced afterwards (bd:JMAP-cyub.5:
# one run failed with its output lost to a filter).
#
# Usage: scripts/interop-test.sh [extra cargo test args...]
#
# - Starts Stalwart with scripts/stalwart-up.sh (honours STALWART_PORT,
#   STALWART_CONTAINER, STALWART_ENV_FILE) and always stops it on exit.
# - Runs `cargo test -p jmap-interop --no-fail-fast -- --ignored --nocapture`
#   with RUST_BACKTRACE=1, so every test binary runs even after a failure.
# - Writes the complete output, plus the Stalwart version and the git
#   commit, to target/interop/logs/<UTC timestamp>.log (override the
#   directory with INTEROP_LOG_DIR).
# - Prints a summary: passed/failed counts, the name of every failing test
#   and every "known divergence" line, and the log path.
# - Exits non-zero when any test failed.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

log_dir="${INTEROP_LOG_DIR:-target/interop/logs}"
mkdir -p "$log_dir"
log="$log_dir/$(date -u +%Y%m%dT%H%M%SZ).log"
env_file="${STALWART_ENV_FILE:-target/interop/stalwart.env}"

cleanup() { scripts/stalwart-down.sh >>"$log" 2>&1 || true; }
trap cleanup EXIT

{
    echo "# interop run $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "# commit $(git rev-parse --short HEAD)$(git diff --quiet || echo ' (dirty)')"
    sed -n 's/^STALWART_VERSION=/# stalwart /p; s/^STALWART_IMAGE=/# image /p' scripts/stalwart-up.sh
    echo
} >"$log"

scripts/stalwart-up.sh >>"$log" 2>&1
set -a
# shellcheck disable=SC1090
. "$env_file"
set +a

status=0
RUST_BACKTRACE=1 cargo test -p jmap-interop --no-fail-fast "$@" -- --ignored --nocapture \
    >>"$log" 2>&1 || status=$?

passed=$(grep -E '^test result:' "$log" | awk '{p+=$4} END {print p+0}')
failed=$(grep -E '^test result:' "$log" | awk '{f+=$6} END {print f+0}')
echo "interop: $passed passed, $failed failed (cargo exit $status)"
if grep -qE '^test .* FAILED$' "$log"; then
    echo "failing tests:"
    grep -E '^test .* FAILED$' "$log" | sed -E 's/^test (.*) \.\.\. FAILED$/  \1/'
fi
divergences=$(grep -c 'known divergence' "$log" || true)
echo "known divergences reported: $divergences"
echo "full log: $log"
exit "$status"

#!/usr/bin/env bash
# Stop and remove the Stalwart interop container started by
# scripts/stalwart-up.sh (bd:JMAP-cyub.1). Idempotent: succeeds when there
# is nothing to remove.
#
# The container keeps its state on tmpfs mounts, so removing it removes the
# data. `rm -v` also drops any anonymous volume, should one exist.
#
# Usage: scripts/stalwart-down.sh
#   STALWART_CONTAINER  container name (default crate-jmap-interop-stalwart)
#   STALWART_ENV_FILE   env file to delete (default target/interop/stalwart.env)
set -euo pipefail

cd "$(dirname "$0")/.."

name="${STALWART_CONTAINER:-crate-jmap-interop-stalwart}"
env_file="${STALWART_ENV_FILE:-$PWD/target/interop/stalwart.env}"

if podman container exists "$name"; then
  podman rm -f -v -t 5 "$name" >/dev/null
  echo "Removed container $name."
else
  echo "No container $name."
fi
rm -f "$env_file"

#!/usr/bin/env bash
# Start a throwaway Stalwart JMAP server for the opt-in interop tests
# (bd:JMAP-cyub.1, crate-jmap-interop; docs/interop/README.md).
#
# Stalwart is AGPL-licensed. It runs here only as an external test server
# in a container: nothing in this workspace links, vendors or copies it.
#
# What it does, unattended:
#   1. podman run the official image, pinned by digest, published on
#      127.0.0.1 only, with tmpfs for /etc/stalwart and /var/lib/stalwart
#      (no volume: every run starts empty, and nothing survives `down`).
#   2. Writes the one-line config.json (RocksDB data store) inside the
#      container, so Stalwart skips bootstrap mode and starts normally.
#   3. Pins the administrator with STALWART_RECOVERY_ADMIN.
#   4. Creates the domain and two users in the internal directory through
#      Stalwart's JMAP management API (`urn:stalwart:jmap`, `x:Domain/set`,
#      `x:Account/set`). Stalwart creates each user's default mailboxes
#      (Inbox, Drafts, Sent, Junk, Trash) and identity itself.
#   5. Waits until /.well-known/jmap answers a Session for alice, then
#      prints the settings and writes them to an env file.
#
# Usage: scripts/stalwart-up.sh
#   STALWART_PORT       host port on 127.0.0.1 (default 18080)
#   STALWART_CONTAINER  container name (default crate-jmap-interop-stalwart)
#   STALWART_ENV_FILE   env file to write (default target/interop/stalwart.env)
#
# Then: . target/interop/stalwart.env
#       cargo test -p jmap-interop -- --ignored
#       scripts/stalwart-down.sh
#
# Needs podman (rootless is fine), curl and jq.
set -euo pipefail

# Stalwart v1.1.1 (2026-11-16, latest stable), multi-arch manifest list
# digest of docker.io/stalwartlabs/stalwart:v1.1.1.
STALWART_VERSION=v1.1.1
STALWART_IMAGE=docker.io/stalwartlabs/stalwart@sha256:5ebdf474b237161742fe4309396af4ae8797c938e8e6eb7e2d4837332c4f9851

cd "$(dirname "$0")/.."

port="${STALWART_PORT:-18080}"
name="${STALWART_CONTAINER:-crate-jmap-interop-stalwart}"
env_file="${STALWART_ENV_FILE:-$PWD/target/interop/stalwart.env}"
base_url="http://127.0.0.1:$port"

# Fixed test credentials: the server is reachable from this host only and
# lives for one test run.
admin_user=admin
admin_pass=interop-admin-secret
domain=example.org
alice_user="alice@$domain"
alice_pass=alice-interop-secret
bob_user="bob@$domain"
bob_pass=bob-interop-secret

for tool in podman curl jq; do
  command -v "$tool" >/dev/null || { echo "stalwart-up.sh: $tool not found" >&2; exit 1; }
done

if podman container exists "$name"; then
  echo "stalwart-up.sh: container $name already exists; run scripts/stalwart-down.sh first" >&2
  exit 1
fi

# The container's entrypoint writes config.json into the tmpfs and then
# runs Stalwart. With a config.json present Stalwart starts in normal mode
# with its default listeners (plain HTTP on 8080 among them).
# STALWART_PUBLIC_URL makes the Session's apiUrl and other URLs point at
# the published port instead of https://<container hostname>.
podman run -d --rm \
  --name "$name" \
  --label io.github.markatwood.crate-jmap=interop \
  -p "127.0.0.1:$port:8080" \
  --mount type=tmpfs,destination=/etc/stalwart \
  --mount type=tmpfs,destination=/var/lib/stalwart \
  -e STALWART_RECOVERY_ADMIN="$admin_user:$admin_pass" \
  -e STALWART_PUBLIC_URL="$base_url" \
  -e STALWART_HOSTNAME=mail.$domain \
  --entrypoint /bin/sh \
  "$STALWART_IMAGE" \
  -c 'printf "%s" "{\"@type\":\"RocksDb\",\"path\":\"/var/lib/stalwart/\"}" > /etc/stalwart/config.json && exec /usr/local/bin/stalwart --config /etc/stalwart/config.json' \
  >/dev/null

fail() {
  echo "stalwart-up.sh: $*" >&2
  podman logs --tail 40 "$name" >&2 || true
  exit 1
}

# Wait for the HTTP listener.
for _ in $(seq 1 60); do
  if curl -fsS -o /dev/null "$base_url/healthz/live" 2>/dev/null; then
    break
  fi
  sleep 1
done
curl -fsS -o /dev/null "$base_url/healthz/live" || fail "server did not become live at $base_url"

# One JMAP request to the management API as the recovery administrator.
jmap_admin() {
  curl -fsS -u "$admin_user:$admin_pass" -H 'Content-Type: application/json' \
    --data "$1" "$base_url/jmap/"
}

resp=$(jmap_admin "$(jq -cn \
  --arg domain "$domain" \
  --arg alice "${alice_user%@*}" --arg alice_pass "$alice_pass" \
  --arg bob "${bob_user%@*}" --arg bob_pass "$bob_pass" '
  {using: ["urn:ietf:params:jmap:core", "urn:stalwart:jmap"],
   methodCalls: [
     ["x:Domain/set", {create: {d: {name: $domain}}}, "d"],
     ["x:Account/set", {create: {
        alice: {"@type": "User", name: $alice, domainId: "#d",
                credentials: {"0": {"@type": "Password", secret: $alice_pass}}},
        bob:   {"@type": "User", name: $bob, domainId: "#d",
                credentials: {"0": {"@type": "Password", secret: $bob_pass}}}}},
      "a"]]}')") || fail "management request failed"

echo "$resp" | jq -e '
  .methodResponses[0][0] == "x:Domain/set"
  and (.methodResponses[0][1].created | has("d"))
  and .methodResponses[1][0] == "x:Account/set"
  and (.methodResponses[1][1].created | has("alice") and has("bob"))' >/dev/null \
  || fail "provisioning was refused: $resp"

# Wait until the Session resource answers for alice (RFC 8620 §2).
session=""
for _ in $(seq 1 30); do
  if session=$(curl -fsSL -u "$alice_user:$alice_pass" "$base_url/.well-known/jmap" 2>/dev/null) \
     && echo "$session" | jq -e '.apiUrl and .primaryAccounts["urn:ietf:params:jmap:mail"]' >/dev/null 2>&1; then
    break
  fi
  session=""
  sleep 1
done
[ -n "$session" ] || fail "no Session for $alice_user at $base_url/.well-known/jmap"

mkdir -p "$(dirname "$env_file")"
cat > "$env_file" <<EOF
# Written by scripts/stalwart-up.sh for Stalwart $STALWART_VERSION
# (container $name). Source it before running the interop tests.
export JMAP_INTEROP_URL=$base_url
export JMAP_INTEROP_USER=$alice_user
export JMAP_INTEROP_PASSWORD=$alice_pass
export JMAP_INTEROP_USER2=$bob_user
export JMAP_INTEROP_PASSWORD2=$bob_pass
export STALWART_CONTAINER=$name
EOF

cat <<EOF
Stalwart $STALWART_VERSION is up in container $name.
  JMAP base URL: $base_url  (apiUrl $(echo "$session" | jq -r .apiUrl))
  alice:         $alice_user / $alice_pass
  bob:           $bob_user / $bob_pass
  admin:         $admin_user / $admin_pass
Env file: $env_file
  . $env_file && cargo test -p jmap-interop -- --ignored
Stop with scripts/stalwart-down.sh
EOF

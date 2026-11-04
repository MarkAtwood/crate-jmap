# Releasing

The 57 publishable `jmap-*` crates in this workspace must reach crates.io
in **topological order**: each crate must find its dependencies on the
registry. `cargo publish` with several `-p` flags orders them itself, and
`scripts/publish.sh` wraps it (see "Publish procedure").

(`jmap-testjig` is also a workspace member but carries `publish = false`
and is never published.)

This file documents the dependency graph and the publish procedure.

## Why ordering matters

A single-crate `cargo publish --dry-run -p <crate>` fails for the
non-foundation crates when the new version is not yet on the registry:
each `Cargo.toml` references `jmap-*` siblings via
`{ path = "...", version = "0.5" }`. When publishing, cargo strips the path
and resolves purely by version against the registry, so resolution fails
with errors like:

```
failed to select a version for the requirement `jmap-types = "^0.5"`
candidate versions found which didn't match: 0.4.2
```

`cargo publish --workspace --dry-run` does not have this problem: it
resolves the siblings from the workspace, so it checks every crate before
anything is published. The real publish still has to go in dependency
order, because each crate must find its dependencies on the registry.

## Dependency graph

The layers below are computed from `cargo metadata`: a crate's layer is one
more than the highest layer among the publishable `jmap-*` crates it depends
on. Normal, build and dev-dependencies all count, because `cargo publish`
resolves versioned dev-dependencies against the registry too (see blocker 1
below). A crate's own self-dev-dependency is not an ordering edge.

| Layer | Crates |
|---|---|
| 1 | `jmap-cid-types`, `jmap-jscontact-types`, `jmap-metadata-client`, `jmap-metadata-types`, `jmap-types` |
| 2 | `jmap-contacts-types`, `jmap-did-types`, `jmap-filenode-types`, `jmap-graph-types`, `jmap-jscalendar-types`, `jmap-mail-types`, `jmap-metadata-server`, `jmap-scene-types`, `jmap-server`, `jmap-sharing-types`, `jmap-vtc-types`, `jmap-web-push` |
| 3 | `jmap-base-client`, `jmap-calendars-types`, `jmap-chat-types`, `jmap-contacts-server`, `jmap-did-resolver`, `jmap-filenode-server`, `jmap-graph-server`, `jmap-mime`, `jmap-peer-auth`, `jmap-scene-server`, `jmap-sharing-server`, `jmap-tasks-types`, `jmap-vtc-server` |
| 4 | `jmap-calendars-client`, `jmap-calendars-server`, `jmap-chat-client`, `jmap-chat-federation-types`, `jmap-chat-mimi-types`, `jmap-chat-scene-server`, `jmap-chat-server`, `jmap-contacts-client`, `jmap-filenode-client`, `jmap-graph-client`, `jmap-mail-client`, `jmap-mail-server`, `jmap-peer-auth-io`, `jmap-scene-client`, `jmap-scene-vtc-server`, `jmap-sharing-client`, `jmap-tasks-client`, `jmap-tasks-server`, `jmap-vtc-client` |
| 5 | `jmap-chat-calendars-server`, `jmap-chat-federation-server`, `jmap-chat-filenode-server`, `jmap-chat-mimi-client`, `jmap-chat-mimi-server`, `jmap-chat-tasks-server`, `jmap-chat-vtc-server`, `jmap-graph-projection-server` |

Crates within the same layer have no dependencies on each other. Direct
`jmap-*` edges per crate (dev-dependencies that are not also normal
dependencies are listed separately):

| Crate | Layer | `jmap-*` dependencies | `jmap-*` dev-dependencies |
|---|---|---|---|
| `jmap-cid-types` | 1 | (none) | (none) |
| `jmap-jscontact-types` | 1 | (none) | (none) |
| `jmap-metadata-types` | 1 | (none) | (none) |
| `jmap-metadata-client` | 1 | (none) | (none) |
| `jmap-types` | 1 | (none) | (none) |
| `jmap-contacts-types` | 2 | `jmap-jscontact-types`, `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-did-types` | 2 | `jmap-types` | (none) |
| `jmap-filenode-types` | 2 | `jmap-cid-types`, `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-graph-types` | 2 | `jmap-types` | (none) |
| `jmap-jscalendar-types` | 2 | `jmap-types` | (none) |
| `jmap-mail-types` | 2 | `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-metadata-server` | 2 | `jmap-metadata-types` | (none) |
| `jmap-scene-types` | 2 | `jmap-types` | (none) |
| `jmap-server` | 2 | `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-sharing-types` | 2 | `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-vtc-types` | 2 | `jmap-types` | (none) |
| `jmap-web-push` | 2 | `jmap-types` | (none) |
| `jmap-base-client` | 3 | `jmap-cid-types`, `jmap-did-types`, `jmap-types` | (none) |
| `jmap-calendars-types` | 3 | `jmap-jscalendar-types`, `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-chat-types` | 3 | `jmap-cid-types`, `jmap-did-types`, `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-contacts-server` | 3 | `jmap-contacts-types`, `jmap-metadata-server`, `jmap-metadata-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-did-resolver` | 3 | `jmap-did-types` | (none) |
| `jmap-filenode-server` | 3 | `jmap-cid-types`, `jmap-filenode-types`, `jmap-metadata-server`, `jmap-metadata-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-graph-server` | 3 | `jmap-graph-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-mime` | 3 | `jmap-mail-types`, `jmap-types` | (none) |
| `jmap-peer-auth` | 3 | `jmap-did-types` | (none) |
| `jmap-scene-server` | 3 | `jmap-scene-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-sharing-server` | 3 | `jmap-metadata-server`, `jmap-metadata-types`, `jmap-server`, `jmap-sharing-types`, `jmap-types` | (none) |
| `jmap-tasks-types` | 3 | `jmap-jscalendar-types`, `jmap-metadata-types`, `jmap-types` | (none) |
| `jmap-vtc-server` | 3 | `jmap-server`, `jmap-types`, `jmap-vtc-types` | (none) |
| `jmap-calendars-client` | 4 | `jmap-base-client`, `jmap-calendars-types`, `jmap-types` | `jmap-metadata-types` |
| `jmap-calendars-server` | 4 | `jmap-calendars-types`, `jmap-metadata-server`, `jmap-metadata-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-client` | 4 | `jmap-base-client`, `jmap-chat-types`, `jmap-types` | `jmap-metadata-types` |
| `jmap-chat-federation-types` | 4 | `jmap-chat-types`, `jmap-cid-types`, `jmap-types` | `jmap-base-client` |
| `jmap-chat-mimi-types` | 4 | `jmap-types` | `jmap-chat-types` |
| `jmap-chat-scene-server` | 4 | `jmap-scene-server`, `jmap-scene-types`, `jmap-types` | (none) |
| `jmap-chat-server` | 4 | `jmap-chat-types`, `jmap-did-types`, `jmap-metadata-server`, `jmap-metadata-types`, `jmap-peer-auth`, `jmap-server`, `jmap-types` | (none) |
| `jmap-contacts-client` | 4 | `jmap-base-client`, `jmap-contacts-types`, `jmap-types` | (none) |
| `jmap-filenode-client` | 4 | `jmap-base-client`, `jmap-filenode-types`, `jmap-types` | (none) |
| `jmap-graph-client` | 4 | `jmap-base-client`, `jmap-graph-types`, `jmap-types` | (none) |
| `jmap-mail-client` | 4 | `jmap-base-client`, `jmap-mail-types`, `jmap-types` | `jmap-metadata-types` |
| `jmap-mail-server` | 4 | `jmap-mail-types`, `jmap-metadata-server`, `jmap-metadata-types`, `jmap-mime`, `jmap-server`, `jmap-types` | (none) |
| `jmap-peer-auth-io` | 4 | `jmap-peer-auth` | (none) |
| `jmap-scene-client` | 4 | `jmap-base-client`, `jmap-cid-types`, `jmap-scene-types`, `jmap-types` | (none) |
| `jmap-scene-vtc-server` | 4 | `jmap-scene-server`, `jmap-scene-types`, `jmap-server`, `jmap-types`, `jmap-vtc-types` | `jmap-vtc-server` |
| `jmap-sharing-client` | 4 | `jmap-base-client`, `jmap-sharing-types`, `jmap-types` | (none) |
| `jmap-tasks-client` | 4 | `jmap-base-client`, `jmap-tasks-types`, `jmap-types` | (none) |
| `jmap-tasks-server` | 4 | `jmap-metadata-server`, `jmap-metadata-types`, `jmap-server`, `jmap-tasks-types`, `jmap-types` | (none) |
| `jmap-vtc-client` | 4 | `jmap-base-client`, `jmap-types`, `jmap-vtc-types` | (none) |
| `jmap-chat-calendars-server` | 5 | `jmap-calendars-server`, `jmap-calendars-types`, `jmap-chat-types`, `jmap-server`, `jmap-types` | `jmap-chat-server` |
| `jmap-chat-federation-server` | 5 | `jmap-chat-federation-types`, `jmap-chat-server`, `jmap-chat-types`, `jmap-cid-types`, `jmap-peer-auth`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-filenode-server` | 5 | `jmap-chat-server`, `jmap-chat-types`, `jmap-filenode-server`, `jmap-filenode-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-mimi-client` | 5 | `jmap-base-client`, `jmap-chat-mimi-types`, `jmap-types` | (none) |
| `jmap-chat-mimi-server` | 5 | `jmap-chat-mimi-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-tasks-server` | 5 | `jmap-chat-server`, `jmap-chat-types`, `jmap-server`, `jmap-tasks-server`, `jmap-tasks-types`, `jmap-types` | (none) |
| `jmap-chat-vtc-server` | 5 | `jmap-chat-server`, `jmap-chat-types`, `jmap-server`, `jmap-types`, `jmap-vtc-server`, `jmap-vtc-types` | (none) |
| `jmap-graph-projection-server` | 5 | `jmap-contacts-types`, `jmap-graph-types`, `jmap-mail-types`, `jmap-server`, `jmap-types` | `jmap-mail-server` |

To recheck after a manifest change, list each publishable crate's
`jmap-*` edges:

```bash
cargo metadata --format-version 1 --no-deps | jq -r '.packages[]
  | select(.publish != []) | .name as $n
  | "\($n): \([.dependencies[] | select(.name | startswith("jmap-"))
  | select(.name != $n) | .name] | unique | join(" "))"'
```

## Publish procedure

1. Tag the release commit (`git tag v<version>`); do not push the tag yet.
2. Publish from a clean worktree at the tag, so no local change can slip
   into a package:
   ```bash
   git worktree add ../rel-<version> v<version>
   ../rel-<version>/scripts/publish.sh <version>
   ```
   The script lists the publishable crates from `cargo metadata`, checks
   the crates.io sparse index for each one's `<version>`, and runs one
   `cargo publish -p ... -p ...` for the rest; cargo publishes them in
   dependency order and waits for each to be indexed. On `429 Too Many
   Requests` it sleeps 70 seconds and retries the remainder. Any other
   failure stops it. It is safe to rerun: published crates are skipped.
3. After every crate is up, push the tag (`git push origin v<version>`)
   and remove the worktree.

crates.io allows a burst of 30 new versions, then about one per minute.
The 0.5.0 release published the first 30 crates at once and the last 27
in 26 minutes (01:55 to 02:21 UTC on 2026-11-03). New crates have a
stricter limit; see blocker 3 below.

## Known publish blockers

The 2026-05-13 publish wave surfaced these. Read before the next release.

### 1. Server-crate self-dev-dep + new features (bd:JMAP-31o4) — resolved

Server crates enable their own `memory` (and `mdn`, `sieve`) features for
integration tests through a self-dev-dependency. It used to be
`{ workspace = true, features = [...] }`, which carries a `version`; on
publish cargo resolved it against crates.io and failed when a feature was
newer than the last published release:

```
package `jmap-mail-server` depends on `jmap-mail-server` with feature
`mdn` but `jmap-mail-server` does not have that feature.
```

The self-dev-dep is now path-only, with no `version`:

```toml
[dev-dependencies]
jmap-<X>-server = { path = ".", features = ["memory"] }
```

`cargo publish` drops version-less path dev-dependencies from the published
manifest, so nothing is resolved against the registry and no temporary
`Cargo.toml` edit is needed. Tests still run with plain
`cargo test -p <crate>`. Do not switch these back to `workspace = true`.

### 2. crates.io 20-char keyword limit

`[package] keywords = [...]` entries on crates.io are rejected at upload
time if any entry exceeds 20 characters:

```
the remote server responded with an error (status 400 Bad Request):
"<keyword>" is an invalid keyword (keywords must have less than 20
characters)
```

The chat extension's natural keyword `draft-atwood-jmap-chat` (22 chars)
trips this. The current workspace uses the shortened `draft-atwood-chat`
(17 chars) on `jmap-chat-{server,client}`. The four other extension
drafts (`draft-jmap-calendars`, `draft-jmap-tasks`, `draft-jmap-filenode`,
`draft-jmap-metadata`) are all ≤20 chars and unaffected.

If a new extension introduces another long-ish draft name, check the
keyword length against this limit before the first publish.

### 3. crates.io new-crate rate limit

crates.io rate-limits the number of *new* crates a user may publish: a
burst of five, then one more every ten minutes. Publishing a new crate
beyond that allowance returns:

```
the remote server responded with an error (status 429 Too Many Requests):
You have published too many new crates in a short period of time. Please
try again after <timestamp>.
```

Bumps to existing crates do not count. The 2026-05-13 wave introduced
six new crates (`jmap-jscalendar-types`, `jmap-jscontact-types`,
`jmap-cid-types`, `jmap-metadata-types`, `jmap-metadata-server`,
`jmap-metadata-client`) and hit the limit twice. The 2026-10-24 0.4.0
wave introduced 26 new crates (the Scene, VTC, Graph, DID, peer-auth,
web-push, MIMI and chat-bridge crates): the first five published within
a minute, then crates.io accepted one new crate every ten minutes, so the
wave took 3.5 hours. Budget ten minutes per new crate beyond the fifth,
and publish with a script that retries on 429 after a few minutes rather
than failing the wave.

### 4. Uncommitted changes block `cargo publish`

`cargo publish` refuses to run with a dirty working tree:

```
error: 1 files in the working directory contain changes that were not
yet committed into git
```

This is a safety check, not a defect. Either commit first, or pass
`--allow-dirty` (required when applying the bd:JMAP-31o4 workaround
above).

## Pre-publish checklist

Run from the workspace root before the first `cargo publish` of a release:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
```

All five must succeed. The `README.md`, `PLAN.md`, and `AGENTS.md` files
are checked-in artifacts and do not need a separate action; `cargo publish`
includes them automatically.

Check every crate in one run:

```bash
cargo publish --workspace --dry-run
```

It packages and builds all crates against each other's new versions, so a
failure here is a real defect. A per-crate `cargo publish --dry-run -p
<crate-name>` fails for any crate whose dependencies are not yet on the
registry under the new version; use it only for the foundation crates or
after their dependencies are published.

## Version bump conventions

This workspace uses a shared `0.1.X` line. Bumping a `*-types` crate is a
potential SemVer event for every consumer that re-exports its types — see
the canonical-template propagation rule in `AGENTS.md`. When in doubt,
bump every consumer in the same release wave.

After the 2026-05-13 publish wave the registry holds:

| Version | Crates |
|---|---|
| `0.1.0` | `jmap-cid-types`, `jmap-metadata-types`, `jmap-metadata-server`, `jmap-metadata-client` (all first-time publishes) |
| `0.1.1` | All 24 other publishable crates |
| `0.1.2` | `jmap-server`, `jmap-mail-server` (ahead of the rest, reflecting single-crate fixes shipped between coordinated releases) |

The next coordinated release should bring all crates onto a single shared
patch level (the `0.1.0` set to `0.1.1` and everything else to `0.1.2`
or higher).

The 2026-10-18 wave moved every publishable crate to `0.2.0`. Changes since
June break the 0.1 API (jmap-server's patch helpers and `PatchPathError`,
new `ChatBackend` and `CalendarsBackend` behaviour, serde defaults in the
`*-types` crates), so the shared line is now `0.2.x` and the workspace
dependency requirements are `"0.2"`.

The 2026-10-19 wave moved every publishable crate to `0.3.0`. Changes since
`0.2.0` break the 0.2 API (`JmapBackend::query_objects` anchor arguments
and `BackendQueryError`, new required `PushSubscriptionBackend` and
`ChatBackend` methods, per-account `supports_type`, `SenderId` removal and
other `jmap-chat-types` field changes), so the shared line is now `0.3.x`
and the workspace dependency requirements are `"0.3"`. `CHANGELOG.md` lists
every break.

The 2026-10-24 wave moved every publishable crate to `0.4.0`. Changes since
`0.3.0` break the 0.3 API (metadata-04 handling hoisted into `jmap-server`
with `JmapBackend::metadata_info`, `using` arguments on the extension
servers' handlers, new required `copy_object` methods on the Tasks,
Calendars and FileNode backends, stricter wire validation of Ids, sorts
and metadata namespaces), so the shared line is now `0.4.x` and the
workspace dependency requirements are `"0.4"`. `CHANGELOG.md` lists every
break.

The 0.5.0 release moves every publishable crate to `0.5.0`. Changes since
`0.4.2` break the 0.4 API (`OutboundEvent::Receipt` and the `Peer/*`
argument types gain fields, `AuthenticatedPeer` and `FederationBackend`
change, FileNode `depth`, `onExists` and `forbiddenNodeNames` follow
draft-ietf-jmap-filenode-19), so the shared line is now `0.5.x` and the
workspace dependency requirements are `"0.5"`. `CHANGELOG.md` lists every
break.

### Third-party majors in public APIs

Some crates expose third-party types: `jmap_peer_auth::ed25519_dalek`
(re-export), `reqwest::Client` and `reqwest::ClientBuilder` in
`jmap-base-client`, `jmap-did-resolver`, `jmap-peer-auth-io`,
`jmap-scene-client` and `jmap-web-push`. A major bump of such a
dependency is a minor bump of ours (the pre-1.0 breaking bump), listed as
**BREAKING** in `CHANGELOG.md`.

`ed25519-dalek` is on 3. `reqwest` stays on 0.12 because 0.13 cannot
give every client ring-backed TLS with the bundled webpki-roots. Its
`rustls` feature uses aws-lc-rs and the OS trust store
(rustls-platform-verifier) instead. With `rustls-no-provider`, a client
we build can still use ring and webpki-roots with no global setup, by
passing a `rustls::ClientConfig` to `ClientBuilder::use_preconfigured_tls`.
But `ClientBuilder::build` without preconfigured TLS panics unless a
rustls `CryptoProvider` is installed process-wide (reqwest 0.13.5
`default_rustls_crypto_provider`), and the `reqwest::Client`s our callers
build and pass in cannot be forced onto the preconfigured path. Moving to
0.13 is a later breaking release that picks one of those trade-offs
(bd:JMAP-wf92.14).

## Tracking

This file was first written for issue **JMAP-sc1b.23** (P3 publish-readiness
sweep) and was substantially updated after the 2026-05-13 publish wave
to reflect the current crate set and the four known publish blockers
(bd:JMAP-31o4 for the self-dev-dep pattern).

Update this file in any PR that:

- Adds, removes, or renames a crate in this workspace.
- Reshapes a dependency edge between two `jmap-*` crates.
- Changes the version-bump convention (e.g. moves to a `0.2.x` line).
- Discovers a new publish blocker not already documented above.

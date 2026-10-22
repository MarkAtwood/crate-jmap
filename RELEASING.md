# Releasing

The 57 publishable `jmap-*` crates in this workspace must be published to
crates.io in **topological order**. Each layer depends on the previous one
being indexed on the registry, so a single `cargo publish` of a leaf crate
fails until all its transitive dependencies have published.

(`jmap-testjig` is also a workspace member but carries `publish = false`
and is skipped at every step below.)

This file documents the dependency graph and the resulting publish ordering.

## Why ordering matters

`cargo publish --dry-run` will fail for the non-foundation crates against
an empty registry: each `Cargo.toml` references `jmap-*` siblings via
`{ path = "...", version = "0.1" }`. When publishing, cargo strips the path
and resolves purely by version against the registry. If the registry does
not yet have a matching version of every transitive dependency, resolution
fails with errors like:

```
failed to select a version for the requirement `jmap-types = "^0.1.1"`
candidate versions found which didn't match: 0.1.0
```

This is not a defect; it is the chicken-and-egg of a multi-crate workspace
publishing version bumps. Once each layer is published, the next layer's
dry-run starts to pass.

## Dependency graph

The layers below are computed from `cargo metadata`: a crate's layer is one
more than the highest layer among the publishable `jmap-*` crates it depends
on. Normal, build and dev-dependencies all count, because `cargo publish`
resolves versioned dev-dependencies against the registry too (see blocker 1
below). A crate's own self-dev-dependency is not an ordering edge.

| Layer | Crates |
|---|---|
| 1 | `jmap-cid-types`, `jmap-jscontact-types`, `jmap-metadata-types`, `jmap-types` |
| 2 | `jmap-contacts-types`, `jmap-did-types`, `jmap-filenode-types`, `jmap-graph-types`, `jmap-jscalendar-types`, `jmap-mail-types`, `jmap-metadata-client`, `jmap-metadata-server`, `jmap-scene-types`, `jmap-server`, `jmap-sharing-types`, `jmap-vtc-types`, `jmap-web-push` |
| 3 | `jmap-base-client`, `jmap-calendars-types`, `jmap-chat-types`, `jmap-contacts-server`, `jmap-did-resolver`, `jmap-filenode-server`, `jmap-graph-server`, `jmap-mime`, `jmap-peer-auth`, `jmap-scene-server`, `jmap-sharing-server`, `jmap-tasks-types`, `jmap-vtc-server` |
| 4 | `jmap-calendars-client`, `jmap-calendars-server`, `jmap-chat-client`, `jmap-chat-federation-types`, `jmap-chat-mimi-types`, `jmap-chat-scene-server`, `jmap-chat-server`, `jmap-contacts-client`, `jmap-filenode-client`, `jmap-graph-client`, `jmap-mail-client`, `jmap-mail-server`, `jmap-peer-auth-io`, `jmap-scene-client`, `jmap-scene-vtc-server`, `jmap-sharing-client`, `jmap-tasks-client`, `jmap-tasks-server`, `jmap-vtc-client` |
| 5 | `jmap-chat-calendars-server`, `jmap-chat-federation-server`, `jmap-chat-filenode-server`, `jmap-chat-mimi-client`, `jmap-chat-mimi-server`, `jmap-chat-tasks-server`, `jmap-chat-vtc-server`, `jmap-graph-projection-server` |

Crates within the same layer have no dependencies on each other and can be
published in parallel. Direct `jmap-*` edges per crate (dev-dependencies
that are not also normal dependencies are listed separately):

| Crate | Layer | `jmap-*` dependencies | `jmap-*` dev-dependencies |
|---|---|---|---|
| `jmap-cid-types` | 1 | (none) | (none) |
| `jmap-jscontact-types` | 1 | (none) | (none) |
| `jmap-metadata-types` | 1 | (none) | (none) |
| `jmap-types` | 1 | (none) | (none) |
| `jmap-contacts-types` | 2 | `jmap-jscontact-types`, `jmap-types` | (none) |
| `jmap-did-types` | 2 | `jmap-types` | (none) |
| `jmap-filenode-types` | 2 | `jmap-cid-types`, `jmap-types` | (none) |
| `jmap-graph-types` | 2 | `jmap-types` | (none) |
| `jmap-jscalendar-types` | 2 | `jmap-types` | (none) |
| `jmap-mail-types` | 2 | `jmap-types` | (none) |
| `jmap-metadata-client` | 2 | `jmap-metadata-types` | (none) |
| `jmap-metadata-server` | 2 | `jmap-metadata-types` | (none) |
| `jmap-scene-types` | 2 | `jmap-types` | (none) |
| `jmap-server` | 2 | `jmap-types` | (none) |
| `jmap-sharing-types` | 2 | `jmap-types` | (none) |
| `jmap-vtc-types` | 2 | `jmap-types` | (none) |
| `jmap-web-push` | 2 | `jmap-types` | (none) |
| `jmap-base-client` | 3 | `jmap-cid-types`, `jmap-did-types`, `jmap-types` | (none) |
| `jmap-calendars-types` | 3 | `jmap-jscalendar-types`, `jmap-types` | (none) |
| `jmap-chat-types` | 3 | `jmap-did-types`, `jmap-types` | (none) |
| `jmap-contacts-server` | 3 | `jmap-contacts-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-did-resolver` | 3 | `jmap-did-types` | (none) |
| `jmap-filenode-server` | 3 | `jmap-cid-types`, `jmap-filenode-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-graph-server` | 3 | `jmap-graph-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-mime` | 3 | `jmap-mail-types`, `jmap-types` | (none) |
| `jmap-peer-auth` | 3 | `jmap-did-types` | (none) |
| `jmap-scene-server` | 3 | `jmap-scene-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-sharing-server` | 3 | `jmap-server`, `jmap-sharing-types`, `jmap-types` | (none) |
| `jmap-tasks-types` | 3 | `jmap-jscalendar-types`, `jmap-types` | (none) |
| `jmap-vtc-server` | 3 | `jmap-server`, `jmap-types`, `jmap-vtc-types` | (none) |
| `jmap-calendars-client` | 4 | `jmap-base-client`, `jmap-calendars-types`, `jmap-types` | `jmap-metadata-types` |
| `jmap-calendars-server` | 4 | `jmap-calendars-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-client` | 4 | `jmap-base-client`, `jmap-chat-types`, `jmap-types` | `jmap-metadata-types` |
| `jmap-chat-federation-types` | 4 | `jmap-chat-types`, `jmap-types` | `jmap-base-client` |
| `jmap-chat-mimi-types` | 4 | `jmap-types` | `jmap-chat-types` |
| `jmap-chat-scene-server` | 4 | `jmap-scene-server`, `jmap-scene-types`, `jmap-types` | (none) |
| `jmap-chat-server` | 4 | `jmap-chat-types`, `jmap-did-types`, `jmap-peer-auth`, `jmap-server`, `jmap-types` | (none) |
| `jmap-contacts-client` | 4 | `jmap-base-client`, `jmap-contacts-types`, `jmap-types` | (none) |
| `jmap-filenode-client` | 4 | `jmap-base-client`, `jmap-filenode-types`, `jmap-types` | (none) |
| `jmap-graph-client` | 4 | `jmap-base-client`, `jmap-graph-types`, `jmap-types` | (none) |
| `jmap-mail-client` | 4 | `jmap-base-client`, `jmap-mail-types`, `jmap-types` | `jmap-metadata-types` |
| `jmap-mail-server` | 4 | `jmap-mail-types`, `jmap-mime`, `jmap-server`, `jmap-types` | (none) |
| `jmap-peer-auth-io` | 4 | `jmap-peer-auth` | (none) |
| `jmap-scene-client` | 4 | `jmap-base-client`, `jmap-cid-types`, `jmap-scene-types`, `jmap-types` | (none) |
| `jmap-scene-vtc-server` | 4 | `jmap-scene-server`, `jmap-scene-types`, `jmap-types`, `jmap-vtc-types` | `jmap-server`, `jmap-vtc-server` |
| `jmap-sharing-client` | 4 | `jmap-base-client`, `jmap-sharing-types`, `jmap-types` | (none) |
| `jmap-tasks-client` | 4 | `jmap-base-client`, `jmap-tasks-types`, `jmap-types` | (none) |
| `jmap-tasks-server` | 4 | `jmap-server`, `jmap-tasks-types`, `jmap-types` | (none) |
| `jmap-vtc-client` | 4 | `jmap-base-client`, `jmap-types`, `jmap-vtc-types` | (none) |
| `jmap-chat-calendars-server` | 5 | `jmap-calendars-server`, `jmap-calendars-types`, `jmap-chat-types`, `jmap-types` | `jmap-chat-server` |
| `jmap-chat-federation-server` | 5 | `jmap-chat-federation-types`, `jmap-chat-server`, `jmap-chat-types`, `jmap-peer-auth`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-filenode-server` | 5 | `jmap-chat-server`, `jmap-chat-types`, `jmap-filenode-server`, `jmap-filenode-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-mimi-client` | 5 | `jmap-base-client`, `jmap-chat-mimi-types`, `jmap-types` | (none) |
| `jmap-chat-mimi-server` | 5 | `jmap-chat-mimi-types`, `jmap-server`, `jmap-types` | (none) |
| `jmap-chat-tasks-server` | 5 | `jmap-chat-server`, `jmap-chat-types`, `jmap-tasks-server`, `jmap-tasks-types`, `jmap-types` | (none) |
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

For each layer, in order:

1. Wait for the registry to index the previous layer.
   crates.io typically indexes within seconds; `cargo publish` blocks on
   indexing by default, so sequential publishes within a layer are safe.
2. For each crate in the current layer, run from the workspace root:
   ```bash
   cargo publish -p <crate-name>
   ```
3. Verify the publish succeeded (`Published <crate> v<version> at registry
   crates-io`) before moving to the next layer.

Within a layer, ordering does not matter. Crates may be published in
parallel by spawning multiple `cargo publish` processes.

## Known publish blockers

The 2026-05-13 publish wave surfaced these. Read before the next release.

### 1. Server-crate self-dev-dep + new features (bd:JMAP-31o4)

Every `jmap-*-server` crate carries:

```toml
[dev-dependencies]
jmap-<X>-server = { workspace = true, features = ["memory", ...] }
```

The workspace dep expands to `{ path = "...", version = "0.1" }`. On
publish, cargo strips `path` and tries to resolve the self-dev-dep against
the registry. If the new features (`memory`, `mdn`, `sieve`, …) did not
exist in the previously-published versions of the crate, resolution fails:

```
package `jmap-mail-server` depends on `jmap-mail-server` with feature
`mdn` but `jmap-mail-server` does not have that feature.
failed to select a version for `jmap-mail-server` which could resolve
this conflict
```

`--no-verify` does NOT bypass this — the error fires during pre-publish
manifest resolution, before the build-verify step.

**Workaround (until bd:JMAP-31o4 lands a permanent fix):** for each
affected server crate, temporarily comment out the self-dev-dep line in
its `Cargo.toml`, then `cargo publish --allow-dirty -p <crate>`, then
revert. Dev-dependencies are stripped from the published manifest anyway,
so consumers see no difference.

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

crates.io rate-limits the number of *new* crates a user may publish per
unit time — currently five per ten minutes. Publishing a release wave
that introduces a sixth new crate within the window returns:

```
the remote server responded with an error (status 429 Too Many Requests):
You have published too many new crates in a short period of time. Please
try again after <timestamp>.
```

Bumps to existing crates do not count. The 2026-05-13 wave introduced
six new crates (`jmap-jscalendar-types`, `jmap-jscontact-types`,
`jmap-cid-types`, `jmap-metadata-types`, `jmap-metadata-server`,
`jmap-metadata-client`) and hit the limit twice. Plan the publish
order so first-time publishes are spread across multiple ten-minute
windows when introducing more than five new crates. The first publish of
the Scene, VTC and Graph crates (layers 2 to 5 above) introduces
13 new crates, so it spans at least three windows.

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

For each crate, run:

```bash
cargo publish --dry-run -p <crate-name>
```

This will fail for crates whose dependencies are not yet on the registry
under the new version — that is expected. The dry-run is most useful for
the foundation crates (`jmap-types`, `jmap-mime`, `jmap-server`,
`jmap-base-client`) where dependencies are already published or
self-contained.

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

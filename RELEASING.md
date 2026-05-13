# Releasing

The 30 publishable `jmap-*` crates in this workspace must be published to
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

```
jmap-types                                       (foundation; no jmap deps)
    │
    ├── jmap-{mail,chat,filenode,sharing}-types  (extension types — layer 2)
    │       └── jmap-mime                        (uses jmap-mail-types)
    │       └── jmap-{ext}-server                (also depends on jmap-server)
    │       └── jmap-{ext}-client                (also depends on jmap-base-client)
    │
    ├── jmap-{contacts,calendars,tasks}-types    (also consume a sub-types crate)
    │       └── jmap-{contacts,calendars,tasks}-server
    │       └── jmap-{contacts,calendars,tasks}-client
    │
    ├── jmap-cid-types                           (CidCapability + Sha256 typed shape)
    ├── jmap-metadata-types                      (Metadata, Annotation, …)
    │       └── jmap-metadata-server
    │       └── jmap-metadata-client
    │
    ├── jmap-server                              (dispatcher, parse, helpers)
    │       └── jmap-mail-server                 (also depends on jmap-mime)
    │       └── jmap-{ext}-server (×7)           (chat, contacts, calendars, tasks,
    │                                             filenode, sharing, metadata)
    │
    └── jmap-base-client                         (auth, transport, session, blob, SSE, WS)
            └── jmap-{ext}-client (×8)           (mail, chat, contacts, calendars, tasks,
                                                  filenode, sharing, metadata)

jmap-jscalendar-types       (RFC 8984 typed sub-objects, no JMAP dep, only serde + jmap-types)
    ├── jmap-calendars-types (re-exports as `jscalendar` module alias)
    └── jmap-tasks-types     (re-exports as `jscalendar` module alias)

jmap-jscontact-types        (RFC 9553 typed sub-objects, no JMAP dep, only serde)
    └── jmap-contacts-types  (re-exports as `jscontact` module alias)
```

Layer table:

| Layer | Crates | Depends on |
|---|---|---|
| 1 | `jmap-types` | (nothing) |
| 2a | `jmap-mail-types`, `jmap-chat-types`, `jmap-filenode-types`, `jmap-sharing-types`, `jmap-jscalendar-types`, `jmap-jscontact-types`, `jmap-cid-types`, `jmap-metadata-types` | layer 1 (or just serde, for the two `js*-types`) |
| 2b | `jmap-contacts-types`, `jmap-calendars-types`, `jmap-tasks-types` | layer 2a (consume `jmap-jscontact-types` or `jmap-jscalendar-types`) |
| 3 | `jmap-mime` | layer 2a (`jmap-mail-types`) |
| 4 | `jmap-server` | layer 1 |
| 5 | `jmap-base-client` | layer 1 |
| 6 | `jmap-mail-server` | layers 1, 2a, 3, 4 (incl. `jmap-mime`) |
| 6 | `jmap-chat-server`, `jmap-contacts-server`, `jmap-calendars-server`, `jmap-tasks-server`, `jmap-filenode-server`, `jmap-sharing-server`, `jmap-metadata-server` | layers 1, 2a or 2b (the matching `*-types`), 4 |
| 7 | `jmap-mail-client`, `jmap-chat-client`, `jmap-contacts-client`, `jmap-calendars-client`, `jmap-tasks-client`, `jmap-filenode-client`, `jmap-sharing-client`, `jmap-metadata-client` | layers 1, 2a or 2b, 5 |

Crates within the same layer (and sub-layer) have no dependencies on each
other and can be published in parallel. The 2a/2b split exists because
`jmap-contacts-types` consumes `jmap-jscontact-types`, and both
`jmap-calendars-types` and `jmap-tasks-types` consume `jmap-jscalendar-types`.

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
windows when introducing more than five new crates.

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

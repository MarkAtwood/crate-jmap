# Releasing

The 25 `jmap-*` crates in this workspace must be published to crates.io in
**topological order**. Each layer depends on the previous one being indexed
on the registry, so a single `cargo publish` of a leaf crate fails until all
its transitive dependencies have published.

This file documents the dependency graph and the resulting publish ordering.

## Why ordering matters

`cargo publish --dry-run` will fail for 24 of the 25 crates against an empty
registry: each Cargo.toml references `jmap-*` siblings via
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
jmap-types                                   (foundation; no jmap deps)
    ├── jmap-{mail,chat,contacts,calendars,tasks,filenode,sharing}-types
    │     └── jmap-mime              (uses jmap-mail-types)
    │     └── jmap-{ext}-server      (also depends on jmap-server)
    │     └── jmap-{ext}-client      (also depends on jmap-base-client)
    ├── jmap-server                  (dispatcher, parse, helpers)
    │     └── jmap-mail-server       (also depends on jmap-mime)
    │     └── jmap-{ext}-server (×6) (chat, calendars, tasks, contacts, filenode, sharing)
    └── jmap-base-client             (auth, transport, session, blob, SSE, WS)
          └── jmap-{ext}-client (×7) (mail, chat, calendars, tasks, contacts, filenode, sharing)
```

Layers, ordered:

| Layer | Crates | Depends on |
|---|---|---|
| 1 | `jmap-types` | (nothing) |
| 2 | `jmap-mail-types`, `jmap-chat-types`, `jmap-contacts-types`, `jmap-calendars-types`, `jmap-tasks-types`, `jmap-filenode-types`, `jmap-sharing-types` | layer 1 |
| 3 | `jmap-mime` | layer 2 (`jmap-mail-types`) |
| 4 | `jmap-server` | layer 1 |
| 5 | `jmap-base-client` | layer 1 |
| 6 | `jmap-mail-server` | layers 1–4 (incl. `jmap-mime`) |
| 6 | `jmap-chat-server`, `jmap-calendars-server`, `jmap-tasks-server`, `jmap-contacts-server`, `jmap-filenode-server`, `jmap-sharing-server` | layers 1, 2, 4 |
| 7 | `jmap-mail-client`, `jmap-chat-client`, `jmap-calendars-client`, `jmap-tasks-client`, `jmap-contacts-client`, `jmap-filenode-client`, `jmap-sharing-client` | layers 1, 2, 5 |

Crates within the same layer have no dependencies on each other and can be
published in parallel.

## Publish procedure

For each layer, in order:

1. Wait for the registry to index the previous layer.
   crates.io typically indexes within seconds; in CI, polling
   `cargo search <crate-name>` for the new version is reliable.
2. For each crate in the current layer, run from the workspace root:
   ```bash
   cargo publish -p <crate-name>
   ```
3. Verify the publish succeeded (HTTP 200, version visible in
   `cargo search`) before moving to the next layer.

Within a layer, ordering does not matter. Crates may be published in
parallel — this is most useful for layer 2 (7 crates) and layers 6, 7
(6–7 crates each).

## Pre-publish checklist

Run from the workspace root before the first `cargo publish` of a release:

```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

All four must succeed. The README, PLAN.md, and AGENTS.md files are
checked-in artifacts and do not need a separate action; cargo publish
includes them automatically.

For each crate, run:

```bash
cargo publish --dry-run -p <crate-name>
```

This will fail for crates whose dependencies are not yet on the registry —
that is expected and is the reason this file exists. The dry-run is most
useful for the foundation crates (`jmap-types`, `jmap-mime`, `jmap-server`,
`jmap-base-client`) where dependencies are already published or
self-contained.

## Version bump conventions

This workspace uses a shared `0.1.X` line. Bumping a `*-types` crate is a
potential SemVer event for every consumer that re-exports its types — see
the canonical-template propagation rule in `AGENTS.md`. When in doubt,
bump every consumer in the same release wave.

The two crates currently at `0.1.2` (`jmap-server` and `jmap-mail-server`)
are ahead of the rest of the workspace at `0.1.1`; this is intentional and
reflects two single-crate fixes shipped between coordinated releases. The
next coordinated release should bring all crates up to `0.1.3` together.

## Tracking

This file was first written for issue **JMAP-sc1b.23** (P3 publish-readiness
sweep). Update this file in any PR that:

- Adds, removes, or renames a crate in this workspace.
- Reshapes a dependency edge between two `jmap-*` crates.
- Changes the version-bump convention (e.g. moves to a `0.2.x` line).

# Agent Instructions — jmap-cid-types

## What this crate is

draft-atwood-jmap-cid-00 wire-format types for the `jmap-*` crate family.
The `urn:ietf:params:jmap:cid` capability plus the `sha256` typed string
shape. Pure data types: no JMAP-method semantics, no async, no network I/O.

## Crate family context

```
jmap-types
    └── jmap-cid-types  ← this crate
```

Consumed by `jmap-base-client` for the Blob upload response surface
and Session capability detection (landed under bd:JMAP-v9py.13 / .14).
CID is independent of any single consumer extension — Chat, FileNode,
and a future RFC 8620 bis all reference the same `sha256` field
defined here.

## Before starting any work

1. Read `PLAN.md` — scope, target structure, follow-up beads
2. Run `bd ready` — check for open issues before creating new ones
3. Read draft-atwood-jmap-cid-00 (the normative reference) before adding
   or changing types

## Source material

Normative reference: draft-atwood-jmap-cid-00 at
`~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md`.

## Build & Test

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

Run all four before considering any work done.

## Design constraints (settled)

| Decision | Choice |
|---|---|
| Async | None — this crate is sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | serde (runtime), serde_json (dev) — no jmap-types, no jmap-server, no async |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| All public structs | `#[non_exhaustive]` |

## Non-interactive shell commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add async, tokio, or axum
- Do not add a `jmap-server` or `jmap-base-client` dependency — this
  crate sits below them in the graph

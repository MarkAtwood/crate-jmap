# Agent Instructions — jmap-mail-types

This crate's public API, wire format, module layout, type names, field names,
serde attributes, and design conventions are stabilized. Treat it as a high-care
surface: RFC 8621 wire compatibility depends on the type names and serde
attributes here matching the spec exactly. Prefer non-breaking additions
(new variants on `#[non_exhaustive]` enums, new methods, new accessors) over
reshaping existing types. Document any breaking change in the commit message
and the relevant `PLAN.md`.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — planned types, module layout, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Read the relevant section of RFC 8621 before implementing any type

## What This Is

RFC 8621 (JMAP for Mail) data types. Types only — no method handlers. No async.
Both `jmap-mail-server` and any future mail client depend on this.

## Crate Family Context

```
jmap-types
    └── jmap-mail-types  ← this crate
            └── jmap-mail-server
```

When changing a public type or field, check that `jmap-mail-server` still compiles.

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/rfc8621.txt   ← normative
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← base protocol context
```

**Before naming any field or type**, grep the RFC 8621 text to confirm the exact name.
Field name mismatches cause silent serde failures.

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.

## Build & Test

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | None — sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json only |
| Field names | Must match RFC 8621 exactly |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| Test oracle | Hand-written JSON from RFC 8621 examples — never from code under test |

## Subagent Guidance

- Spawn subagents for parallel work on independent modules (`mailbox.rs` vs `email.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant RFC 8621 section explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond jmap-types, serde, serde_json
- Do not add async, tokio, or axum
- Do not add method handler logic — types only

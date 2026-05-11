# Agent Instructions — jmap-jscalendar-types

## What this crate is

RFC 8984 JSCalendar typed sub-types for the `jmap-*` crate family.
Pure data types: no JMAP-method semantics, no async, no network I/O.
Consumed by `jmap-calendars-types` and `jmap-tasks-types` via path-dep
+ re-export.

## Crate family context

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-jscalendar-types  ← this crate
            ├── jmap-calendars-types (consumes via path-dep + re-export)
            └── jmap-tasks-types     (consumes via path-dep + re-export)
```

This crate's types existed previously inside
`crate-jmap-calendars-types/src/jscalendar.rs`. They were extracted
into a dedicated crate so `jmap-tasks-types` can also consume the
typed JSCalendar sub-types without taking on a `jmap-calendars-types`
dependency. See `PLAN.md` for the extraction rationale and history.

## Before starting any work

1. Read `PLAN.md` — public API, module layout, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Read RFC 8984 (the normative reference) before adding or changing types

## Source material

Normative reference: RFC 8984 (JSCalendar).

Spec section coverage (see `PLAN.md` for the full table):

| Type | RFC 8984 section |
|---|---|
| `LocalDateTime`, `Duration`, `SignedDuration` | §1.4.5, §1.4.6, §1.4.7 |
| `NDay`, `RecurrenceRule` | §4.3.3 |
| `Location`, `VirtualLocation` | §4.2.5, §4.2.6 |
| `Link` | §1.4.11 |
| `Relation` | §1.4.10 |
| `Participant` | §4.4.6 |
| `OffsetTrigger`, `AbsoluteTrigger`, `AlertTrigger`, `Alert` | §4.5.2 |

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
| Dependencies | jmap-types, serde, serde_json only |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| All public structs | `#[non_exhaustive]` |
| `@type` discriminator | Wire field `"@type"`; Rust field `at_type: String` |

## Non-interactive shell commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond jmap-types, serde, serde_json
- Do not add async, tokio, or axum
- Do not duplicate type definitions that already live in `jmap-types`

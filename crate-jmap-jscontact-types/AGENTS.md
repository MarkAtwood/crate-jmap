# Agent Instructions — jmap-jscontact-types

## What this crate is

RFC 9553 JSContact typed sub-types for the `jmap-*` crate family.
Pure data types: no JMAP-method semantics, no async, no network I/O.
Consumed by `jmap-contacts-types` via path-dep + re-export.

## Crate family context

```
(no JMAP dep)
    └── jmap-jscontact-types  ← this crate
            └── jmap-contacts-types (consumes via path-dep + re-export)
```

This crate has no JMAP dependency. The sub-object types defined here
are pure RFC 9553 JSContact structures with no JMAP-specific extensions.

## Before starting any work

1. Read `PLAN.md` — public API, module layout, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Read RFC 9553 (the normative reference) before adding or changing types

## Source material

Normative reference: RFC 9553 (JSContact).

Spec section coverage (see `PLAN.md` for the full table).

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
| Dependencies | serde, serde_json only — no JMAP dep |
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
- Do not add dependencies beyond serde, serde_json
- Do not add async, tokio, or axum
- Do not add a JMAP dependency — this crate is JMAP-free

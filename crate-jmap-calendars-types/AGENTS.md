# Agent Instructions — jmap-calendars-types

## 🧬 Sibling of the canonical jmap-mail-types — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-types`
extension-types template**. Module layout, type-naming idioms,
`#[non_exhaustive]` policy, serde attribute style, doc-comment
style, and test layout must mirror `jmap-mail-types`. Differences
are *only* the spec content (draft-ietf-jmap-calendars-26 + RFC 8984
JSCalendar here; RFC 8621 in mail-types).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-types`, **change `jmap-mail-types` first, then
  propagate** to every other extension-types sibling in the same
  pass.
- If `jmap-mail-types` changes, propagate the change here in the
  same pass (or file a follow-up sweep bead before merging).
- A divergent sibling without a matching change on the canonical
  is a regression of the cookie-cutter intent — review will catch
  it.

Prefer non-breaking additions (new variants on `#[non_exhaustive]`
enums, new methods, new accessors) over reshaping existing types.

This project uses **bd** (beads) for issue tracking. Run `bd prime`
for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — planned types, module layout, source material
2. Read the relevant draft-ietf-jmap-calendars-26 section before
   implementing any type
3. Cross-check the canonical sibling
   `~/PROJECT/crate-jmap/crate-jmap-mail-types/` for the type/serde
   pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Calendars extension data types (draft-ietf-jmap-calendars-26).
Types only — no method handlers. No async. Consumed by
`jmap-calendars-server` and `jmap-calendars-client`. Also re-exports
typed JSCalendar (RFC 8984) sub-types via `jmap-jscalendar-types`
under the `jscalendar` module alias for callers that want to opt
into structured value-shapes for the hybrid Sloppy-Value fields.

## Crate Family Context

```
jmap-types
    ├── jmap-jscalendar-types     RFC 8984 typed sub-types
    └── jmap-calendars-types  ← this crate (sibling of canonical jmap-mail-types)
            ├── jmap-calendars-server
            └── jmap-calendars-client
```

When changing a public type or field, check that both consumers
still compile.

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt   ← normative
~/PROJECT/jmap-chat-spec/references/rfc8984.txt                        ← JSCalendar format
~/PROJECT/jmap-chat-spec/references/rfc8620.txt                        ← base protocol
```

**Before naming any field or type**, grep the relevant draft text
to confirm the exact name. Field name mismatches cause silent serde
failures.

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-calendars-types -- -D warnings
cargo test -p jmap-calendars-types
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-calendars-types --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | None — sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, jmap-jscalendar-types, serde, serde_json only |
| Field names | Must match draft-ietf-jmap-calendars-26 exactly |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| Test oracle | Hand-written JSON from spec examples — never from code under test |
| Attribute order | `#[non_exhaustive]` → `#[derive(...)]` → `#[serde(...)]` on every type |
| Sloppy-Value | `Option<serde_json::Value>` for IETF-defined nested objects (RFC 8984 JSCalendar value shapes); see workspace AGENTS.md and crate PLAN.md §1–§8 |

## Subagent Guidance

- Spawn subagents for parallel work on independent modules
  (`calendar.rs` vs `event.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant draft section explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace
  AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond jmap-types, jmap-jscalendar-types,
  serde, serde_json
- Do not add async, tokio, or axum
- Do not add method handler logic — types only
- Do not add fields or types not present in draft-ietf-jmap-calendars-26
  unless explicitly directed

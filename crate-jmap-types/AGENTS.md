# Agent Instructions — jmap-types

## 🧬 Canonical foundation — every other crate mirrors this one

This crate is the **foundation that the entire `jmap-*` family is cookie-cut
from**. Every shared wire type (`Id`, `State`, `UTCDate`, `JmapError`,
`JmapRequest`, `JmapResponse`, `Invocation`, `ResultReference`, `PatchObject`,
`SetError`, etc.) lives here, and every downstream crate uses these types
directly — they are not re-implemented elsewhere.

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- Any change to a public type, field, variant, derive, serde attribute, or
  doc-style here ripples through every downstream `jmap-*` crate. If you
  change something here, plan the downstream propagation in the same pass
  (or file a follow-up sweep bead before merging).
- Any time you find yourself wanting to change a downstream crate in a way
  that *would* be more consistent if it were grounded here, change here
  first, then propagate.

Prefer non-breaking additions (new variants on `#[non_exhaustive]` enums,
new methods, new accessors) over reshaping existing types — every break
here is amplified seven times by the extension-types crates and seven more
times by the extension-server / extension-client crates.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — public API, module layout, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Grep kith-core for the relevant type before implementing from scratch

## What This Is

Shared JMAP wire types for the jmap-* crate family. Minimal deps (serde, serde_json,
thiserror). No async. Both client and server crates depend on this.

## Crate Family Context

```
jmap-types  ← this crate
    ├── jmap-server          adds dispatcher, axum, tokio
    ├── jmap-mail-types      adds RFC 8621 data types
    ├── jmap-chat-types      adds Chat extension data types
    └── (future extensions)
```

When changing a public type or adding a new one, check that downstream crates
(`jmap-server`, `jmap-mail-types`, `jmap-chat-types`) still compile.

## Source Material

Extract from kith-core — do not rewrite from scratch.

| Item | Location |
|---|---|
| `JmapRequest`, `JmapResponse`, `Invocation` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |
| `JmapError` | `~/PROJECT/kith/crates/kith-core/src/error.rs` |
| `ResultReference`, `Argument<T>` | `~/PROJECT/kith/crates/kith-core/src/resultref.rs` |
| `Id`, `UTCDate`, `State` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |

Do NOT copy kith-specific items: `Role`, `Identity`, `AuthError`, `KithError`.

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← normative
```

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
| Async | None — this crate is sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | serde, serde_json, thiserror only |
| Error strings | Must match RFC 8620 §7.1 verbatim |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |

## Subagent Guidance

- Spawn subagents for parallel work on independent modules (`id.rs` vs `wire.rs`)
- Never spawn two subagents editing the same file
- Each subagent reads only what it needs — grep rather than loading full files
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond serde, serde_json, thiserror
- Do not add async, tokio, or axum
- Do not add kith-specific types (Role, Identity, etc.)

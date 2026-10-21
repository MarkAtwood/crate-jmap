# Agent Instructions — jmap-metadata-server

## 🧬 Sibling of the canonical jmap-mail-server — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-server`
extension-server template**. Doc-comment style, lint settings, error
conventions, and test layout must mirror `jmap-mail-server`.
Differences are *only* the spec content (draft-ietf-jmap-metadata-04
here; RFC 8621 in mail-server). draft-04 defines no methods of its
own, so this crate has no `Backend` trait, handler registration,
`/set` boilerplate, or `MemoryBackend`; that is a spec-mandated
difference, not drift.

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-server`, **change `jmap-mail-server` first, then
  propagate** to every other extension-server sibling in the same
  pass.
- If `jmap-mail-server` changes, propagate the change here in the
  same pass (or file a follow-up sweep bead before merging).
- A divergent sibling without a matching change on the canonical is
  a regression of the cookie-cutter intent — review will catch it.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — module layout, helper contracts
2. Read the relevant draft-ietf-jmap-metadata-04 section for the helper you are changing
3. Run `bd ready` — check for open issues before creating new ones

## What This Is

Pure, stateless helpers for draft-ietf-jmap-metadata-04: depth and
namespace validation (§2.1, §3.2), `/changes` classification (§3.3),
and `metadata*` filter predicates (§3.6). Extension servers call them
from their own `Foo/get`, `Foo/set`, `Foo/changes`, and `Foo/query`
handlers (integration tracked by bd JMAP-s4kp).

## Crate Family Context

```
jmap-metadata-types    data types
    └── jmap-metadata-server  ← this crate (sibling of canonical jmap-mail-server)
```

## Spec Reference

```
https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04   ← normative
~/PROJECT/jmap-chat-spec/references/rfc8620.txt                     ← base protocol
```

Before changing a helper, read its section in the draft. Semantics
(depth counting, namespace support, match rules) must match exactly.

## Permission enforcement: caller canonical

Per the workspace AGENTS.md "Caller identity (foundation seam)"
section, **backends are canonical for permission enforcement**. The
helpers here take no caller context and do NO permission checking.

The metadata-specific implication: `privateMetadata` is per-user
(§4). Every listed data type's `/get`, `/changes`, `/query`, and
`/queryChanges` must filter `privateMetadata` against the caller's
identity, and a private write advances state only for the writer
(§2.2). That enforcement lives in each extension server's backend,
read via `JmapBackend::principal_id`. Backends returning `None` cannot
implement it and must not advertise `supportsPrivate: true` (§2.2).

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Use `-o BatchMode=yes` for scp/ssh. Use `-y` for apt-get.

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-metadata-server -- -D warnings
cargo test -p jmap-metadata-server
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-metadata-server --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | None — pure synchronous functions |
| State | None — no backend trait, no storage, no dispatcher registration |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in helpers — identity-dependent rules live in the calling backend (see above) |
| Dependencies | jmap-metadata-types, serde_json |

## Subagent Guidance

- Never spawn two subagents editing the same file
- Give each subagent the relevant draft section explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond: jmap-metadata-types, serde_json
- Do not add handlers, a backend trait, or auth logic (draft-04 has no `Metadata/*` methods)
- Do not implement behavior not in draft-ietf-jmap-metadata-04 unless explicitly directed

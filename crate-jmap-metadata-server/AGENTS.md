# Agent Instructions — jmap-metadata-server

## 🧬 Sibling of the canonical jmap-mail-server — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-server`
extension-server template**. Module layout, `Backend` trait shape,
handler-registration pattern, `/set` boilerplate, error-mapping
helpers, and test-harness layout must mirror `jmap-mail-server`.
Differences are *only* the spec content (draft-ietf-jmap-metadata-01
here; RFC 8621 in mail-server).

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

1. Read `PLAN.md` — `MetadataBackend` trait, module layout, handler pattern
2. Read the relevant draft-ietf-jmap-metadata-01 section for the method you are implementing
3. Cross-check the canonical sibling `~/PROJECT/JMAP/crate-jmap-mail-server/` for the handler/backend pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Metadata extension method handlers (draft-ietf-jmap-metadata-01).
Plugs into `jmap-server`'s `Dispatcher` via `register_metadata_handlers`.
Defines `MetadataBackend` trait; consumers provide the storage impl. A
reference in-memory implementation (`memory::MemoryBackend`) is gated
behind the `memory` feature for tests and demos.

## Crate Family Context

```
jmap-types
    ├── jmap-server            dispatcher
    └── jmap-metadata-types    data types
            └── jmap-metadata-server  ← this crate (sibling of canonical jmap-mail-server)
```

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-metadata-01.txt   ← normative
~/PROJECT/jmap-chat-spec/references/rfc8620.txt                       ← base protocol
```

Before implementing any method, read its section in the relevant
draft. Method names, error conditions, and response shapes must
match exactly.

## Reference Pattern

Study `~/PROJECT/JMAP/crate-jmap-mail-server/` (the canonical
extension-server template) for:
- How the `Backend` trait is structured (analog of `MetadataBackend`)
- How handlers are registered with `Dispatcher`
- How `MemoryBackend` serves as both harness and example

Do NOT copy mail-specific types or logic.

## Permission enforcement: backend canonical

Per the workspace AGENTS.md "Caller identity (foundation seam)"
section, **backends are canonical for permission enforcement**.
Handlers in this crate do NO permission checking. Defense-in-depth
handler-side pre-checks are allowed but the backend MUST re-verify
atomically with the mutation. A handler that "trusts" a handler-side
check and skips the backend re-check is a bug.

Caller identity is read via the foundation seam
`JmapBackend::principal_id(caller: &Self::CallerCtx) -> Option<&jmap_types::Id>`.
Backends that have not wired identity (test fixtures, single-user
dev servers) return `None`; such backends CANNOT correctly implement
draft-ietf-jmap-metadata-01 `isPrivate` visibility scoping. Multi-user
production deployments MUST override the default impl.

The metadata-specific implications: the `isPrivate` flag on
`Metadata` / `Annotation` records mandates per-caller visibility
scoping — a private annotation must be visible only to the caller
that authored it. Every `Metadata/get`, `Metadata/changes`,
`Metadata/query`, and `Metadata/queryChanges` response must be
filtered by the backend against the caller's identity. The handler
computes the candidate query; the backend's read impl is the
canonical point of enforcement.

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
| Async | Always async (tokio) |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in handlers — caller's responsibility before `dispatch()`; permission enforcement lives in the backend per the section above |
| Dependencies | jmap-types, jmap-metadata-types, jmap-server, serde_json |
| Method names | Must match draft-ietf-jmap-metadata-01 exactly (`Metadata/get`, `Metadata/set`, `Metadata/query`, etc.) |
| Test harness | `MemoryBackend` gated behind `feature = "memory"` — HashMap-based, no external deps. Not production. |

## Subagent Guidance

- Spawn subagents for parallel handler work on independent methods
- Never spawn two subagents editing the same file
- Give each subagent the relevant draft section and the `MetadataBackend` trait explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond: jmap-types, jmap-metadata-types, jmap-server, serde_json
- Do not add auth logic or role checks inside handlers (backend canonical — see section above)
- Do not implement methods not in draft-ietf-jmap-metadata-01 unless explicitly directed

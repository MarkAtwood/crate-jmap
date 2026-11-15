# Agent Instructions — jmap-blob-server

## 🧬 Sibling of the canonical jmap-mail-server — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-server`
extension-server template**. Module layout, `Backend` trait shape,
handler-registration pattern, error-mapping helpers, and test-harness
layout must mirror `jmap-mail-server`. Differences are *only* the spec
content (blobext-03 / RFC 9404 here; RFC 8621 in mail-server).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-server`, **change `jmap-mail-server` first, then
  propagate** to every other extension-server sibling in the same pass.
- If `jmap-mail-server` changes, propagate the change here in the same
  pass (or file a follow-up sweep bead before merging).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — `BlobBackend` trait, module layout, handler pattern
2. Read the relevant blobext-03 / RFC 9404 section for the method you are changing
3. Cross-check `~/PROJECT/crate-jmap/crate-jmap-mail-server/` for the
   handler/backend pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Blob Management method handlers (draft-ietf-jmap-blobext-03):
`Blob/get`, `Blob/set`, `Blob/lookup`, `Blob/convert`, and RFC 9404
`Blob/upload` with the RFC 9404 modes of get and lookup. Plugs into
`jmap-server`'s `Dispatcher` via `register_blob_handlers`. Defines the
`BlobBackend` trait; a reference in-memory implementation
(`memory::MemoryBackend`) is gated behind the `memory` feature, with test
knobs behind `test-helpers`.

Design: `docs/design/blobext.md` §5–§7. The crate owns wire validation,
method semantics and error mapping; blob storage, the reverse lookup index
and media conversion are backend hooks. Scaffold only until
bd JMAP-tna1.2.4.

## Crate Family Context

```
jmap-types (incl. the data_types registry)
    ├── jmap-server            dispatcher
    └── jmap-blob-types      data types
            └── jmap-blob-server  ← this crate (sibling of canonical jmap-mail-server)
```

## Spec Reference

```
~/PROJECT/IETF/specs/draft-ietf-jmap-blobext-03.txt  ← normative (blob2)
~/PROJECT/IETF/specs/rfc9404.txt    ← normative (blob, served for older clients)
~/PROJECT/IETF/specs/rfc8620.txt    ← base protocol
```

## Non-Interactive Shell Commands

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

## Build & Test

```bash
cargo fmt --all
cargo clippy -p jmap-blob-server --all-features --all-targets -- -D warnings
cargo test -p jmap-blob-server
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-blob-server --all-features --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | Backend methods return `impl Future + Send`; no tokio runtime dep (dev-only, for tests) |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in handlers — the backend decides what a caller may see |
| Dependencies | jmap-types, jmap-blob-types, jmap-server; `tokio` dev-only plus a dev self-dependency enabling `memory` and `test-helpers` |
| Method names | Must match blobext-03 / RFC 9404 exactly |
| Test harness | `MemoryBackend` behind `feature = "memory"`. Not production. |

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond those listed above
- Do not add methods not in blobext-03 / RFC 9404 (or RFC 8620 §6.3 `Blob/copy`) unless explicitly directed

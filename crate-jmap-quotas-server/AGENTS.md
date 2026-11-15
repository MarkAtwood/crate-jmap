# Agent Instructions — jmap-quotas-server

## 🧬 Sibling of the canonical jmap-mail-server — mirror its shape

This crate is a **sibling under the canonical `jmap-mail-server`
extension-server template**. Module layout, `Backend` trait shape,
handler-registration pattern, error-mapping helpers, and test-harness
layout must mirror `jmap-mail-server`. Differences are *only* the spec
content (RFC 9425 here; RFC 8621 in mail-server).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape this crate in a way that diverges from
  `jmap-mail-server`, **change `jmap-mail-server` first, then
  propagate** to every other extension-server sibling in the same pass.
- If `jmap-mail-server` changes, propagate the change here in the same
  pass (or file a follow-up sweep bead before merging).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — `QuotasBackend` trait, module layout, handler pattern
2. Read the relevant RFC 9425 section for the method you are changing
3. Cross-check `~/PROJECT/crate-jmap/crate-jmap-mail-server/` for the
   handler/backend pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Quotas method handlers (RFC 9425): `Quota/get`, `/changes`,
`/query`, `/queryChanges`. No `/set`: RFC 9425 defines none. Plugs into
`jmap-server`'s `Dispatcher` via `register_quotas_handlers`. Defines the
`QuotasBackend` trait; a reference in-memory implementation
(`memory::MemoryBackend`) is gated behind the `memory` feature.

The handlers add RFC 9425 §4.1's `using` rule to the generic RFC 8620
handlers: `types` is cut to the recognized types and a Quota with none
is not returned, in `/get` and in `/query` / `/queryChanges`.

## Crate Family Context

```
jmap-types (incl. the data_types registry)
    ├── jmap-server            dispatcher
    └── jmap-quotas-types      data types
            └── jmap-quotas-server  ← this crate (sibling of canonical jmap-mail-server)
```

## Spec Reference

```
~/PROJECT/IETF/specs/rfc9425.txt    ← normative (JMAP Quotas)
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
cargo clippy -p jmap-quotas-server --all-features --all-targets -- -D warnings
cargo test -p jmap-quotas-server
RUSTDOCFLAGS="-D warnings" cargo doc -p jmap-quotas-server --all-features --no-deps
```

Run all four before considering any work done.

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | Backend methods return `impl Future + Send`; no tokio runtime dep (dev-only, for tests) |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in handlers — the backend decides what a caller may see |
| Dependencies | jmap-types, jmap-quotas-types, jmap-server, serde_json; `tokio` dev-only plus a dev self-dependency enabling `memory` |
| Method names | Must match RFC 9425 exactly |
| Test harness | `MemoryBackend` behind `feature = "memory"`. Not production. |

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond those listed above
- Do not add `Quota/set` or other methods not in RFC 9425 unless explicitly directed

# Agent Instructions — jmap-mail-server

## 🧬 Canonical extension-server template — siblings mirror this one

This crate is the **canonical template for the extension-server family**.
Every other `jmap-*-server` extension crate (chat, calendars, tasks, contacts,
filenode, sharing) is cookie-cut from this one — same module layout, same
`Backend` trait shape, same handler-registration pattern, same `/set`
boilerplate, same error-mapping helpers, same test-harness layout.
Differences are *only* the spec content (RFC 8621 here; the calendars
draft in calendars-server; etc.).

**The propagation rule** (workspace AGENTS.md "Canonical Templates"):

- If you reshape an extension-server sibling in a way that diverges from
  this crate, **change this crate first, then propagate** to every other
  extension-server sibling in the same pass.
- If you change this crate, **propagate the change to every extension-server
  sibling** in the same pass (or file a follow-up sweep bead before merging).
- A divergent sibling without a matching change here is a regression of
  the cookie-cutter intent — review will catch it.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — `MailBackend` trait, module layout, handler pattern
2. Read the relevant RFC 8621 section for the method you are implementing
3. Run `bd ready` — check for open issues before creating new ones

## What This Is

RFC 8621 (JMAP for Mail) method handlers. Plugs into `jmap-server`'s `Dispatcher` via
`register_mail_handlers`. Defines `MailBackend` trait; consumers provide the storage impl.
A reference in-memory implementation (`memory::MemoryBackend`) is gated behind the
`memory` feature for tests and demos.

## Crate Family Context

```
jmap-types
    ├── jmap-server          dispatcher
    └── jmap-mail-types      data types
            └── jmap-mail-server  ← this crate
```

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/rfc8621.txt   ← normative
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← base protocol
```

Before implementing any method, read its section in RFC 8621. Method names,
error conditions, and response shapes must match exactly.

## Reference Pattern

This crate **is** the canonical extension-server template. The Reference Pattern
for siblings (chat, calendars, tasks, contacts, filenode, metadata, sharing) is
to mirror this crate's shape:

- How the `MailBackend` trait is structured
- How handlers are registered with `Dispatcher` via `register_mail_handlers`
- How `MemoryBackend` (gated behind `feature = "memory"`) serves as both
  integration-test harness and runnable reference example

Outside reference: `~/PROJECT/crate-jmapchat-server/` predates this workspace
and was the original `StorageBackend` prototype; it is no longer the canonical
shape and should not be copied verbatim. Do NOT copy chat-specific types or
logic from any sibling crate.

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
shared-mailbox ACLs or per-user keyword state. Multi-user
production deployments MUST override the default impl.

The mail-specific implications: per-user keyword state on Emails in
shared Mailboxes (notably `$seen`, governed by
`MailboxRights.maySetSeen` per RFC 8621 §2) and the
`Mailbox.myRights` property itself (RFC 8621 §2) require the
backend to scope reads against the caller principal. The ownership
relationship between `Identity` records and `EmailSubmission/send`
callers (RFC 8621 §6 and §7) requires the backend to verify the
caller owns the Identity being used. The handler computes the
candidate mutation; the backend's `/set` impl is the canonical
point of enforcement.

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
| Async | `impl Future + Send` return types on backend trait methods, no `async fn`. Crate has NO tokio runtime dep — consumers pick the runtime. `tokio` is a dev-dep only, used to run integration tests (see `bd:JMAP-tco1`). |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in handlers — caller's responsibility before `dispatch()`; permission enforcement lives in the backend per the section above |
| Dependencies | jmap-types, jmap-mail-types, jmap-server, serde, serde_json, mime-tree (runtime); uuid + jmap-mime (optional, gated behind `memory` feature). `mime-tree` is the workspace's single RFC 5322 parsing gateway — see `bd:JMAP-g7wu.11`. `tokio` is dev-only. No `thiserror`. |
| Method names | Must match RFC 8621 exactly (`Email/get`, `Mailbox/set`, etc.) |
| Test harness | `MemoryBackend` gated behind `feature = "memory"` — HashMap-based, no external deps. Not production. |

## Subagent Guidance

- Spawn subagents for parallel handler work on independent methods (`email.rs` vs `mailbox.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant RFC 8621 section and the `MailBackend` trait explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond: jmap-types, jmap-mail-types, jmap-server, serde, serde_json, mime-tree (runtime), plus uuid + jmap-mime when the `memory` feature is enabled. `tokio` is dev-only.
- Do not add auth logic or role checks inside handlers (backend canonical — see section above)
- Do not implement methods not in RFC 8621 unless explicitly directed

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
3. Study `~/PROJECT/crate-jmapchat-server/` for the handler/backend pattern to follow
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

RFC 8621 (JMAP for Mail) method handlers. Plugs into `jmap-server`'s `Dispatcher` via
`register_mail_handlers`. Defines `MailBackend` trait; consumers provide the storage impl.

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

Study `~/PROJECT/crate-jmapchat-server/` for:
- How `StorageBackend` trait is structured (analog of `MailBackend`)
- How handlers are registered with `Dispatcher`
- How `MemoryBackend` in `tests/` serves as both harness and example

Do NOT copy chat-specific types or logic.

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
| Async | Always async (tokio) |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Auth | Not in handlers — caller's responsibility before `dispatch()` |
| Dependencies | jmap-types, jmap-mail-types, jmap-server, tokio, thiserror only |
| Method names | Must match RFC 8621 exactly (`Email/get`, `Mailbox/set`, etc.) |
| Test harness | `MemoryMailBackend` in `tests/` — HashMap-based, no external deps |

## Subagent Guidance

- Spawn subagents for parallel handler work on independent methods (`email.rs` vs `mailbox.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant RFC 8621 section and the `MailBackend` trait explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual (workspace AGENTS.md "Git Commit and Push Policy")
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond: jmap-types, jmap-mail-types, jmap-server, tokio, thiserror
- Do not add auth logic or role checks inside handlers
- Do not implement methods not in RFC 8621 unless explicitly directed

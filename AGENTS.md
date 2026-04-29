# Agent Instructions — jmap-mail-server

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

- Do not commit or push without explicit user approval
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond: jmap-types, jmap-mail-types, jmap-server, tokio, thiserror
- Do not add auth logic or role checks inside handlers
- Do not implement methods not in RFC 8621 unless explicitly directed

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, complete ALL steps below.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **SYNC BEADS DATA**:
   ```bash
   git pull --rebase
   bd dolt push
   git status
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Report to user** - State what is staged/unstaged; ask for approval before committing or pushing
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- git commit and git push require explicit user approval — never run them without asking
- Stage changes and report what is ready; wait for the user to say "commit" or "push"
<!-- END BEADS INTEGRATION -->

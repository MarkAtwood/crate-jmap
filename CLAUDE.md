# Project Instructions for AI Agents

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


## What This Is

RFC 8621 (JMAP for Mail) data types. Types only — no method handlers.
Depends on `jmap-types` + serde/serde_json. No tokio, no axum, no async.

**Read `PLAN.md` before starting any work.**

### Crate family position

```
jmap-types
    └── jmap-mail-types  ← this crate
            └── jmap-mail-server
```

### Related projects

| Location | Role |
|---|---|
| `~/PROJECT/crate-jmap-types/` | Dependency — shared wire types |
| `~/PROJECT/crate-jmap-mail-server/` | Consumer — method handlers |
| `~/PROJECT/stoa/` | End consumer — mail server |

## Build & Test

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

**Pre-commit gate:**
```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

## Spec References

```
~/PROJECT/jmap-chat-spec/references/rfc8621.txt   ← normative (JMAP for Mail)
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← base protocol context
```

## Architecture Overview

```
src/
  lib.rs            re-exports
  mailbox.rs        Mailbox, MailboxRole
  thread.rs         Thread
  email.rs          Email, EmailAddress, EmailBodyPart, EmailBodyValue
  identity.rs       Identity
  submission.rs     EmailSubmission, Envelope, Address
  snippet.rs        SearchSnippet
```

## Conventions & Patterns

- `#[forbid(unsafe_code)]` at crate root
- No async, no tokio
- No `.unwrap()` or `.expect()` — propagate with `?`
- Wire format is camelCase JSON — `#[serde(rename_all = "camelCase")]` on structs
- All field names and type names must match RFC 8621 exactly — verify against spec before naming anything
- Dependencies: jmap-types, serde, serde_json — no others
- Test fixtures in `tests/fixtures/` as committed `.json` files derived from RFC 8621 examples

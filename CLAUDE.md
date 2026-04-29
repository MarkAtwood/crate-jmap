# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

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

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->


## Build & Test

```bash
# Check all crates
cargo check --workspace

# Run all tests
cargo test --workspace

# Check a single crate
cargo check -p jmap-types
cargo test -p jmap-server

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

## Architecture Overview

Cargo workspace containing the full `jmap-*` crate family (RFC 8620 + RFC 8621 + Chat extension):

```
jmap-types      — shared wire types: Id, JmapRequest/Response, ResultReference, JmapError. No async.
    ├── jmap-server     — dispatcher, parse_request, ResultReference resolution, HTTP helpers.
    ├── jmap-client     — RFC 8620 base client: auth, session fetch, blob, SSE, WebSocket.
    │       ├── jmap-chat-client   — JMAP Chat method implementations.
    │       └── jmap-mail-client   — RFC 8621 method implementations.
    ├── jmap-mail-types — RFC 8621 data types: Email, Mailbox, Thread, etc. No async.
    │       ├── jmap-mail-server   — RFC 8621 method handlers, MailBackend trait.
    │       └── (jmap-mail-client also depends on this)
    └── jmap-chat-types — JMAP Chat extension types: Chat, Message, Space, etc. No async.
            ├── jmap-chat-server   — Chat method handlers, ChatBackend trait.
            └── (jmap-chat-client also depends on this)
```

Dependency rule: type crates (`*-types`) have no async deps. Server crates (`*-server`)
may depend on tokio/http. Client crates (not yet in this workspace) follow the same rule.

Each crate has a `PLAN.md` with full design rationale, source material references, and
test strategy. Read it before implementing anything in that crate.

## Source Material

### Specs (normative)

`~/PROJECT/jmap-chat-spec/references/` contains every relevant RFC and IETF draft as plain text:
- `rfc8620.txt` — JMAP base protocol (wire types, ResultReference, Session, errors)
- `rfc8621.txt` — JMAP for Mail (Email, Mailbox, Thread, Identity, EmailSubmission)
- `draft-ietf-jmap-*.txt` — calendars, contacts, blob extensions, etc.

JMAP Chat extension drafts are in `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-*.md`.

Read the spec section before implementing anything. Do not guess at wire field names.

### Reference Implementations

These are **read-only** — do not modify, do not add to this workspace:

| Path | Key content |
|---|---|
| `~/PROJECT/crate-jmapchat-server/jmapchat-server/` | `StorageBackend` trait, `RefStore`, dispatch/ResultReference tests |
| `~/PROJECT/crate-jmapchat-server/jmapchat-types/` | `Clearable<T>`, `#[non_exhaustive]`, serde conventions |
| `~/PROJECT/crate-jmapchat-client/` | Client-side type usage patterns |
| `~/PROJECT/kith/crates/kith-core/` | Original `JmapError`, wire types, `ResultReference` |
| `~/PROJECT/kith/crates/kith-jmap/` | Dispatcher, `parse_request`, ResultReference resolution |
| `~/PROJECT/stoa/crates/mail/` | JMAP mail consumer — dispatch, session/capability structs |

Each crate's `PLAN.md` cites exact files and line numbers to extract from.

For Rust crates not in `~/PROJECT`, check `~/GIT` and `~/WORK` before fetching from the network.

## Conventions & Patterns

- **Path deps**: each crate references siblings via `path = "../crate-jmap-*"` — do not
  change to version deps until publishing.
- **Test oracles**: tests must use independent fixtures (hand-written JSON from RFC examples,
  or OpenSSL/pyca output). Never derive expected values from the code under test.
- **No async in type crates**: `jmap-types`, `jmap-mail-types`, `jmap-chat-types` must not
  depend on tokio or any async runtime.
- **`crate-jmapchat-*` dirs** (outside this workspace): reference/inspiration only — not
  members of this workspace and not to be modified here.
- **Crate naming**: crate name = `jmap-*`, directory name = `crate-jmap-*`.

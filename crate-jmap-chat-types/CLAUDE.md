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

JMAP Chat extension data types. Types only — no method handlers.
Depends on `jmap-types` + serde/serde_json. No tokio, no axum, no async.

Will supersede the type bundling in `crate-jmapchat-server` and `crate-jmapchat-client`.

**Read `PLAN.md` before starting any work.**

### Crate family position

```
jmap-types
    └── jmap-chat-types  ← this crate
            ├── jmap-chat-server  (currently crate-jmapchat-server)
            └── jmap-chat-client  (currently crate-jmapchat-client)
```

### Related projects

| Location | Role |
|---|---|
| `~/PROJECT/crate-jmap-types/` | Dependency — shared wire types |
| `~/PROJECT/crate-jmapchat-server/` | Current consumer (bundles its own types; will migrate) |
| `~/PROJECT/crate-jmapchat-client/` | Current consumer (bundles its own types; will migrate) |
| `~/PROJECT/jmap-chat-spec/` | Normative spec drafts |
| `~/PROJECT/kith/` | Original implementation source |

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

The normative source for all type names and field names:

| Draft | Path | Covers |
|---|---|---|
| Core objects | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md` | Chat, Message, Space, ChatContact, ReadPosition |
| Push | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md` | Push payload types |
| WebSocket | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md` | Ephemeral events |
| Federation | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-federation-00.md` | Peer types |
| FileNode | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md` | File attachment objects |
| CID | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md` | Content identifier scheme |

Stale copies exist in `jmap-chat-js/docs/` and `jmap-chat-jsbig/docs/` — always use
`~/PROJECT/jmap-chat-spec/` as the authoritative source.

## Architecture Overview

```
src/
  lib.rs        re-exports
  chat.rs       Chat
  message.rs    Message
  space.rs      Space
  contact.rs    ChatContact
  position.rs   ReadPosition
  ephemeral.rs  EphemeralEvent (WebSocket ephemeral events)
  push.rs       Push subscription payload types
```

## Conventions & Patterns

- `#[forbid(unsafe_code)]` at crate root
- No async, no tokio
- No `.unwrap()` or `.expect()` — propagate with `?`
- Wire format is camelCase JSON — `#[serde(rename_all = "camelCase")]`
- All field and type names must match the spec drafts exactly — grep the spec before naming anything
- Dependencies: jmap-types, serde, serde_json — no others
- Test fixtures in `tests/fixtures/` as committed `.json` files derived from spec examples

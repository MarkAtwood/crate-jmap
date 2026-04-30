# Agent Instructions

This is a **Cargo workspace** for the `jmap-*` Rust crate family (RFC 8620, RFC 8621, and
the JMAP Chat extension). All five crates live in `crate-jmap-*/` subdirectories.

## Crate Map

| Directory | Crate | Role |
|---|---|---|
| `crate-jmap-types/` | `jmap-types` | Shared wire types — foundation, no async |
| `crate-jmap-mail-types/` | `jmap-mail-types` | RFC 8621 data types, no async |
| `crate-jmap-chat-types/` | `jmap-chat-types` | JMAP Chat extension types, no async |
| `crate-jmap-server/` | `jmap-server` | Dispatcher + parse + HTTP helpers |
| `crate-jmap-client/` | `jmap-client` | RFC 8620 base client: auth, session, blob, SSE, WebSocket |
| `crate-jmap-mail-server/` | `jmap-mail-server` | RFC 8621 method handlers (greenfield) |
| `crate-jmap-mail-client/` | `jmap-mail-client` | RFC 8621 client methods (greenfield) |
| `crate-jmap-chat-server/` | `jmap-chat-server` | JMAP Chat method handlers (greenfield) |
| `crate-jmap-chat-client/` | `jmap-chat-client` | JMAP Chat client methods (greenfield) |

Read the crate's `PLAN.md` before touching its code.

## 🔒 Locked Crates

The following crates are **locked** — public API, wire format, type names, field
names, serde attributes, and design conventions are stabilized. Agents must not
modify them without explicit per-change user approval:

| Crate | Directory |
|---|---|
| `jmap-types` | `crate-jmap-types/` |
| `jmap-mail-types` | `crate-jmap-mail-types/` |
| `jmap-chat-types` | `crate-jmap-chat-types/` |
| `jmap-client` | `crate-jmap-client/` |
| `jmap-mail-server` | `crate-jmap-mail-server/` |

Each crate's `AGENTS.md` lists the full restriction. When in doubt: stop and ask.

## Source Material

### Specs (normative)

All live at `~/PROJECT/jmap-chat-spec/references/`:

| File | Covers |
|---|---|
| `rfc8620.txt` | JMAP base protocol — wire types, ResultReference, Session, error codes |
| `rfc8621.txt` | JMAP for Mail — Email, Mailbox, Thread, Identity, EmailSubmission |
| `draft-atwood-jmap-chat-*.md` | JMAP Chat extension (in `~/PROJECT/jmap-chat-spec/`) |
| `draft-ietf-jmap-*.txt` | Other IETF JMAP extensions (calendars, contacts, blob, etc.) |

When implementing anything, read the relevant RFC section first. Do not guess at wire field names.

### Reference Implementations (local — read, do not modify)

| Path | What to look for |
|---|---|
| `~/PROJECT/crate-jmapchat-server/jmapchat-server/` | Handler/backend pattern, `StorageBackend` trait, `RefStore`, dispatch tests |
| `~/PROJECT/crate-jmapchat-server/jmapchat-types/` | Type idioms: `Clearable<T>`, `#[non_exhaustive]`, serde rename conventions |
| `~/PROJECT/crate-jmapchat-client/` | Client-side type usage |
| `~/PROJECT/kith/crates/kith-core/` | Original `JmapError`, `JmapRequest/Response`, `ResultReference` source |
| `~/PROJECT/kith/crates/kith-jmap/` | Original dispatcher, `parse_request`, ResultReference resolution |
| `~/PROJECT/stoa/crates/mail/` | JMAP mail consumer — `dispatch.rs`, session/capability structs |

The PLAN.md in each crate identifies exactly which files and line numbers to draw from.

### Broader Ecosystem

For Rust crates not in `~/PROJECT`, check `~/GIT` and `~/WORK` before reaching for the network.

## Git Commit Policy

Always ask before `git commit` or `git push`.

**Exception — fix/test loops**: When operating in a review or fix loop (e.g. invoked via a `~/PROMPT-*.md` prompt, a beads workflow, or any iterative fix-test cycle), committing code changes after each fix is permitted without asking. Pushing to remote still requires explicit user confirmation.

## Key Rules

- **`cargo test --workspace`** must pass before any commit.
- **No async** in `*-types` crates — no tokio, no futures.
- **`crate-jmapchat-*`** directories in `../` are reference/inspiration only; do not
  modify them and do not add them to this workspace.
- Test oracles must be independent of the code under test (RFC example JSON, OpenSSL output).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd dolt push          # Push beads data to remote
```

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

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

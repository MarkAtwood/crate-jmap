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

Shared JMAP wire types for the jmap-* crate family. Depends only on serde, serde_json,
and thiserror — no tokio, no axum, no async. Client and server crates both depend on this.

**Read `PLAN.md` before starting any work.**

### Crate family

```
jmap-types  ← this crate
    ├── jmap-server
    ├── jmap-mail-types
    ├── jmap-chat-types
    └── (future extensions)
```

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
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← normative
```

## Architecture Overview

```
src/
  lib.rs        re-exports
  id.rs         Id, UTCDate, State
  error.rs      JmapError and constructors
  wire.rs       JmapRequest, JmapResponse, Invocation
  resultref.rs  ResultReference, Argument<T>
```

No dispatcher, no HTTP glue. Pure types + serde + thiserror.

## Conventions & Patterns

- `#[forbid(unsafe_code)]` at crate root
- No async, no tokio
- No `.unwrap()` or `.expect()` — propagate with `?`
- Wire format is camelCase JSON — `#[serde(rename_all = "camelCase")]` on structs
- Dependencies: serde, serde_json, thiserror — no others
- Test fixtures in `tests/fixtures/` as committed `.json` files derived from RFC 8620 examples

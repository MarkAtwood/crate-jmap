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


## What This Is

A backend-agnostic JMAP server framework library for Rust. Implements the RFC 8620 wire
protocol, request parsing, ResultReference resolution, and the `Dispatcher` machinery.

No opinion on authentication, method sets, capability URIs, or storage. Two known consumers:
`kith` (JMAP Chat over Tailscale) and `stoa` (Usenet/email over JMAP).

**Read `PLAN.md` before starting any work.** It has the full public API, module layout,
source material locations, and migration notes for kith and stoa.

### Planned crate family

```
jmap-types                   serde/serde_json only — shared wire types
    ├── jmap-server          + tokio, axum — this crate
    ├── jmap-mail-types      + RFC 8621 data types
    │       └── jmap-mail-server
    ├── jmap-chat-types      + Chat extension data types
    │       ├── jmap-chat-server
    │       └── jmap-chat-client
    └── (future extensions)
```

Everything is fluid. See PLAN.md for details and migration notes.

### Related projects

| Location | Role |
|---|---|
| `~/PROJECT/kith/` | Primary source — types and dispatch logic being extracted |
| `~/PROJECT/stoa/` | Consumer (email/Usenet, possibly Chat too) |
| `~/GIT/jmap-client/` | Reference JMAP client (Stalwart) — patterns to study |
| `~/PROJECT/crate-jmap-types/` | Planned — shared wire types (does not exist yet) |
| `~/PROJECT/crate-jmapchat-server/` | Existing Chat server lib (will be renamed `crate-jmap-chat-server`) |
| `~/PROJECT/crate-jmapchat-client/` | Existing Chat client lib (will be renamed `crate-jmap-chat-client`) |

## Build & Test

```bash
# Build
cargo build

# Test
cargo test

# Lint (must pass before committing)
cargo clippy -- -D warnings

# Format check
cargo fmt --check

# Apply format (commit result if anything changes)
cargo fmt

# Docs
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

**Pre-commit gate** — run all of these before any commit:
```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

## Architecture Overview

```
src/
  lib.rs        re-exports; Dispatcher<CallerCtx>
  types.rs      JmapError, JmapRequest, JmapResponse, Invocation
  resultref.rs  ResultReference, Argument<T>
  parse.rs      parse_request, resolve_args
  response.rs   error_invocation, error_status, RequestError, request_error
```

The `Dispatcher<CallerCtx>` receives a `JmapRequest` (a batch of method calls) and processes
them sequentially per RFC 8620 §3.2. For each call it:
1. Resolves any `ResultReference` fields (`#ids`, `#properties`) from prior call results
2. Dispatches to the registered `JmapHandler<CallerCtx>` for that method name
3. Accumulates `createdIds` across `/set` calls (RFC 8620 §3.4)

Unknown method → `unknownMethod` error invocation (not a crash).
Each handler runs in `tokio::task::spawn` for panic isolation.

`CallerCtx` is whatever the auth layer produces — `Identity`, `()`, a session token. The
library never inspects it.

## Spec References

This crate implements **RFC 8620** (JMAP base protocol). The authoritative plaintext lives at:

```
~/PROJECT/jmap-chat-spec/references/rfc8620.txt
~/PROJECT/jmap-chat-spec/references/rfc8621.txt   (JMAP for Mail — useful structural analogue)
```

The JMAP Chat extension drafts (not this crate's concern, but relevant to consumers):

| Draft | Path |
|---|---|
| Core Chat objects | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md` |
| Push | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md` |
| WebSocket events | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md` |
| Federation | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-federation-00.md` |
| FileNode | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md` |
| CID scheme | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md` |

Copies of these drafts exist in `jmap-chat-js/docs/`, `jmap-chat-jsbig/docs/`, and
`~/GIT/ideas/` — treat those as potentially stale. `~/PROJECT/jmap-chat-spec/` is authoritative.

## Conventions & Patterns

- `#[forbid(unsafe_code)]` at crate root — no unsafe anywhere
- Always async (tokio); no sync path, no `maybe-async`
- No `.unwrap()` or `.expect()` in library code — propagate errors with `?`
- Wire format is camelCase JSON — use `#[serde(rename_all = "camelCase")]` on structs
- Role/authorization checks are NOT in the dispatcher — that is the caller's responsibility
- `max_calls` is a `parse_request` parameter, not a crate constant — each consumer sets its own
- Dependencies: serde, serde_json, tokio, axum, thiserror — no others

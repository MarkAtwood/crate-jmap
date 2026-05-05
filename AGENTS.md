# Agent Instructions

This is a **Cargo workspace** for the `jmap-*` Rust crate family (RFC 8620, RFC 8621, and
the JMAP Chat extension). All crates live in `crate-jmap-*/` subdirectories.

Read the crate's `PLAN.md` before touching its code.

## Crate Map

| Directory | Crate | Role |
|---|---|---|
| `crate-jmap-types/` | `jmap-types` | Shared wire types — foundation, no async |
| `crate-jmap-mail-types/` | `jmap-mail-types` | RFC 8621 data types, no async |
| `crate-jmap-chat-types/` | `jmap-chat-types` | JMAP Chat extension types, no async |
| `crate-jmap-server/` | `jmap-server` | Dispatcher + parse + HTTP helpers |
| `crate-jmap-base-client/` | `jmap-base-client` | RFC 8620 base client: auth, session, blob, SSE, WebSocket |
| `crate-jmap-mime/` | `jmap-mime` | MIME adapter: mime-tree → jmap-mail-types (greenfield) |
| `crate-jmap-mail-server/` | `jmap-mail-server` | RFC 8621 method handlers (greenfield) |
| `crate-jmap-mail-client/` | `jmap-mail-client` | RFC 8621 client methods (greenfield) |
| `crate-jmap-chat-server/` | `jmap-chat-server` | JMAP Chat method handlers (greenfield) |
| `crate-jmap-chat-client/` | `jmap-chat-client` | JMAP Chat client methods (greenfield) |

## Dependency Tree

```
jmap-types      — shared wire types: Id, JmapRequest/Response, ResultReference, JmapError. No async.
    ├── jmap-server         — dispatcher, parse_request, ResultReference resolution, HTTP helpers.
    ├── jmap-base-client    — RFC 8620 base client: auth, session fetch, blob, SSE, WebSocket.
    │       ├── jmap-chat-client   — JMAP Chat method implementations.
    │       └── jmap-mail-client   — RFC 8621 method implementations.
    ├── jmap-mail-types     — RFC 8621 data types: Email, Mailbox, Thread, etc. No async.
    │       ├── jmap-mime        — MIME parser adapter: mime-tree → jmap-mail-types. No async.
    │       ├── jmap-mail-server   — RFC 8621 method handlers, MailBackend trait.
    │       └── (jmap-mail-client also depends on this)
    └── jmap-chat-types     — JMAP Chat extension types: Chat, Message, Space, etc. No async.
            ├── jmap-chat-server   — Chat method handlers, ChatBackend trait.
            └── (jmap-chat-client also depends on this)
```

Type crates (`*-types`) have no async deps. Server crates may depend on tokio/http.

## Locked Crates

The following crates are **locked** — public API, wire format, type names, field names,
serde attributes, and design conventions are stabilized. Agents must not modify them
without explicit per-change user approval:

| Crate | Directory |
|---|---|
| `jmap-types` | `crate-jmap-types/` |
| `jmap-mail-types` | `crate-jmap-mail-types/` |
| `jmap-chat-types` | `crate-jmap-chat-types/` |
| `jmap-base-client` | `crate-jmap-base-client/` |
| `jmap-mail-server` | `crate-jmap-mail-server/` |

Each crate's `AGENTS.md` lists the full restriction. When in doubt: stop and ask.

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

**Pre-commit gate — run all of these before any commit:**
```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

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

The `PLAN.md` in each crate identifies exactly which files and line numbers to draw from.

### Broader Ecosystem

For Rust crates not in `~/PROJECT`, check `~/GIT` and `~/WORK` before reaching for the network.

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
- **`#[forbid(unsafe_code)]`** at every crate root.
- **No `.unwrap()` or `.expect()`** in library code — propagate errors with `?`.
- **Wire format**: camelCase JSON — `#[serde(rename_all = "camelCase")]` on all structs.

## Key Rules

- **`cargo test --workspace`** must pass before any commit.
- **No async** in `*-types` crates — no tokio, no futures.
- **`crate-jmapchat-*`** directories in `../` are reference/inspiration only.
- **Test oracles** must be independent of the code under test (RFC example JSON, OpenSSL output).

## Non-Interactive Shell Commands

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` on this system.
Always use explicit non-interactive flags:

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Other commands that may prompt: `scp`/`ssh` — use `-o BatchMode=yes`; `apt-get` — use `-y`.

## Git Commit Policy

Always ask before `git commit` or `git push`.

**Exception — fix/test loops**: When operating in a review or fix loop (e.g. invoked via a
`~/PROMPT-*.md` prompt, a beads workflow, or any iterative fix-test cycle), committing code
changes after each fix is permitted without asking. Pushing to remote still requires explicit
user confirmation.

## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` for full workflow context.

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd dolt push          # Push beads data to remote
```

Use `bd` for ALL task tracking — do NOT use TodoWrite or markdown TODO lists.
Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files.

**Beads is the only task and planning tool.** Do NOT use:
- TodoWrite / markdown TODO lists
- Scratchpad or audit files (`audit-*.md`, `plan-scratch.md`, or any similar throwaway planning file)
- MEMORY.md or any other markdown file as a knowledge store

The only permitted markdown planning artifact is a crate's `PLAN.md`, which is a permanent
design document checked into the repo — not a scratchpad. Use `bd remember` for persistent
knowledge and `bd create` for all task tracking.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete
until `git push` succeeds.

1. **File issues for remaining work** — create issues for anything needing follow-up
2. **Run quality gates** — `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`
3. **Update issue status** — close finished work, update in-progress items
4. **Push to remote**:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** — clear stashes, prune remote branches
6. **Hand off** — provide context for next session

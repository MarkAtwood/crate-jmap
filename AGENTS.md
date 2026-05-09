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

## Canonical Templates (cookie-cutter consistency)

The 25 `jmap-*` crates are deliberately cookie-cutter siblings: every type
crate looks like every other type crate, every server crate looks like
every other server crate, every client crate looks like every other client
crate, **modulo only the differences mandated by the relevant RFC or
draft**. Identical idioms, identical helper names, identical doc-comment
style, identical test layout. The differences should be the specific
JMAP capability the crate covers, nothing else.

To enforce that, certain crates are anointed as **canonical templates** for
their family. When you change a non-canonical sibling and the change
diverges from the canonical template, the rule is: **change the canonical
first, then propagate**. When you change the canonical, the rule is:
**propagate the change to every sibling in the same pass** (or file a
follow-up sweep bead before merging).

| Family | Canonical | Siblings (must mirror) |
|---|---|---|
| Foundation types | `jmap-types` | (none — sole foundation) |
| Extension types | `jmap-mail-types` | `jmap-chat-types`, `jmap-calendars-types`, `jmap-tasks-types`, `jmap-contacts-types`, `jmap-filenode-types`, `jmap-sharing-types` |
| Foundation server | `jmap-server` | (none — sole foundation) |
| Extension server | `jmap-mail-server` | `jmap-chat-server`, `jmap-calendars-server`, `jmap-tasks-server`, `jmap-contacts-server`, `jmap-filenode-server`, `jmap-sharing-server` |
| Foundation client | `jmap-base-client` | (none — sole foundation) |
| Extension client | `jmap-mail-client` | `jmap-chat-client`, `jmap-calendars-client`, `jmap-tasks-client`, `jmap-contacts-client`, `jmap-filenode-client`, `jmap-sharing-client` |

`jmap-chat-types` is *also* a canonical reference for the JMAP Chat draft
specifically (its wire format is normative for that extension), even
though the broader extension-types family takes its idiom shape from
`jmap-mail-types`.

Each canonical crate's `AGENTS.md` carries a short canonical-template
banner reminding contributors of the propagation rule. **The previous
"LOCKED — explicit permission required" framing was misleading**: those
banners were never about API stability lockdown; they were about
divergence prevention. The new wording makes the consistency intent
explicit.

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
- **Licensing**: the workspace `Cargo.toml` declares
  `license = "MIT OR Apache-2.0"` at the workspace level, and every crate
  inherits via `license.workspace = true`. **Do NOT add `LICENSE-MIT` or
  `LICENSE-APACHE` files** to any crate or to the repo root. The TOML
  metadata is sufficient for crates.io and `cargo deny`. Do not "fix"
  this convention — it is intentional.
- **Sloppy-Value pattern for IETF-defined nested objects**: type crates use
  `Option<serde_json::Value>` for fields whose value shape is defined by an
  external IETF spec (JSCalendar / RFC 8984, JSContact / RFC 9553, etc.)
  and is large or extensible. Each affected crate's `PLAN.md` documents
  the per-field rationale (e.g.
  `crate-jmap-calendars-types/PLAN.md` §1–§8,
  `crate-jmap-contacts-types/PLAN.md` §10). Do not "type out" these
  sloppy fields without explicit user approval — doing so creates large
  public types that drift as the upstream specs evolve. The preferred
  hybrid is the calendars approach: keep the public field as
  `serde_json::Value` for round-trip fidelity, and add parallel typed
  sub-types in a sibling module (e.g. `jscalendar.rs`) that consumers
  can opt into via `serde_json::from_value`.
- **TLS stack**: this workspace uses **rustls**, NOT native-tls / openssl.
  Both `reqwest` and `tokio-tungstenite` MUST be declared with
  `default-features = false` and only `rustls-tls-*` features enabled.
  Rationale: openssl pulls in C code and a recurring stream of CVEs
  (e.g. CVE-2026-42327, CVE-2026-44662 on rust-openssl 0.10.78). rustls is
  pure Rust on top of the RustCrypto stack, has a smaller attack surface,
  and aligns with this project's RustCrypto-first stance. Do not add
  `native-tls`, `default-tls`, or any feature that would re-introduce
  openssl as a transitive dependency. To verify, run
  `cargo tree -i openssl --workspace` — it MUST report
  "did not match any packages".

## Key Rules

- **`cargo test --workspace`** must pass before any commit.
- **No async** in `*-types` crates — no tokio, no futures.
- **`crate-jmapchat-*`** directories in `../` are reference/inspiration only.
- **Test oracles** must be independent of the code under test (RFC example JSON, OpenSSL output).
- **Do NOT add LICENSE files** — the workspace TOML `license = "MIT OR Apache-2.0"`
  declaration is the entire license-metadata story. See the Conventions list above.

## Non-Interactive Shell Commands

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` on this system.
Always use explicit non-interactive flags:

```bash
cp -f source dest       mv -f source dest       rm -f file
rm -rf directory        cp -rf source dest
```

Other commands that may prompt: `scp`/`ssh` — use `-o BatchMode=yes`; `apt-get` — use `-y`.

## Git Commit and Push Policy

Commit freely after completing logical units of work — no need to ask permission per
commit. Push freely too: just `git push`. The agent is the only thing pushing to
`origin/main`, so there is no `pull --rebase` ritual to dance through before each
push. If the push fails because someone else snuck a commit in (rare but possible),
*then* run `git pull --rebase && git push` — but make the simple `git push` the
default.

Exceptions where you should still pause:
- `git push --force` to `main` — never without explicit user instruction.
- Any push that would land secrets, credentials, or `.env`-shaped files.
- Any commit that creates files the user explicitly did not ask for (the
  "don't make doc files unless asked" rule still applies regardless of push policy).

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
   git push                         # simple push; no pull --rebase ritual
   bd dolt push                     # only if a Dolt remote is configured
   git status                       # MUST show "up to date with origin"
   ```
   If `git push` is rejected because someone else pushed in the meantime
   (rare — agent is the only writer), then `git pull --rebase && git push`.
5. **Clean up** — clear stashes, prune remote branches
6. **Hand off** — provide context for next session

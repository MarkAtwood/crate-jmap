# Agent Instructions — jmap-chat-types

## 🔒 LOCKED CRATE — EXPLICIT PERMISSION REQUIRED BEFORE ANY CHANGE

This crate's public API, wire format, module layout, type names, field names,
serde attributes, and design conventions are **locked and stabilized**.

**You may NOT, under any circumstances:**
- Add, rename, or remove any public type, field, or variant
- Change any serde attribute, derive, or wire format
- Change constructor signatures or add/remove constructors
- Add, remove, or upgrade dependencies
- Alter `#[non_exhaustive]` annotations
- Modify test oracles or fixture files
- Refactor or "clean up" any existing code

**To make ANY change to this crate** you must first describe the exact change to
the user and receive explicit written approval for that specific change. "Fixing
a bug" or "improving the code" is not sufficient — stop and report, then wait.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — planned types, module layout, source material
2. Read the relevant spec draft in `~/PROJECT/jmap-chat-spec/` for the types you are implementing
3. Check existing types in `~/PROJECT/crate-jmapchat-server/` before implementing from scratch
4. Run `bd ready` — check for open issues before creating new ones

## What This Is

JMAP Chat extension data types. Types only — no method handlers. No async.
Will supersede type bundling in `crate-jmapchat-server` and `crate-jmapchat-client`.
Both server and client crates will depend on this once it exists.

## Crate Family Context

```
jmap-types
    └── jmap-chat-types  ← this crate
            ├── jmap-chat-server  (currently crate-jmapchat-server)
            └── jmap-chat-client  (currently crate-jmapchat-client)
```

When changing a public type or field, check that both consumers still compile.

## Spec References

| Draft | Path | Covers |
|---|---|---|
| Core objects | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md` | Chat, Message, Space, ChatContact, ReadPosition |
| Push | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md` | Push payload types |
| WebSocket | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md` | Ephemeral events |
| Federation | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-federation-00.md` | Peer types |
| FileNode | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md` | File attachment objects |
| CID | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md` | Content identifier scheme |

Stale copies in `jmap-chat-js/docs/` and `jmap-chat-jsbig/docs/` — always use
`~/PROJECT/jmap-chat-spec/` as authoritative.

When unsure which draft defines a field: `grep -r <term> ~/PROJECT/jmap-chat-spec --include='*.md'`

## Existing Implementations to Study

- `~/PROJECT/crate-jmapchat-server/` — current type definitions (bundled with server)
- `~/PROJECT/crate-jmapchat-client/` — client-side type definitions
- `~/PROJECT/kith/crates/kith-core/` — original source

Extract and consolidate — do not rewrite from scratch.

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

## Sibling Crate Congruence (REQUIRED)

This crate must stay congruent with two siblings at all times:

| Crate | Path | Relationship | What to watch |
|---|---|---|---|
| `jmap-types` | `~/PROJECT/crate-jmap-types/` | **Dependency** — provides `Id`, `State`, `UTCDate`, `Date` | If it adds/renames/removes those types, update imports and usages here |
| `jmap-mail-types` | `~/PROJECT/crate-jmap-mail-types/` | **Pattern sibling** — the template this crate follows | If it changes struct conventions, `#[non_exhaustive]` policy, serde attribute style, or test patterns, apply the same changes here |

**Rules:**

1. Before touching any type that wraps or uses `Id`, `State`, `UTCDate`, or `Date`: read `~/PROJECT/crate-jmap-types/src/id.rs` to confirm the current API.
2. Before adding new public structs: check how `~/PROJECT/crate-jmap-mail-types/` models analogous types — copy the pattern, not the content.
3. If a change here would break `jmap-mail-types` conventions, stop and flag it to the user.
4. Do not introduce a dependency that `jmap-mail-types` does not also have, without explicit user approval.

**Check congruence with:**
```bash
cargo build -p jmap-types 2>&1        # confirm dep still compiles
diff <(cd ../crate-jmap-types && cargo metadata --no-deps --format-version 1 | jq '.packages[0].dependencies') \
     <(cargo metadata --no-deps --format-version 1 | jq '.packages[0].dependencies')
```

## Design Constraints (Settled)

| Decision | Choice |
|---|---|
| Async | None — sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json only |
| Field names | Must match spec drafts exactly |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| Test oracle | Hand-written JSON from spec examples — never from code under test |
| Constructors | **None.** No `::new()` methods in this crate. Construction is the consumer's responsibility. Downstream crates use `serde_json` deserialization to create instances. |
| Attribute order | `#[non_exhaustive]` → `#[derive(...)]` → `#[serde(...)]` on every type |

## Subagent Guidance

- Spawn subagents for parallel work on independent modules (`chat.rs` vs `message.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant spec draft section explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Do not commit or push without explicit user approval
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond jmap-types, serde, serde_json
- Do not add async, tokio, or axum
- Do not add method handler logic — types only
- Do not add fields or types not present in the spec drafts unless explicitly directed

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

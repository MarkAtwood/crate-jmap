# Agent Instructions — jmap-types

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — public API, module layout, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Grep kith-core for the relevant type before implementing from scratch

## What This Is

Shared JMAP wire types for the jmap-* crate family. Minimal deps (serde, serde_json,
thiserror). No async. Both client and server crates depend on this.

## Crate Family Context

```
jmap-types  ← this crate
    ├── jmap-server          adds dispatcher, axum, tokio
    ├── jmap-mail-types      adds RFC 8621 data types
    ├── jmap-chat-types      adds Chat extension data types
    └── (future extensions)
```

When changing a public type or adding a new one, check that downstream crates
(`jmap-server`, `jmap-mail-types`, `jmap-chat-types`) still compile.

## Source Material

Extract from kith-core — do not rewrite from scratch.

| Item | Location |
|---|---|
| `JmapRequest`, `JmapResponse`, `Invocation` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |
| `JmapError` | `~/PROJECT/kith/crates/kith-core/src/error.rs` |
| `ResultReference`, `Argument<T>` | `~/PROJECT/kith/crates/kith-core/src/resultref.rs` |
| `Id`, `UTCDate`, `State` | `~/PROJECT/kith/crates/kith-core/src/jmap.rs` |

Do NOT copy kith-specific items: `Role`, `Identity`, `AuthError`, `KithError`.

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← normative
```

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
| Async | None — this crate is sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | serde, serde_json, thiserror only |
| Error strings | Must match RFC 8620 §7.1 verbatim |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |

## Subagent Guidance

- Spawn subagents for parallel work on independent modules (`id.rs` vs `wire.rs`)
- Never spawn two subagents editing the same file
- Each subagent reads only what it needs — grep rather than loading full files
- Escalate after 3 failed attempts at the same error

## Restrictions

- Do not commit or push without explicit user approval
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond serde, serde_json, thiserror
- Do not add async, tokio, or axum
- Do not add kith-specific types (Role, Identity, etc.)

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

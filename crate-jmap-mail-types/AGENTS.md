# Agent Instructions — jmap-mail-types

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Before Starting Any Work

1. Read `PLAN.md` — planned types, module layout, source material
2. Run `bd ready` — check for open issues before creating new ones
3. Read the relevant section of RFC 8621 before implementing any type

## What This Is

RFC 8621 (JMAP for Mail) data types. Types only — no method handlers. No async.
Both `jmap-mail-server` and any future mail client depend on this.

## Crate Family Context

```
jmap-types
    └── jmap-mail-types  ← this crate
            └── jmap-mail-server
```

When changing a public type or field, check that `jmap-mail-server` still compiles.

## Spec Reference

```
~/PROJECT/jmap-chat-spec/references/rfc8621.txt   ← normative
~/PROJECT/jmap-chat-spec/references/rfc8620.txt   ← base protocol context
```

**Before naming any field or type**, grep the RFC 8621 text to confirm the exact name.
Field name mismatches cause silent serde failures.

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
| Async | None — sync only |
| Unsafe | Forbidden — `#[forbid(unsafe_code)]` |
| Dependencies | jmap-types, serde, serde_json only |
| Field names | Must match RFC 8621 exactly |
| Wire format | camelCase JSON — `#[serde(rename_all = "camelCase")]` |
| Test oracle | Hand-written JSON from RFC 8621 examples — never from code under test |

## Subagent Guidance

- Spawn subagents for parallel work on independent modules (`mailbox.rs` vs `email.rs`)
- Never spawn two subagents editing the same file
- Give each subagent the relevant RFC 8621 section explicitly
- Escalate after 3 failed attempts at the same error

## Restrictions

- Do not commit or push without explicit user approval
- Do not use TodoWrite or markdown task lists — use `bd create`
- Do not add dependencies beyond jmap-types, serde, serde_json
- Do not add async, tokio, or axum
- Do not add method handler logic — types only

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

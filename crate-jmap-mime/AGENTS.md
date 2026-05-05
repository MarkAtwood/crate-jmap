# Agent Instructions — jmap-mime

Thin adapter crate in the JMAP workspace. Converts `mime-tree` output types to
`jmap-mail-types` types. No parsing logic lives here.

Read `~/PROJECT/JMAP/AGENTS.md` before doing anything.

## Role in the Workspace

| Depends on | Why |
|---|---|
| `mime-tree` | Source of `ParsedPart`, `ParsedMessage`, `DecodedBodyValue` |
| `jmap-mail-types` | Target types: `EmailBodyPart`, `EmailBodyValue`, `EmailHeader` |
| `jmap-types` | For `Id` type used in `blobId` |

Used by: `jmap-mail-server` (and `stoa-mail` via path dep).

## The One Rule

**This crate must stay thin.** Every function here should be obvious field mapping.
If you find yourself writing a loop, a match on content-type, or any parsing logic,
stop — it belongs in `mime-tree` instead.

## Conversion Functions

```rust
// Part tree: ParsedPart → EmailBodyPart (recursive)
pub fn part_to_jmap(part: &ParsedPart, blob_id_for: impl Fn(&ParsedPart) -> Id) -> EmailBodyPart;

// Body value: DecodedBodyValue → EmailBodyValue
pub fn body_value_to_jmap(val: DecodedBodyValue) -> EmailBodyValue;

// Full body fields from a ParsedMessage
pub fn message_to_jmap_body(msg: &ParsedMessage, blob_id_for: impl Fn(&ParsedPart) -> Id) -> JmapBodyFields;
```

## Quality Gate

```bash
cargo fmt --all
cargo clippy -p jmap-mime -- -D warnings
cargo test -p jmap-mime
```

## Non-Interactive Shell Commands

```bash
cp -f source dest       # NOT: cp source dest
mv -f source dest       # NOT: mv source dest
rm -f file              # NOT: rm file
rm -rf dir              # NOT: rm -r dir
```

## Git Commit Policy

git commit and git push require explicit user approval.

**Exception — fix/test loops**: When operating in a review or fix loop (invoked via a
`~/PROMPT-*.md` prompt or beads workflow), committing after each fix is permitted without
asking. Push to remote still requires explicit user confirmation.

## Beads Issue Tracker

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
bd dolt push          # Sync beads data
```

Use `bd` for ALL task tracking. Do not use TodoWrite, TaskCreate, or markdown TODO lists.

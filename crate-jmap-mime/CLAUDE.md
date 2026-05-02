# jmap-mime — Project Instructions for AI Agents

Thin adapter crate within the JMAP workspace. Converts `mime-tree` types to
`jmap-mail-types` types. All heavy parsing lives in `mime-tree`; this crate only bridges.

## What This Is

Depends on two crates:

- `mime-tree` — standalone MIME parser (see `~/PROJECT/MIME/`)
- `jmap-mail-types` — RFC 8621 data types (see `../crate-jmap-mail-types/`)

Provides:

- `ParsedPart → EmailBodyPart` conversion (caller supplies `blobId`)
- `DecodedBodyValue → EmailBodyValue` conversion
- Helper to build the full `bodyStructure` / `textBody` / `htmlBody` / `attachments`
  arrays from a `ParsedMessage`, given a `blobId` resolver

## Hard Design Invariants

1. **Stay thin.** If logic belongs in the MIME parser, put it in `mime-tree`, not here.
   This crate must not re-implement any MIME parsing or RFC 8621 §4.1.4 traversal.
2. **Caller supplies `blobId`.** This crate cannot know how the storage layer identifies
   blobs. All conversion functions accept a `blob_id_for: impl Fn(&ParsedPart) -> Id`
   closure that the caller provides.
3. **No async.** This is a type-conversion crate. No tokio, no futures.
4. **Locked once stable.** When the public API stabilizes, treat it as locked — changes
   require explicit user approval. See the workspace `AGENTS.md` for the locked crate list.

## API Shape

```rust
/// Convert a ParsedPart (and its children) into an EmailBodyPart tree.
/// `blob_id_for` is called for each non-multipart leaf to assign a blobId.
pub fn part_to_jmap(
    part: &ParsedPart,
    blob_id_for: impl Fn(&ParsedPart) -> Id,
) -> EmailBodyPart;

/// Convert a DecodedBodyValue into an EmailBodyValue.
pub fn body_value_to_jmap(val: DecodedBodyValue) -> EmailBodyValue;

/// Build the full body fields from a ParsedMessage.
/// Returns (body_structure, text_body, html_body, attachments, body_values_keys).
/// body_values_keys lists the partIds whose content should be fetched via
/// decode_body_value() and inserted into the bodyValues map by the caller.
pub fn message_to_jmap_body(
    msg: &ParsedMessage,
    blob_id_for: impl Fn(&ParsedPart) -> Id,
) -> JmapBodyFields;
```

## Build & Test

This crate is a member of the JMAP workspace. Run from workspace root:

```bash
cargo check -p jmap-mime
cargo test -p jmap-mime
cargo clippy -p jmap-mime -- -D warnings
cargo fmt --all
```

## Relation to mime-tree

`mime-tree` is the authoritative source for:
- All MIME parsing logic
- RFC 8621 §4.1.4 body structure traversal
- Transfer-encoding decode and charset conversion

Do not duplicate any of that here. If a conversion requires logic beyond field mapping,
the logic belongs in `mime-tree`.

## Workspace Context

See `~/PROJECT/JMAP/CLAUDE.md` for full workspace conventions, build commands,
locked crate list, source material, and session completion protocol.

Crate naming convention: crate name = `jmap-mime`, directory = `crate-jmap-mime`.

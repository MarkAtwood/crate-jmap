# jmap-chat-types — Implementation Plan

JMAP Chat extension data types. No method handlers — types only.
Depends on `jmap-types` + serde/serde_json. No tokio, no axum, no async.

## Crate Family Position

```
jmap-types
    └── jmap-chat-types  ← this crate
            ├── jmap-chat-server   adds method handlers, tokio
            └── jmap-chat-client   HTTP client
```

## What This Crate Is

All data types defined by the JMAP Chat extension drafts: `Chat`, `Message`, `Space`,
`ChatContact`, `ReadPosition`, and supporting types. Extracted so both server and client
crates share one definition without pulling in server-side deps.

Will supersede the type bundling currently in `crate-jmapchat-server`. The existing
`crate-jmapchat-server` and `crate-jmapchat-client` will depend on this crate once it exists.

## What This Crate Is Not

- Not a method dispatcher or handler
- Not async

## Source Material

The normative source is the JMAP Chat spec drafts. Existing type implementations
live in `crate-jmapchat-server` and `crate-jmapchat-client` — extract, don't rewrite.

| Draft | Path | Covers |
|---|---|---|
| Core objects | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-00.md` | Chat, Message, Space, ChatContact, ReadPosition |
| Push | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-push-00.md` | Push subscription payloads |
| WebSocket | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-wss-00.md` | Ephemeral events |
| Federation | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-federation-00.md` | Peer types |
| FileNode | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md` | File attachment objects |
| CID | `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-cid-00.md` | Content identifier scheme |

Existing implementations to study:
- `~/PROJECT/crate-jmapchat-server/` — current type definitions (bundled with server)
- `~/PROJECT/crate-jmapchat-client/` — client-side type definitions
- `~/PROJECT/kith/crates/kith-core/` — original source types

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

No other dependencies.

## Module Layout

```
src/
  lib.rs        re-exports
  chat.rs       Chat, ChatMember, ChannelPermission
  clearable.rs  Clearable<T> — null-vs-absent patch helper
  contact.rs    ChatContact, Endpoint
  emoji.rs      CustomEmoji
  ephemeral.rs  EphemeralMessage (WebSocket events)
  message.rs    Message, Attachment, Mention, MessageAction, Reaction, MessageRevision, DeliveryReceipt
  position.rs   ReadPosition
  presence.rs   PresenceStatus
  push.rs       ChatMessagePush, ChatMessageEntry, ChatPushConfig
  space.rs      Space, SpaceRole, SpaceMember, Category, SpaceInvite, SpaceBan
```

## Test Strategy

- Serde round-trips against hand-written JSON derived from spec examples
- Tests are inline `#[cfg(test)]` modules in each source file
- No live network, no external services

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

Every public `Deserialize` struct that appears on the JMAP wire carries an
`extra` field per the workspace extras-preservation policy (see workspace
`AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

This preserves vendor / site / private-extension fields across
deserialize/serialize round-trip. Wire format is byte-identical when extras
are empty.

In scope in this crate (each has at least one round-trip preservation test):

- `Chat`, `ChatMember`, `ChannelPermission` (`chat.rs`).
- `ChatContact`, `Endpoint` (`contact.rs`).
- `CustomEmoji` (`emoji.rs`).
- `ChatStreamEnable`, `ChatStreamDisable`, `ChatTypingEvent`,
  `ChatPresenceEvent` (`ephemeral.rs`).
- `Message`, `Attachment`, `Mention`, `MessageAction`, `Reaction`,
  `MessageRevision`, `DeliveryReceipt` (`message.rs`).
- `ReadPosition` (`position.rs`).
- `PresenceStatus` (`presence.rs`).
- `ChatPushConfig`, `ChatMessageEntry`, `ChatMessagePush` (`push.rs`).
- `Space`, `SpaceRole`, `SpaceMember`, `Category`, `SpaceInvite`,
  `SpaceBan` (`space.rs`).
- `ChannelCreate`, `RolePatch`, `MemberPatch`, `ChannelPatch`,
  `CategoryPatch` (`space_set.rs` — Space/set method-argument structs).

Out of scope (explicitly excluded by the workspace policy):

- String enums (`ChatKind`, `Presence`, `DeliveryState`, `SenderId`,
  `ReadDisposition`, etc.) — result enums tracked via separate
  `Unknown(String)` propagation; control enums get neither.
- `Clearable<T>` — internal three-state helper, not a wire object.
- `SpacePatchOp` — internal Rust representation of Space/set wire keys;
  has no wire form of its own.
- Newtypes wrapping a single value.

### New-type rule

Any new public `Deserialize` struct added to this crate that appears on
the JMAP wire MUST include the `extra` field from day one with the
documented serde attributes and at least one round-trip preservation
test. This crate is normative for the JMAP Chat draft, so the policy
applies even more strictly: vendor/site fields on Chat objects MUST
round-trip unchanged regardless of which crate version saw them first.

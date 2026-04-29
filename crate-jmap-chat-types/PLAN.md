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

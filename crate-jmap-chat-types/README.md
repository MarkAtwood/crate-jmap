# jmap-chat-types

Serde-annotated Rust types for the JMAP Chat extension
([draft-atwood-jmap-chat](https://github.com/MarkAtwood/jmap-chat-spec)).

**Types only** — no async, no network I/O, no method handlers.

## What's in here

| Module | Types |
|--------|-------|
| `chat` | `Chat`, `ChatMember`, `ChannelPermission` |
| `message` | `Message`, `Attachment`, `Mention`, `MessageAction`, `Reaction`, `MessageRevision`, `DeliveryReceipt` |
| `space` | `Space`, `SpaceRole`, `SpaceMember`, `Category`, `SpaceInvite`, `SpaceBan` |
| `contact` | `ChatContact`, `Endpoint` |
| `position` | `ReadPosition` |
| `presence` | `PresenceStatus` |
| `emoji` | `CustomEmoji` |
| `ephemeral` | `EphemeralMessage` — WebSocket ephemeral events |
| `push` | `ChatMessagePush`, `ChatMessageEntry`, `ChatPushConfig` |
| `clearable` | `Clearable<T>` — null-vs-absent JSON patch helper |

## Usage

Add to `Cargo.toml`:

```toml
jmap-chat-types = { path = "../crate-jmap-chat-types" }
```

Deserialize from a JMAP Chat JSON response:

```rust
use jmap_chat_types::Chat;
use serde_json;

let chat: Chat = serde_json::from_str(json_str)?;
```

The `EphemeralMessage` enum covers all WebSocket events. Unknown `@type` values
deserialize to `EphemeralMessage::Unknown` for forward compatibility.

## Spec references

| Draft | Covers |
|-------|--------|
| `draft-atwood-jmap-chat-00` | Core objects: Chat, Message, Space, ChatContact, ReadPosition |
| `draft-atwood-jmap-chat-push-00` | Push notification payloads |
| `draft-atwood-jmap-chat-wss-00` | WebSocket ephemeral events |

## Crate family

```
jmap-types
    └── jmap-chat-types  ← this crate
            ├── jmap-chat-server
            └── jmap-chat-client
```

## License

MIT OR Apache-2.0

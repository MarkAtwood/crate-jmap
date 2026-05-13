# jmap-chat-types

Serde-annotated Rust types for the JMAP Chat extension
([draft-atwood-jmap-chat](https://github.com/MarkAtwood/jmap-chat-spec)).

**Types only** — no async, no network I/O, no method handlers.

## What it is

The full set of public re-exports from `lib.rs`. Every item below is also
available from the crate root (e.g. `jmap_chat_types::Chat`).

### Object types

| Module | Types |
|--------|-------|
| `chat` | `Chat`, `ChatKind`, `ChatMember`, `ChannelPermission` |
| `message` | `Message`, `Attachment`, `Mention`, `MessageAction`, `Reaction`, `MessageRevision`, `DeliveryReceipt`, `DeliveryState`, `ReadDisposition`, `SenderId` |
| `space` | `Space`, `SpaceRole`, `SpaceMember`, `Category`, `SpaceInvite`, `SpaceBan` |
| `contact` | `ChatContact`, `Endpoint` |
| `position` | `ReadPosition` |
| `presence` | `Presence`, `PresenceStatus` |
| `emoji` | `CustomEmoji` |
| `clearable` | `Clearable<T>` — null-vs-absent JSON patch helper |

### Push and ephemeral transports

| Module | Types |
|--------|-------|
| `ephemeral` | `EphemeralMessage` — outer WebSocket envelope, plus typed events: `ChatPresenceEvent`, `ChatStreamDisable`, `ChatStreamEnable`, `ChatTypingEvent` |
| `push` | `ChatMessagePush`, `ChatMessageEntry`, `ChatPushConfig` |

### `properties` enums (for `*/get` projections)

Re-exported from the `backend` module. Each enumerates the legal property
names for the corresponding object in a JMAP `*/get` request:

| Enum | Object |
|---|---|
| `ChatProperty` | `Chat` |
| `MessageProperty` | `Message` |
| `SpaceProperty` | `Space` |
| `SpaceBanProperty` | `SpaceBan` |
| `SpaceInviteProperty` | `SpaceInvite` |
| `ChatContactProperty` | `ChatContact` |
| `CustomEmojiProperty` | `CustomEmoji` |
| `PresenceStatusProperty` | `PresenceStatus` |
| `ReadPositionProperty` | `ReadPosition` |

## What it's for

Consumed by `jmap-chat-server` (method handlers + the `ChatBackend` trait)
and `jmap-chat-client` (typed method bindings). This crate is the canonical
reference for the `draft-atwood-jmap-chat-00` wire format — type names,
serde attributes, and field structure here are normative for the draft.
Sibling to `jmap-mail-types` in the workspace's extension-types family;
shape (module layout, doc style, test patterns) mirrors that template
while content tracks the JMAP Chat draft.

## How to use

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

## How it works

No async — `jmap-chat-types` depends only on `jmap-types`, `serde`, and
`serde_json`. All structs carry `#[serde(rename_all = "camelCase")]` so
Rust `snake_case` field names map to the JMAP `camelCase` wire format
automatically. Every public struct and enum is `#[non_exhaustive]` so
new variants and fields can be added without breaking downstream
consumers, and the crate root declares `#[forbid(unsafe_code)]`. Data-
object types (Chat, Message, Space, etc.) carry a flattened `extra:
serde_json::Map<String, serde_json::Value>` catch-all so vendor and site
fields round-trip losslessly, and wire-format result enums carry an
`Unknown(String)` variant via `#[serde(other)]` so unrecognised result
strings round-trip back to the same wire value.

## Gotchas

- **No validation of message content.** `Message.body` is an unvalidated string; length limits and character restrictions defined by the draft are not enforced at the type layer.
- **`EphemeralMessage::Unknown` for forward compatibility.** Any WebSocket event type not recognized by this crate deserializes to `EphemeralMessage::Unknown { type_name, payload }`. Callers must handle this variant; match exhaustion without it will not compile.
- **Draft spec only.** `draft-atwood-jmap-chat` has not been submitted to the IETF. Wire format and type names may change.

## References

| Document | Covers |
|---|---|
| [draft-atwood-jmap-chat-00] | Core objects: Chat, Message, Space, ChatContact, ReadPosition |
| [draft-atwood-jmap-chat-push-00] | Push notification payloads |
| [draft-atwood-jmap-chat-wss-00] | WebSocket ephemeral events |

## Crate family

```
jmap-types
    └── jmap-chat-types  ← this crate
            ├── jmap-chat-server
            └── jmap-chat-client
```

[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-push-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec

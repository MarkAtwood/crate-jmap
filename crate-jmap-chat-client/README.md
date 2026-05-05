# jmap-chat-client

Typed client methods for the JMAP Chat extension ([draft-atwood-jmap-chat]).

Implements an extension trait on `jmap-base-client::JmapClient` that adds all
JMAP Chat method calls as typed async methods, following the same session-bound
`SessionClient` pattern used by `jmap-mail-client`.

## Usage

```rust
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_chat_client::{JmapChatExt, GetResponse};
use jmap_chat_types::Chat;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auth = BearerAuth::new("my-token")?;
    let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

    let session = client.fetch_session().await?;
    let chat = client.with_chat_session(session);

    // Fetch all chats for the primary account.
    let resp: GetResponse<Chat> = chat.chat_get(None, None).await?;
    for c in &resp.list {
        println!("{}: {:?}", c.id, c.name);
    }
    Ok(())
}
```

## Registered methods

All JMAP Chat methods are available as typed async methods on `SessionClient`:

### Chat/*

| Method | Parameters | Returns |
|---|---|---|
| `chat_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<Chat>` |
| `chat_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `chat_query` | `input: &ChatQueryInput` | `QueryResponse` |
| `chat_query_changes` | `since_query_state: &str, max_changes: Option<u64>` | `QueryChangesResponse` |
| `chat_create` | `input: &ChatCreateInput<'_>` | `SetResponse` |
| `chat_update` | `input: &ChatUpdateInput<'_>` | `SetResponse` |
| `chat_destroy` | `ids: &[&str]` | `SetResponse` |
| `chat_typing` | `chat_id: &str, typing: bool` | `TypingResponse` |

### Message/*

| Method | Parameters | Returns |
|---|---|---|
| `message_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<Message>` |
| `message_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `message_query` | `input: &MessageQueryInput` | `QueryResponse` |
| `message_query_changes` | `since_query_state: &str, max_changes: Option<u64>` | `QueryChangesResponse` |
| `message_create` | `input: &MessageCreateInput<'_>` | `SetResponse` |
| `message_update` | `input: &MessageUpdateInput<'_>` | `SetResponse` |
| `message_destroy` | `ids: &[&str]` | `SetResponse` |

### Space/*

| Method | Parameters | Returns |
|---|---|---|
| `space_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<Space>` |
| `space_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `space_query` | `input: &SpaceQueryInput` | `QueryResponse` |
| `space_query_changes` | `since_query_state: &str, max_changes: Option<u64>` | `QueryChangesResponse` |
| `space_create` | `input: &SpaceCreateInput<'_>` | `SetResponse` |
| `space_update` | `input: &SpaceUpdateInput<'_>` | `SetResponse` |
| `space_destroy` | `ids: &[&str]` | `SetResponse` |
| `space_join` | `input: &SpaceJoinInput<'_>` | `SpaceJoinResponse` |

### SpaceInvite/*

| Method | Parameters | Returns |
|---|---|---|
| `space_invite_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<SpaceInvite>` |
| `space_invite_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `space_invite_create` | `input: &SpaceInviteCreateInput<'_>` | `SetResponse` |
| `space_invite_destroy` | `ids: &[&str]` | `SetResponse` |

### SpaceBan/*

| Method | Parameters | Returns |
|---|---|---|
| `space_ban_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<SpaceBan>` |
| `space_ban_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `space_ban_create` | `input: &SpaceBanCreateInput<'_>` | `SetResponse` |
| `space_ban_destroy` | `ids: &[&str]` | `SetResponse` |

### ChatContact/*

| Method | Parameters | Returns |
|---|---|---|
| `chat_contact_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<ChatContact>` |
| `chat_contact_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `chat_contact_query` | `input: &ChatContactQueryInput` | `QueryResponse` |
| `chat_contact_query_changes` | `since_query_state: &str, max_changes: Option<u64>` | `QueryChangesResponse` |
| `chat_contact_update` | `input: &ChatContactUpdateInput<'_>` | `SetResponse` |

### CustomEmoji/*

| Method | Parameters | Returns |
|---|---|---|
| `custom_emoji_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<CustomEmoji>` |
| `custom_emoji_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `custom_emoji_query` | `input: &CustomEmojiQueryInput` | `QueryResponse` |
| `custom_emoji_query_changes` | `since_query_state: &str, max_changes: Option<u64>` | `QueryChangesResponse` |
| `custom_emoji_create` | `input: &CustomEmojiCreateInput<'_>` | `SetResponse` |
| `custom_emoji_destroy` | `ids: &[&str]` | `SetResponse` |

### ReadPosition/* and PresenceStatus/*

| Method | Parameters | Returns |
|---|---|---|
| `read_position_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<ReadPosition>` |
| `read_position_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `read_position_update` | `input: &ReadPositionUpdateInput<'_>` | `SetResponse` |
| `presence_status_get` | _(none)_ | `GetResponse<PresenceStatus>` |
| `presence_status_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `presence_status_update` | `input: &PresenceStatusUpdateInput<'_>` | `SetResponse` |

### Push subscriptions

| Method | Parameters | Returns |
|---|---|---|
| `push_subscription_create` | `input: &PushSubscriptionCreateInput<'_>` | `PushSubscriptionCreateResponse` |

## Known Limitations

- **`space_join` is non-standard.** `Space/join` is a JMAP Chat extension method
  that does not follow the standard `/set` request shape. It takes a
  `SpaceJoinInput` struct (not a `create`/`update`/`destroy` map) and returns a
  `SpaceJoinResponse` (not a `SetResponse`). It cannot be used with
  `JmapRequestBuilder::add_call` in combination with other `/set` invocations in
  a multi-method request — use it as a standalone call.
- **No `Chat/queryChanges` in the current server spec.** Some object types have
  `queryChanges` on the client without a corresponding handler in
  `jmap-chat-server`; check server capability before calling.

## References

- **[draft-atwood-jmap-chat]** — JMAP Chat extension (all sub-drafts: core
  objects, push, WebSocket events, federation, FileNode, CID scheme)
  — <https://github.com/MarkAtwood/jmap-chat-spec>
- **[RFC 8620]** — JMAP Core (request/response envelope, `/set` and `/query`
  shapes, push subscription, SSE, WebSocket)

[draft-atwood-jmap-chat]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0

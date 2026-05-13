# jmap-chat-client

## What it is

JMAP Chat extension client methods (draft-atwood-jmap-chat-00) — typed bindings on top of `jmap-base-client`.

Typed client methods for the JMAP Chat extension ([draft-atwood-jmap-chat]).

Implements an extension trait on `jmap-base-client::JmapClient` that adds all
JMAP Chat method calls as typed async methods, following the same session-bound
`SessionClient` pattern used by `jmap-mail-client`.

## What it's for

Implements draft-atwood-jmap-chat-00 method bindings (`Chat/*`, `Message/*`,
`Space/*`, `SpaceInvite/*`, `SpaceBan/*`, `ChatContact/*`, `CustomEmoji/*`,
`ReadPosition/*`, `PresenceStatus/*`, and push-subscription methods) on top of
`jmap-base-client`. Depends on `jmap-base-client` for transport and session,
and on `jmap-chat-types` for the wire types. Sibling of `jmap-mail-client` in
the extension-client family; mirrors that crate's shape.

## How to use

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
| `chat_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<Chat>` |
| `chat_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `chat_query` | `input: &ChatQueryInput` | `QueryResponse` |
| `chat_query_changes` | `since_query_state: &State, max_changes: Option<u64>` | `QueryChangesResponse` |
| `chat_create` | `input: &ChatCreateInput<'_>` | `SetResponse` |
| `chat_update` | `id: &Id, patch: &ChatPatch<'_>` | `SetResponse` |
| `chat_destroy` | `ids: &[Id]` | `SetResponse` |
| `chat_typing` | `chat_id: &Id, typing: bool` | `TypingResponse` |

### Message/*

| Method | Parameters | Returns |
|---|---|---|
| `message_get` | `ids: &[Id], properties: Option<&[&str]>` | `GetResponse<Message>` |
| `message_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `message_query` | `input: &MessageQueryInput` | `QueryResponse` |
| `message_query_changes` | `since_query_state: &State, max_changes: Option<u64>` | `QueryChangesResponse` |
| `message_create` | `input: &MessageCreateInput<'_>` | `SetResponse` |
| `message_update` | `id: &Id, patch: &MessagePatch<'_>` | `SetResponse` |
| `message_destroy` | `ids: &[Id]` | `SetResponse` |

### Space/*

| Method | Parameters | Returns |
|---|---|---|
| `space_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<Space>` |
| `space_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `space_query` | `input: &SpaceQueryInput` | `QueryResponse` |
| `space_query_changes` | `since_query_state: &State, max_changes: Option<u64>` | `QueryChangesResponse` |
| `space_create` | `input: &SpaceCreateInput<'_>` | `SetResponse` |
| `space_update` | `id: &Id, patch: &SpacePatch<'_>` | `SetResponse` |
| `space_destroy` | `ids: &[Id]` | `SetResponse` |
| `space_join` | `input: &SpaceJoinInput<'_>` | `SpaceJoinResponse` |

### SpaceInvite/*

| Method | Parameters | Returns |
|---|---|---|
| `space_invite_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<SpaceInvite>` |
| `space_invite_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `space_invite_create` | `input: &SpaceInviteCreateInput<'_>` | `SetResponse` |
| `space_invite_destroy` | `ids: &[Id]` | `SetResponse` |

### SpaceBan/*

| Method | Parameters | Returns |
|---|---|---|
| `space_ban_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<SpaceBan>` |
| `space_ban_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `space_ban_create` | `input: &SpaceBanCreateInput<'_>` | `SetResponse` |
| `space_ban_destroy` | `ids: &[Id]` | `SetResponse` |

### ChatContact/*

| Method | Parameters | Returns |
|---|---|---|
| `chat_contact_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<ChatContact>` |
| `chat_contact_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `chat_contact_query` | `input: &ChatContactQueryInput` | `QueryResponse` |
| `chat_contact_query_changes` | `since_query_state: &State, max_changes: Option<u64>` | `QueryChangesResponse` |
| `chat_contact_update` | `id: &Id, patch: &ChatContactPatch<'_>` | `SetResponse` |

### CustomEmoji/*

| Method | Parameters | Returns |
|---|---|---|
| `custom_emoji_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<CustomEmoji>` |
| `custom_emoji_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `custom_emoji_query` | `input: &CustomEmojiQueryInput<'_>` | `QueryResponse` |
| `custom_emoji_query_changes` | `since_query_state: &State, max_changes: Option<u64>` | `QueryChangesResponse` |
| `custom_emoji_create` | `input: &CustomEmojiCreateInput<'_>` | `SetResponse` |
| `custom_emoji_destroy` | `ids: &[Id]` | `SetResponse` |

### ReadPosition/* and PresenceStatus/*

| Method | Parameters | Returns |
|---|---|---|
| `read_position_get` | `ids: Option<&[Id]>` | `GetResponse<ReadPosition>` |
| `read_position_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `read_position_update` | `read_position_id: &Id, last_read_message_id: &Id` | `SetResponse` |
| `presence_status_get` | _(none)_ | `GetResponse<PresenceStatus>` |
| `presence_status_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `presence_status_update` | `id: &Id, patch: &PresenceStatusPatch<'_>` | `SetResponse` |

### Push subscriptions

| Method | Parameters | Returns |
|---|---|---|
| `push_subscription_create` | `input: &PushSubscriptionCreateInput<'_>` | `PushSubscriptionCreateResponse` |

## Push transport

Two real-time push transports are supported, both layered on top of the
`jmap-base-client` HTTP client. They expose typed Chat-specific event types
without forcing the application to parse raw JSON.

### Modules

| Module | Public items | Purpose |
|---|---|---|
| `pub mod ws` | `ChatWsExt`, `ChatWsFrame` | RFC 8887 WebSocket binding for JMAP, with Chat-specific event types decoded from the wire frames. |
| `pub mod sse` | `ChatSseEvent`, `ChatSseFrame`, `parse_chat_sse_block` | Server-Sent Events parser specialized for JMAP Chat push payloads. |
| `pub mod session` | `ChatSessionExt`, `ChatCapability`, `ChatPushCapability` | Capability discovery — answers "does this server advertise WebSocket push and what is the URL?" before opening a connection. |

`ChatWsExt` is an extension trait on `JmapClient` that opens a WebSocket
connection to the URL advertised in the JMAP Session's `webSocketUrl`
capability. `parse_chat_sse_block` consumes a single SSE event block and
returns a typed `ChatSseEvent` (or an error if the block is malformed).

### Usage sketch

```rust,no_run
use jmap_base_client::{connect_ws, Session};
use jmap_chat_client::{ChatSessionExt, ChatWsExt};

async fn drive_chat_ws(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Discover whether the server advertises JMAP WebSocket transport
    //    AND that the Chat capability rides on it.
    if !session.supports_chat_websocket() {
        return Ok(());
    }

    // 2. Read the WebSocket URL from the RFC 8887 capability object.
    let ws_cap = session.websocket_capability()?
        .expect("supports_chat_websocket implies WebSocketCapability is present");

    // 3. Open the WsSession (auth header tuple is built by your auth code).
    let mut ws = connect_ws(&ws_cap.url, /* auth_header = */ None).await?;

    // 4. Drive it via ChatWsExt — each yielded frame is a typed ChatWsFrame:
    //    StateChange, ChatTyping, ChatPresence, ResponseFrame, RequestError,
    //    Unknown { type_name, .. }, etc.
    while let Some(frame) = ws.next_chat_frame().await {
        let _frame = frame?;
    }
    Ok(())
}
```

For SSE, use the base-client event-source loop (`JmapClient::subscribe_events`)
and feed each raw event block to `parse_chat_sse_block`, which returns a
`ChatSseFrame` carrying the typed `ChatSseEvent` payload. The same Chat event
variants are delivered over both transports; choose based on what the server
advertises (see `ChatSessionExt::supports_chat_websocket` and
`chat_push_capability`) and the deployment's network constraints (proxies,
long-lived connections, etc.).

## Examples

The crate ships two runnable examples under `examples/`. Both spin up an
in-process tokio listener, emit synthetic JMAP push frames, and drive the
client read loop. Useful as starting points for integration tests and as a
demonstration of the public push API.

- [`examples/sse_listen.rs`](examples/sse_listen.rs) — consume a synthetic
  Server-Sent Events stream via `JmapClient::subscribe_events`. Two
  `event: state` frames per RFC 8620 §7.3 / JMAP Chat draft §StateChange.
  Run: `cargo run --example sse_listen -p jmap-chat-client`.
- [`examples/ws_loop.rs`](examples/ws_loop.rs) — drive a synthetic JMAP
  WebSocket via `connect_ws` + `ChatWsExt::next_chat_frame`. Emits one
  `Response` frame and one `StateChange` push frame per RFC 8887 / JMAP
  Chat WSS draft. Run: `cargo run --example ws_loop -p jmap-chat-client`.

NOT FOR PRODUCTION — single-shot, no retry, no auth, no TLS. Demonstrates
the consume-side API only.

## How it works

Each method on `SessionClient` runs the same pipeline:

1. Validate arguments (typed `&Id` / `&[Id]` makes invalid Ids unrepresentable;
   empty-state defence-in-depth guards return `InvalidArgument` before any I/O).
2. Resolve `(api_url, account_id)` from the bound session for
   `urn:ietf:params:jmap:chat`.
3. Build the method-arguments JSON.
4. Wrap it into a `JmapRequest` via `JmapRequestBuilder` with
   `using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]`.
5. POST it via `jmap_base_client::JmapClient::call`.
6. `extract_response::<T>` finds the typed result for call ID `"r1"`.

The `Jmap*Ext` extension trait (`JmapChatExt`) adds the
`with_chat_session(session)` accessor to `JmapClient`. The returned
`SessionClient` carries the session and exposes every JMAP Chat method as a
typed `async fn`. The push-transport modules (`ws`, `sse`, `session`) layer
typed Chat-specific event types on top of the base-client transports.

## Gotchas

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

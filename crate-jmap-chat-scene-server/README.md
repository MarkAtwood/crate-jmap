# jmap-chat-scene-server

> **Draft, not published.** Tracks a draft spec that is still changing
> (jmap-chat-spec 63e9d08 revised it); `publish = false` until the crate
> matches it (JMAP-0x0q, JMAP-38uq.14).

The Chat binding for `jmap-scene-server`: the rules of
draft-atwood-jmap-scene-wss-00 that apply when one server offers both
`urn:ietf:params:jmap:chat:websocket` and
`urn:ietf:params:jmap:scene:websocket`. `jmap-scene-server` and
`jmap-chat-server` do not depend on each other; this crate depends on
`jmap-scene-server` only, since the one Chat input it needs is a flag the
transport keeps.

## What it is

| Item | Spec |
|---|---|
| `should_deliver_scene` | scene-wss §Interoperability with JMAP Chat WebSocket: the union of the `ChatStreamEnable` `"scene"` path and `SceneStreamEnable`, one decision per event |

## What it's for

A WebSocket transport that serves both capabilities calls
`should_deliver_scene` before sending a Scene event on a connection,
instead of `jmap_scene_server::should_deliver`.

## How to use

```rust,ignore
// Per connection: `chat_scene` from the accepted ChatStreamEnable,
// `scene` from SceneStreamEnable / SceneStreamDisable.
if should_deliver_scene(&backend, &caller, &account_id, conn.chat_scene, &conn.scene, &event).await? {
    conn.send(&event).await?;
}
```

## How it works

The `"scene"` path covers every Scene event type in every region where
the user has an active avatar: a `SceneStreamEnable` with `regionIds` and
`eventTypes` both `null`. Any `SceneStreamEnable` covers a subset of that,
so the union is the `"scene"` path when it is on and the
`SceneStreamEnable` subscription otherwise. `jmap_scene_server::should_deliver`
decides either way, with the same access, visibility and blocked-sender
checks. One `bool` per event and connection means no duplicates.

## Gotchas

- Pass `jmap_chat_server::EphemeralSubscription::scene` as `chat_scene`.
  It is `true` only when the accepted `ChatStreamEnable` listed `"scene"`
  and `CompanionStreams` says the server advertises
  `urn:ietf:params:jmap:scene:websocket`; a failed `replace_with` keeps
  it and `ChatStreamDisable` clears it.
- A rejected `SceneStreamEnable` leaves the Scene subscription unchanged
  (`replace_with` errors without writing) and never touches the flag, so
  "errors in one mechanism" do not reach the other.

## References

- [draft-atwood-jmap-scene-wss-00] — JMAP Scene WebSocket
- [draft-atwood-jmap-chat-wss-00] — JMAP Chat WebSocket
- [RFC 8887] — JMAP over WebSocket

[draft-atwood-jmap-scene-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8887]: https://www.rfc-editor.org/rfc/rfc8887

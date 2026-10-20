# jmap-chat-vtc-server

The Chat binding for `jmap-vtc-server`: the rules of
draft-atwood-jmap-vtc-00 and draft-atwood-jmap-chat-00 that apply when one
server offers both `urn:ietf:params:jmap:vtc` and
`urn:ietf:params:jmap:chat`. `jmap-vtc-server` and `jmap-chat-server` do
not depend on each other; this crate depends on both.

## What it is

| Item | Spec |
|---|---|
| `check_call_create` | vtc §Creating a Ring Call, §Creating a Room Call: `chatId` access and membership, the `start_call` permission |
| `sync_active_call_id` | chat §Chat `activeCallId`: set on `"active"`, cleared on `"ended"` |
| `clear_space_references`, `clear_channel_references` | vtc §VTCCall: clear `spaceId` / `channelId` when the Space or channel is destroyed |
| `contact_identity` | chat §Terminology: the identity a ChatContact id (or the accountId) names, as VTC `userId` uses it |
| `is_blocked_by_contact` | vtc §Blocked-Sender Suppression, through `ChatContact.blocked` |
| `START_CALL` | chat §SpaceRole `"start_call"` |
| `should_deliver_vtc` | vtc-wss §Interoperability with JMAP Chat WebSocket: the union of the `ChatStreamEnable` `"vtc"` path and `VTCStreamEnable`, one decision per event |

## What it's for

A deployment that implements both `VtcBackend` and `ChatBackend` calls
these functions from its trait methods:

| Backend method | Call |
|---|---|
| `VtcBackend::create_call`, before the insert | `check_call_create` |
| `VtcBackend::create_call` and `update_object::<VtcCall>`, after the write | `sync_active_call_id` |
| `VtcBackend::is_user_blocked` | `is_blocked_by_contact` |
| `ChatBackend::on_chat_destroyed` | `clear_channel_references` |
| `ChatBackend::on_space_destroyed` | `clear_space_references` |
| WebSocket transport, before sending a VTC event | `should_deliver_vtc` |

## How to use

```rust,ignore
async fn create_call(&self, caller: &Ctx, account_id: &Id, call: VtcCall, targets: &[String])
    -> Result<VtcCall, BackendSetError<Error>>
{
    check_call_create(self, caller, account_id, &call, &self.everyone_permissions)
        .await
        .map_err(BackendSetError::Other)?
        .map_err(BackendSetError::SetError)?;
    let stored = self.insert_call(caller, account_id, call, targets).await?;
    sync_active_call_id(self, caller, account_id, &stored).await?;
    Ok(stored)
}
```

`tests/common/mod.rs` wires the two reference `MemoryBackend`s this way.

## How it works

Each function takes only the backend it reads or writes, so the VTC and
Chat backends may have different caller contexts and error types. Writes
go through `update_object` on the owning backend, which advances that
type's state, so each change produces the `StateChange` the drafts
require.

## Gotchas

- `sync_active_call_id` must run in the same atomic step as the VTCCall
  write, before the VTCCall `StateChange` goes out: the draft requires the
  Chat update to be "applied atomically with it from the client's
  perspective". The crate cannot enforce this; your backend's transaction
  does.
- `check_call_create` takes the `@everyone` role's permissions as an
  argument. The draft says every member holds that role but puts its
  permissions on no wire object. Controller principals that "bypass this
  resolution" are also deployment-defined; skip the call for them.
- Space ban enforcement (vtc §Access Control) is not here yet: it writes
  `VTCParticipant.leftAt`, and `VtcParticipant` has no `SetObject` impl
  yet (bd:JMAP-8mem.10). `contact_identity` is its first step.
- `should_deliver_vtc` takes a `chat_vtc` flag the transport keeps: set
  it from an accepted `ChatStreamEnable` that lists `"vtc"`, clear it on
  `ChatStreamDisable`, and pass `false` when the server does not
  advertise `urn:ietf:params:jmap:vtc:websocket`.
- `clear_*_references` and `is_blocked_by_contact`
  read every record of the type; each carries a `ponytail:` note with the
  upgrade path.

## References

- [draft-atwood-jmap-vtc-00] — JMAP VTC
- [draft-atwood-jmap-chat-00] — JMAP Chat
- [draft-atwood-jmap-vtc-wss-00] — JMAP VTC WebSocket
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-vtc-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

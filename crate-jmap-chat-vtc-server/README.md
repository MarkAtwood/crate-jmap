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
| `eject_banned_user` | vtc §Chat-Bound Calls: on a SpaceBan, set `leftAt` on the banned user's active participants in the Space's calls, apply the §Leaving a Call side effects, and return one `"left"` `VTCParticipantEvent` each |
| `is_blocked_by_contact` | vtc §Blocked-Sender Suppression, through `ChatContact.blocked` |
| `may_access_call` | vtc §Joining a Room or Scheduled Call and §Chat-Bound Calls: `false` when the caller is blocked, is not a member of the `chatId` Chat or the bound Space, or lacks `"view"` on the bound channel |
| `should_deliver_vtc` | vtc-wss §Interoperability with JMAP Chat WebSocket: the union of the `ChatStreamEnable` `"vtc"` path and `VTCStreamEnable`, one decision per event |

## What it's for

A deployment that implements both `VtcBackend` and `ChatBackend` calls
these functions from its trait methods:

| Backend method | Call |
|---|---|
| `VtcBackend::create_call`, before the insert | `check_call_create` |
| `VtcBackend::create_call`, `update_object::<VtcCall>` and `update_object_if::<VtcCall>`, after the write | `sync_active_call_id` |
| `VtcBackend::is_user_blocked` | `is_blocked_by_contact` |
| `JmapBackend::get_objects`, `query_objects` and `query_changes`, per VTCCall (`VtcBackend` §Per-call access) | `may_access_call` |
| `ChatBackend::on_chat_destroyed` | `clear_channel_references` |
| `ChatBackend::on_space_destroyed` | `clear_space_references` |
| `ChatBackend::on_space_ban_created` | `contact_identity`, then `eject_banned_user` |
| WebSocket transport, before sending a VTC event | `should_deliver_vtc` |

## How to use

```rust,ignore
async fn create_call(&self, caller: &Ctx, account_id: &Id, call: VtcCall, targets: &[String])
    -> Result<VtcCall, BackendSetError<Error>>
{
    check_call_create(self, caller, account_id, &call)
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
- `check_call_create` reads the bound Space's `@everyone` permissions
  from `ChatBackend::everyone_permissions`. The draft says every member
  holds that role but puts its permissions on no wire object. Controller
  principals that "bypass this
  resolution" are also deployment-defined; skip the call for them.
- `eject_banned_user` returns the `"left"` events and does not deliver
  them; pass each to `VtcBackend::deliver_event`. Like a
  `VTCParticipant/set` leave, it then runs `jmap_vtc_server::after_leave`:
  a ring or scheduled call left with no active participant ends, and a
  call whose last moderator was ejected hands the role on.
- Pass `jmap_chat_server::EphemeralSubscription::vtc` as the `chat_vtc`
  flag of `should_deliver_vtc`. It is `true` only when the accepted
  `ChatStreamEnable` listed `"vtc"` and `CompanionStreams` says the server
  advertises `urn:ietf:params:jmap:vtc:websocket`.
- `clear_*_references`, `eject_banned_user` and `is_blocked_by_contact`
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

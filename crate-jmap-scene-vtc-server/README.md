# jmap-scene-vtc-server

> **Draft, not published.** Tracks a draft spec that is still changing
> (jmap-chat-spec 63e9d08 revised it); `publish = false` until the crate
> matches it (JMAP-0x0q, JMAP-38uq.14).

The VTC binding for `jmap-scene-server`: the rules of
draft-atwood-jmap-scene-00 that apply when one server offers both
`urn:ietf:params:jmap:scene` and `urn:ietf:params:jmap:vtc`.
`jmap-scene-server` and `jmap-vtc-server` do not depend on each other;
this crate depends on `jmap-scene-server` and `jmap-vtc-types`.

## What it is

| Item | Spec |
|---|---|
| `clear_active_call_id` | scene §SceneRegion `activeCallId`: cleared to `null`, with a SceneRegion `StateChange`, when the referenced VTCCall transitions to `"ended"` |
| `may_see_region_avatars` | scene §Security Considerations: a region bound to a VTCCall with `e2eeEnabled` hides its SceneAvatar records and `SceneAvatarEvent`s from users who are not participants in the call |

## What it's for

A deployment that implements both `SceneBackend` and `VtcBackend` calls
it from `VtcBackend::update_object` after a `VtcCall` write. Both
`VTCCall/set` and `jmap_vtc_server::end_call` (the server's own timeout
or all-participants-left end) write through that method, so one call
site covers every way a call ends.

## How to use

```rust,ignore
async fn update_object<O: SetObject + Send + Sync>(&self, caller: &Ctx, account_id: &Id,
    id: &Id, patch: O::Patch) -> Result<Option<O>, BackendSetError<Error>>
{
    let echo = self.write(caller, account_id, id, patch).await?;
    if O::TYPE_NAME == VtcCall::TYPE_NAME {
        let call = self.load_call(caller, account_id, id).await?;
        clear_active_call_id(&self.scene, caller, account_id, &call).await?;
    }
    Ok(echo)
}
```

`tests/common/mod.rs` wires the two reference `MemoryBackend`s this way.

`SceneBackend::may_see_region_avatars` loads the region and, when its
`activeCallId` is set, the VTCCall as stored, then returns
`may_see_region_avatars(&self.vtc, caller, account_id, &call, &user_id)`;
a region with no `activeCallId` answers `true`. The same answer must
drive the omission in your `query_objects` and `query_changes`.

## How it works

The function does nothing unless the call is `"ended"`. Otherwise it
patches `activeCallId` to `null` on every SceneRegion naming the call,
through `SceneBackend::update_object`, which advances the SceneRegion
state.

## Gotchas

- Run it in the same atomic step as the VTCCall write: a region still
  naming an ended call is, in the integration guide's words, "a data
  integrity bug". The crate cannot enforce this; your backend's
  transaction does.
- Setting `activeCallId` when a call becomes `"active"` is not here: the
  scene-vtc integration guide calls that direction "not normatively
  required by either spec".
- It reads every SceneRegion in the account; the `ponytail:` note names
  the upgrade path. `may_see_region_avatars` likewise reads every
  VTCParticipant of the account.
- A participant is anyone with a VTCParticipant record in the call, left
  or not (vtc §Participant Visibility: "users who do not have a
  VTCParticipant record in it").

## References

- [draft-atwood-jmap-scene-00] — JMAP Scene
- [draft-atwood-jmap-vtc-00] — JMAP VTC

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec

//! VTC binding for the JMAP Scene method handlers: the rules of
//! draft-atwood-jmap-scene-00 that apply when a server offers both
//! `urn:ietf:params:jmap:scene` and `urn:ietf:params:jmap:vtc`.
//!
//! `jmap-scene-server` and `jmap-vtc-server` do not depend on each other.
//! A deployment that offers both implements
//! [`jmap_scene_server::SceneBackend`] and `jmap_vtc_server::VtcBackend`
//! and calls these functions from its trait methods:
//!
//! | Backend method | Call |
//! |---|---|
//! | `VtcBackend::update_object::<VtcCall>` and `update_object_if::<VtcCall>`, after the write | [`clear_active_call_id`] |
//! | `SceneBackend::may_see_region_avatars`, for a region with an `activeCallId` | [`may_see_region_avatars`] |

#![forbid(unsafe_code)]

use jmap_scene_server::{BackendSetError, JmapBackend, SceneBackend};
use jmap_scene_types::SceneRegion;
use jmap_types::{Id, PatchObject};
use jmap_vtc_types::{CallState, VtcCall, VtcParticipant};
use serde_json::Value;

/// Clear `activeCallId` on every SceneRegion that names `call`, the
/// VTCCall as stored after an update, once it has ended.
///
/// draft-atwood-jmap-scene-00 §SceneRegion `activeCallId`: "When the
/// VTCCall referenced by `activeCallId` transitions to state `"ended"`,
/// the server MUST clear this field to `null` and emit a `StateChange`
/// for the SceneRegion type. This update is a side effect of the VTCCall
/// state transition."
///
/// A call in any state other than `"ended"` changes nothing, so this is
/// safe to call after every VTCCall update. Both `VTCCall/set` and
/// `jmap_vtc_server::end_call` (the server's own inactivity or
/// all-participants-left end) end a call through
/// `VtcBackend::update_object_if`, and other VTCCall patches go through
/// `VtcBackend::update_object`, so calling it from both covers every
/// end. Each write goes through
/// [`SceneBackend::update_object`], which advances the SceneRegion state,
/// so the SceneRegion `StateChange` follows. Call it in the same atomic
/// step as the VTCCall write.
///
/// # Errors
///
/// Returns the Scene backend's error from reading or updating a region.
/// Regions cleared before the error stay cleared.
// Same error type as `SceneBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn clear_active_call_id<S: SceneBackend>(
    scene: &S,
    caller: &S::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
) -> Result<(), BackendSetError<S::Error>> {
    if call.state != CallState::Ended {
        return Ok(());
    }
    // ponytail: reads every SceneRegion; O(regions) per ended call.
    // Upgrade: a backend index from activeCallId to region.
    let (regions, _) = scene
        .get_objects::<SceneRegion>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    for region in regions
        .iter()
        .filter(|r| r.active_call_id.as_ref().and_then(Option::as_ref) == Some(&call.id))
    {
        let mut patch = PatchObject::new();
        patch
            .as_map_mut()
            .insert("activeCallId".to_owned(), Value::Null);
        scene
            .update_object::<SceneRegion>(caller, account_id, &region.id, patch)
            .await?;
    }
    Ok(())
}

/// Whether `user_id` may see the SceneAvatar records of a region whose
/// `activeCallId` names `call`, the VTCCall as stored.
///
/// draft-atwood-jmap-scene-00 at jmap-chat-spec 6bcd9d8, L2654: "When a
/// SceneRegion's `activeCallId` references a VTCCall with `e2eeEnabled`
/// set to `true`, the server SHOULD NOT expose the region's SceneAvatar
/// records to users who are not participants in the bound VTCCall". A
/// participant is a user with a VTCParticipant record in the call, as in
/// draft-atwood-jmap-vtc-00 at 6bcd9d8, L1644 ("users who do not have a
/// VTCParticipant record in it"), whether or not that record has left.
///
/// A call with `e2eeEnabled` `false` hides nothing, and no VTC read is
/// made. Otherwise `vtc` is the VTC storage, read as `caller`, whose
/// userId is `user_id`, for the VTCParticipant records of `account_id`,
/// the call's hosting account. A `VtcBackend` shows the caller its own
/// record in an E2EE call, so a caller-scoped read is enough.
///
/// A `SceneBackend` returns this from
/// [`SceneBackend::may_see_region_avatars`] after loading the region and,
/// when its `activeCallId` is set, that VTCCall; a region with no
/// `activeCallId` answers `true`. The Scene handlers then hide the avatars
/// from `SceneAvatar/get` and drop their `SceneAvatarEvent`s, and the
/// backend's `query_objects` and `query_changes` omit them.
///
/// # Errors
///
/// Returns the VTC backend's error from reading the participants.
pub async fn may_see_region_avatars<B: JmapBackend>(
    vtc: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
    user_id: &str,
) -> Result<bool, B::Error> {
    if !call.e2ee_enabled {
        return Ok(true);
    }
    // ponytail: reads every VTCParticipant of the account; O(participants)
    // per check. Upgrade: a `callId` filtered query_objects.
    let (participants, _) = vtc
        .get_objects::<VtcParticipant>(caller, account_id, None, None)
        .await?;
    Ok(participants
        .iter()
        .any(|p| p.call_id == call.id && p.user_id.as_deref() == Some(user_id)))
}

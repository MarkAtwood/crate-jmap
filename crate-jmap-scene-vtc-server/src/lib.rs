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
//! | `VtcBackend::update_object::<VtcCall>`, after the write | [`clear_active_call_id`] |

#![forbid(unsafe_code)]

use jmap_scene_server::{BackendSetError, SceneBackend};
use jmap_scene_types::SceneRegion;
use jmap_types::{Id, PatchObject};
use jmap_vtc_types::{CallState, VtcCall};
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
/// all-participants-left end) write through `VtcBackend::update_object`,
/// so calling it there covers both. Each write goes through
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

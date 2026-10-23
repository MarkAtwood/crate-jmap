//! Shared fixtures. "scene Lnnn" cites draft-atwood-jmap-scene-00.md and
//! "vtc Lnnn" draft-atwood-jmap-vtc-00.md, both at jmap-chat-spec
//! 0cd3af0. Records are hand-written JSON from the drafts' field lists.

#![allow(dead_code)]

use jmap_scene_server::memory::{MemoryBackend as SceneMemory, MemoryError as SceneError};
use jmap_scene_server::{handle_scene_region_changes, handle_scene_region_get};
use jmap_scene_types::SceneCapability;
use jmap_scene_vtc_server::clear_active_call_id;
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    HandlerOutput, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetObject,
};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::{MemoryBackend as VtcMemory, MemoryError as VtcError};
use jmap_vtc_server::{StatefulObject, VtcBackend, handle_vtc_call_get, handle_vtc_call_set};
use jmap_vtc_types::{VtcCall, VtcCapability, VtcEvent, VtcRecordingStateEvent};
use serde_json::{Value, json};

pub const ACCOUNT: &str = "A1";
pub const OWNER: &str = "user:owner@example.com";

/// A deployment backend that offers both VTC and Scene, wired to this
/// crate as its docs prescribe: VTC storage in `vtc`, Scene storage in
/// `scene`, one account [`ACCOUNT`] in each.
pub struct Bound {
    pub vtc: VtcMemory,
    pub scene: SceneMemory,
}

impl Bound {
    /// [`ACCOUNT`] registered in both backends: VTC with room calls
    /// enabled (capability object hand-written from vtc L148-187), Scene
    /// with the capability fields of scene L160-181.
    pub fn new() -> Self {
        let vtc = VtcMemory::new();
        let cap: VtcCapability = serde_json::from_value(json!({
            "mayCreateCall": true,
            "supportsRingCalls": true,
            "supportsRoomCalls": true,
            "supportsScheduledCalls": true,
            "supportsRecording": false,
            "supportsLivestream": false,
            "supportsLobby": false,
            "supportsBreakoutRooms": false,
            "supportedMediaTypes": ["audio", "video"],
            "maxParticipantsPerCall": null,
            "maxConcurrentCalls": null,
            "maxBreakoutRooms": null
        }))
        .expect("VTC capability fixture");
        vtc.register_account(&Id::from(ACCOUNT), cap);
        let scene = SceneMemory::new();
        let cap: SceneCapability = serde_json::from_value(json!({
            "mayCreateRegion": true,
            "mayCreateObject": true,
            "supportedVisualTypes": ["model/gltf-binary"],
            "maxRegionsPerAccount": null,
            "maxObjectsPerRegion": null,
            "maxAvatarsPerRegion": null,
            "maxAssetSizeBytes": null
        }))
        .expect("Scene capability fixture");
        scene.register_account(&Id::from(ACCOUNT), cap);
        Self { vtc, scene }
    }

    /// Store SceneRegion `id`, owned by [`OWNER`], with `activeCallId`
    /// set to `call_id`.
    pub fn seed_region(&self, id: &str, call_id: &str) {
        self.scene.insert_object_for_test(
            "SceneRegion",
            &Id::from(ACCOUNT),
            id,
            region(id, call_id),
        );
    }
}

fn scene_set_error(e: BackendSetError<SceneError>) -> BackendSetError<VtcError> {
    match e {
        BackendSetError::SetError(s) => BackendSetError::SetError(s),
        other => BackendSetError::Other(VtcError::new(other.to_string())),
    }
}

impl JmapBackend for Bound {
    type Error = VtcError;
    type CallerCtx = String;

    async fn account_exists(&self, caller: &String, account_id: &Id) -> Result<bool, VtcError> {
        self.vtc.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), VtcError> {
        self.vtc
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<State, VtcError> {
        self.vtc.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<VtcError>> {
        self.vtc.get_changes::<O>(caller, account_id, params).await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<VtcError>> {
        self.vtc
            .query_objects::<O>(caller, account_id, params)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<VtcError>> {
        self.vtc
            .query_changes::<O>(caller, account_id, params)
            .await
    }
}

impl VtcBackend for Bound {
    async fn vtc_capability(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<VtcCapability, VtcError> {
        self.vtc.vtc_capability(caller, account_id).await
    }

    fn has_capability(&self, caller: &String, account_id: &Id, uri: &str) -> bool {
        self.vtc.has_capability(caller, account_id, uri)
    }

    async fn caller_user_id(&self, caller: &String, account_id: &Id) -> Result<String, VtcError> {
        self.vtc.caller_user_id(caller, account_id).await
    }

    async fn create_call<F>(
        &self,
        caller: &String,
        account_id: &Id,
        build: F,
        ring_targets: &[String],
    ) -> Result<VtcCall, BackendSetError<VtcError>>
    where
        F: FnOnce(&Id) -> VtcCall + Send,
    {
        self.vtc
            .create_call(caller, account_id, build, ring_targets)
            .await
    }

    async fn create_object<O, F>(
        &self,
        caller: &String,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<VtcError>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        self.vtc
            .create_object(caller, account_id, create_id, build)
            .await
    }

    async fn deliver_recording_state_event(
        &self,
        caller: &String,
        account_id: &Id,
        event: VtcRecordingStateEvent,
    ) {
        self.vtc
            .deliver_recording_state_event(caller, account_id, event)
            .await
    }

    async fn deliver_event(&self, caller: &String, account_id: &Id, event: VtcEvent) {
        self.vtc.deliver_event(caller, account_id, event).await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<VtcError>> {
        let echo = self
            .vtc
            .update_object::<O>(caller, account_id, id, patch)
            .await?;
        if O::TYPE_NAME == VtcCall::TYPE_NAME {
            let (found, _) = self
                .vtc
                .get_objects::<VtcCall>(caller, account_id, Some(std::slice::from_ref(id)), None)
                .await
                .map_err(BackendSetError::Other)?;
            for call in &found {
                clear_active_call_id(&self.scene, caller, account_id, call)
                    .await
                    .map_err(scene_set_error)?;
            }
        }
        Ok(echo)
    }

    async fn update_object_if<O: StatefulObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
        expected: &[O::State],
        patch: O::Patch,
    ) -> Result<Result<Option<O>, O::State>, BackendSetError<VtcError>> {
        let applied = self
            .vtc
            .update_object_if::<O>(caller, account_id, id, expected, patch)
            .await?;
        if applied.is_ok() && O::TYPE_NAME == VtcCall::TYPE_NAME {
            let (found, _) = self
                .vtc
                .get_objects::<VtcCall>(caller, account_id, Some(std::slice::from_ref(id)), None)
                .await
                .map_err(BackendSetError::Other)?;
            for call in &found {
                clear_active_call_id(&self.scene, caller, account_id, call)
                    .await
                    .map_err(scene_set_error)?;
            }
        }
        Ok(applied)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<VtcError>> {
        self.vtc.destroy_object::<O>(caller, account_id, id).await
    }

    async fn active_call_count(&self, caller: &String, account_id: &Id) -> Result<u64, VtcError> {
        self.vtc.active_call_count(caller, account_id).await
    }

    async fn get_objects_unscoped<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
    ) -> Result<(Vec<O>, Vec<Id>), VtcError> {
        self.vtc
            .get_objects_unscoped::<O>(caller, account_id, ids)
            .await
    }

    fn supports_type<O: JmapObject>(&self, caller: &String, account_id: &Id) -> bool {
        self.vtc.supports_type::<O>(caller, account_id)
    }
}

/// Run `VTCCall/set` on [`ACCOUNT`] as [`OWNER`] with `args` (without
/// `accountId`).
pub async fn vtc_set(backend: &Bound, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    let HandlerOutput { primary, .. } = handle_vtc_call_set(
        backend,
        &OWNER.to_owned(),
        &std::collections::BTreeMap::new(),
        args,
    )
    .await
    .expect("VTCCall/set");
    primary
}

/// Create a room call, which vtc L1946 says "is immediately `"active"`",
/// and return its id.
pub async fn create_room_call(backend: &Bound) -> String {
    let resp = vtc_set(
        backend,
        json!({ "create": { "c0": { "callType": "room", "mediaTypes": ["audio"] } } }),
    )
    .await;
    assert!(resp["notCreated"].is_null(), "create failed: {resp}");
    resp["created"]["c0"]["id"]
        .as_str()
        .expect("call id")
        .to_owned()
}

/// One VTCCall as `VTCCall/get` returns it.
pub async fn get_call(backend: &Bound, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_vtc_call_get(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCCall/get");
    primary["list"][0].clone()
}

/// One SceneRegion as `SceneRegion/get` returns it.
pub async fn get_region(backend: &Bound, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_scene_region_get(
        &backend.scene,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("SceneRegion/get");
    primary["list"][0].clone()
}

/// The current SceneRegion `state` string.
pub async fn region_state(backend: &Bound) -> String {
    let HandlerOutput { primary, .. } = handle_scene_region_get(
        &backend.scene,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [] }),
    )
    .await
    .expect("SceneRegion/get");
    primary["state"].as_str().expect("state").to_owned()
}

/// The `updated` ids of `SceneRegion/changes` since `since`.
pub async fn regions_updated_since(backend: &Bound, since: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_scene_region_changes(
        &backend.scene,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
    )
    .await
    .expect("SceneRegion/changes");
    primary["updated"].clone()
}

/// A SceneRegion `id` bound to VTCCall `call_id`, hand-written from the
/// scene L261-418 field list.
pub fn region(id: &str, call_id: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "ownerId": OWNER,
        "name": "Plaza",
        "description": null,
        "bounds": { "min": [-10.0, 0.0, -10.0], "max": [10.0, 5.0, 10.0] },
        "environment": null,
        "simulationUri": null,
        "inputUri": null,
        "spawnPosition": [0.0, 0.0, 0.0],
        "spawnOrientation": [0.0, 0.0, 0.0, 1.0],
        "activeAvatarCount": 0,
        "viewHint": null,
        "geoAnchor": null,
        "accessPolicy": "open",
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "worldState": null,
        "activeCallId": call_id
    })
}

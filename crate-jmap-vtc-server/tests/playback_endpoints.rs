//! Per-viewer `playbackEndpoints` in `VTCLivestream/get` (bd JMAP-t8ej.11).
//!
//! Oracle: draft-atwood-jmap-vtc-00 §Playback Authorization: "The server
//! computes `playbackEndpoints` for each requesting user: `url`,
//! `credential`, and `expiresAt` can differ between users and between
//! requests. Minting a new credential does not change the VTCLivestream
//! state string." and "The server MAY return an empty `playbackEndpoints`
//! list to a user it does not authorize to view".

#[macro_use]
mod common;

use common::*;
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapObject,
    QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult, SetObject,
};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::{MemoryBackend, MemoryError};
use jmap_vtc_server::{StatefulObject, VtcBackend, handle_vtc_livestream_get};
use jmap_vtc_types::{
    VtcCall, VtcCapability, VtcEvent, VtcPlaybackEndpoint, VtcRecordingStateEvent,
};
use serde_json::{Value, json};

/// [`MemoryBackend`] that mints a credential per caller and gives [`EVE`]
/// no endpoints.
struct PerViewer(MemoryBackend);

impl jmap_server::JmapAccounts for PerViewer {
    type Error = MemoryError;
    type CallerCtx = String;

    async fn account_exists(&self, caller: &String, account_id: &Id) -> Result<bool, MemoryError> {
        self.0.account_exists(caller, account_id).await
    }
}

impl jmap_server::JmapReads for PerViewer {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        self.0
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        self.0.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        self.0.get_changes::<O>(caller, account_id, params).await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        self.0.query_objects::<O>(caller, account_id, params).await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        self.0.query_changes::<O>(caller, account_id, params).await
    }
}

impl VtcBackend for PerViewer {
    async fn vtc_capability(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<VtcCapability, MemoryError> {
        self.0.vtc_capability(caller, account_id).await
    }

    fn has_capability(&self, caller: &String, account_id: &Id, uri: &str) -> bool {
        self.0.has_capability(caller, account_id, uri)
    }

    async fn caller_user_id(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<String, MemoryError> {
        self.0.caller_user_id(caller, account_id).await
    }

    async fn create_call<F>(
        &self,
        caller: &String,
        account_id: &Id,
        build: F,
        ring_targets: &[String],
    ) -> Result<VtcCall, BackendSetError<MemoryError>>
    where
        F: FnOnce(&Id) -> VtcCall + Send,
    {
        self.0
            .create_call(caller, account_id, build, ring_targets)
            .await
    }

    async fn create_object<O, F>(
        &self,
        caller: &String,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<MemoryError>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        self.0
            .create_object(caller, account_id, create_id, build)
            .await
    }

    async fn deliver_recording_state_event(
        &self,
        caller: &String,
        account_id: &Id,
        event: VtcRecordingStateEvent,
    ) {
        self.0
            .deliver_recording_state_event(caller, account_id, event)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        self.0
            .update_object::<O>(caller, account_id, id, patch)
            .await
    }

    async fn update_object_if<O: StatefulObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
        expected: &[O::State],
        patch: O::Patch,
    ) -> Result<Result<Option<O>, O::State>, BackendSetError<MemoryError>> {
        self.0
            .update_object_if::<O>(caller, account_id, id, expected, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        self.0.destroy_object::<O>(caller, account_id, id).await
    }

    async fn deliver_event(&self, caller: &String, account_id: &Id, event: VtcEvent) {
        self.0.deliver_event(caller, account_id, event).await
    }

    async fn is_moderator(
        &self,
        caller: &String,
        account_id: &Id,
        call: &VtcCall,
    ) -> Result<bool, MemoryError> {
        self.0.is_moderator(caller, account_id, call).await
    }

    async fn active_call_count(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<u64, MemoryError> {
        self.0.active_call_count(caller, account_id).await
    }

    async fn get_objects_unscoped<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        self.0
            .get_objects_unscoped::<O>(caller, account_id, ids)
            .await
    }

    async fn is_user_blocked(&self, caller: &String, user_id: &str) -> Result<bool, MemoryError> {
        self.0.is_user_blocked(caller, user_id).await
    }

    fn supports_type<O: JmapObject>(&self, caller: &String, account_id: &Id) -> bool {
        self.0.supports_type::<O>(caller, account_id)
    }

    async fn playback_endpoints_for(
        &self,
        caller: &String,
        _account_id: &Id,
        livestream_id: &Id,
        stored: Vec<VtcPlaybackEndpoint>,
    ) -> Result<Vec<VtcPlaybackEndpoint>, MemoryError> {
        // A restricting deployment: a per-user credential, none for EVE.
        if caller == EVE {
            return Ok(Vec::new());
        }
        Ok(stored
            .into_iter()
            .map(|mut e| {
                e.credential = Some(format!("tok:{caller}:{livestream_id}"));
                e
            })
            .collect())
    }
}

fn hosted_live() -> Value {
    json!({
        "id": "L1",
        "callId": "C1",
        "mode": "hosted",
        "state": "live",
        "failureReason": null,
        "platform": null,
        "streamUri": null,
        "streamKey": null,
        "playbackEndpoints": [{
            "protocol": "hls",
            "url": "https://cdn.example.com/live/L1/index.m3u8",
            "latencyHint": "standard",
            "credential": null,
            "expiresAt": null,
            "timeReference": null,
            "tracks": [],
            "encrypted": false,
            "keySystems": []
        }],
        "startedAt": "2026-06-05T14:41:00Z",
        "stoppedAt": null
    })
}

async fn get_as(backend: &PerViewer, caller: &str) -> Value {
    call!(
        handle_vtc_livestream_get,
        backend,
        caller,
        json!({ "ids": ["L1"] })
    )
}

#[tokio::test]
async fn each_caller_gets_its_own_endpoints() {
    let backend = PerViewer(media_backend());
    seed_as(&backend.0, "VTCLivestream", hosted_live());

    let bob = get_as(&backend, BOB).await;
    let alice = get_as(&backend, ALICE).await;
    assert_eq!(
        bob["list"][0]["playbackEndpoints"][0]["credential"],
        format!("tok:{BOB}:L1"),
        "{bob}"
    );
    assert_eq!(
        alice["list"][0]["playbackEndpoints"][0]["credential"],
        format!("tok:{ALICE}:L1"),
        "{alice}"
    );
    // "Minting a new credential does not change the VTCLivestream state
    // string."
    assert_eq!(bob["state"], alice["state"]);

    // "MAY return an empty playbackEndpoints list to a user it does not
    // authorize to view, even when that user may retrieve the livestream."
    let eve = get_as(&backend, EVE).await;
    assert_eq!(eve["list"][0]["id"], "L1", "{eve}");
    assert_eq!(eve["list"][0]["playbackEndpoints"], json!([]), "{eve}");
}

#[tokio::test]
async fn default_hook_serves_the_stored_list() {
    let backend = media_backend();
    seed_as(&backend, "VTCLivestream", hosted_live());
    let got = call!(
        handle_vtc_livestream_get,
        &backend,
        BOB,
        json!({ "ids": ["L1"] })
    );
    assert_eq!(
        got["list"][0]["playbackEndpoints"],
        hosted_live()["playbackEndpoints"],
        "{got}"
    );
}

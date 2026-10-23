//! Concurrent state changes between a handler's read and its write
//! (bd: JMAP-ace5). Line numbers and quotes cite
//! draft-atwood-jmap-vtc-00.md at jmap-chat-spec 29e9a41; each
//! assertion's oracle is the quoted draft text or a hand-written record.

#[macro_use]
mod common;

use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use common::*;
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapObject,
    QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult, SetObject,
};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::{MemoryBackend, MemoryError};
use jmap_vtc_server::{
    StatefulObject, VtcBackend, after_leave, end_call, expire_ring, handle_vtc_call_get,
    handle_vtc_call_set, handle_vtc_livestream_get, handle_vtc_livestream_set,
    handle_vtc_participant_query, handle_vtc_participant_set, handle_vtc_recording_get,
    handle_vtc_recording_set,
};
use jmap_vtc_types::{
    CallEndReason, CallRole, CallState, VtcCall, VtcCapability, VtcEvent, VtcRecordingStateEvent,
};
use serde_json::{Value, json};

/// [`MemoryBackend`] that stores one record, as a concurrent request
/// would, right after the first `get_objects` of that record's type (or a
/// later one, [`Racing::after`]): the handler holds the earlier read while
/// the store already has the new state.
struct Racing(MemoryBackend, Mutex<Option<(&'static str, Value, usize)>>);

impl Racing {
    fn new(backend: MemoryBackend, type_name: &'static str, obj: Value) -> Self {
        Self::after(backend, type_name, obj, 0)
    }

    /// Store `obj` right after read number `skip + 1` of its type.
    fn after(backend: MemoryBackend, type_name: &'static str, obj: Value, skip: usize) -> Self {
        Self(backend, Mutex::new(Some((type_name, obj, skip))))
    }
}

impl jmap_server::JmapBackend for Racing {
    type Error = MemoryError;
    type CallerCtx = String;

    async fn account_exists(&self, caller: &String, account_id: &Id) -> Result<bool, MemoryError> {
        self.0.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        let read = self
            .0
            .get_objects::<O>(caller, account_id, ids, properties)
            .await;
        let race = {
            let mut slot = self.1.lock().expect("race lock");
            match slot.as_mut() {
                Some((t, _, skip)) if *t == O::TYPE_NAME && *skip > 0 => {
                    *skip -= 1;
                    None
                }
                _ => slot.take_if(|(t, _, _)| *t == O::TYPE_NAME),
            }
        };
        if let Some((type_name, obj, _)) = race {
            seed_as(&self.0, type_name, obj);
        }
        read
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

impl VtcBackend for Racing {
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
}

/// [`stored_call`] in state `"ended"` with `end_reason`.
fn ended_call(id: &str, call_type: &str, end_reason: &str) -> Value {
    let mut call = stored_call(id, call_type, "ended", "2026-06-05T14:30:00Z");
    call["endedAt"] = json!("2026-06-05T15:00:00Z");
    call["endReason"] = json!(end_reason);
    call
}

/// The `notUpdated` entry for `id`, which the test expects.
fn not_updated(resp: &Value, id: &str) -> Value {
    resp["notUpdated"][id].clone()
}

fn assert_state_refused(err: &Value) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["state"]), "{err}");
}

#[tokio::test]
async fn pause_after_a_concurrent_stop_is_refused() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCRecording",
        stored_recording("R1", "recording", "2026-06-05T14:40:00Z"),
    );
    let mut stopped = stored_recording("R1", "stopped", "2026-06-05T14:40:00Z");
    stopped["stoppedAt"] = json!("2026-06-05T14:50:00Z");
    let backend = Racing::new(backend, "VTCRecording", stopped);

    let resp = call!(ids handle_vtc_recording_set,
        &backend,
        BOB,
        json!({ "update": { "R1": { "state": "paused" } } }),
    );

    // L1149-1151 list `"recording"` -> `"paused"`, `"paused"` ->
    // `"recording"` and `"recording"` or `"paused"` -> `"stopped"`; L1165
    // "The server MUST return `invalidProperties` naming `state` for any
    // state transition not listed above (e.g., `"stopped"` ->
    // `"recording"`, ...)". The stored recording was stopped when the
    // pause reached the store.
    assert_state_refused(&not_updated(&resp, "R1"));
    let stored = call!(
        handle_vtc_recording_get,
        &backend.0,
        BOB,
        json!({ "ids": ["R1"] }),
    )["list"][0]
        .clone();
    assert_eq!(stored["state"], "stopped");
    assert_eq!(stored["stoppedAt"], "2026-06-05T14:50:00Z");
    // L1149 delivers the `"paused"` event only for the pause transition.
    assert!(backend.0.recording_events_for_test().is_empty());
}

#[tokio::test]
async fn stop_after_a_concurrent_failure_is_refused() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCLivestream",
        stored_livestream("L1", "live", json!("2026-06-05T14:40:00Z")),
    );
    let failed = stored_livestream("L1", "failed", json!("2026-06-05T14:40:00Z"));
    let backend = Racing::new(backend, "VTCLivestream", failed);

    let resp = call!(ids handle_vtc_livestream_set,
        &backend,
        BOB,
        json!({ "update": { "L1": { "state": "stopped" } } }),
    );

    // L1266 "The server MUST return `invalidProperties` naming `state` if
    // the livestream is not in state `"starting"` or `"live"`."
    assert_state_refused(&not_updated(&resp, "L1"));
    let stored = call!(
        handle_vtc_livestream_get,
        &backend.0,
        BOB,
        json!({ "ids": ["L1"] }),
    )["list"][0]
        .clone();
    assert_eq!(stored["state"], "failed");
    assert_eq!(stored["stoppedAt"], Value::Null);
}

#[tokio::test]
async fn end_after_a_concurrent_end_is_refused() {
    let backend = Racing::new(
        media_backend(),
        "VTCCall",
        ended_call("C1", "room", "timeout"),
    );

    let resp = call!(ids handle_vtc_call_set,
        &backend,
        BOB,
        json!({ "update": { "C1": { "state": "ended" } } }),
    );

    // L506 "For all call types, `"ended"` is a terminal state. The server
    // MUST NOT transition a call out of `"ended"`." The call's `endedAt`
    // and `endReason` are those of the end that reached the store first.
    assert_state_refused(&not_updated(&resp, "C1"));
    let stored = call!(
        handle_vtc_call_get,
        &backend.0,
        BOB,
        json!({ "ids": ["C1"] })
    )["list"][0]
        .clone();
    assert_eq!(stored["state"], "ended");
    assert_eq!(stored["endReason"], "timeout");
    assert_eq!(stored["endedAt"], "2026-06-05T15:00:00Z");
}

#[tokio::test]
async fn join_after_a_concurrent_cancel_leaves_the_call_ended() {
    let backend = backend();
    seed(
        &backend,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    let backend = Racing::new(
        backend,
        "VTCCall",
        ended_call("S1", "scheduled", "cancelled"),
    );

    let resp = call!(ids handle_vtc_participant_set,
        &backend,
        ALICE,
        json!({ "create": { "j0": { "callId": "S1", "joinMethod": "webrtc" } } }),
    );
    // The join read the call as `"pending"` and is stored, so the
    // `"pending"` -> `"active"` transition (L503 "The first participant
    // joins") follows it.
    assert!(resp["created"]["j0"]["id"].is_string(), "resp: {resp}");

    // L506 "The server MUST NOT transition a call out of `"ended"`": the
    // moderator's cancel (L504 `"pending"` -> `"ended"`) reached the store
    // first.
    let stored = call!(
        handle_vtc_call_get,
        &backend.0,
        BOB,
        json!({ "ids": ["S1"] })
    )["list"][0]
        .clone();
    assert_eq!(stored["state"], "ended");
    assert_eq!(stored["endReason"], "cancelled");
}

/// The stored call `id`, read from `backend` by [`BOB`].
async fn stored(backend: &MemoryBackend, id: &str) -> Value {
    call!(handle_vtc_call_get, backend, BOB, json!({ "ids": [id] }))["list"][0].clone()
}

#[tokio::test]
async fn ring_timeout_after_a_concurrent_answer_leaves_the_call_active() {
    let backend = backend();
    seed(
        &backend,
        stored_call("R1", "ring", "ringing", "2026-06-05T14:30:00Z"),
    );
    let backend = Racing::new(
        backend,
        "VTCCall",
        stored_call("R1", "ring", "active", "2026-06-05T14:30:00Z"),
    );

    // 2027-01-15, long after createdAt plus the 30 s period of L1615.
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let delivered = expire_ring(
        &backend,
        &BOB.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from("R1"),
        now,
    )
    .await
    .expect("expire_ring");

    // L1615 "If the call is already `"active"` when the period expires,
    // the call continues; the server dispatches `VTCCallEndEvent` with
    // `endReason: "timeout"` to the devices of targets whose
    // `callResponse` is still `"pending"`." The call has no pending
    // target, so nothing is delivered.
    assert!(!delivered);
    let call = stored(&backend.0, "R1").await;
    assert_eq!(call["state"], "active");
    assert_eq!(call["endReason"], Value::Null);
    assert!(backend.0.events_for_test().is_empty());
}

#[tokio::test]
async fn leave_after_a_concurrent_answer_ends_the_call_completed() {
    let backend = backend();
    seed(
        &backend,
        stored_call("R1", "ring", "active", "2026-06-05T14:30:00Z"),
    );
    // The leave's read of the call, from before a target answered.
    let stale: VtcCall =
        serde_json::from_value(stored_call("R1", "ring", "ringing", "2026-06-05T14:30:00Z"))
            .expect("call");

    after_leave(
        &backend,
        &BOB.to_owned(),
        &Id::from(ACCOUNT),
        &stale,
        &CallRole::Moderator,
    )
    .await
    .expect("after_leave");

    // L966 "When the last active participant leaves a ring or scheduled
    // call, the server transitions the VTCCall to `"ended"` with
    // `endReason: "completed"`, or `"cancelled"` if the call was still
    // `"ringing"`." The stored call was active, so no ring ends either.
    let call = stored(&backend, "R1").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "completed");
    assert!(backend.events_for_test().is_empty());
}

#[tokio::test]
async fn decline_end_of_an_answered_call_is_refused() {
    let backend = backend();
    seed(
        &backend,
        stored_call("R1", "ring", "active", "2026-06-05T14:30:00Z"),
    );

    // L930 "When all target participants have declined and the call is
    // still `"ringing"`, the server transitions the call to `"ended"` with
    // `endReason: "declined"`": the decline path ends from `"ringing"`
    // only, and this call was answered first.
    match end_call(
        &backend,
        &BOB.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from("R1"),
        &[CallState::Ringing],
        CallEndReason::Declined,
    )
    .await
    {
        Err(BackendSetError::SetError(e)) => {
            assert_state_refused(&serde_json::to_value(&e).expect("SetError"))
        }
        other => panic!("expected invalidProperties, got {other:?}"),
    }
    let call = stored(&backend, "R1").await;
    assert_eq!(call["state"], "active");
    assert_eq!(call["endReason"], Value::Null);
}

#[tokio::test]
async fn moderator_end_of_a_concurrently_answered_call_completes() {
    let backend = backend();
    seed(
        &backend,
        stored_call("R1", "ring", "ringing", "2026-06-05T14:30:00Z"),
    );
    let backend = Racing::new(
        backend,
        "VTCCall",
        stored_call("R1", "ring", "active", "2026-06-05T14:30:00Z"),
    );

    let resp = call!(ids handle_vtc_call_set,
        &backend,
        BOB,
        json!({ "update": { "R1": { "state": "ended" } } }),
    );

    // L488 "`"active"` → `"ended"`: All participants leave or a moderator
    // ends the call." and L744 "`"completed"`: The call was in state
    // `"active"` when ended." The call was answered before the end reached
    // the store, so the end applies with the reason for `"active"`.
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(resp["updated"]["R1"]["endReason"], "completed", "{resp}");
    let call = stored(&backend.0, "R1").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "completed");
    // L1422 "When a ringing call is ... cancelled ... the server delivers a
    // `VTCCallEndEvent`" does not apply: the call was no longer ringing.
    assert!(backend.0.events_for_test().is_empty());
}

#[tokio::test]
async fn decline_of_a_call_answered_after_the_reload_leaves_it_active() {
    let backend = backend();
    let call_id = create(
        &backend,
        BOB,
        json!({"callType": "ring", "targetParticipantIds": [ALICE], "mediaTypes": ["audio"]}),
    )
    .await["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let alice = call!(
        handle_vtc_participant_query,
        &backend,
        BOB,
        json!({ "filter": { "callId": call_id, "userId": ALICE } }),
    )["ids"][0]
        .as_str()
        .expect("ring record")
        .to_owned();
    let mut answered = stored(&backend, &call_id).await;
    assert_eq!(answered["state"], "ringing");
    answered["state"] = json!("active");
    // The decline reads the call twice (to validate, then to reload it for
    // the transitions); the answer lands after the reload.
    let backend = Racing::after(backend, "VTCCall", answered, 1);

    let resp = call!(ids handle_vtc_participant_set,
        &backend,
        ALICE,
        json!({ "update": { alice.as_str(): { "callResponse": "declined" } } }),
    );

    // L930 "When all target participants have declined and the call is
    // still `"ringing"`, the server transitions the call to `"ended"` with
    // `endReason: "declined"`." The call was active when the end reached
    // the store, so it stays active.
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let call = stored(&backend.0, &call_id).await;
    assert_eq!(call["state"], "active");
    assert_eq!(call["endReason"], Value::Null);
}

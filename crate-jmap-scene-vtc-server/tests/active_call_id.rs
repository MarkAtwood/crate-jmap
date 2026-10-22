//! `SceneRegion.activeCallId` is cleared when its VTCCall ends.
//!
//! Oracle: scene L418: "When the VTCCall referenced by `activeCallId`
//! transitions to state `"ended"`, the server MUST clear this field to
//! `null` and emit a `StateChange` for the SceneRegion type. This update
//! is a side effect of the VTCCall state transition."

mod common;

use common::*;
use jmap_types::Id;
use jmap_vtc_server::end_call;
use jmap_vtc_types::{CallEndReason, CallState};
use serde_json::{json, Value};

/// A moderator ends the call through `VTCCall/set` (vtc L444: "A
/// moderator explicitly closes the room"). Only the region bound to that
/// call changes.
#[tokio::test]
async fn ending_via_set_clears_bound_region_with_state_change() {
    let backend = Bound::new();
    let call_id = create_room_call(&backend).await;
    let other_id = create_room_call(&backend).await;
    backend.seed_region("r-bound", &call_id);
    backend.seed_region("r-other", &other_id);
    let before = region_state(&backend).await;

    let resp = vtc_set(
        &backend,
        json!({ "update": { call_id.as_str(): { "state": "ended" } } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "end failed: {resp}");

    assert_eq!(get_call(&backend, &call_id).await["state"], "ended");
    assert_eq!(
        get_region(&backend, "r-bound").await["activeCallId"],
        Value::Null
    );
    assert_eq!(
        get_region(&backend, "r-other").await["activeCallId"],
        other_id.as_str()
    );
    assert_eq!(
        regions_updated_since(&backend, &before).await,
        json!(["r-bound"])
    );
}

/// The server ends the call itself (vtc L444: "a deployment-defined
/// inactivity timeout fires"; L367 `"timeout"`), which no `VTCCall/set`
/// makes.
#[tokio::test]
async fn auto_end_clears_bound_region_with_state_change() {
    let backend = Bound::new();
    let call_id = create_room_call(&backend).await;
    backend.seed_region("r-bound", &call_id);
    let before = region_state(&backend).await;

    end_call(
        &backend,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(call_id.as_str()),
        &[CallState::Active],
        CallEndReason::Timeout,
    )
    .await
    .expect("end_call");

    assert_eq!(get_call(&backend, &call_id).await["state"], "ended");
    assert_eq!(
        get_region(&backend, "r-bound").await["activeCallId"],
        Value::Null
    );
    assert_eq!(
        regions_updated_since(&backend, &before).await,
        json!(["r-bound"])
    );
}

/// The clear is a side effect of the `"ended"` transition only: an update
/// that leaves the call `"active"` (vtc L406: a moderator "MAY set"
/// `e2eeEnabled` "via update") keeps the binding and the state.
#[tokio::test]
async fn update_that_does_not_end_keeps_binding() {
    let backend = Bound::new();
    let call_id = create_room_call(&backend).await;
    backend.seed_region("r-bound", &call_id);
    let before = region_state(&backend).await;

    let resp = vtc_set(
        &backend,
        json!({ "update": { call_id.as_str(): { "e2eeEnabled": true } } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "update failed: {resp}");

    assert_eq!(get_call(&backend, &call_id).await["state"], "active");
    assert_eq!(
        get_region(&backend, "r-bound").await["activeCallId"],
        call_id.as_str()
    );
    assert_eq!(regions_updated_since(&backend, &before).await, json!([]));
}

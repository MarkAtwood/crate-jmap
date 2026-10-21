//! `VTCCall/set` update: the state machine (draft-atwood-jmap-vtc-00 §State
//! Machine, L426-455), §Ending a Call (L687-706) and §Moderator Actions on
//! VTCCall (L708-719). Line numbers cite draft-atwood-jmap-vtc-00.md at
//! jmap-chat-spec 3227c0f.
//!
//! The participant-driven transitions (`"ringing"` → `"active"` on answer,
//! `"pending"` → `"active"` on join, ending when all leave or decline) and
//! the ring timeout belong to VTCParticipant/set and the ring machinery;
//! here they only appear as requests `VTCCall/set` must refuse.

mod common;

use common::*;
use serde_json::{json, Value};

fn ring() -> Value {
    json!({
        "callType": "ring",
        "targetParticipantIds": [ALICE],
        "mediaTypes": ["audio"]
    })
}

fn room() -> Value {
    json!({ "callType": "room", "mediaTypes": ["audio"] })
}

fn scheduled() -> Value {
    json!({
        "callType": "scheduled",
        "mediaTypes": ["audio"],
        "scheduledStartAt": "2099-01-01T00:00:00Z"
    })
}

async fn create_id(backend: &jmap_vtc_server::memory::MemoryBackend, call: Value) -> String {
    create(backend, BOB, call).await["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

fn assert_invalid_state(err: Value) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["state"]), "{err}");
}

/// L697-703: the example update `{"state": "ended"}`.
fn end() -> Value {
    json!({ "state": "ended" })
}

#[tokio::test]
async fn ending_an_active_call_completes_it() {
    let backend = backend();
    let id = create_id(&backend, room()).await;

    // L689 "The server sets `endedAt` ... The server sets `endReason` based
    // on context"; L691 "`"completed"`: The call was in state `"active"`
    // when ended."
    let echo = update(&backend, BOB, &id, end()).await.expect("end");
    let call = get(&backend, &id).await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "completed");
    assert!(call["endedAt"].is_string(), "{call}");
    // RFC 8620 §5.3 `updated`: "any property that changed in a way not
    // explicitly requested by the PatchObject".
    assert_eq!(echo["endReason"], "completed");
    assert_eq!(echo["endedAt"], call["endedAt"]);
}

#[tokio::test]
async fn initiator_ending_a_ringing_call_cancels_it() {
    let backend = backend();
    let id = create_id(&backend, ring()).await;
    // L436 "`"ringing"` → `"ended"`: Timeout, all targets decline, or
    // initiator cancels."; L692 "`"cancelled"`: The call was in state
    // `"ringing"` when the initiator ended it".
    update(&backend, BOB, &id, end()).await.expect("cancel");
    let call = get(&backend, &id).await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "cancelled");
}

/// The `VTCCallEndEvent`s delivered so far, as wire JSON.
fn call_end_events(backend: &jmap_vtc_server::memory::MemoryBackend) -> Vec<Value> {
    backend
        .events_for_test()
        .into_iter()
        .filter_map(|e| match e {
            jmap_vtc_types::VtcEvent::CallEnd(e) => {
                Some(serde_json::to_value(e).expect("serialize"))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn cancelling_a_ringing_call_stops_the_ring() {
    let backend = backend();
    let id = create_id(&backend, ring()).await;
    // jmap-chat-spec 33702a5 L1422 "When a ringing call is answered,
    // declined, cancelled, or times out, the server delivers a
    // `VTCCallEndEvent` to all devices that received the ring."; L1431
    // "`"cancelled"` (initiator withdrew the ring)".
    update(&backend, BOB, &id, end()).await.expect("cancel");
    assert_eq!(
        call_end_events(&backend),
        [json!({"accountId": ACCOUNT, "callId": id, "endReason": "cancelled"})]
    );
}

#[tokio::test]
async fn cancelling_a_pending_scheduled_call_rings_no_device() {
    let backend = backend();
    let id = create_id(&backend, scheduled()).await;
    // jmap-chat-spec 33702a5 L1422: the event is for "a ringing call" and
    // "devices that received the ring"; a scheduled call never rang.
    update(&backend, BOB, &id, end()).await.expect("cancel");
    assert_eq!(get(&backend, &id).await["endReason"], "cancelled");
    assert!(call_end_events(&backend).is_empty());
}

#[tokio::test]
async fn ring_target_cannot_end_the_call() {
    let backend = backend();
    let id = create_id(&backend, ring()).await;
    // L706 "The server MUST return `forbidden` if the caller is not a
    // moderator on this call." Alice is a ring target with role
    // "participant" (L294: only the initiator is moderator by default).
    let err = update(&backend, ALICE, &id, end())
        .await
        .expect_err("forbidden");
    assert_eq!(err["type"], "forbidden", "{err}");
    assert_eq!(get(&backend, &id).await["state"], "ringing");
}

#[tokio::test]
async fn answering_is_not_a_vtc_call_set_transition() {
    let backend = backend();
    let id = create_id(&backend, ring()).await;
    // L435 "`"ringing"` → `"active"`: A target participant answers"; L428
    // "clients request transitions via `VTCCall/set` and the server
    // validates them"; L706 "The server MUST return `invalidProperties`
    // naming `state` for any `state` transition not permitted by the state
    // machine".
    let err = update(&backend, BOB, &id, json!({ "state": "active" }))
        .await
        .expect_err("refused");
    assert_invalid_state(err);
    assert_eq!(get(&backend, &id).await["state"], "ringing");
}

#[tokio::test]
async fn active_ring_call_ends_completed() {
    let backend = backend();
    seed(
        &backend,
        stored_call("R1", "ring", "active", "2026-06-05T14:30:00Z"),
    );
    // L437 "`"active"` → `"ended"`: All participants leave or a moderator
    // ends the call."; L691.
    update(&backend, BOB, "R1", end()).await.expect("end");
    assert_eq!(get(&backend, "R1").await["endReason"], "completed");
}

#[tokio::test]
async fn creating_cannot_jump_to_ended() {
    let backend = backend();
    seed(
        &backend,
        stored_call("R2", "ring", "creating", "2026-06-05T14:30:00Z"),
    );
    // L434: the only transition out of `"creating"` is `"ringing"`, made by
    // the server; L706 refuses any other.
    let err = update(&backend, BOB, "R2", end())
        .await
        .expect_err("refused");
    assert_invalid_state(err);
}

#[tokio::test]
async fn non_moderator_cannot_close_a_room() {
    let backend = backend();
    let id = create_id(&backend, room()).await;
    // L444 "`"active"` → `"ended"`: A moderator explicitly closes the
    // room"; L706 "forbidden if the caller is not a moderator".
    let err = update(&backend, EVE, &id, end())
        .await
        .expect_err("forbidden");
    assert_eq!(err["type"], "forbidden", "{err}");
    assert_eq!(get(&backend, &id).await["state"], "active");
}

#[tokio::test]
async fn cancelling_a_pending_scheduled_call() {
    let backend = backend();
    let id = create_id(&backend, scheduled()).await;
    // L452 "`"pending"` → `"ended"`: A moderator cancels the scheduled call
    // before it starts."; L365 "`"cancelled"`: The initiator withdrew a
    // ringing or pending call before anyone answered/joined."
    update(&backend, BOB, &id, end()).await.expect("cancel");
    let call = get(&backend, &id).await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "cancelled");
}

#[tokio::test]
async fn pending_becomes_active_only_by_joining() {
    let backend = backend();
    let id = create_id(&backend, scheduled()).await;
    // L451 "`"pending"` → `"active"`: A participant joins at or after
    // `scheduledStartAt`." — not a `VTCCall/set` request (L706).
    let err = update(&backend, BOB, &id, json!({ "state": "active" }))
        .await
        .expect_err("refused");
    assert_invalid_state(err);
}

#[tokio::test]
async fn active_scheduled_call_ends_completed() {
    let backend = backend();
    seed(
        &backend,
        stored_call("S1", "scheduled", "active", "2026-06-05T14:30:00Z"),
    );
    // L453 "`"active"` → `"ended"`: All participants leave or a moderator
    // ends the call."; L691.
    update(&backend, BOB, "S1", end()).await.expect("end");
    assert_eq!(get(&backend, "S1").await["endReason"], "completed");
}

#[tokio::test]
async fn ended_is_terminal() {
    let backend = backend();
    let id = create_id(&backend, room()).await;
    update(&backend, BOB, &id, end()).await.expect("end");
    // L706 "The server MUST return `invalidProperties` naming `state` if
    // the call is already in state `"ended"`."; L455 "`"ended"` is a
    // terminal state. The server MUST NOT transition a call out of
    // `"ended"`."
    for state in ["ended", "active", "pending", "ringing"] {
        let err = update(&backend, BOB, &id, json!({ "state": state }))
            .await
            .expect_err("terminal");
        assert_invalid_state(err);
    }
    assert_eq!(get(&backend, &id).await["state"], "ended");
}

#[tokio::test]
async fn lobby_toggle_is_a_moderator_action() {
    let backend = backend();
    let id = create_id(&backend, room()).await;
    // L719 "**Toggle lobby:** Patch `lobbyEnabled` between `true` and
    // `false`."; L710 "Non-moderators MUST receive `forbidden` for these
    // operations."
    let err = update(&backend, EVE, &id, json!({ "lobbyEnabled": true }))
        .await
        .expect_err("forbidden");
    assert_eq!(err["type"], "forbidden", "{err}");
    update(&backend, BOB, &id, json!({ "lobbyEnabled": true }))
        .await
        .expect("moderator");
    assert_eq!(get(&backend, &id).await["lobbyEnabled"], true);
}

#[tokio::test]
async fn policy_and_e2ee_are_moderator_only() {
    let backend = backend();
    let id = create_id(&backend, room()).await;
    // L391 `policy`: "Mutable by moderators only."; L406 "the server MUST
    // return `forbidden` if a non-moderator attempts to update
    // `e2eeEnabled`".
    for patch in [
        json!({ "policy/muteOnEntry": true }),
        json!({ "e2eeEnabled": true }),
    ] {
        let err = update(&backend, EVE, &id, patch.clone())
            .await
            .expect_err("forbidden");
        assert_eq!(err["type"], "forbidden", "{patch}: {err}");
        update(&backend, BOB, &id, patch).await.expect("moderator");
    }
    let call = get(&backend, &id).await;
    assert_eq!(call["policy"]["muteOnEntry"], true);
    assert_eq!(call["e2eeEnabled"], true);
}

#[tokio::test]
async fn immutable_and_server_set_properties_are_refused() {
    let backend = backend();
    let id = create_id(&backend, room()).await;
    // L345-L397: `callType` and `parentCallId` are "immutable";
    // `initiatorId` is "immutable, server-set"; `endReason` and
    // `breakoutRoomIds` are "server-set".
    for (key, value) in [
        ("callType", json!("ring")),
        ("initiatorId", json!(EVE)),
        ("endReason", json!("missed")),
        ("parentCallId", json!("P1")),
        ("breakoutRoomIds", json!(["X"])),
    ] {
        let err = update(&backend, BOB, &id, json!({ key: value }))
            .await
            .expect_err("refused");
        assert_eq!(err["type"], "invalidProperties", "{key}: {err}");
        assert_eq!(err["properties"], json!([key]), "{key}: {err}");
    }
}

#[tokio::test]
async fn breakout_rooms_track_the_parent() {
    let backend = backend();
    let parent = create_id(&backend, room()).await;
    let mut child = room();
    child["parentCallId"] = json!(parent);

    // L712-713 "**Create a breakout room:** Create a new VTCCall with
    // `parentCallId` set to the current call's id."; L710 "Non-moderators
    // MUST receive `forbidden` for these operations."
    let err = create_error(&backend, EVE, child.clone()).await;
    assert_eq!(err["type"], "forbidden", "{err}");

    // L713 "The server adds the new call's id to the parent's
    // `breakoutRoomIds`."
    let child_id = create(&backend, BOB, child).await["id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(
        get(&backend, &parent).await["breakoutRoomIds"],
        json!([child_id])
    );

    // L715-716 "**Close a breakout room:** Transition a child VTCCall to
    // `"ended"`. The server removes its id from the parent's
    // `breakoutRoomIds`."
    update(&backend, BOB, &child_id, end())
        .await
        .expect("close");
    assert_eq!(get(&backend, &parent).await["breakoutRoomIds"], json!([]));
}

/// `{"destroy": [id]}` as `caller`; returns `Ok(())` or `Err(notDestroyed
/// entry)`.
async fn destroy(
    backend: &jmap_vtc_server::memory::MemoryBackend,
    caller: &str,
    id: &str,
) -> Result<(), Value> {
    let resp = set(backend, caller, json!({ "destroy": [id] })).await;
    match resp["notDestroyed"].get(id) {
        Some(err) => Err(err.clone()),
        None => {
            assert_eq!(resp["destroyed"], json!([id]), "{resp}");
            Ok(())
        }
    }
}

#[tokio::test]
async fn ring_target_cannot_destroy_the_call() {
    let backend = backend();
    let id = create_id(&backend, ring()).await;
    // L1647 "A participant with `role: "participant"` MUST receive
    // `forbidden` ... for any `VTCCall/set` operation restricted to
    // moderators (ending a call, ...)". Destroy removes the call, so it
    // cannot be a way around L706. Alice is a ring target with role
    // "participant" (L294).
    let err = destroy(&backend, ALICE, &id).await.expect_err("forbidden");
    assert_eq!(err["type"], "forbidden", "{err}");
    assert_eq!(get(&backend, &id).await["state"], "ringing");
}

#[tokio::test]
async fn destroying_a_breakout_room_leaves_the_parent() {
    let backend = backend();
    let parent = create_id(&backend, room()).await;
    let mut child = room();
    child["parentCallId"] = json!(parent);
    let child_id = create(&backend, BOB, child).await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    // L1647: "closing breakout rooms" is moderator-only.
    let err = destroy(&backend, EVE, &child_id)
        .await
        .expect_err("forbidden");
    assert_eq!(err["type"], "forbidden", "{err}");

    // L397 "`breakoutRoomIds` ... ids of child breakout-room VTCCalls": a
    // destroyed child is no longer one.
    destroy(&backend, BOB, &child_id).await.expect("destroy");
    assert_eq!(get(&backend, &parent).await["breakoutRoomIds"], json!([]));
}

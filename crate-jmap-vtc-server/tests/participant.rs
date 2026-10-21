//! `VTCParticipant/*` (draft-atwood-jmap-vtc-00 §VTCParticipant Methods).
//! Line numbers and quotes cite draft-atwood-jmap-vtc-00.md at
//! jmap-chat-spec 6bcd9d8; each assertion's oracle is the quoted draft
//! text or a hand-written record.

mod common;

use common::*;
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapObject,
    QueryChangesResult, QueryObject, QueryResult, SetErrorType, SetObject,
};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::{MemoryBackend, MemoryError};
use jmap_vtc_server::{
    expire_ring, handle_vtc_participant_get, handle_vtc_participant_query,
    handle_vtc_participant_set, VtcBackend, RING_TIMEOUT,
};
use jmap_vtc_types::{SignalDirection, VtcCall, VtcCapability, VtcEvent, VtcRecordingStateEvent};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CAROL: &str = "user:carol@example.com";

/// Run `VTCParticipant/set` as `caller` and return the response.
async fn pset(backend: &MemoryBackend, caller: &str, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_participant_set(backend, &caller.to_owned(), args)
        .await
        .expect("VTCParticipant/set")
        .primary
}

/// Join `call_id` as `caller` with `extra` create properties; returns
/// `Ok(created entry)` or `Err(notCreated entry)`.
async fn join_with(
    backend: &MemoryBackend,
    caller: &str,
    call_id: &str,
    extra: Value,
) -> Result<Value, Value> {
    let mut obj = json!({ "callId": call_id, "joinMethod": "webrtc" });
    for (k, v) in extra.as_object().expect("object") {
        obj[k] = v.clone();
    }
    let resp = pset(backend, caller, json!({ "create": { "p0": obj } })).await;
    match resp["notCreated"].get("p0") {
        Some(e) => Err(e.clone()),
        None => Ok(resp["created"]["p0"].clone()),
    }
}

async fn join(backend: &MemoryBackend, caller: &str, call_id: &str) -> Value {
    join_with(backend, caller, call_id, json!({}))
        .await
        .expect("join")
}

/// One update as `caller`: `Ok(updated echo)` or `Err(notUpdated entry)`.
async fn pupdate(
    backend: &MemoryBackend,
    caller: &str,
    id: &str,
    patch: Value,
) -> Result<Value, Value> {
    let resp = pset(backend, caller, json!({ "update": { id: patch } })).await;
    match resp["notUpdated"].get(id) {
        Some(e) => Err(e.clone()),
        None => Ok(resp["updated"][id].clone()),
    }
}

async fn participant(backend: &MemoryBackend, id: &str) -> Value {
    handle_vtc_participant_get(
        backend,
        &BOB.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCParticipant/get")
    .primary["list"][0]
        .clone()
}

async fn pquery(backend: &MemoryBackend, args: Value) -> Result<Value, jmap_types::JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_participant_query(backend, &BOB.to_owned(), args)
        .await
        .map(|o| o.primary)
}

fn id_of(v: &Value) -> String {
    v["id"].as_str().expect("id").to_owned()
}

fn assert_error(err: &Value, kind: &str, properties: Option<&[&str]>) {
    assert_eq!(err["type"], kind, "{err}");
    if let Some(props) = properties {
        assert_eq!(err["properties"], json!(props), "{err}");
    }
}

/// A backend holding room call `C1` (active, initiator [`BOB`]) with
/// `call` merged into the stored call.
fn room_backend(cap: Value, call: Value) -> MemoryBackend {
    let backend = backend_with(cap);
    let mut stored = stored_call("C1", "room", "active", "2026-06-05T14:30:00Z");
    for (k, v) in call.as_object().expect("object") {
        stored[k] = v.clone();
    }
    seed(&backend, stored);
    backend
}

fn room() -> MemoryBackend {
    room_backend(capability(), json!({}))
}

/// A hand-written participant record (field list L329-381).
fn record(id: &str, call_id: &str, user: Option<&str>, role: &str, joined: Option<&str>) -> Value {
    json!({
        "id": id,
        "callId": call_id,
        "userId": user,
        "displayName": user.unwrap_or("+14155559876"),
        "role": role,
        "joinedAt": joined,
        "leftAt": null,
        "callResponse": null,
        "joinMethod": if user.is_some() { "webrtc" } else { "pstn" },
        "gatewayData": if user.is_some() { json!(null) } else { json!({"callerIdNumber": "+14155559876"}) },
        "lobbyState": null,
        "mediaState": {"audio": true, "video": false, "screen": false, "raisedHand": false},
        "speakerTimeMs": 0,
        "kickedBy": null,
        "e2eeFingerprint": null,
        "unmuteRequested": false,
        "breakoutCallId": null
    })
}

fn seed_participant(backend: &MemoryBackend, p: Value) {
    seed_as(backend, "VTCParticipant", p);
}

// ---- Joining ----

#[tokio::test]
async fn joining_a_room_call_returns_the_complete_record() {
    let backend = room();
    let p = join(&backend, ALICE, "C1").await;
    // L872 "The server sets `id`, `userId`, `displayName`, `role`,
    // `joinedAt`, `mediaState` (defaults), and `speakerTimeMs` (to `0`)."
    // The L896-L923 response lists every property; its mediaState is
    // audio and video on, screen and raisedHand off.
    let mut keys: Vec<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut want = vec![
        "id",
        "callId",
        "userId",
        "displayName",
        "role",
        "joinedAt",
        "leftAt",
        "callResponse",
        "joinMethod",
        "gatewayData",
        "lobbyState",
        "mediaState",
        "speakerTimeMs",
        "kickedBy",
        "e2eeFingerprint",
        "unmuteRequested",
        "breakoutCallId",
    ];
    want.sort_unstable();
    assert_eq!(keys, want);
    assert_eq!(p["callId"], "C1");
    assert_eq!(p["userId"], ALICE);
    assert_eq!(p["role"], "participant");
    assert!(p["joinedAt"].is_string(), "{p}");
    assert_eq!(p["leftAt"], json!(null));
    assert_eq!(p["callResponse"], json!(null));
    assert_eq!(p["lobbyState"], json!(null));
    assert_eq!(
        p["mediaState"],
        json!({"audio": true, "video": true, "screen": false, "raisedHand": false})
    );
    assert_eq!(p["speakerTimeMs"], 0);
    assert_eq!(p["breakoutCallId"], json!(null));
    // L436 activeParticipantCount: "Number of participants currently in
    // the call (joined and not yet left)."
    assert_eq!(get(&backend, "C1").await["activeParticipantCount"], 1);
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    let backend = room();
    // L872: the server sets `userId`, `role` and `joinedAt`.
    let err = join_with(&backend, ALICE, "C1", json!({"role": "moderator"}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["role"]));
}

#[tokio::test]
async fn joining_a_pending_scheduled_call_activates_it() {
    let backend = backend();
    seed(
        &backend,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    // L876 "When a participant joins a `"pending"` scheduled call, the
    // server transitions the call to `"active"`."
    join(&backend, ALICE, "S1").await;
    assert_eq!(get(&backend, "S1").await["state"], "active");
}

#[tokio::test]
async fn join_errors() {
    let backend = room();
    // L926 "The server SHOULD return `notFound` if the `callId` does not
    // exist".
    let err = join_with(&backend, ALICE, "nope", json!({}))
        .await
        .unwrap_err();
    assert_error(&err, "notFound", None);

    // L926 "The server MUST return `invalidProperties` naming `callId` if
    // the call is in state `"ended"`."
    let mut ended = stored_call("E1", "room", "ended", "2026-06-05T14:30:00Z");
    ended["endReason"] = json!("completed");
    ended["endedAt"] = json!("2026-06-05T15:00:00Z");
    seed(&backend, ended);
    let err = join_with(&backend, ALICE, "E1", json!({}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["callId"]));

    // L984 "A `create` for a user who already has a record in the call
    // fails with an `alreadyExists` SetError ... whose `existingId` names
    // that record."
    let first = join(&backend, ALICE, "C1").await;
    let err = join_with(&backend, ALICE, "C1", json!({}))
        .await
        .unwrap_err();
    assert_error(&err, "alreadyExists", None);
    assert_eq!(err["existingId"], first["id"]);
}

#[tokio::test]
async fn joining_a_full_call_is_over_quota() {
    let mut cap = capability();
    cap["maxParticipantsPerCall"] = json!(1);
    let backend = room_backend(cap, json!({}));
    join(&backend, ALICE, "C1").await;
    // L926 "If the call's `activeParticipantCount` has reached
    // `maxParticipantsPerCall`, the server MUST return `overQuota`."
    let err = join_with(&backend, CAROL, "C1", json!({}))
        .await
        .unwrap_err();
    assert_error(&err, "overQuota", None);
}

#[tokio::test]
async fn joining_activates_no_call_beyond_max_concurrent_calls() {
    let mut cap = capability();
    cap["maxConcurrentCalls"] = json!(1);
    let backend = room_backend(cap, json!({}));
    seed(
        &backend,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    // L926 "If the account already has `maxConcurrentCalls` active calls,
    // the server MUST return `overQuota`." C1 is the one active call.
    let err = join_with(&backend, ALICE, "S1", json!({}))
        .await
        .unwrap_err();
    assert_error(&err, "overQuota", None);
}

#[tokio::test]
async fn max_concurrent_calls_counts_calls_the_caller_cannot_access() {
    let mut cap = capability();
    cap["maxConcurrentCalls"] = json!(1);
    let backend = room_backend(cap, json!({}));
    seed(
        &backend,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    backend.deny_call_access(ALICE, "C1");
    // L926 "If the account already has `maxConcurrentCalls` active calls,
    // the server MUST return `overQuota`." The limit is the account's:
    // C1 is active in the account whether or not ALICE may see it.
    let err = join_with(&backend, ALICE, "S1", json!({}))
        .await
        .unwrap_err();
    assert_error(&err, "overQuota", None);
}

// ---- Media policy ----

#[tokio::test]
async fn entry_policy_mutes_regardless_of_the_request() {
    let backend = room_backend(
        capability(),
        json!({"policy": {"muteOnEntry": true, "videoOffOnEntry": true}}),
    );
    // L960 "When `policy.muteOnEntry` is `true`, the server sets
    // `mediaState.audio` to `false` ... regardless of the client's
    // requested value. Similarly for `policy.videoOffOnEntry`".
    let p = join_with(
        &backend,
        ALICE,
        "C1",
        json!({"mediaState": {"audio": true, "video": true, "screen": false, "raisedHand": false}}),
    )
    .await
    .expect("join");
    assert_eq!(p["mediaState"]["audio"], false);
    assert_eq!(p["mediaState"]["video"], false);
}

#[tokio::test]
async fn participants_can_unmute_false_needs_an_unmute_request() {
    let backend = room_backend(
        capability(),
        json!({"policy": {"participantsCanUnmute": false}}),
    );
    let p = join(&backend, ALICE, "C1").await;
    let id = id_of(&p);
    assert_eq!(p["mediaState"]["audio"], false, "{p}");

    // L311 "When `false`, non-moderator participants SHOULD receive
    // `forbidden` when attempting to set their own `mediaState.audio` to
    // `true` via `VTCParticipant/set`, unless their `unmuteRequested` is
    // `true`."
    let err = pupdate(&backend, ALICE, &id, json!({"mediaState/audio": true}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L994 "a moderator may request that a participant unmute by updating
    // the target's `unmuteRequested` to `true`. The server delivers a
    // `VTCUnmuteRequestEvent`"; L1579-1583 its `participantId` and
    // `requestedBy`.
    pupdate(&backend, BOB, &id, json!({"unmuteRequested": true}))
        .await
        .expect("ask to unmute");
    let events = backend.events_for_test();
    let [VtcEvent::UnmuteRequest(e)] = events.as_slice() else {
        panic!("one VTCUnmuteRequestEvent: {events:?}")
    };
    assert_eq!(e.participant_id, Id::from(id.as_str()));
    assert_eq!(e.call_id, Id::from("C1"));
    assert_eq!(e.requested_by, BOB);
    // draft-atwood-jmap-vtc-wss-00 L296-303: every event carries "the id
    // of the call's hosting account".
    assert_eq!(e.account_id, Id::from(ACCOUNT));

    // L378 unmuteRequested: "Cleared to `false` when the participant
    // unmutes".
    let echo = pupdate(&backend, ALICE, &id, json!({"mediaState/audio": true}))
        .await
        .expect("unmute");
    assert_eq!(echo["unmuteRequested"], false);
    let stored = participant(&backend, &id).await;
    assert_eq!(stored["mediaState"]["audio"], true);
    assert_eq!(stored["unmuteRequested"], false);
}

#[tokio::test]
async fn screen_and_video_policies_bind_non_moderators() {
    let backend = room_backend(
        capability(),
        json!({"policy": {"participantsCanShareScreen": false, "participantsCanStartVideo": false}}),
    );
    let id = id_of(&join(&backend, ALICE, "C1").await);
    // L314 / L317: non-moderators "SHOULD receive `forbidden` when
    // attempting to set their own `mediaState.screen`" / "`video` to
    // `true`".
    for patch in [
        json!({"mediaState/screen": true}),
        json!({"mediaState/video": true}),
    ] {
        let err = pupdate(&backend, ALICE, &id, patch).await.unwrap_err();
        assert_error(&err, "forbidden", None);
    }
    // The policy binds only non-moderators.
    let bob = id_of(&join(&backend, BOB, "C1").await);
    pupdate(&backend, BOB, &bob, json!({"mediaState/screen": true}))
        .await
        .expect("moderator shares screen");
}

#[tokio::test]
async fn only_moderators_update_other_participants() {
    let backend = room();
    let alice = id_of(&join(&backend, ALICE, "C1").await);
    join(&backend, CAROL, "C1").await;
    // L988 "Non-moderators SHOULD receive `forbidden` for updates targeting
    // other participants."
    let err = pupdate(&backend, CAROL, &alice, json!({"mediaState/audio": false}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);
    // L991 "Mute another participant: Update the target's
    // `mediaState/audio` to `false`."
    pupdate(&backend, BOB, &alice, json!({"mediaState/audio": false}))
        .await
        .expect("moderator mutes");
    assert_eq!(
        participant(&backend, &alice).await["mediaState"]["audio"],
        false
    );
}

// ---- Lobby ----

#[tokio::test]
async fn lobby_admission_is_a_moderator_action() {
    let backend = room_backend(capability(), json!({"lobbyEnabled": true}));
    // L878 "When `lobbyEnabled` is `true`, the server sets the new
    // participant's `lobbyState` to `"waiting"` and leaves `joinedAt`
    // `null`."
    let p = join(&backend, ALICE, "C1").await;
    let id = id_of(&p);
    assert_eq!(p["lobbyState"], "waiting");
    assert_eq!(p["joinedAt"], json!(null));
    assert_eq!(get(&backend, "C1").await["activeParticipantCount"], 0);

    // L1727 "the server SHOULD NOT allow participants to skip the lobby via
    // direct `VTCParticipant/set` manipulation."
    let err = pupdate(&backend, ALICE, &id, json!({"lobbyState": "admitted"}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L1001 "Update the target's `lobbyState` from `"waiting"` to
    // `"admitted"`. The server sets the target's `joinedAt` to the current
    // time."
    let echo = pupdate(&backend, BOB, &id, json!({"lobbyState": "admitted"}))
        .await
        .expect("admit");
    assert!(echo["joinedAt"].is_string(), "{echo}");
    assert_eq!(get(&backend, "C1").await["activeParticipantCount"], 1);

    // L1027 "`invalidProperties` naming `lobbyState` when a lobby-state
    // transition is invalid (e.g., `"admitted"` → `"waiting"`".
    let err = pupdate(&backend, BOB, &id, json!({"lobbyState": "waiting"}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["lobbyState"]));
}

#[tokio::test]
async fn lobby_rejection_sets_left_at_and_moderators_bypass() {
    let backend = room_backend(capability(), json!({"lobbyEnabled": true}));
    let id = id_of(&join(&backend, ALICE, "C1").await);
    // L1004 "from `"waiting"` to `"rejected"`. The server sets the
    // target's `leftAt` to the current time."
    let echo = pupdate(&backend, BOB, &id, json!({"lobbyState": "rejected"}))
        .await
        .expect("reject");
    assert!(echo["leftAt"].is_string(), "{echo}");
    // L363 lobbyState: "`null` when ... the participant bypassed the lobby
    // (moderators)."
    let bob = join(&backend, BOB, "C1").await;
    assert_eq!(bob["lobbyState"], json!(null));
    assert!(bob["joinedAt"].is_string(), "{bob}");
}

// ---- Leaving, kicking, reconnecting ----

#[tokio::test]
async fn leaving_records_server_time_once() {
    let backend = room();
    let alice = id_of(&join(&backend, ALICE, "C1").await);
    let carol = id_of(&join(&backend, CAROL, "C1").await);
    // L980 "The server SHOULD return `forbidden` if a caller who is not a
    // moderator on the call sets `leftAt` on a VTCParticipant record whose
    // userId does not match their own."
    let err = pupdate(
        &backend,
        CAROL,
        &alice,
        json!({"leftAt": "2026-06-05T15:02:00Z"}),
    )
    .await
    .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L964 "The server records its own current time as `leftAt`; the value
    // the client supplies only signals the intent to leave."
    let echo = pupdate(
        &backend,
        CAROL,
        &carol,
        json!({"leftAt": "2000-01-01T00:00:00Z"}),
    )
    .await
    .expect("leave");
    assert_ne!(echo["leftAt"], "2000-01-01T00:00:00Z");
    assert_eq!(
        participant(&backend, &carol).await["leftAt"],
        echo["leftAt"]
    );
    // L980 "The server MUST return `invalidProperties` naming `leftAt` if
    // the participant has already left".
    let err = pupdate(
        &backend,
        CAROL,
        &carol,
        json!({"leftAt": "2026-06-05T15:03:00Z"}),
    )
    .await
    .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["leftAt"]));
    // L966 "A room call stays `"active"` when it empties".
    pupdate(
        &backend,
        ALICE,
        &alice,
        json!({"leftAt": "2026-06-05T15:04:00Z"}),
    )
    .await
    .expect("leave");
    let call = get(&backend, "C1").await;
    assert_eq!(call["state"], "active");
    assert_eq!(call["activeParticipantCount"], 0);
}

#[tokio::test]
async fn kicked_participant_rejoins_only_after_the_kick_is_cleared() {
    let backend = room();
    let alice = id_of(&join(&backend, ALICE, "C1").await);
    // L998 "Kick a participant: Update the target's `leftAt`. The server
    // records its own current time as `leftAt` and sets `kickedBy` to the
    // moderator's userId."
    let echo = pupdate(
        &backend,
        BOB,
        &alice,
        json!({"leftAt": "2026-06-05T15:02:00Z"}),
    )
    .await
    .expect("kick");
    assert_eq!(echo["kickedBy"], BOB);

    // L984 "if the record's `kickedBy` is non-null, the server SHOULD
    // reject the update with `forbidden`."
    let err = pupdate(&backend, ALICE, &alice, json!({"leftAt": null}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L998 "Clients can set `kickedBy` only to `null`; any other value is
    // rejected as for a server-set property".
    let err = pupdate(&backend, BOB, &alice, json!({"kickedBy": EVE}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["kickedBy"]));
    // L372 "a moderator may clear it to `null`".
    pupdate(&backend, BOB, &alice, json!({"kickedBy": null}))
        .await
        .expect("clear kick");

    // L984 "re-joins the same call by updating their existing
    // VTCParticipant record to set `leftAt` to `null`. The server sets
    // `joinedAt` to the current time".
    let echo = pupdate(&backend, ALICE, &alice, json!({"leftAt": null}))
        .await
        .expect("rejoin");
    assert!(echo["joinedAt"].is_string(), "{echo}");
    let stored = participant(&backend, &alice).await;
    assert_eq!(stored["leftAt"], json!(null));
    assert_eq!(stored["id"], json!(alice));
}

#[tokio::test]
async fn last_departure_ends_a_scheduled_call() {
    let backend = backend();
    seed(
        &backend,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    let alice = id_of(&join(&backend, ALICE, "S1").await);
    // L966 "When the last active participant leaves a ring or scheduled
    // call, the server transitions the VTCCall to `"ended"` with
    // `endReason: "completed"`".
    pupdate(
        &backend,
        ALICE,
        &alice,
        json!({"leftAt": "2026-06-05T15:00:00Z"}),
    )
    .await
    .expect("leave");
    let call = get(&backend, "S1").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "completed");
}

#[tokio::test]
async fn leaving_a_call_a_moderator_ended_succeeds() {
    let backend = backend();
    seed(
        &backend,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    let alice = id_of(&join(&backend, ALICE, "S1").await);
    // L488 "`"active"` → `"ended"`: ... a moderator ends the call."
    update(&backend, BOB, "S1", json!({"state": "ended"}))
        .await
        .expect("end");
    let ended = get(&backend, "S1").await;

    // L964 "A participant calls `VTCParticipant/set` with an `update`
    // setting their own `leftAt`."
    let resp = pset(
        &backend,
        ALICE,
        json!({"update": {&alice: {"leftAt": "2026-06-05T15:00:00Z"}}}),
    )
    .await;
    assert_eq!(resp["notUpdated"], json!(null), "{resp}");
    assert!(resp["updated"].get(&alice).is_some(), "{resp}");
    // RFC 8620 §5.3 `newState`: "The state string that will now be
    // returned by Foo/get" — it differs once `leftAt` is stored.
    assert_ne!(resp["newState"], resp["oldState"], "{resp}");
    assert!(
        participant(&backend, &alice).await["leftAt"].is_string(),
        "leftAt stored"
    );
    // L761 "invalidProperties naming `state` if the call is already in
    // state `"ended"`": the leave starts no second end, and the
    // moderator's end stands.
    let after = get(&backend, "S1").await;
    for key in ["state", "endReason", "endedAt"] {
        assert_eq!(after[key], ended[key], "{key}");
    }
}

#[tokio::test]
async fn last_moderator_leaving_promotes_the_earliest_authenticated_joiner() {
    let backend = room();
    seed_participant(
        &backend,
        record(
            "PB",
            "C1",
            Some(BOB),
            "moderator",
            Some("2026-06-05T14:31:00Z"),
        ),
    );
    seed_participant(
        &backend,
        record(
            "PG",
            "C1",
            None,
            "participant",
            Some("2026-06-05T14:32:00Z"),
        ),
    );
    seed_participant(
        &backend,
        record(
            "PA",
            "C1",
            Some(ALICE),
            "participant",
            Some("2026-06-05T14:33:00Z"),
        ),
    );
    seed_participant(
        &backend,
        record(
            "PC",
            "C1",
            Some(CAROL),
            "participant",
            Some("2026-06-05T14:34:00Z"),
        ),
    );
    // L968 "the server SHOULD grant `"moderator"` to the remaining
    // authenticated participant (non-null `userId`) who joined earliest".
    pupdate(
        &backend,
        BOB,
        "PB",
        json!({"leftAt": "2026-06-05T15:00:00Z"}),
    )
    .await
    .expect("leave");
    assert_eq!(participant(&backend, "PA").await["role"], "moderator");
    assert_eq!(participant(&backend, "PG").await["role"], "participant");
    assert_eq!(participant(&backend, "PC").await["role"], "participant");
}

#[tokio::test]
async fn last_moderator_cannot_revoke_their_own_role() {
    let backend = room();
    seed_participant(
        &backend,
        record(
            "PB",
            "C1",
            Some(BOB),
            "moderator",
            Some("2026-06-05T14:31:00Z"),
        ),
    );
    // L1007 "A moderator MUST NOT revoke their own moderator role if they
    // are the last moderator; the server MUST reject this with
    // `forbidden`."
    let err = pupdate(&backend, BOB, "PB", json!({"role": "participant"}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);
}

// ---- Ring calls ----

async fn ring_to(backend: &MemoryBackend, targets: &[&str]) -> String {
    create(
        backend,
        BOB,
        json!({"callType": "ring", "targetParticipantIds": targets, "mediaTypes": ["audio"]}),
    )
    .await["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

/// The ring record of `user` in `call_id`, found as L838 says: "with
/// `VTCParticipant/query` filtering on `callId` and `userId`".
async fn ring_record(backend: &MemoryBackend, call_id: &str, user: &str) -> String {
    let resp = pquery(
        backend,
        json!({"filter": {"callId": call_id, "userId": user}}),
    )
    .await
    .expect("query");
    resp["ids"][0].as_str().expect("one record").to_owned()
}

#[tokio::test]
async fn answering_a_ringing_call_activates_it() {
    let backend = backend();
    let call = ring_to(&backend, &[ALICE]).await;
    let alice = ring_record(&backend, &call, ALICE).await;

    // L860 "The server SHOULD return `forbidden` if the caller's userId
    // does not match the VTCParticipant record's userId."
    let err = pupdate(&backend, EVE, &alice, json!({"joinMethod": "webrtc"}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L838 "The server sets `joinedAt` to the current time and
    // `callResponse` to `"accepted"`."; L846 "if the call is `"ringing"`,
    // the server transitions it to `"active"`."
    let echo = pupdate(&backend, ALICE, &alice, json!({"joinMethod": "webrtc"}))
        .await
        .expect("answer");
    assert_eq!(echo["callResponse"], "accepted");
    assert!(echo["joinedAt"].is_string(), "{echo}");
    assert_eq!(get(&backend, &call).await["state"], "active");

    // L860 "`invalidProperties` naming `joinMethod` if ... the record's
    // `callResponse` is not `"pending"`."
    let err = pupdate(&backend, ALICE, &alice, json!({"joinMethod": "webrtc"}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["joinMethod"]));
}

#[tokio::test]
async fn all_targets_declining_ends_the_ring() {
    let backend = backend();
    let call = ring_to(&backend, &[ALICE, CAROL]).await;
    let alice = ring_record(&backend, &call, ALICE).await;
    let carol = ring_record(&backend, &call, CAROL).await;
    // L930 "The server sets `leftAt` to the current time."
    let echo = pupdate(&backend, ALICE, &alice, json!({"callResponse": "declined"}))
        .await
        .expect("decline");
    assert!(echo["leftAt"].is_string(), "{echo}");
    assert_eq!(get(&backend, &call).await["state"], "ringing");
    // L942 forbidden "if the caller's userId does not match".
    let err = pupdate(&backend, ALICE, &carol, json!({"callResponse": "declined"}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);
    // L930 "When all target participants have declined and the call is
    // still `"ringing"`, the server transitions the call to `"ended"` with
    // `endReason: "declined"`."
    pupdate(&backend, CAROL, &carol, json!({"callResponse": "declined"}))
        .await
        .expect("decline");
    let ended = get(&backend, &call).await;
    assert_eq!(ended["state"], "ended");
    assert_eq!(ended["endReason"], "declined");
    // L942 "`invalidProperties` naming `callResponse` if the call is not in
    // state `"ringing"` or `"active"`".
    let err = pupdate(&backend, CAROL, &carol, json!({"callResponse": "declined"}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["callResponse"]));
}

// ---- Breakout rooms ----

#[tokio::test]
async fn breakout_assignment_names_a_child_room_and_resets_when_it_closes() {
    let backend = room();
    let alice = id_of(&join(&backend, ALICE, "C1").await);
    // L1010 "The server MUST return `invalidProperties` naming
    // `breakoutCallId` if a non-null value is not in the parent call's
    // `breakoutRoomIds`."
    let err = pupdate(&backend, BOB, &alice, json!({"breakoutCallId": "C1"}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["breakoutCallId"]));

    let child = create(
        &backend,
        BOB,
        json!({"callType": "room", "mediaTypes": ["audio"], "parentCallId": "C1"}),
    )
    .await["id"]
        .as_str()
        .expect("id")
        .to_owned();
    // L381 "Only moderators set it; non-moderators SHOULD receive
    // `forbidden`".
    let err = pupdate(&backend, ALICE, &alice, json!({"breakoutCallId": child}))
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);
    pupdate(&backend, BOB, &alice, json!({"breakoutCallId": child}))
        .await
        .expect("assign");
    assert_eq!(
        participant(&backend, &alice).await["breakoutCallId"],
        json!(child)
    );

    // L771 "Transition a child VTCCall to `"ended"`. The server ... resets
    // to `null` any `breakoutCallId` on the parent call's participants
    // that names it."
    update(&backend, BOB, &child, json!({"state": "ended"}))
        .await
        .expect("close room");
    assert_eq!(
        participant(&backend, &alice).await["breakoutCallId"],
        json!(null)
    );
}

// ---- Gateway dial-out and signals ----

fn gateway_capability() -> Value {
    let mut cap = capability();
    // L247-257: a PSTN gateway that dials out, a SIP one that does not.
    cap["gateways"] = json!([
        {"protocol": "pstn", "supportsDialOut": true, "supportedMediaTypes": ["audio"]},
        {"protocol": "sip", "supportsDialOut": false, "supportedMediaTypes": ["audio"]}
    ]);
    cap
}

/// The L1052-L1061 dial-out create.
fn dial_out() -> Value {
    json!({
        "joinMethod": "pstn",
        "gatewayData": {"dialNumber": "+14155559876"},
        "displayName": "Alice (mobile)"
    })
}

#[tokio::test]
async fn moderator_dials_out_through_a_dial_out_gateway() {
    let backend = room_backend(gateway_capability(), json!({}));
    // L1064 "Non-moderators SHOULD receive `forbidden` for dial-out
    // creates."
    let err = join_with(&backend, ALICE, "C1", dial_out())
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L1064 "`invalidProperties` naming `joinMethod` when `joinMethod` does
    // not match any VTCGateway `protocol` whose `supportsDialOut` is
    // `true`."
    let mut sip = dial_out();
    sip["joinMethod"] = json!("sip");
    let err = join_with(&backend, BOB, "C1", sip).await.unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["joinMethod"]));

    // L1047 "The server creates a VTCParticipant with `joinedAt: null`";
    // L1068 a gateway record has `userId` `null`.
    let p = join_with(&backend, BOB, "C1", dial_out())
        .await
        .expect("dial out");
    assert_eq!(p["joinedAt"], json!(null));
    assert_eq!(p["userId"], json!(null));
    assert_eq!(p["joinMethod"], "pstn");
    assert_eq!(p["displayName"], "Alice (mobile)");
    assert_eq!(p["gatewayData"], json!({"dialNumber": "+14155559876"}));
}

#[tokio::test]
async fn e2ee_call_refuses_dial_out() {
    let backend = room_backend(gateway_capability(), json!({"e2eeEnabled": true}));
    // L1064 "When the call has `e2eeEnabled: true`, a dial-out whose
    // gateway cannot join as a disclosed key-holding participant is
    // rejected with `forbidden`".
    let err = join_with(&backend, BOB, "C1", dial_out())
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);
}

#[tokio::test]
async fn pending_signal_is_relayed_to_the_gateway_and_not_stored() {
    let backend = room_backend(gateway_capability(), json!({}));
    seed_participant(
        &backend,
        record(
            "PG",
            "C1",
            None,
            "participant",
            Some("2026-06-05T14:32:00Z"),
        ),
    );
    join(&backend, ALICE, "C1").await;
    // The L1498-L1512 example update.
    let patch = json!({
        "gatewayData/pendingSignal": {
            "signal": "refer",
            "data": {"referTo": "sip:bob@example.com", "referredBy": "sip:alice@example.com"}
        }
    });
    // L1739 "Outbound gateway signals are sent through a
    // `VTCParticipant/set` update on another participant's record, which
    // only moderators may make".
    let err = pupdate(&backend, ALICE, "PG", patch.clone())
        .await
        .unwrap_err();
    assert_error(&err, "forbidden", None);

    // L1479 "The server extracts the signal, delivers it to the gateway,
    // and removes the key."
    pupdate(&backend, BOB, "PG", patch).await.expect("signal");
    let events = backend.events_for_test();
    let [VtcEvent::GatewaySignal(s)] = events.as_slice() else {
        panic!("one VTCGatewaySignal: {events:?}")
    };
    assert_eq!(s.call_id, Id::from("C1"));
    assert_eq!(s.participant_id, Id::from("PG"));
    // draft-atwood-jmap-vtc-wss-00 L296-303: the hosting account's id.
    assert_eq!(s.account_id, Id::from(ACCOUNT));
    assert_eq!(s.signal, "refer");
    assert_eq!(s.data["referTo"], "sip:bob@example.com");
    // L1474 "`"outbound"` (from a moderator toward the external party)".
    assert_eq!(s.direction, SignalDirection::Outbound);
    assert_eq!(
        participant(&backend, "PG").await["gatewayData"],
        json!({"callerIdNumber": "+14155559876"})
    );
}

// ---- Query and destroy ----

#[tokio::test]
async fn query_needs_a_call_or_user_scope_and_sorts_by_joined_at() {
    let backend = room();
    seed_participant(
        &backend,
        record(
            "P2",
            "C1",
            Some(ALICE),
            "participant",
            Some("2026-06-05T14:40:00Z"),
        ),
    );
    seed_participant(
        &backend,
        record(
            "P1",
            "C1",
            Some(CAROL),
            "participant",
            Some("2026-06-05T14:35:00Z"),
        ),
    );
    let mut left = record(
        "P3",
        "C1",
        Some(EVE),
        "participant",
        Some("2026-06-05T14:30:00Z"),
    );
    left["leftAt"] = json!("2026-06-05T14:31:00Z");
    seed_participant(&backend, left);

    // L1077 "a query that omits both `callId` and `userId` MAY be rejected
    // with `unsupportedFilter`."
    let err = pquery(&backend, json!({"filter": {"role": "participant"}}))
        .await
        .unwrap_err();
    assert_eq!(err, jmap_types::JmapError::unsupported_filter());

    // L1096 "Default sort: `joinedAt` ascending."
    let resp = pquery(&backend, json!({"filter": {"callId": "C1"}}))
        .await
        .expect("query");
    assert_eq!(resp["ids"], json!(["P3", "P1", "P2"]));
    // L1086 isActive: "When `true`, filter to participants with `joinedAt
    // != null` and `leftAt == null`. When `false`, filter to participants
    // who have left."
    let resp = pquery(
        &backend,
        json!({"filter": {"callId": "C1", "isActive": true}}),
    )
    .await
    .expect("query");
    assert_eq!(resp["ids"], json!(["P1", "P2"]));
    let resp = pquery(
        &backend,
        json!({"filter": {"callId": "C1", "isActive": false}}),
    )
    .await
    .expect("query");
    assert_eq!(resp["ids"], json!(["P3"]));
}

#[tokio::test]
async fn records_are_not_destroyed() {
    let backend = room();
    let alice = id_of(&join(&backend, ALICE, "C1").await);
    // L325 "An authenticated user has at most one VTCParticipant record per
    // call, which is reused when the user rejoins"; the draft defines no
    // destroy.
    let resp = pset(&backend, BOB, json!({"destroy": [alice.clone(), "nope"]})).await;
    assert_error(&resp["notDestroyed"][&alice], "forbidden", None);
    assert_error(&resp["notDestroyed"]["nope"], "notFound", None);
    assert_eq!(participant(&backend, &alice).await["id"], json!(alice));
}

/// The `VTCCallEndEvent`s delivered so far, as wire JSON.
fn call_end_events(backend: &MemoryBackend) -> Vec<Value> {
    backend
        .events_for_test()
        .into_iter()
        .filter_map(|e| match e {
            VtcEvent::CallEnd(e) => Some(serde_json::to_value(e).expect("serialize")),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn answering_forks_answered_elsewhere_to_other_devices() {
    let backend = backend();
    let call = ring_to(&backend, &[ALICE, CAROL]).await;
    let alice = ring_record(&backend, &call, ALICE).await;
    let carol = ring_record(&backend, &call, CAROL).await;

    // L846 "On success, if the call is `"ringing"`, the server transitions
    // it to `"active"`. In either case the server dispatches a
    // `VTCCallEndEvent` with `endReason: "answered_elsewhere"` to the
    // answering user's other ringing devices".
    pupdate(&backend, ALICE, &alice, json!({"joinMethod": "webrtc"}))
        .await
        .expect("first answer");
    assert_eq!(get(&backend, &call).await["state"], "active");
    let expected = json!({
        "accountId": ACCOUNT,
        "callId": call,
        "endReason": "answered_elsewhere"
    });
    assert_eq!(call_end_events(&backend), std::slice::from_ref(&expected));

    // L1611 "The other targets keep ringing: each may still answer (and
    // join)"; "In either case" covers the answer to the active call.
    pupdate(&backend, CAROL, &carol, json!({"joinMethod": "webrtc"}))
        .await
        .expect("second answer");
    assert_eq!(call_end_events(&backend), [expected.clone(), expected]);
}

#[tokio::test]
async fn declining_does_not_fork_answered_elsewhere() {
    let backend = backend();
    let call = ring_to(&backend, &[ALICE, CAROL]).await;
    let alice = ring_record(&backend, &call, ALICE).await;
    // L1431 "`\"answered_elsewhere\"` (the same user answered on a
    // different device)": a decline is not an answer.
    pupdate(&backend, ALICE, &alice, json!({"callResponse": "declined"}))
        .await
        .expect("decline");
    assert!(call_end_events(&backend)
        .iter()
        .all(|e| e["endReason"] != "answered_elsewhere"));
}

#[tokio::test]
async fn declining_forks_declined_to_other_devices() {
    let backend = backend();
    let call = ring_to(&backend, &[ALICE, CAROL]).await;
    let alice = ring_record(&backend, &call, ALICE).await;
    let carol = ring_record(&backend, &call, CAROL).await;
    // jmap-chat-spec 33702a5 L1422 "When a ringing call is answered,
    // declined, cancelled, or times out, the server delivers a
    // `VTCCallEndEvent` to all devices that received the ring."; L1443
    // "When a ring call is answered or declined on one device ... all
    // other devices that received the original `VTCCallPush`, carrying the
    // `endReason` (`"answered_elsewhere"`, `"declined"`, or `"timeout"`)".
    let declined = json!({"accountId": ACCOUNT, "callId": call, "endReason": "declined"});
    pupdate(&backend, ALICE, &alice, json!({"callResponse": "declined"}))
        .await
        .expect("decline");
    assert_eq!(call_end_events(&backend), std::slice::from_ref(&declined));
    // L930 the last decline ends the call `"declined"`, not `"cancelled"`:
    // the second target's devices get its own decline, and nothing more.
    pupdate(&backend, CAROL, &carol, json!({"callResponse": "declined"}))
        .await
        .expect("decline");
    assert_eq!(get(&backend, &call).await["endReason"], "declined");
    assert_eq!(call_end_events(&backend), [declined.clone(), declined]);
}

#[tokio::test]
async fn initiator_leaving_a_ringing_call_cancels_the_ring() {
    let backend = backend();
    let call = ring_to(&backend, &[ALICE]).await;
    let bob = ring_record(&backend, &call, BOB).await;
    // jmap-chat-spec 33702a5 L966 "When the last active participant leaves
    // a ring or scheduled call, the server transitions the VTCCall to
    // `"ended"` with ... `"cancelled"` if the call was still `"ringing"`";
    // L1422 a cancelled ringing call delivers a `VTCCallEndEvent`; L1431
    // "`"cancelled"` (initiator withdrew the ring)".
    pupdate(
        &backend,
        BOB,
        &bob,
        json!({"leftAt": "2026-06-05T15:00:00Z"}),
    )
    .await
    .expect("leave");
    assert_eq!(get(&backend, &call).await["endReason"], "cancelled");
    assert_eq!(
        call_end_events(&backend),
        [json!({"accountId": ACCOUNT, "callId": call, "endReason": "cancelled"})]
    );
}

/// 2026-10-20T12:00:00Z plus `secs`, from `date -u -d 2026-10-20T12:00:00Z
/// +%s` = 1792497600.
fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_792_497_600 + secs)
}

/// Ring call `R1` created 2026-10-20T12:00:00Z by [`BOB`], in `state`,
/// with targets [`ALICE`] and [`CAROL`] whose `callResponse`s are given.
fn ring_fixture(state: &str, alice: &str, carol: &str) -> MemoryBackend {
    let backend = backend();
    seed(
        &backend,
        stored_call("R1", "ring", state, "2026-10-20T12:00:00Z"),
    );
    seed_participant(
        &backend,
        record(
            "P-BOB",
            "R1",
            Some(BOB),
            "moderator",
            Some("2026-10-20T12:00:00Z"),
        ),
    );
    for (id, user, response) in [("P-ALICE", ALICE, alice), ("P-CAROL", CAROL, carol)] {
        let joined = (response == "accepted").then_some("2026-10-20T12:00:05Z");
        let mut p = record(id, "R1", Some(user), "participant", joined);
        p["callResponse"] = json!(response);
        seed_participant(&backend, p);
    }
    backend
}

async fn expire(backend: &MemoryBackend, now: SystemTime) -> bool {
    expire_ring(
        backend,
        &BOB.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from("R1"),
        now,
    )
    .await
    .expect("expire_ring")
}

#[test]
fn ring_timeout_is_thirty_seconds() {
    // L1615 "a deployment-defined period (RECOMMENDED: 30 seconds)".
    assert_eq!(RING_TIMEOUT, Duration::from_secs(30));
    assert_eq!(backend().ring_timeout(), Duration::from_secs(30));
}

#[tokio::test]
async fn unanswered_ring_ends_missed_at_thirty_seconds() {
    let backend = ring_fixture("ringing", "pending", "pending");

    assert!(!expire(&backend, at(29)).await);
    assert_eq!(get(&backend, "R1").await["state"], "ringing");
    assert!(call_end_events(&backend).is_empty());

    // L1615 "If no target participant answers within a deployment-defined
    // period (RECOMMENDED: 30 seconds), the server transitions the call to
    // `"ended"` with `endReason: "missed"` and dispatches `VTCCallEndEvent`
    // with `endReason: "timeout"` to all ringing devices."
    assert!(expire(&backend, at(30)).await);
    let call = get(&backend, "R1").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "missed");
    assert_eq!(
        call_end_events(&backend),
        [json!({"accountId": ACCOUNT, "callId": "R1", "endReason": "timeout"})]
    );

    // An ended call has no ring left to time out.
    assert!(!expire(&backend, at(31)).await);
    assert_eq!(call_end_events(&backend).len(), 1);
}

#[tokio::test]
async fn timeout_on_active_ring_keeps_the_call() {
    let backend = ring_fixture("active", "accepted", "pending");
    // L1615 "If the call is already `"active"` when the period expires, the
    // call continues; the server dispatches `VTCCallEndEvent` with
    // `endReason: "timeout"` to the devices of targets whose
    // `callResponse` is still `"pending"`."
    assert!(expire(&backend, at(30)).await);
    assert_eq!(get(&backend, "R1").await["state"], "active");
    assert_eq!(
        call_end_events(&backend),
        [json!({"accountId": ACCOUNT, "callId": "R1", "endReason": "timeout"})]
    );
}

#[tokio::test]
async fn timeout_on_active_ring_without_pending_targets_is_silent() {
    // L1615: the event goes only to "targets whose `callResponse` is still
    // `"pending"`"; here there are none.
    let backend = ring_fixture("active", "accepted", "declined");
    assert!(!expire(&backend, at(30)).await);
    assert_eq!(get(&backend, "R1").await["state"], "active");
    assert!(call_end_events(&backend).is_empty());
}

#[tokio::test]
async fn late_answer_or_decline_after_ring_timeout_is_rejected() {
    // L1611 "each may still answer (and join) or decline until the ring
    // timeout expires"; L1615 "a deployment-defined period (RECOMMENDED: 30
    // seconds)". R1 was created 2026-10-20T12:00:00Z, so the default period
    // ended long before the wall clock this test reads.
    let backend = ring_fixture("active", "accepted", "pending");
    let err = pupdate(&backend, CAROL, "P-CAROL", json!({"joinMethod": "webrtc"}))
        .await
        .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["joinMethod"]));
    let err = pupdate(
        &backend,
        CAROL,
        "P-CAROL",
        json!({"callResponse": "declined"}),
    )
    .await
    .unwrap_err();
    assert_error(&err, "invalidProperties", Some(&["callResponse"]));
    assert_eq!(get(&backend, "R1").await["state"], "active");
}

#[tokio::test]
async fn timeout_ignores_non_ring_and_settled_calls() {
    // L1615 times out a ring that "no target participant answers", and an
    // `"active"` call's still-`"pending"` targets. A room call has no
    // targets; a `"pending"` or `"ended"` ring call is neither case.
    for (call_type, state) in [("room", "active"), ("ring", "pending"), ("ring", "ended")] {
        let backend = ring_fixture("active", "pending", "pending");
        seed(
            &backend,
            stored_call("R1", call_type, state, "2026-10-20T12:00:00Z"),
        );
        assert!(!expire(&backend, at(60)).await, "{call_type} {state}");
        assert_eq!(get(&backend, "R1").await["state"], state);
        assert!(call_end_events(&backend).is_empty(), "{call_type} {state}");
    }
}

#[tokio::test]
async fn timeout_of_unknown_call_is_not_found() {
    let backend = backend();
    match expire_ring(
        &backend,
        &BOB.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from("nope"),
        at(30),
    )
    .await
    {
        Err(BackendSetError::SetError(e)) => assert_eq!(e.error_type, SetErrorType::NotFound),
        other => panic!("expected notFound, got {other:?}"),
    }
}

#[tokio::test]
async fn timeout_with_malformed_created_at_names_it() {
    let backend = ring_fixture("ringing", "pending", "pending");
    // RFC 8620 §1.4: a UTCDate is "a Date where the time-offset component
    // MUST be Z"; this one has no date at all.
    seed(&backend, stored_call("R1", "ring", "ringing", "yesterday"));
    match expire_ring(
        &backend,
        &BOB.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from("R1"),
        at(30),
    )
    .await
    {
        Err(BackendSetError::SetError(e)) => {
            assert_eq!(e.error_type, SetErrorType::InvalidProperties);
            assert_eq!(e.properties, Some(vec!["createdAt".to_owned()]));
        }
        other => panic!("expected invalidProperties, got {other:?}"),
    }
    assert_eq!(get(&backend, "R1").await["state"], "ringing");
    assert!(call_end_events(&backend).is_empty());
}

/// [`MemoryBackend`] whose `update_object::<VtcCall>` always fails, so the
/// call-state transition that follows a stored join cannot commit.
struct FailCallUpdate(MemoryBackend);

impl jmap_server::JmapBackend for FailCallUpdate {
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
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        self.0
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        self.0
            .query_objects::<O>(
                caller,
                account_id,
                filter,
                sort,
                limit,
                position,
                anchor,
                anchor_offset,
            )
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        self.0
            .query_changes::<O>(
                caller,
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

impl VtcBackend for FailCallUpdate {
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

    async fn create_call(
        &self,
        caller: &String,
        account_id: &Id,
        call: VtcCall,
        ring_targets: &[String],
    ) -> Result<VtcCall, BackendSetError<MemoryError>> {
        self.0
            .create_call(caller, account_id, call, ring_targets)
            .await
    }

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<MemoryError>> {
        self.0
            .create_object::<O>(caller, account_id, create_id, obj)
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
        if O::TYPE_NAME == VtcCall::TYPE_NAME {
            return Err(BackendSetError::Other(MemoryError::new(
                "injected VTCCall update failure",
            )));
        }
        self.0
            .update_object::<O>(caller, account_id, id, patch)
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

    async fn is_user_blocked(
        &self,
        caller: &String,
        account_id: &Id,
        user_id: &str,
    ) -> Result<bool, MemoryError> {
        self.0.is_user_blocked(caller, account_id, user_id).await
    }

    fn supports_type<O: JmapObject>(&self, caller: &String, account_id: &Id) -> bool {
        self.0.supports_type::<O>(caller, account_id)
    }
}

#[tokio::test]
async fn failed_pending_activation_still_reports_the_committed_join() {
    let backend = FailCallUpdate(backend());
    seed(
        &backend.0,
        stored_call("S1", "scheduled", "pending", "2026-06-05T14:30:00Z"),
    );
    let resp = handle_vtc_participant_set(
        &backend,
        &ALICE.to_owned(),
        json!({
            "accountId": ACCOUNT,
            "create": { "j0": { "callId": "S1", "joinMethod": "webrtc" } }
        }),
    )
    .await
    .expect("VTCParticipant/set")
    .primary;

    // The join is stored before the `"pending"` -> `"active"` transition,
    // which the backend refuses here; the stored join is the create's
    // outcome.
    assert!(resp["notCreated"].is_null(), "resp: {resp}");
    assert!(resp["created"]["j0"]["id"].is_string(), "resp: {resp}");
    // RFC 8620 §5.3 `newState`: "The state string that will now be
    // returned by Foo/get."
    assert_ne!(resp["newState"], resp["oldState"], "resp: {resp}");
    assert_eq!(get(&backend.0, "S1").await["state"], "pending");
}

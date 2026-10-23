//! Chat-side events applied to VTC records: Space and channel destroy, and
//! SpaceBan ejection.

mod common;

use common::*;
use jmap_chat_vtc_server::{
    clear_channel_references, clear_space_references, contact_identity, eject_banned_user,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use jmap_vtc_server::{
    handle_vtc_call_changes, handle_vtc_participant_changes, handle_vtc_participant_get,
};
use serde_json::{Value, json};
use std::collections::HashSet;

const SPACE: &str = "S1";
const CHANNEL: &str = "CH1";

/// A backend whose owner may start calls in [`SPACE`] and [`CHANNEL`].
fn bound_with_space() -> Bound {
    let backend = Bound::new();
    backend
        .chat
        .set_everyone_permissions(HashSet::from([jmap_chat_types::ChatPermission::StartCall]));
    backend.seed_chat_object("Space", space(SPACE, &[], &[(ACCOUNT, &[])]));
    backend.seed_chat_object("Space", space("S2", &[], &[(ACCOUNT, &[])]));
    backend.seed_chat_object("Chat", channel_chat(CHANNEL, SPACE, json!([])));
    backend
}

async fn room_call(backend: &Bound, space_id: &str) -> String {
    let call = create_call(
        backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "spaceId": space_id, "channelId": CHANNEL }),
    )
    .await;
    call["id"].as_str().expect("call id").to_owned()
}

async fn vtc_state(backend: &Bound) -> String {
    let HandlerOutput { primary, .. } = jmap_vtc_server::handle_vtc_call_get(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [] }),
    )
    .await
    .expect("VTCCall/get");
    primary["state"].as_str().expect("state").to_owned()
}

async fn vtc_updated_since(backend: &Bound, since: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_vtc_call_changes(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
    )
    .await
    .expect("VTCCall/changes");
    primary["updated"].clone()
}

/// vtc L421: "When a Space or channel Chat referenced by `spaceId` or
/// `channelId` is destroyed, the server MUST clear the corresponding field
/// to `null` on all VTCCalls that reference it. This is a side effect of
/// the Space or Chat destruction and MUST emit a `StateChange` for the
/// VTCCall type."
#[tokio::test]
async fn space_destroy_clears_space_id_with_state_change() {
    let backend = bound_with_space();
    let bound_call = room_call(&backend, SPACE).await;
    let other_call = room_call(&backend, "S2").await;
    let before = vtc_state(&backend).await;

    clear_space_references(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
    )
    .await
    .expect("clear spaceId");

    assert_eq!(
        get_call(&backend, &bound_call).await["spaceId"],
        Value::Null
    );
    assert_eq!(get_call(&backend, &other_call).await["spaceId"], "S2");
    assert_eq!(
        vtc_updated_since(&backend, &before).await,
        json!([bound_call])
    );
}

/// vtc L421, for `channelId`.
#[tokio::test]
async fn channel_destroy_clears_channel_id_with_state_change() {
    let backend = bound_with_space();
    let call = room_call(&backend, SPACE).await;
    let before = vtc_state(&backend).await;

    clear_channel_references(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(CHANNEL),
    )
    .await
    .expect("clear channelId");

    let stored = get_call(&backend, &call).await;
    assert_eq!(stored["channelId"], Value::Null);
    assert_eq!(
        stored["spaceId"], SPACE,
        "only the destroyed binding clears"
    );
    assert_eq!(vtc_updated_since(&backend, &before).await, json!([call]));
}

/// chat L217: a user's identity is "recorded in the `identity` property
/// of the user's ChatContact"; "The owner has no ChatContact: a
/// client-facing reference to the owner carries the accountId of the
/// owner's account"; "Between servers, the owner's identity is the
/// `ownerUserId` Session field". chat L893: SpaceBan `userId` is "The
/// ChatContact.id of the banned user".
#[tokio::test]
async fn contact_identity_maps_contacts_and_the_owner() {
    let backend = Bound::new();
    backend.seed_chat_object("ChatContact", contact("c-bob", BOB, false));
    let account = Id::from(ACCOUNT);

    let bob = contact_identity(&backend.chat, &(), &account, &Id::from("c-bob"))
        .await
        .expect("contact read");
    assert_eq!(bob.as_deref(), Some(BOB));
    let owner = contact_identity(&backend.chat, &(), &account, &account)
        .await
        .expect("owner read");
    assert_eq!(owner.as_deref(), Some(OWNER));
    let missing = contact_identity(&backend.chat, &(), &account, &Id::from("c-nobody"))
        .await
        .expect("contact read");
    assert_eq!(missing, None);
}

/// A VTCParticipant `id` in call `call_id` for `user_id`, hand-written from
/// the vtc §VTCParticipant field list.
fn participant(id: &str, call_id: &str, user_id: &str, joined_at: Value) -> Value {
    json!({
        "id": id,
        "callId": call_id,
        "userId": user_id,
        "displayName": id,
        "role": "participant",
        "joinedAt": joined_at,
        "leftAt": null,
        "callResponse": null,
        "joinMethod": "webrtc",
        "gatewayData": null,
        "lobbyState": null,
        "mediaState": { "audio": true, "video": false, "screen": false, "raisedHand": false },
        "speakerTimeMs": 0,
        "kickedBy": null,
        "e2eeFingerprint": null,
        "unmuteRequested": false,
        "breakoutCallId": null
    })
}

async fn participant_state(backend: &Bound) -> String {
    let HandlerOutput { primary, .. } = handle_vtc_participant_get(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [] }),
    )
    .await
    .expect("VTCParticipant/get");
    primary["state"].as_str().expect("state").to_owned()
}

async fn left_at(backend: &Bound, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_vtc_participant_get(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCParticipant/get");
    primary["list"][0]["leftAt"].clone()
}

/// vtc L1663 (jmap-chat-spec 6bcd9d8): "When a user is banned from a Space
/// (via SpaceBan creation) and has an active VTCParticipant record in a
/// call bound to that Space (via `spaceId`), the server SHOULD set
/// `leftAt` on that VTCParticipant record and emit a `VTCParticipantEvent`
/// with event `"left"`." Active is vtc L1086: "`joinedAt != null` and
/// `leftAt == null`". The event fields are vtc L1518-1538.
#[tokio::test]
async fn space_ban_ejects_only_active_in_space_records() {
    let backend = bound_with_space();
    let in_space = room_call(&backend, SPACE).await;
    let other_space = room_call(&backend, "S2").await;
    let joined = json!("2026-01-01T00:00:00Z");
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-active", &in_space, BOB, joined.clone()),
    );
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-ringing", &in_space, BOB, Value::Null),
    );
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-elsewhere", &other_space, BOB, joined.clone()),
    );
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-alice", &in_space, ALICE, joined),
    );
    let before = participant_state(&backend).await;

    let events = eject_banned_user(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
        BOB,
    )
    .await
    .expect("eject");

    assert_eq!(
        serde_json::to_value(&events).expect("events serialize"),
        json!([{
            "accountId": ACCOUNT,
            "callId": in_space,
            "participantId": "P-active",
            "event": "left",
            "displayName": "P-active",
            "role": "participant"
        }])
    );
    assert!(left_at(&backend, "P-active").await.is_string());
    for untouched in ["P-ringing", "P-elsewhere", "P-alice"] {
        assert_eq!(
            left_at(&backend, untouched).await,
            Value::Null,
            "{untouched}"
        );
    }
    let HandlerOutput { primary, .. } = handle_vtc_participant_changes(
        &backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "sinceState": before }),
    )
    .await
    .expect("VTCParticipant/changes");
    assert_eq!(primary["updated"], json!(["P-active"]));
}

/// A VTCCall `id` of `call_type` in state `"active"` bound to [`SPACE`],
/// hand-written from the vtc §VTCCall field list.
fn active_space_call(id: &str, call_type: &str) -> Value {
    json!({
        "id": id,
        "callType": call_type,
        "state": "active",
        "createdAt": "2026-01-01T00:00:00Z",
        "endedAt": null,
        "endReason": null,
        "initiatorId": OWNER,
        "subject": null,
        "joinUri": format!("https://vtc.example.com/join/{id}"),
        "joinPassword": null,
        "mediaTypes": ["audio"],
        "activeParticipantCount": 1,
        "parentCallId": null,
        "scheduledStartAt": "2026-01-01T00:00:00Z",
        "gatewayPin": null,
        "spaceId": SPACE,
        "channelId": CHANNEL
    })
}

/// vtc L966 (jmap-chat-spec 6bcd9d8): "When the last active participant
/// leaves a ring or scheduled call, the server transitions the VTCCall to
/// `"ended"` with `endReason: "completed"`". vtc L504, scheduled calls:
/// "`"active"` → `"ended"`: All participants leave". A ban ejection is
/// such a leave (vtc L1663).
#[tokio::test]
async fn space_ban_of_last_participant_ends_scheduled_call() {
    let backend = bound_with_space();
    backend.seed_vtc_object("VTCCall", active_space_call("C-sched", "scheduled"));
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-bob", "C-sched", BOB, json!("2026-01-01T00:00:00Z")),
    );

    eject_banned_user(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
        BOB,
    )
    .await
    .expect("eject");

    let call = get_call(&backend, "C-sched").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "completed");
}

/// vtc L968 (jmap-chat-spec 6bcd9d8): "When the last moderator leaves a
/// call that still has active participants, the server SHOULD grant
/// `"moderator"` to the remaining authenticated participant (non-null
/// `userId`) who joined earliest". vtc L966: "A room call stays
/// `"active"` when it empties", so it stays `"active"` here too.
#[tokio::test]
async fn space_ban_of_last_moderator_hands_on_the_role() {
    let backend = bound_with_space();
    let call = room_call(&backend, SPACE).await;
    let mut bob = participant("P-bob", &call, BOB, json!("2026-01-01T00:00:00Z"));
    bob["role"] = json!("moderator");
    backend.seed_vtc_object("VTCParticipant", bob);
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-alice", &call, ALICE, json!("2026-01-01T00:05:00Z")),
    );
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-owner", &call, OWNER, json!("2026-01-01T00:10:00Z")),
    );

    let events = eject_banned_user(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
        BOB,
    )
    .await
    .expect("eject");

    assert_eq!(events.len(), 1);
    let HandlerOutput { primary, .. } = handle_vtc_participant_get(
        &backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": ["P-alice", "P-owner"] }),
    )
    .await
    .expect("VTCParticipant/get");
    let role = |id: &str| {
        primary["list"]
            .as_array()
            .expect("list")
            .iter()
            .find(|p| p["id"] == id)
            .expect("participant listed")["role"]
            .clone()
    };
    assert_eq!(role("P-alice"), "moderator");
    assert_eq!(role("P-owner"), "participant");
    assert_eq!(get_call(&backend, &call).await["state"], "active");
}

/// vtc L966 (jmap-chat-spec 6c38e2f): "When the last active participant
/// leaves a ring or scheduled call, the server transitions the VTCCall to
/// `"ended"` with `endReason: "completed"`, or `"cancelled"` if the call
/// was still `"ringing"`." Here the banned initiator is the only joined
/// participant of a ringing ring call; the callee never answered.
#[tokio::test]
async fn space_ban_of_ringing_initiator_cancels_ring_call() {
    let backend = bound_with_space();
    let mut call = active_space_call("C-ring", "ring");
    call["state"] = json!("ringing");
    call["initiatorId"] = json!(BOB);
    call["scheduledStartAt"] = Value::Null;
    backend.seed_vtc_object("VTCCall", call);
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-bob", "C-ring", BOB, json!("2026-01-01T00:00:00Z")),
    );
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-alice", "C-ring", ALICE, Value::Null),
    );

    let events = eject_banned_user(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
        BOB,
    )
    .await
    .expect("eject");

    assert_eq!(events.len(), 1);
    let call = get_call(&backend, "C-ring").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "cancelled");
}

/// vtc L506 (jmap-chat-spec 6c38e2f): "For all call types, `"ended"` is a
/// terminal state. The server MUST NOT transition a call out of
/// `"ended"`." vtc L1663 applies the ban to "active calls", so a
/// participant record left open in an ended call bound to the Space is
/// not ejected, and the ban does not fail on it.
#[tokio::test]
async fn space_ban_skips_ended_calls() {
    let backend = bound_with_space();
    let mut call = active_space_call("C-done", "scheduled");
    call["state"] = json!("ended");
    call["endedAt"] = json!("2026-01-01T01:00:00Z");
    call["endReason"] = json!("completed");
    call["activeParticipantCount"] = json!(0);
    backend.seed_vtc_object("VTCCall", call);
    backend.seed_vtc_object(
        "VTCParticipant",
        participant("P-bob", "C-done", BOB, json!("2026-01-01T00:00:00Z")),
    );
    let before = vtc_state(&backend).await;

    let events = eject_banned_user(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
        BOB,
    )
    .await
    .expect("eject");

    assert!(events.is_empty());
    assert_eq!(left_at(&backend, "P-bob").await, Value::Null);
    assert_eq!(vtc_updated_since(&backend, &before).await, json!([]));
    let call = get_call(&backend, "C-done").await;
    assert_eq!(call["state"], "ended");
    assert_eq!(call["endReason"], "completed");
}

//! Block checks use the recipient's account, not the call's hosting
//! account.
//!
//! Oracle: draft-atwood-jmap-vtc-wss-00.md at jmap-chat-spec 6bcd9d8.
//! L296-301: every VTC event "carries an `accountId` (String) property ...
//! the id of the call's hosting account, the account that contains the
//! VTCCall and its VTCParticipant records ({{JMAP-VTC}}). This is usually
//! not the recipient's personal account." L344-348: the server "SHOULD NOT
//! deliver a `VTCRingEvent` or `VTCCallEndEvent` (with `endReason` of
//! `"cancelled"` or `"timeout"`) if the `initiatorId` corresponds to a
//! contact whose `blocked` field is `true` on the recipient's contact
//! list". L357-361: an in-call event from "a participant whose
//! corresponding contact has `blocked` set to `true` on the recipient's
//! contact list" is dropped. Records and events are hand-written JSON.

mod common;

use common::*;
use jmap_chat_vtc_server::should_deliver_vtc;
use jmap_types::Id;
use jmap_vtc_server::VtcEphemeralSubscription;
use jmap_vtc_types::VtcEvent;
use serde_json::{Value, json};

/// The hosting account of call `C1`; [`ACCOUNT`] is the recipient's
/// (the [`OWNER`]'s) personal account.
const HOST: &str = "H1";

/// A stored active ring call `C1` started by [`BOB`], hand-written from
/// the VTCCall field list of draft-atwood-jmap-vtc-00.
fn call() -> Value {
    json!({
        "id": "C1",
        "accountId": HOST,
        "callType": "ring",
        "state": "active",
        "createdAt": "2026-06-05T14:30:00Z",
        "endedAt": null,
        "endReason": null,
        "initiatorId": BOB,
        "subject": null,
        "joinUri": "https://meet.example.com/room/xkz",
        "joinPassword": null,
        "mediaTypes": ["audio"],
        "activeParticipantCount": 3,
        "lobbyEnabled": false,
        "parentCallId": null,
        "breakoutRoomIds": [],
        "scheduledStartAt": null,
        "gatewayPin": null,
        "e2eeEnabled": false
    })
}

/// A current VTCParticipant of `C1`, hand-written from the VTCParticipant
/// field list of draft-atwood-jmap-vtc-00.
fn participant(id: &str, user: &str) -> Value {
    json!({
        "id": id,
        "callId": "C1",
        "userId": user,
        "displayName": user,
        "role": "participant",
        "joinedAt": "2026-06-05T14:30:00Z",
        "leftAt": null,
        "callResponse": null,
        "joinMethod": "webrtc",
        "gatewayData": null,
        "lobbyState": null,
        "mediaState": { "audio": true, "video": false, "screen": false, "raisedHand": false },
        "speakerTimeMs": 0,
        "kickedBy": null,
        "e2eeFingerprint": null,
        "unmuteRequested": false
    })
}

/// Call `C1` in [`HOST`] with [`OWNER`], [`BOB`] and [`ALICE`] in it. The
/// recipient's contact list (in [`ACCOUNT`]) blocks [`BOB`]; the hosting
/// account's contact list blocks [`ALICE`] instead.
fn fixture() -> Bound {
    let backend = Bound::new();
    let host = Id::from(HOST);
    backend.chat.register_account(&host);
    backend.seed_chat_object("ChatContact", contact("c-bob", BOB, true));
    backend.seed_chat_object("ChatContact", contact("c-alice", ALICE, false));
    backend.chat.insert_object_for_test(
        "ChatContact",
        HOST,
        "h-alice",
        contact("h-alice", ALICE, true),
    );
    backend
        .chat
        .insert_object_for_test("ChatContact", HOST, "h-bob", contact("h-bob", BOB, false));
    backend
        .vtc
        .insert_object_for_test("VTCCall", &host, "C1", call());
    for (id, user) in [("P-OWNER", OWNER), ("P-BOB", BOB), ("P-ALICE", ALICE)] {
        backend
            .vtc
            .insert_object_for_test("VTCParticipant", &host, id, participant(id, user));
    }
    backend
}

/// Decode `raw` as delivered over the WebSocket, with the hosting
/// account's `accountId` (L296-301).
fn event(mut raw: Value) -> VtcEvent {
    raw["accountId"] = json!(HOST);
    serde_json::from_value(raw).expect("VTC event fixture")
}

async fn delivers(ev: &VtcEvent) -> bool {
    should_deliver_vtc(
        &fixture(),
        &OWNER.to_owned(),
        &Id::from(HOST),
        &Id::from(ACCOUNT),
        true,
        &VtcEphemeralSubscription::default(),
        ev,
    )
    .await
    .expect("should_deliver_vtc")
}

fn ring_from(initiator: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCRingEvent",
        "callId": "C1",
        "initiatorId": initiator,
        "mediaTypes": ["audio"],
        "joinUri": "https://meet.example.com/room/xkz"
    }))
}

fn media_state(participant_id: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCMediaStateEvent",
        "callId": "C1",
        "participantId": participant_id,
        "mediaState": { "audio": false, "video": false, "screen": false, "raisedHand": true }
    }))
}

#[tokio::test]
async fn ring_uses_recipient_contact_list() {
    assert!(
        !delivers(&ring_from(BOB)).await,
        "BOB is blocked on the recipient's contact list"
    );
    assert!(
        delivers(&ring_from(ALICE)).await,
        "ALICE is blocked only on the hosting account's contact list"
    );
}

/// The call is read from the hosting account to find its initiator,
/// whose block status comes from the recipient's account.
#[tokio::test]
async fn cancelled_call_end_uses_recipient_contact_list() {
    let end = event(json!({
        "@type": "VTCCallEndEvent",
        "callId": "C1",
        "endReason": "cancelled"
    }));
    assert!(
        !delivers(&end).await,
        "initiator BOB is blocked on the recipient's contact list"
    );
}

/// Participation is read from the hosting account; block status from the
/// recipient's account.
#[tokio::test]
async fn in_call_events_use_recipient_contact_list() {
    assert!(
        !delivers(&media_state("P-BOB")).await,
        "BOB is blocked on the recipient's contact list"
    );
    assert!(
        delivers(&media_state("P-ALICE")).await,
        "ALICE is blocked only on the hosting account's contact list"
    );
}

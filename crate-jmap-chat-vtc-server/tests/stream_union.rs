//! The union of the `ChatStreamEnable` `"vtc"` path and `VTCStreamEnable`
//! (`should_deliver_vtc`).
//!
//! Oracle: draft-atwood-jmap-vtc-wss-00.md at jmap-chat-spec 0cd3af0; "wss
//! Lnnn" cites it, "vtc Lnnn" cites draft-atwood-jmap-vtc-00.md. Records
//! and events are hand-written JSON.

use jmap_chat_vtc_server::should_deliver_vtc;
use jmap_types::Id;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::VtcEphemeralSubscription;
use jmap_vtc_types::{VtcCapability, VtcEvent, VtcStreamEnable};
use serde_json::{json, Value};

const ACCOUNT: &str = "A1";
const BOB: &str = "user:bob@example.com";
const ALICE: &str = "user:alice@example.com";

/// A stored active room call, hand-written from the field list at vtc
/// L342-406.
fn call(id: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "callType": "room",
        "state": "active",
        "createdAt": "2026-06-05T14:30:00Z",
        "endedAt": null,
        "endReason": null,
        "initiatorId": BOB,
        "subject": null,
        "joinUri": "https://meet.example.com/room/xkz",
        "joinPassword": null,
        "mediaTypes": ["audio"],
        "activeParticipantCount": 0,
        "lobbyEnabled": false,
        "parentCallId": null,
        "breakoutRoomIds": [],
        "scheduledStartAt": null,
        "gatewayPin": null,
        "e2eeEnabled": false
    })
}

/// A current VTCParticipant, hand-written from the field list at vtc
/// L281-330.
fn participant(id: &str, call_id: &str, user: &str) -> Value {
    json!({
        "id": id,
        "callId": call_id,
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

/// Calls `C1` and `C2` with [`ALICE`] and [`BOB`] in both, and `C3` with
/// only [`BOB`].
fn fixture() -> MemoryBackend {
    let backend = MemoryBackend::new();
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
    .expect("capability fixture");
    let account = Id::from(ACCOUNT);
    backend.register_account(&account, cap);
    for id in ["C1", "C2", "C3"] {
        backend.insert_object_for_test("VTCCall", &account, id, call(id));
    }
    for (id, call_id, user) in [
        ("P-ALICE-1", "C1", ALICE),
        ("P-BOB-1", "C1", BOB),
        ("P-ALICE-2", "C2", ALICE),
        ("P-BOB-2", "C2", BOB),
        ("P-BOB-3", "C3", BOB),
    ] {
        backend.insert_object_for_test(
            "VTCParticipant",
            &account,
            id,
            participant(id, call_id, user),
        );
    }
    backend
}

fn subscribed(enable: Value) -> VtcEphemeralSubscription {
    let enable: VtcStreamEnable = serde_json::from_value(enable).expect("VTCStreamEnable");
    let mut sub = VtcEphemeralSubscription::default();
    sub.replace_with(&enable).expect("valid VTCStreamEnable");
    sub
}

/// A `VTCMediaStateEvent` from [`BOB`]'s participant in `call_id`.
fn media_state(call_id: &str, participant_id: &str) -> VtcEvent {
    serde_json::from_value(json!({
        "@type": "VTCMediaStateEvent",
        "accountId": ACCOUNT,
        "callId": call_id,
        "participantId": participant_id,
        "mediaState": { "audio": false, "video": false, "screen": false, "raisedHand": true }
    }))
    .expect("VTCMediaStateEvent")
}

async fn delivers(chat_vtc: bool, sub: &VtcEphemeralSubscription, ev: &VtcEvent) -> bool {
    should_deliver_vtc(
        &fixture(),
        &ALICE.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(ACCOUNT),
        chat_vtc,
        sub,
        ev,
    )
    .await
    .expect("should_deliver_vtc")
}

/// wss L347-350: "`ChatStreamEnable` with `"vtc"` in `dataTypes`:
/// Subscribes to all VTC event types for all calls in which the user
/// participates. This is the coarse-grained path and requires no
/// additional control messages."
#[tokio::test]
async fn chat_path_alone_covers_every_participating_call() {
    let none = VtcEphemeralSubscription::default();
    assert!(delivers(true, &none, &media_state("C1", "P-BOB-1")).await);
    assert!(delivers(true, &none, &media_state("C2", "P-BOB-2")).await);
    assert!(
        !delivers(true, &none, &media_state("C3", "P-BOB-3")).await,
        "C3 is not a call in which the user participates"
    );
}

/// wss L358-361: "If the client has an active `ChatStreamEnable`
/// subscription that includes `"vtc"` AND an active `VTCStreamEnable`
/// subscription, the server MUST deliver the union of both subscriptions.
/// The server MUST NOT deliver duplicate events; if an event matches both
/// subscriptions, it is delivered exactly once."
#[tokio::test]
async fn union_covers_what_either_path_covers() {
    let only_c1_rings = subscribed(json!({ "callIds": ["C1"], "eventTypes": ["VTCRingEvent"] }));
    let c2 = media_state("C2", "P-BOB-2");
    assert!(
        !delivers(false, &only_c1_rings, &c2).await,
        "VTCStreamEnable alone does not cover C2 media state"
    );
    assert!(
        delivers(true, &only_c1_rings, &c2).await,
        "the chat path covers it, so the union does"
    );

    let only_c1 = subscribed(json!({ "callIds": ["C1"], "eventTypes": null }));
    let c1 = media_state("C1", "P-BOB-1");
    assert!(
        delivers(true, &only_c1, &c1).await,
        "an event matching both paths gets one delivery decision: send"
    );
}

/// wss L365-367: "`VTCStreamDisable` cancels only the VTC-specific
/// subscription established by `VTCStreamEnable`. It does not affect VTC
/// events delivered through `ChatStreamEnable`."
#[tokio::test]
async fn vtc_stream_disable_leaves_chat_path() {
    let disabled = VtcEphemeralSubscription::default();
    assert!(delivers(true, &disabled, &media_state("C1", "P-BOB-1")).await);
}

/// wss L369-372: "`ChatStreamDisable` cancels only the Chat ephemeral
/// subscription. If `"vtc"` was included in the `ChatStreamEnable`
/// `dataTypes`, VTC events delivered through that path stop; events
/// delivered through an active `VTCStreamEnable` subscription are
/// unaffected."
#[tokio::test]
async fn chat_stream_disable_leaves_vtc_path() {
    let only_c1 = subscribed(json!({ "callIds": ["C1"] }));
    assert!(delivers(false, &only_c1, &media_state("C1", "P-BOB-1")).await);
    assert!(
        !delivers(false, &only_c1, &media_state("C2", "P-BOB-2")).await,
        "C2 came only through the chat path, which stopped"
    );
    let none = VtcEphemeralSubscription::default();
    assert!(!delivers(false, &none, &media_state("C1", "P-BOB-1")).await);
}

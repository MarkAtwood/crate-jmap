//! Delivery-time checks for VTC ephemeral events (`should_deliver`).
//!
//! Oracle: draft-atwood-jmap-vtc-wss-00.md at jmap-chat-spec 3227c0f;
//! "wss Lnnn" cites it, "vtc Lnnn" cites draft-atwood-jmap-vtc-00.md.
//! Events and records are hand-written JSON.

#[macro_use]
mod common;

use common::*;
use jmap_types::Id;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{should_deliver, VtcEphemeralSubscription};
use jmap_vtc_types::{VtcEvent, VtcStreamEnable};
use serde_json::{json, Value};

/// A VTCParticipant record in call `call_id`, hand-written from the field
/// list at vtc L281-330.
fn participant(id: &str, call_id: &str, user: &str, role: &str, joined: bool, left: bool) -> Value {
    json!({
        "id": id,
        "callId": call_id,
        "userId": user,
        "displayName": user,
        "role": role,
        "joinedAt": if joined { json!("2026-06-05T14:30:00Z") } else { Value::Null },
        "leftAt": if left { json!("2026-06-05T14:40:00Z") } else { Value::Null },
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

fn seed_participant(backend: &MemoryBackend, record: Value) {
    let id = record["id"].as_str().expect("id").to_owned();
    backend.insert_object_for_test("VTCParticipant", &Id::from(ACCOUNT), &id, record);
}

/// Active room call `C1` (initiator [`BOB`]) with [`BOB`] as moderator and
/// [`ALICE`] and [`EVE`] as current participants, in an account that
/// advertises Chat, so the contact-list block rules apply.
fn fixture(e2ee: bool) -> MemoryBackend {
    let backend = backend();
    backend.advertise(&Id::from(ACCOUNT), CHAT_URI);
    let mut call = stored_call("C1", "room", "active", "2026-06-05T14:30:00Z");
    call["e2eeEnabled"] = json!(e2ee);
    seed(&backend, call);
    seed_participant(
        &backend,
        participant("P-BOB", "C1", BOB, "moderator", true, false),
    );
    seed_participant(
        &backend,
        participant("P-ALICE", "C1", ALICE, "participant", true, false),
    );
    seed_participant(
        &backend,
        participant("P-EVE", "C1", EVE, "participant", true, false),
    );
    backend
}

fn subscribed(enable: Value) -> VtcEphemeralSubscription {
    let enable: VtcStreamEnable = serde_json::from_value(enable).unwrap();
    let mut sub = VtcEphemeralSubscription::default();
    sub.replace_with(&enable).unwrap();
    sub
}

fn all_events() -> VtcEphemeralSubscription {
    subscribed(json!({ "callIds": null, "eventTypes": null }))
}

/// Decode `raw` as delivered over the WebSocket, which adds the hosting
/// account's `accountId` to every VTC event (draft-atwood-jmap-vtc-wss-00
/// L296-303).
fn event(mut raw: Value) -> VtcEvent {
    raw["accountId"] = json!(ACCOUNT);
    serde_json::from_value(raw).unwrap()
}

async fn delivers(
    backend: &MemoryBackend,
    recipient: &str,
    sub: &VtcEphemeralSubscription,
    ev: &VtcEvent,
) -> bool {
    let account = Id::from(ACCOUNT);
    should_deliver(backend, &recipient.to_owned(), &account, &account, sub, ev)
        .await
        .expect("should_deliver")
}

fn ring() -> VtcEvent {
    event(json!({
        "@type": "VTCRingEvent",
        "callId": "C1",
        "initiatorId": BOB,
        "mediaTypes": ["audio"],
        "joinUri": "https://meet.example.com/room/xkz"
    }))
}

fn call_end(reason: &str) -> VtcEvent {
    event(json!({ "@type": "VTCCallEndEvent", "callId": "C1", "endReason": reason }))
}

fn participant_event(participant_id: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCParticipantEvent",
        "callId": "C1",
        "participantId": participant_id,
        "event": "joined",
        "displayName": "Eve",
        "role": "participant"
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

fn active_speaker(participant_id: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCActiveSpeakerEvent",
        "callId": "C1",
        "participantId": participant_id
    }))
}

fn unmute_request(requested_by: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCUnmuteRequestEvent",
        "callId": "C1",
        "participantId": "P-ALICE",
        "requestedBy": requested_by
    }))
}

fn recording_state(initiated_by: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCRecordingStateEvent",
        "callId": "C1",
        "recordingId": "R1",
        "state": "recording",
        "initiatedBy": initiated_by
    }))
}

fn gateway_signal() -> VtcEvent {
    event(json!({
        "@type": "VTCGatewaySignal",
        "callId": "C1",
        "participantId": "P-EVE",
        "protocol": "sip",
        "signal": "dtmf",
        "data": { "digits": "1" },
        "direction": "inbound",
        "timestamp": "2026-06-05T14:31:00Z"
    }))
}

/// wss L196-197: `VTCStreamDisable` cancels "all ephemeral VTC event
/// delivery"; a connection that never enabled has nothing to deliver.
#[tokio::test]
async fn nothing_delivered_without_subscription() {
    let backend = fixture(false);
    let none = VtcEphemeralSubscription::default();
    assert!(!delivers(&backend, ALICE, &none, &ring()).await);
    assert!(!delivers(&backend, ALICE, &none, &participant_event("P-EVE")).await);
}

/// wss L147-149: `eventTypes` is "An explicit list of event type names
/// the client wishes to receive".
#[tokio::test]
async fn event_types_filter_applies() {
    let backend = fixture(false);
    let sub = subscribed(json!({ "eventTypes": ["VTCParticipantEvent"] }));
    assert!(delivers(&backend, ALICE, &sub, &participant_event("P-EVE")).await);
    assert!(!delivers(&backend, ALICE, &sub, &media_state("P-EVE")).await);
    assert!(!delivers(&backend, ALICE, &sub, &ring()).await);
}

/// wss L168-170 (jmap-chat-spec 6bcd9d8): "`VTCRecordingStateEvent` is
/// the recording consent signal ({{JMAP-VTC}}), so the server delivers it
/// for every call the subscription covers whether or not `eventTypes`
/// lists it." The `callIds` and current-participant checks still apply.
#[tokio::test]
async fn recording_state_ignores_event_types() {
    let backend = fixture(false);
    let sub = subscribed(json!({ "eventTypes": ["VTCParticipantEvent"] }));
    assert!(delivers(&backend, ALICE, &sub, &recording_state(BOB)).await);
    let other_call = subscribed(json!({
        "callIds": ["C2"],
        "eventTypes": ["VTCParticipantEvent"]
    }));
    assert!(!delivers(&backend, ALICE, &other_call, &recording_state(BOB)).await);
    let carol = "user:carol@example.com";
    assert!(!delivers(&backend, carol, &sub, &recording_state(BOB)).await);
}

/// wss L252-256: "Per-user events (`VTCRingEvent`, `VTCCallEndEvent`) are
/// delivered based on the authenticated user's identity and are not
/// scoped to `callIds` in the subscription. A `VTCStreamEnable` with any
/// non-empty subscription — regardless of `callIds` — implicitly enables
/// delivery of per-user events."
#[tokio::test]
async fn per_user_events_ignore_call_ids() {
    let backend = fixture(false);
    let other_call = subscribed(json!({ "callIds": ["C2"] }));
    let no_call = subscribed(json!({ "callIds": [] }));
    for sub in [&other_call, &no_call] {
        // Not scoped to callIds, and not to participation: a ring target
        // has not joined.
        assert!(delivers(&backend, "user:carol@example.com", sub, &ring()).await);
        assert!(delivers(&backend, ALICE, sub, &call_end("cancelled")).await);
    }
}

/// wss L258-260: "Per-call events are delivered only for calls that match
/// the active subscription's `callIds` filter (or all calls if `callIds`
/// is `null`)".
#[tokio::test]
async fn per_call_events_follow_call_ids() {
    let backend = fixture(false);
    let ev = participant_event("P-EVE");
    assert!(delivers(&backend, ALICE, &all_events(), &ev).await);
    assert!(
        delivers(
            &backend,
            ALICE,
            &subscribed(json!({ "callIds": ["C1"] })),
            &ev
        )
        .await
    );
    assert!(
        !delivers(
            &backend,
            ALICE,
            &subscribed(json!({ "callIds": ["C2"] })),
            &ev
        )
        .await
    );
    assert!(!delivers(&backend, ALICE, &subscribed(json!({ "callIds": [] })), &ev).await);
}

/// wss L548-551: per-call events "MUST only be delivered for calls in
/// which the authenticated user is a current participant at delivery
/// time"; vtc L1019 defines active as "`joinedAt != null` and `leftAt ==
/// null`".
#[tokio::test]
async fn per_call_events_need_current_participation() {
    let backend = fixture(false);
    let sub = all_events();
    let events = [
        participant_event("P-EVE"),
        media_state("P-EVE"),
        active_speaker("P-EVE"),
        unmute_request(BOB),
        recording_state(BOB),
        gateway_signal(),
    ];
    let carol = "user:carol@example.com";
    for ev in &events {
        assert!(!delivers(&backend, carol, &sub, ev).await, "{ev:?}");
    }
    // Invited (ring target) but not joined.
    seed_participant(
        &backend,
        participant("P-CAROL", "C1", carol, "participant", false, false),
    );
    for ev in &events {
        assert!(!delivers(&backend, carol, &sub, ev).await, "{ev:?}");
    }
    // A record in another call does not count.
    seed_participant(
        &backend,
        participant("P-CAROL", "C2", carol, "participant", true, false),
    );
    for ev in &events {
        assert!(!delivers(&backend, carol, &sub, ev).await, "{ev:?}");
    }
}

/// wss L605-608: "Servers SHOULD verify authorization at the time of each
/// ephemeral event delivery, not only at the time `VTCStreamEnable` is
/// received, since participation, permissions, and block status can
/// change while the connection is open".
#[tokio::test]
async fn participation_rechecked_per_delivery() {
    let backend = fixture(false);
    let sub = subscribed(json!({ "callIds": ["C1"] }));
    let ev = participant_event("P-EVE");
    assert!(delivers(&backend, ALICE, &sub, &ev).await);
    seed_participant(
        &backend,
        participant("P-ALICE", "C1", ALICE, "participant", true, true),
    );
    assert!(!delivers(&backend, ALICE, &sub, &ev).await);
    seed_participant(
        &backend,
        participant("P-ALICE", "C1", ALICE, "participant", true, false),
    );
    assert!(delivers(&backend, ALICE, &sub, &ev).await);
}

/// wss L290-293: "The server MUST NOT deliver a `VTCRingEvent` or
/// `VTCCallEndEvent` (with `endReason` of `"cancelled"` or `"timeout"`) if
/// the `initiatorId` corresponds to a contact whose `blocked` field is
/// `true` on the recipient's contact list"; wss L553-555 applies it "at
/// delivery time".
#[tokio::test]
async fn blocked_initiator_suppresses_ring_and_ring_end() {
    let backend = fixture(false);
    let sub = all_events();
    assert!(delivers(&backend, ALICE, &sub, &ring()).await);
    backend.block(ALICE, BOB);
    assert!(!delivers(&backend, ALICE, &sub, &ring()).await);
    assert!(!delivers(&backend, ALICE, &sub, &call_end("cancelled")).await);
    assert!(!delivers(&backend, ALICE, &sub, &call_end("timeout")).await);
    // The rule names only those two endReason values.
    assert!(delivers(&backend, ALICE, &sub, &call_end("answered_elsewhere")).await);
    // The block list is the recipient's: EVE has not blocked BOB.
    assert!(delivers(&backend, EVE, &sub, &ring()).await);
}

/// wss L357-376: "If a per-call event originates from a participant whose
/// corresponding contact has `blocked` set to `true` on the recipient's
/// contact list ..., the server SHOULD silently drop the event for that
/// recipient", for `VTCParticipantEvent`, `VTCMediaStateEvent`,
/// `VTCActiveSpeakerEvent` ("naming a blocked participant") and
/// `VTCUnmuteRequestEvent` ("sent by a blocked participant acting as
/// moderator").
#[tokio::test]
async fn blocked_participant_suppresses_in_call_events() {
    let backend = fixture(false);
    let sub = all_events();
    backend.block(ALICE, EVE);
    assert!(!delivers(&backend, ALICE, &sub, &participant_event("P-EVE")).await);
    assert!(!delivers(&backend, ALICE, &sub, &media_state("P-EVE")).await);
    assert!(!delivers(&backend, ALICE, &sub, &active_speaker("P-EVE")).await);
    assert!(!delivers(&backend, ALICE, &sub, &unmute_request(EVE)).await);
    // Events from an unblocked participant still flow.
    assert!(delivers(&backend, ALICE, &sub, &participant_event("P-BOB")).await);
    assert!(delivers(&backend, ALICE, &sub, &media_state("P-BOB")).await);
    assert!(delivers(&backend, ALICE, &sub, &active_speaker("P-BOB")).await);
    assert!(delivers(&backend, ALICE, &sub, &unmute_request(BOB)).await);
    // Only the blocking recipient is affected.
    assert!(delivers(&backend, BOB, &sub, &participant_event("P-EVE")).await);
}

/// wss at 29e9a41 L344-348: the contact-list block applies "when
/// {{JMAP-CHAT}} is present". The recipient's account here does not
/// advertise `urn:ietf:params:jmap:chat`, so a block recorded for the
/// recipient suppresses nothing (bd:JMAP-skk9.4: the Scene rule, wss
/// L826-830 of draft-atwood-jmap-scene-wss-00, for every extension).
#[tokio::test]
async fn blocks_need_chat_on_the_recipient_account() {
    let backend = backend();
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    seed_participant(
        &backend,
        participant("P-ALICE", "C1", ALICE, "participant", true, false),
    );
    seed_participant(
        &backend,
        participant("P-EVE", "C1", EVE, "participant", true, false),
    );
    backend.block(ALICE, BOB);
    backend.block(ALICE, EVE);
    let sub = all_events();
    assert!(delivers(&backend, ALICE, &sub, &ring()).await);
    assert!(delivers(&backend, ALICE, &sub, &call_end("cancelled")).await);
    assert!(delivers(&backend, ALICE, &sub, &participant_event("P-EVE")).await);
}

/// wss L378-382: "This rule does not apply to `VTCRecordingStateEvent`,
/// the consent signal that {{JMAP-VTC}} delivers to all participants
/// regardless of block status. It also does not apply to
/// `VTCGatewaySignal`, which is already restricted to moderators".
#[tokio::test]
async fn recording_state_and_gateway_signal_ignore_blocks() {
    let backend = fixture(false);
    let sub = all_events();
    backend.block(ALICE, EVE);
    backend.block(BOB, EVE);
    assert!(delivers(&backend, ALICE, &sub, &recording_state(EVE)).await);
    // The signal's participantId is EVE's, whom BOB blocked.
    assert!(delivers(&backend, BOB, &sub, &gateway_signal()).await);
}

/// wss L557-559: "`VTCGatewaySignal` MUST only be delivered to
/// participants with `moderator` role unless the deployment explicitly
/// permits broader delivery."
#[tokio::test]
async fn gateway_signal_only_to_moderators() {
    let backend = fixture(false);
    let sub = all_events();
    assert!(delivers(&backend, BOB, &sub, &gateway_signal()).await);
    assert!(!delivers(&backend, ALICE, &sub, &gateway_signal()).await);
    seed_participant(
        &backend,
        participant("P-ALICE", "C1", ALICE, "moderator", true, false),
    );
    assert!(delivers(&backend, ALICE, &sub, &gateway_signal()).await);
}

/// wss L589-591: "For calls with `e2eeEnabled: true`, the server SHOULD
/// NOT map media streams to participant ids and SHOULD NOT deliver
/// `VTCActiveSpeakerEvent`".
#[tokio::test]
async fn no_active_speaker_on_e2ee_calls() {
    let sub = all_events();
    let plain = fixture(false);
    assert!(delivers(&plain, ALICE, &sub, &active_speaker("P-BOB")).await);
    let e2ee = fixture(true);
    assert!(!delivers(&e2ee, ALICE, &sub, &active_speaker("P-BOB")).await);
    // Only that event: "The server generates or relays every
    // `VTCParticipantEvent`" (wss L583-584).
    assert!(delivers(&e2ee, ALICE, &sub, &participant_event("P-BOB")).await);
}

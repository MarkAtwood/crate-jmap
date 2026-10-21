//! Round-trips the JSON examples of draft-atwood-jmap-vtc-00 that are
//! instances of this crate's types, verbatim. Each constant is copied
//! byte-for-byte from the draft at jmap-chat-spec 3227c0f; the line numbers
//! cite that revision. The oracle is the draft text, never the code under
//! test: each example is parsed as plain JSON, parsed as the typed value,
//! and the typed value must serialize back to the same JSON (null and
//! absent kept distinct).
//!
//! The `/set` argument examples (L593-L1197, L1430-L1443) are method
//! request and response fragments, not instances of these types; the
//! method beads test them.

use jmap_vtc_types::{
    CallState, CallType, JoinMethod, MediaKind, RingEndReason, SignalDirection, VtcCall,
    VtcCallPush, VtcEvent, VtcParticipant,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Parse `json` as `T`, assert it re-serializes to the same JSON value,
/// and return it.
fn round_trip<T: DeserializeOwned + Serialize>(json: &Value) -> T {
    let typed: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(&serde_json::to_value(&typed).unwrap(), json);
    typed
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// VTCCall/get example response object, "properties abridged" (draft-atwood-jmap-vtc-00 L530-L560).
const VTC_CALL_GET: &str = r#"{
  "id": "01J4XKZQN4MWVT8PPBEHTJ3AB",
  "callType": "ring",
  "state": "active",
  "createdAt": "2026-06-05T14:30:00Z",
  "endedAt": null,
  "endReason": null,
  "initiatorId": "user:bob@example.com",
  "subject": "Quick sync",
  "joinUri": "https://meet.example.com/room/xkz",
  "joinPassword": null,
  "mediaTypes": ["audio", "video"],
  "activeParticipantCount": 2,
  "lobbyEnabled": false,
  "policy": {
    "muteOnEntry": false,
    "videoOffOnEntry": false,
    "participantsCanUnmute": true,
    "participantsCanShareScreen": true,
    "participantsCanStartVideo": true
  },
  "parentCallId": null,
  "breakoutRoomIds": [],
  "scheduledStartAt": null,
  "gatewayPin": null,
  "e2eeEnabled": false,
  "chatId": "01J4XL2RN5NXWS9QQCFIUJ4BC",
  "spaceId": null,
  "channelId": null,
  "calendarEventId": null
}"#;

/// VTCParticipant/set `created` response: "the complete VTCParticipant record"
/// (draft-atwood-jmap-vtc-00 L896-L923 at jmap-chat-spec 6bcd9d8, which adds
/// `breakoutCallId`).
const VTC_PARTICIPANT_CREATED: &str = r#"{
  "created": {
    "p0": {
      "id": "01J4XMVQK7NRYS9PPCEHTK4CD0",
      "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
      "userId": "user:alice@example.com",
      "displayName": "Alice Chen",
      "role": "participant",
      "joinedAt": "2026-06-05T14:35:22Z",
      "leftAt": null,
      "callResponse": null,
      "joinMethod": "webrtc",
      "gatewayData": null,
      "lobbyState": null,
      "mediaState": {
        "audio": true,
        "video": true,
        "screen": false,
        "raisedHand": false
      },
      "speakerTimeMs": 0,
      "kickedBy": null,
      "e2eeFingerprint": null,
      "unmuteRequested": false,
      "breakoutCallId": null
    }
  }
}"#;

/// Example `VTCCallPush` payload (draft-atwood-jmap-vtc-00 L1279-L1291).
const VTC_CALL_PUSH_FULL: &str = r#"{
  "@type": "VTCCallPush",
  "accountId": "alice-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
  "callType": "ring",
  "initiatorId": "user:bob@example.com",
  "initiatorDisplayName": "Bob Martinez",
  "subject": "Quick sync",
  "mediaTypes": ["audio", "video"],
  "joinUri": "https://meet.example.com/room/xkz",
  "chatId": null,
  "spaceId": null
}"#;

/// Example content-free `VTCCallPush` payload (draft-atwood-jmap-vtc-00 L1307-L1312).
const VTC_CALL_PUSH_CONTENT_FREE: &str = r#"{
  "@type": "VTCCallPush",
  "accountId": "alice-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
  "callType": "ring"
}"#;

/// VTCCallEndEvent example: call answered on another device (draft-atwood-jmap-vtc-00 L1368-L1372),
/// plus the `accountId` that WebSocket delivery adds to every VTC event
/// (draft-atwood-jmap-vtc-wss-00 L296-303 at jmap-chat-spec 6bcd9d8).
const VTC_CALL_END_EVENT: &str = r#"{
  "@type": "VTCCallEndEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
  "endReason": "answered_elsewhere"
}"#;

/// VTCGatewaySignal example: inbound DTMF from a PSTN participant (draft-atwood-jmap-vtc-00 L1416-L1425),
/// plus the WebSocket `accountId` (draft-atwood-jmap-vtc-wss-00 L296-303 at
/// jmap-chat-spec 6bcd9d8).
const VTC_GATEWAY_SIGNAL: &str = r#"{
  "@type": "VTCGatewaySignal",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
  "participantId": "gw-pstn-14155559876",
  "protocol": "pstn",
  "signal": "dtmf",
  "data": {"digit": "5", "duration": 160},
  "direction": "inbound",
  "timestamp": "2026-06-05T14:42:17Z"
}"#;

#[test]
fn vtc_call_get_example() {
    let call: VtcCall = round_trip(&parse(VTC_CALL_GET));
    assert_eq!(call.call_type, CallType::Ring);
    assert_eq!(call.state, CallState::Active);
    assert_eq!(call.media_types, [MediaKind::Audio, MediaKind::Video]);
    assert_eq!(call.account_id, None);
    assert_eq!(call.chat_id, Some(Some("01J4XL2RN5NXWS9QQCFIUJ4BC".into())));
    assert_eq!(call.space_id, Some(None));
}

#[test]
fn vtc_participant_created_example() {
    let created = parse(VTC_PARTICIPANT_CREATED);
    let p: VtcParticipant = round_trip(&created["created"]["p0"]);
    assert_eq!(p.join_method, JoinMethod::Webrtc);
    assert_eq!(p.user_id.as_deref(), Some("user:alice@example.com"));
    assert!(p.media_state.audio);
}

#[test]
fn vtc_call_push_full_example() {
    let push: VtcCallPush = round_trip(&parse(VTC_CALL_PUSH_FULL));
    assert_eq!(push.initiator_id.as_deref(), Some("user:bob@example.com"));
    assert_eq!(push.subject, Some(Some("Quick sync".to_owned())));
    assert_eq!(push.chat_id, Some(None));
    // The example omits `channelId`; it must stay absent.
    assert_eq!(push.channel_id, None);
}

#[test]
fn vtc_call_push_content_free_example() {
    let push: VtcCallPush = round_trip(&parse(VTC_CALL_PUSH_CONTENT_FREE));
    assert_eq!(push.call_type, CallType::Ring);
    assert!(push.initiator_id.is_none());
    assert!(push.media_types.is_none());
    assert!(push.subject.is_none());
}

#[test]
fn vtc_call_end_event_example() {
    let ev: VtcEvent = round_trip(&parse(VTC_CALL_END_EVENT));
    let VtcEvent::CallEnd(e) = ev else {
        panic!("wrong variant: {ev:?}")
    };
    assert_eq!(e.end_reason, RingEndReason::AnsweredElsewhere);
}

#[test]
fn vtc_gateway_signal_example() {
    let ev: VtcEvent = round_trip(&parse(VTC_GATEWAY_SIGNAL));
    let VtcEvent::GatewaySignal(s) = ev else {
        panic!("wrong variant: {ev:?}")
    };
    assert_eq!(s.signal, "dtmf");
    assert_eq!(s.direction, SignalDirection::Inbound);
    assert_eq!(s.data["digit"], Value::from("5"));
}

/// VTCParticipantEvent example: participant joins (draft-atwood-jmap-vtc-wss-00
/// L501-511 at jmap-chat-spec 6bcd9d8).
const WSS_PARTICIPANT_EVENT: &str = r#"{
  "@type": "VTCParticipantEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "participantId": "01J4XMVQK7NRYS9PPCEHTK4CD0",
  "event": "joined",
  "displayName": "Alice Chen",
  "role": "participant"
}"#;

/// VTCMediaStateEvent example: participant mutes (draft-atwood-jmap-vtc-wss-00
/// L517-530 at jmap-chat-spec 6bcd9d8).
const WSS_MEDIA_STATE_EVENT: &str = r#"{
  "@type": "VTCMediaStateEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "participantId": "01J4XMVQK7NRYS9PPCEHTK4CD0",
  "mediaState": {
    "audio": false,
    "video": true,
    "screen": false,
    "raisedHand": false
  }
}"#;

/// VTCActiveSpeakerEvent example (draft-atwood-jmap-vtc-wss-00 L536-543 at
/// jmap-chat-spec 6bcd9d8).
const WSS_ACTIVE_SPEAKER_EVENT: &str = r#"{
  "@type": "VTCActiveSpeakerEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "participantId": "01J4XNZRM8QWVT0RRDFJVS5EF0"
}"#;

/// VTCRecordingStateEvent example: recording starts
/// (draft-atwood-jmap-vtc-wss-00 L549-558 at jmap-chat-spec 6bcd9d8).
const WSS_RECORDING_STATE_EVENT: &str = r#"{
  "@type": "VTCRecordingStateEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "recordingId": "01J4XP2SN9RXVV1SSEGJVT6GH0",
  "state": "recording",
  "initiatedBy": "user:bob@example.com"
}"#;

/// draft-atwood-jmap-vtc-wss-00 L296-303: over the WebSocket every VTC
/// event "carries an `accountId` (String) property ... the id of the
/// call's hosting account". Each WSS example round-trips verbatim and
/// exposes that account.
#[test]
fn wss_examples_carry_hosting_account() {
    for text in [
        WSS_PARTICIPANT_EVENT,
        WSS_MEDIA_STATE_EVENT,
        WSS_ACTIVE_SPEAKER_EVENT,
        WSS_RECORDING_STATE_EVENT,
    ] {
        let account = match round_trip::<VtcEvent>(&parse(text)) {
            VtcEvent::Participant(e) => e.account_id,
            VtcEvent::MediaState(e) => e.account_id,
            VtcEvent::ActiveSpeaker(e) => e.account_id,
            VtcEvent::RecordingState(e) => e.account_id,
            other => panic!("wrong variant: {other:?}"),
        };
        assert_eq!(account, "bob-account", "{text}");
    }
}

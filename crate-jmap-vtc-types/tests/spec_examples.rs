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

mod common;

use jmap_vtc_types::{
    CallState, CallType, JoinMethod, MediaKind, RingEndReason, SignalDirection, VtcCall,
    VtcCallPush, VtcEvent, VtcParticipant,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
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
fn vtc_call_get() -> String {
    common::fixture("vtc_call_get.json")
}

/// VTCParticipant/set `created` response: "the complete VTCParticipant record"
/// (draft-atwood-jmap-vtc-00 L896-L923 at jmap-chat-spec 6bcd9d8, which adds
/// `breakoutCallId`).
fn vtc_participant_created() -> String {
    common::fixture("vtc_participant_created.json")
}

/// Example `VTCCallPush` payload (draft-atwood-jmap-vtc-00 L1279-L1291).
fn vtc_call_push_full() -> String {
    common::fixture("vtc_call_push_full.json")
}

/// Example content-free `VTCCallPush` payload (draft-atwood-jmap-vtc-00 L1307-L1312).
fn vtc_call_push_content_free() -> String {
    common::fixture("vtc_call_push_content_free.json")
}

/// VTCCallEndEvent example: call answered on another device (draft-atwood-jmap-vtc-00 L1368-L1372),
/// plus the `accountId` that WebSocket delivery adds to every VTC event
/// (draft-atwood-jmap-vtc-wss-00 L296-303 at jmap-chat-spec 6bcd9d8).
fn vtc_call_end_event() -> String {
    common::fixture("vtc_call_end_event.json")
}

/// VTCGatewaySignal example: inbound DTMF from a PSTN participant (draft-atwood-jmap-vtc-00 L1416-L1425),
/// plus the WebSocket `accountId` (draft-atwood-jmap-vtc-wss-00 L296-303 at
/// jmap-chat-spec 6bcd9d8).
fn vtc_gateway_signal() -> String {
    common::fixture("vtc_gateway_signal.json")
}

#[test]
fn vtc_call_get_example() {
    let call: VtcCall = round_trip(&parse(&vtc_call_get()));
    assert_eq!(call.call_type, CallType::Ring);
    assert_eq!(call.state, CallState::Active);
    assert_eq!(call.media_types, [MediaKind::Audio, MediaKind::Video]);
    assert_eq!(call.account_id, None);
    assert_eq!(call.chat_id, Some(Some("01J4XL2RN5NXWS9QQCFIUJ4BC".into())));
    assert_eq!(call.space_id, Some(None));
}

#[test]
fn vtc_participant_created_example() {
    let created = parse(&vtc_participant_created());
    let p: VtcParticipant = round_trip(&created["created"]["p0"]);
    assert_eq!(p.join_method, JoinMethod::Webrtc);
    assert_eq!(p.user_id.as_deref(), Some("user:alice@example.com"));
    assert!(p.media_state.audio);
}

#[test]
fn vtc_call_push_full_example() {
    let push: VtcCallPush = round_trip(&parse(&vtc_call_push_full()));
    assert_eq!(push.initiator_id.as_deref(), Some("user:bob@example.com"));
    assert_eq!(push.subject, Some(Some("Quick sync".to_owned())));
    assert_eq!(push.chat_id, Some(None));
    // The example omits `channelId`; it must stay absent.
    assert_eq!(push.channel_id, None);
}

#[test]
fn vtc_call_push_content_free_example() {
    let push: VtcCallPush = round_trip(&parse(&vtc_call_push_content_free()));
    assert_eq!(push.call_type, CallType::Ring);
    assert!(push.initiator_id.is_none());
    assert!(push.media_types.is_none());
    assert!(push.subject.is_none());
}

#[test]
fn vtc_call_end_event_example() {
    let ev: VtcEvent = round_trip(&parse(&vtc_call_end_event()));
    let VtcEvent::CallEnd(e) = ev else {
        panic!("wrong variant: {ev:?}")
    };
    assert_eq!(e.end_reason, RingEndReason::AnsweredElsewhere);
}

#[test]
fn vtc_gateway_signal_example() {
    let ev: VtcEvent = round_trip(&parse(&vtc_gateway_signal()));
    let VtcEvent::GatewaySignal(s) = ev else {
        panic!("wrong variant: {ev:?}")
    };
    assert_eq!(s.signal, "dtmf");
    assert_eq!(s.direction, SignalDirection::Inbound);
    assert_eq!(s.data["digit"], Value::from("5"));
}

/// VTCParticipantEvent example: participant joins (draft-atwood-jmap-vtc-wss-00
/// L501-511 at jmap-chat-spec 6bcd9d8).
fn wss_participant_event() -> String {
    common::fixture("wss_participant_event.json")
}

/// VTCMediaStateEvent example: participant mutes (draft-atwood-jmap-vtc-wss-00
/// L517-530 at jmap-chat-spec 6bcd9d8).
fn wss_media_state_event() -> String {
    common::fixture("wss_media_state_event.json")
}

/// VTCActiveSpeakerEvent example (draft-atwood-jmap-vtc-wss-00 L536-543 at
/// jmap-chat-spec 6bcd9d8).
fn wss_active_speaker_event() -> String {
    common::fixture("wss_active_speaker_event.json")
}

/// VTCRecordingStateEvent example: recording starts
/// (draft-atwood-jmap-vtc-wss-00 L549-558 at jmap-chat-spec 6bcd9d8).
fn wss_recording_state_event() -> String {
    common::fixture("wss_recording_state_event.json")
}

/// draft-atwood-jmap-vtc-wss-00 L296-303: over the WebSocket every VTC
/// event "carries an `accountId` (String) property ... the id of the
/// call's hosting account". Each WSS example round-trips verbatim and
/// exposes that account.
#[test]
fn wss_examples_carry_hosting_account() {
    for text in [
        &wss_participant_event(),
        &wss_media_state_event(),
        &wss_active_speaker_event(),
        &wss_recording_state_event(),
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

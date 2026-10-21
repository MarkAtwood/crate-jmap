//! `VTCStreamEnable` validation and replace semantics.
//!
//! Oracle: draft-atwood-jmap-vtc-wss-00 §VTCStreamEnable, Semantics,
//! L168-183. Frames are hand-written JSON.

use jmap_types::Id;
use jmap_vtc_server::{CallScope, VtcEphemeralSubscription, VtcStreamFilter};
use jmap_vtc_types::VtcStreamEnable;
use serde_json::json;

fn enable(raw: serde_json::Value) -> VtcStreamEnable {
    serde_json::from_value(raw).unwrap()
}

fn enabled(raw: serde_json::Value) -> VtcEphemeralSubscription {
    let mut sub = VtcEphemeralSubscription::default();
    sub.replace_with(&enable(raw)).unwrap();
    sub
}

fn filter(sub: &VtcEphemeralSubscription) -> &VtcStreamFilter {
    sub.filter().expect("subscription is active")
}

fn id(s: &str) -> Id {
    Id::from(s)
}

/// L168-170: "A subsequent `VTCStreamEnable` message replaces the previous
/// ephemeral subscription in its entirety. There is no partial-update
/// mechanism".
#[test]
fn subsequent_enable_replaces_in_entirety() {
    let mut sub = enabled(json!({
        "callIds": ["CALL-A"],
        "eventTypes": ["VTCParticipantEvent"]
    }));
    sub.replace_with(&enable(json!({ "callIds": ["CALL-B"] })))
        .unwrap();
    let f = filter(&sub);
    assert_eq!(f.call_ids(), &CallScope::Only(vec![id("CALL-B")]));
    // The second frame's eventTypes was absent (Default: null, L147-153),
    // so the first frame's narrowing to VTCParticipantEvent is gone.
    assert!(f.includes("VTCRingEvent"));
    assert!(f.includes("VTCMediaStateEvent"));
}

/// L172-173: "If a `callIds` entry refers to a call in which the
/// authenticated user is not a current participant, that entry is silently
/// ignored." Silently: enabling succeeds and the entry is not an error.
#[test]
fn unknown_call_id_is_not_an_error() {
    let sub = enabled(json!({ "callIds": ["NOT-MY-CALL", "MY-CALL"] }));
    assert_eq!(
        filter(&sub).call_ids(),
        &CallScope::Only(vec![id("NOT-MY-CALL"), id("MY-CALL")])
    );
}

/// L174-175: "If all entries are ignored (or `callIds` is an empty array),
/// the subscription is effectively empty" — accepted, not an error, and
/// covering no call.
#[test]
fn empty_call_ids_is_accepted_and_covers_no_call() {
    let sub = enabled(json!({ "callIds": [] }));
    assert_eq!(filter(&sub).call_ids(), &CallScope::Only(vec![]));
}

/// L142-145: `callIds` is "`null` to receive events for all calls ...
/// Default: `null`."
#[test]
fn null_or_absent_call_ids_is_all() {
    for raw in [json!({ "callIds": null }), json!({})] {
        assert_eq!(filter(&enabled(raw)).call_ids(), &CallScope::All);
    }
}

/// L177-179: "Unrecognized values in `eventTypes` that appear alongside
/// recognized values MUST be silently ignored; only the recognized values
/// take effect."
#[test]
fn unrecognized_event_types_beside_recognized_are_ignored() {
    let sub = enabled(json!({
        "eventTypes": ["VTCFutureEvent", "VTCActiveSpeakerEvent"]
    }));
    let f = filter(&sub);
    assert!(f.includes("VTCActiveSpeakerEvent"));
    assert!(!f.includes("VTCFutureEvent"));
    assert!(!f.includes("VTCRingEvent"));
}

/// L179-183: "If `eventTypes` is an empty array or contains only
/// unrecognized values, the server MUST respond with a `RequestError` ...
/// `"urn:ietf:params:jmap:error:invalidArguments"` and MUST NOT update the
/// current ephemeral subscription state."
#[test]
fn empty_or_unrecognized_event_types_is_invalid_and_keeps_state() {
    for bad in [
        json!({ "callIds": ["CALL-B"], "eventTypes": [] }),
        json!({ "callIds": ["CALL-B"], "eventTypes": ["VTCFutureEvent"] }),
    ] {
        let mut sub = enabled(json!({
            "callIds": ["CALL-A"],
            "eventTypes": ["VTCMediaStateEvent"]
        }));
        let before = sub.clone();
        let err = sub.replace_with(&enable(bad)).unwrap_err();
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        assert_eq!(sub, before);
    }
}

/// The invalid frame also leaves a never-enabled connection inactive
/// (L182-183: "MUST NOT update the current ephemeral subscription state").
#[test]
fn invalid_first_enable_leaves_connection_inactive() {
    let mut sub = VtcEphemeralSubscription::default();
    assert!(sub
        .replace_with(&enable(json!({ "eventTypes": [] })))
        .is_err());
    assert_eq!(sub.filter(), None);
}

/// L147-149: `eventTypes` is "`null` to receive all event types".
#[test]
fn null_event_types_includes_every_type() {
    let sub = enabled(json!({ "eventTypes": null }));
    for t in [
        "VTCRingEvent",
        "VTCCallEndEvent",
        "VTCGatewaySignal",
        "VTCParticipantEvent",
        "VTCMediaStateEvent",
        "VTCActiveSpeakerEvent",
        "VTCUnmuteRequestEvent",
        "VTCRecordingStateEvent",
    ] {
        assert!(filter(&sub).includes(t), "{t}");
    }
}

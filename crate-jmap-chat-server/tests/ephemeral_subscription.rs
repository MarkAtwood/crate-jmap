//! `ChatStreamEnable` validation and replace semantics (bd:JMAP-yuut.1).
//!
//! Oracle: draft-atwood-jmap-chat-wss-00 §ChatStreamEnable at
//! jmap-chat-spec commit 91672c9 (lines 183-195). Each test quotes the
//! sentence it enforces. Frames are hand-written JSON; expected results come
//! only from the draft text.

use jmap_chat_server::{EphemeralSubscription, IdScope};
use jmap_chat_types::ChatStreamEnable;
use jmap_types::Id;
use serde_json::json;

fn frame(value: serde_json::Value) -> ChatStreamEnable {
    serde_json::from_value(value).expect("hand-written ChatStreamEnable frame must parse")
}

fn ids(list: &[&str]) -> IdScope {
    IdScope::Only(list.iter().copied().map(Id::from).collect())
}

/// wss L195: "If `dataTypes` is empty, or contains only unrecognized values
/// (i.e., none of `"typing"` or `"presence"`), the server MUST respond with a
/// `RequestError` frame ... with a `type` of
/// `"urn:ietf:params:jmap:error:invalidArguments"` and MUST NOT update the
/// current ephemeral subscription state."
#[test]
fn invalid_frame_is_invalid_arguments_and_keeps_prior_subscription() {
    let mut sub = EphemeralSubscription::default();
    sub.replace_with(&frame(json!({"dataTypes": ["typing"], "chatIds": ["c1"]})))
        .expect("typing is a recognized dataType");
    let before = sub.clone();

    for bad in [
        json!({"dataTypes": []}),
        json!({"dataTypes": ["x-vendor", "vtc"]}),
    ] {
        let err = sub
            .replace_with(&frame(bad.clone()))
            .expect_err("frame without typing or presence must be rejected");
        assert_eq!(err.error_type, "invalidArguments", "frame {bad}");
        assert_eq!(sub, before, "subscription must be unchanged after {bad}");
    }
    assert_eq!(sub.typing(), Some(&ids(&["c1"])));
    assert_eq!(sub.presence(), None);
}

/// wss L191: "A subsequent `ChatStreamEnable` message replaces the previous
/// ephemeral subscription in its entirety. There is no partial-update
/// mechanism".
#[test]
fn second_enable_replaces_not_merges() {
    let mut sub = EphemeralSubscription::default();
    sub.replace_with(&frame(json!({
        "dataTypes": ["typing", "presence"],
        "chatIds": ["c1"],
        "contactIds": ["p1"]
    })))
    .unwrap();

    sub.replace_with(&frame(
        json!({"dataTypes": ["presence"], "contactIds": ["p2"]}),
    ))
    .unwrap();

    assert_eq!(
        sub.typing(),
        None,
        "typing from the first frame must not survive"
    );
    assert_eq!(
        sub.presence(),
        Some(&ids(&["p2"])),
        "contactIds are replaced, not unioned"
    );
}

/// wss L189: "An absent `chatIds` or `contactIds` is equivalent to `null`";
/// L184/L187: `null` means typing events "for all Chats of which the owner is
/// a current member" / presence events "for all known ChatContacts".
#[test]
fn absent_and_null_ids_both_mean_all() {
    let absent =
        EphemeralSubscription::from_enable(&frame(json!({"dataTypes": ["typing", "presence"]})))
            .unwrap();
    let null = EphemeralSubscription::from_enable(&frame(json!({
        "dataTypes": ["typing", "presence"],
        "chatIds": null,
        "contactIds": null
    })))
    .unwrap();

    assert_eq!(absent, null);
    assert_eq!(absent.typing(), Some(&IdScope::All));
    assert_eq!(absent.presence(), Some(&IdScope::All));
}

/// wss L195: "Unrecognized values in `dataTypes` that appear alongside
/// recognized values MUST be silently ignored; only the recognized values take
/// effect." L184: `chatIds` is "Ignored if `"typing"` is not in `dataTypes`."
#[test]
fn unrecognized_values_ignored_and_inapplicable_ids_dropped() {
    let sub = EphemeralSubscription::from_enable(&frame(json!({
        "dataTypes": ["x-vendor", "presence"],
        "chatIds": ["c1"],
        "contactIds": ["p1"]
    })))
    .expect("presence alongside an unrecognized value is valid");

    assert_eq!(sub.typing(), None);
    assert_eq!(sub.presence(), Some(&ids(&["p1"])));
}

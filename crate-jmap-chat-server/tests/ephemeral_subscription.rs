//! `ChatStreamEnable` validation and replace semantics (bd:JMAP-yuut.1).
//!
//! Oracle: draft-atwood-jmap-chat-wss-00 §ChatStreamEnable at
//! jmap-chat-spec commit 91672c9 (lines 183-195), and companion values
//! at commit 6bcd9d8 (lines 175-196, bd:JMAP-0x0q.9). Each test quotes the
//! sentence it enforces. Frames are hand-written JSON; expected results come
//! only from the draft text.

use jmap_chat_server::{CompanionStreams, EphemeralSubscription, IdScope};
use jmap_chat_types::ChatStreamEnable;
use jmap_types::Id;
use serde_json::json;

fn frame(value: serde_json::Value) -> ChatStreamEnable {
    serde_json::from_value(value).expect("hand-written ChatStreamEnable frame must parse")
}

fn ids(list: &[&str]) -> IdScope {
    IdScope::Only(list.iter().copied().map(Id::from).collect())
}

/// wss 6bcd9d8 L194: "If `dataTypes` is empty, or contains only
/// unrecognized values (i.e., none of `"typing"`, `"presence"`, or a
/// companion value whose capability the server advertises), the server
/// MUST respond with a
/// `RequestError` frame ... with a `type` of
/// `"urn:ietf:params:jmap:error:invalidArguments"` and MUST NOT update the
/// current ephemeral subscription state."
#[test]
fn invalid_frame_is_invalid_arguments_and_keeps_prior_subscription() {
    let mut sub = EphemeralSubscription::default();
    sub.replace_with(
        &frame(json!({"dataTypes": ["typing"], "chatIds": ["c1"]})),
        CompanionStreams::default(),
    )
    .expect("typing is a recognized dataType");
    let before = sub.clone();

    for bad in [
        json!({"dataTypes": []}),
        json!({"dataTypes": ["x-vendor", "vtc"]}),
    ] {
        let err = sub
            .replace_with(&frame(bad.clone()), CompanionStreams::default())
            .expect_err("frame with no recognized value must be rejected");
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
    sub.replace_with(
        &frame(json!({
            "dataTypes": ["typing", "presence"],
            "chatIds": ["c1"],
            "contactIds": ["p1"]
        })),
        CompanionStreams::default(),
    )
    .unwrap();

    sub.replace_with(
        &frame(json!({"dataTypes": ["presence"], "contactIds": ["p2"]})),
        CompanionStreams::default(),
    )
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
    let absent = EphemeralSubscription::from_enable(
        &frame(json!({"dataTypes": ["typing", "presence"]})),
        CompanionStreams::default(),
    )
    .unwrap();
    let null = EphemeralSubscription::from_enable(
        &frame(json!({
            "dataTypes": ["typing", "presence"],
            "chatIds": null,
            "contactIds": null
        })),
        CompanionStreams::default(),
    )
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
    let sub = EphemeralSubscription::from_enable(
        &frame(json!({
            "dataTypes": ["x-vendor", "presence"],
            "chatIds": ["c1"],
            "contactIds": ["p1"]
        })),
        CompanionStreams::default(),
    )
    .expect("presence alongside an unrecognized value is valid");

    assert_eq!(sub.typing(), None);
    assert_eq!(sub.presence(), Some(&ids(&["p1"])));
}

fn advertising(vtc: bool, scene: bool) -> CompanionStreams {
    let mut companions = CompanionStreams::default();
    companions.vtc = vtc;
    companions.scene = scene;
    companions
}

/// wss 6bcd9d8 L175: `"vtc"` is "recognized only when the server advertises
/// `urn:ietf:params:jmap:vtc:websocket`", and `"scene"` "recognized only
/// when the server advertises `urn:ietf:params:jmap:scene:websocket`". L194
/// rejects only a frame with "none of `"typing"`, `"presence"`, or a
/// companion value whose capability the server advertises", so a
/// companion-only frame is accepted when its capability is advertised.
#[test]
fn companion_only_frame_accepted_when_capability_advertised() {
    let vtc = EphemeralSubscription::from_enable(
        &frame(json!({"dataTypes": ["vtc"]})),
        advertising(true, false),
    )
    .expect("\"vtc\" is recognized when its capability is advertised");
    assert!(vtc.vtc());
    assert!(!vtc.scene());
    assert_eq!(vtc.typing(), None);
    assert_eq!(vtc.presence(), None);

    let scene = EphemeralSubscription::from_enable(
        &frame(json!({"dataTypes": ["scene", "x-vendor"]})),
        advertising(false, true),
    )
    .expect("\"scene\" is recognized when its capability is advertised");
    assert!(scene.scene());
    assert!(!scene.vtc());
}

/// wss 6bcd9d8 L196: "When a companion capability (e.g.,
/// `urn:ietf:params:jmap:vtc:websocket`) is not present, its corresponding
/// `dataTypes` value (e.g., `"vtc"`) is treated as unrecognized and
/// silently ignored." With nothing else recognized, L194 requires the
/// `invalidArguments` error and an unchanged subscription.
#[test]
fn companion_value_without_its_capability_is_unrecognized() {
    let mut sub = EphemeralSubscription::default();
    sub.replace_with(
        &frame(json!({"dataTypes": ["scene"]})),
        advertising(false, true),
    )
    .expect("\"scene\" with its capability advertised");
    let before = sub.clone();

    let err = sub
        .replace_with(
            &frame(json!({"dataTypes": ["vtc"]})),
            advertising(false, true),
        )
        .expect_err("\"vtc\" without its capability is unrecognized");
    assert_eq!(err.error_type, "invalidArguments");
    assert_eq!(sub, before);

    let typing = EphemeralSubscription::from_enable(
        &frame(json!({"dataTypes": ["typing", "vtc", "scene"]})),
        CompanionStreams::default(),
    )
    .expect("typing alongside ignored companion values is valid");
    assert_eq!(typing.typing(), Some(&IdScope::All));
    assert!(!typing.vtc());
    assert!(!typing.scene());
}

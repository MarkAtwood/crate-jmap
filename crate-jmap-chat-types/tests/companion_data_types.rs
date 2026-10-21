//! Companion `ChatStreamEnable` `dataTypes` values `"vtc"` and `"scene"`.
//!
//! Oracle: hand-written frames and strings quoted from the drafts at
//! jmap-chat-spec 91672c9.

use jmap_chat_types::vocabulary::{COMPANION_EPHEMERAL_DATA_TYPES, SPEC_EPHEMERAL_DATA_TYPES};
use jmap_chat_types::{ChatStreamEnable, EphemeralDataType};
use serde_json::json;

/// draft-atwood-jmap-chat-wss-00 L179: "This specification defines
/// `"typing"` and `"presence"`; companion specifications MAY register
/// additional values. Currently registered companion values: `"vtc"` for
/// in-call events defined by {{JMAP-VTC-WSS}}, and `"scene"` for spatial
/// events defined by {{JMAP-SCENE-WSS}}."
#[test]
fn companion_values_are_listed_apart_from_chat_values() {
    assert_eq!(COMPANION_EPHEMERAL_DATA_TYPES, &["vtc", "scene"]);
    assert_eq!(SPEC_EPHEMERAL_DATA_TYPES, &["typing", "presence"]);
}

/// draft-atwood-jmap-vtc-wss-00 L347: "`ChatStreamEnable` with `"vtc"` in
/// `dataTypes`: Subscribes to all VTC event types for all calls in which
/// the user participates."
#[test]
fn enable_with_vtc_parses() {
    let frame: ChatStreamEnable = serde_json::from_value(json!({
        "@type": "ChatStreamEnable",
        "dataTypes": ["typing", "vtc"]
    }))
    .expect("frame with \"vtc\" must parse");
    assert_eq!(
        frame.data_types,
        [EphemeralDataType::Typing, EphemeralDataType::Vtc]
    );
    assert_eq!(frame.chat_ids, None);
    assert_eq!(frame.contact_ids, None);
}

/// draft-atwood-jmap-scene-wss-00 L837: "`ChatStreamEnable` with `"scene"`
/// in `dataTypes`: Subscribes to all Scene event types for all regions in
/// which the user has an active avatar."
#[test]
fn enable_with_scene_parses() {
    let frame: ChatStreamEnable = serde_json::from_value(json!({
        "@type": "ChatStreamEnable",
        "dataTypes": ["presence", "scene"],
        "contactIds": ["c1"]
    }))
    .expect("frame with \"scene\" must parse");
    assert_eq!(
        frame.data_types,
        [EphemeralDataType::Presence, EphemeralDataType::Scene]
    );
    let contact_ids = frame.contact_ids.expect("contactIds present");
    assert_eq!(contact_ids.len(), 1);
    assert_eq!(contact_ids[0].as_ref(), "c1");
}

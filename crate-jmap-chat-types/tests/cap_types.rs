//! `urn:ietf:params:jmap:chat:cap:*` MessageAction/Endpoint type constants.
//!
//! Oracle: the type strings quoted from draft-atwood-jmap-chat-00 at
//! jmap-chat-spec 6bcd9d8.

use jmap_chat_types::{
    CHAT_CAP_AVAILABILITY, CHAT_CAP_CALENDAR_EVENT, CHAT_CAP_TASK, CHAT_CAP_VTC,
};

/// draft-atwood-jmap-chat-00 (6bcd9d8) L380 and L383-385 list the
/// Endpoint `type` values `"urn:ietf:params:jmap:chat:cap:vtc"`,
/// `"urn:ietf:params:jmap:chat:cap:calendar-event"`,
/// `"urn:ietf:params:jmap:chat:cap:availability"` and
/// `"urn:ietf:params:jmap:chat:cap:task"`.
#[test]
fn cap_type_constants_match_draft() {
    assert_eq!(CHAT_CAP_VTC, "urn:ietf:params:jmap:chat:cap:vtc");
    assert_eq!(
        CHAT_CAP_CALENDAR_EVENT,
        "urn:ietf:params:jmap:chat:cap:calendar-event"
    );
    assert_eq!(
        CHAT_CAP_AVAILABILITY,
        "urn:ietf:params:jmap:chat:cap:availability"
    );
    assert_eq!(CHAT_CAP_TASK, "urn:ietf:params:jmap:chat:cap:task");
}

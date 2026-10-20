//! `jmap:` URI parser/builder and `urn:ietf:params:jmap:chat:cap:*` constants.
//!
//! Oracle: the literal URI forms quoted from the drafts at jmap-chat-spec
//! 91672c9, and the type strings quoted from draft-atwood-jmap-chat-00 at
//! jmap-chat-spec 6bcd9d8.

use jmap_chat_types::vocabulary::{
    CHAT_CAP_AVAILABILITY, CHAT_CAP_CALENDAR_EVENT, CHAT_CAP_TASK, CHAT_CAP_VTC,
};
use jmap_chat_types::{JmapUri, JmapUriError};
use jmap_types::Id;

/// draft-atwood-jmap-chat-calendars-00 L153: "A `jmap:` URI in the form
/// `jmap:calendarevent:<accountId>:<calendarEventId>`".
#[test]
fn calendar_event_form_round_trips() {
    let uri: JmapUri = "jmap:calendarevent:A13824:ev-7_x".parse().unwrap();
    assert_eq!(uri.data_type(), "calendarevent");
    assert_eq!(uri.account_id().as_ref(), "A13824");
    assert_eq!(uri.object_id().as_ref(), "ev-7_x");
    assert_eq!(uri.object_ids().len(), 1);
    assert_eq!(uri.to_string(), "jmap:calendarevent:A13824:ev-7_x");
}

/// draft-atwood-jmap-chat-calendars-00 L245: "A `jmap:` URI in the form
/// `jmap:principal:<accountId>:<principalId>`".
#[test]
fn principal_form_parses() {
    let uri: JmapUri = "jmap:principal:acct1:p9".parse().unwrap();
    assert_eq!(uri.data_type(), "principal");
    assert_eq!(uri.object_id().as_ref(), "p9");
}

/// draft-atwood-jmap-chat-tasks-00 L185: "A `jmap:` URI in the form
/// `jmap:task:<accountId>:<taskListId>:<taskId>`".
#[test]
fn task_form_carries_task_list_and_task_ids() {
    let uri: JmapUri = "jmap:task:acct1:list2:task3".parse().unwrap();
    assert_eq!(uri.data_type(), "task");
    assert_eq!(uri.account_id().as_ref(), "acct1");
    let ids: Vec<&str> = uri.object_ids().iter().map(AsRef::as_ref).collect();
    assert_eq!(ids, ["list2", "task3"]);
    assert_eq!(uri.object_id().as_ref(), "task3");
    assert_eq!(uri.to_string(), "jmap:task:acct1:list2:task3");
}

/// draft-atwood-jmap-chat-calendars-00 L156: "The syntax is:
/// `jmap:<type>:<accountId>:<objectId>`".
#[test]
fn builder_renders_spec_syntax() {
    let uri = JmapUri::new("calendarevent", Id::from("a1"), vec![Id::from("e1")]).unwrap();
    assert_eq!(uri.to_string(), "jmap:calendarevent:a1:e1");
    let task = JmapUri::new("task", Id::from("a1"), vec![Id::from("l1"), Id::from("t1")]).unwrap();
    assert_eq!(task.to_string(), "jmap:task:a1:l1:t1");
}

/// calendars L156: "`<type>` is a registered JMAP data type name
/// (lowercased), `<accountId>` is a valid JMAP account identifier, and
/// `<objectId>` is the object's `id` value". RFC 8620 §1.2 limits an Id to
/// "A-Za-z0-9", "-" and "_".
#[test]
fn malformed_uris_rejected() {
    let cases: &[(&str, JmapUriError)] = &[
        ("https://example.com/e1", JmapUriError::NotJmapScheme),
        ("webcal:calendarevent:a1:e1", JmapUriError::NotJmapScheme),
        ("calendarevent:a1:e1", JmapUriError::NotJmapScheme),
        ("jmap", JmapUriError::InvalidDataType),
        ("jmap::a1:e1", JmapUriError::InvalidDataType),
        ("jmap:CalendarEvent:a1:e1", JmapUriError::InvalidDataType),
        ("jmap:calendar-event:a1:e1", JmapUriError::InvalidDataType),
        ("jmap:calendarevent", JmapUriError::InvalidAccountId(None)),
        ("jmap:calendarevent:a1", JmapUriError::MissingObjectId),
    ];
    for (input, want) in cases {
        assert_eq!(&input.parse::<JmapUri>().unwrap_err(), want, "{input}");
    }
    for input in [
        "jmap:calendarevent::e1",
        "jmap:calendarevent:a/1:e1",
        "jmap:calendarevent:a1:",
        "jmap:calendarevent:a1:e 1",
        "jmap:task:a1::t1",
    ] {
        let err = input.parse::<JmapUri>().unwrap_err();
        assert!(
            matches!(
                err,
                JmapUriError::InvalidAccountId(Some(_)) | JmapUriError::InvalidObjectId(_)
            ),
            "{input}: {err:?}"
        );
    }
}

/// RFC 3986 §3.1: "schemes are case-insensitive".
#[test]
fn scheme_is_case_insensitive_and_rendered_lowercase() {
    let uri: JmapUri = "JMAP:calendarevent:a1:e1".parse().unwrap();
    assert_eq!(uri.to_string(), "jmap:calendarevent:a1:e1");
}

/// The builder must not render a URI the parser would split differently:
/// RFC 8620 §1.2 Ids exclude ":".
#[test]
fn builder_rejects_unvalidated_ids() {
    assert!(matches!(
        JmapUri::new("task", Id::from("a:1"), vec![Id::from("t1")]),
        Err(JmapUriError::InvalidAccountId(Some(_)))
    ));
    assert!(matches!(
        JmapUri::new("task", Id::from("a1"), vec![Id::from("l1:t1")]),
        Err(JmapUriError::InvalidObjectId(_))
    ));
    assert_eq!(
        JmapUri::new("task", Id::from("a1"), vec![]),
        Err(JmapUriError::MissingObjectId)
    );
    assert_eq!(
        JmapUri::new("Task", Id::from("a1"), vec![Id::from("t1")]),
        Err(JmapUriError::InvalidDataType)
    );
}

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

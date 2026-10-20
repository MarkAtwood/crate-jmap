//! `jmap:` object URI parser/builder.
//!
//! Oracle: the ABNF and literal URI forms quoted from the drafts at
//! jmap-chat-spec 6bcd9d8.

use jmap_types::{Id, JmapUri, JmapUriError};

/// draft-atwood-jmap-object-uri-00 L80: "For example,
/// `jmap:calendarevent:A13824:E6B2` refers to the CalendarEvent with id
/// `E6B2` in account `A13824`."
#[test]
fn object_uri_draft_example_round_trips() {
    let uri: JmapUri = "jmap:calendarevent:A13824:E6B2".parse().unwrap();
    assert_eq!(uri.data_type(), "calendarevent");
    assert_eq!(uri.account_id().as_ref(), "A13824");
    assert_eq!(uri.object_id().as_ref(), "E6B2");
    assert_eq!(uri.to_string(), "jmap:calendarevent:A13824:E6B2");
}

/// draft-atwood-jmap-object-uri-00 L76: "id-char = ALPHA / DIGIT / "-" /
/// "_"".
#[test]
fn id_chars_include_hyphen_and_underscore() {
    let uri: JmapUri = "jmap:calendarevent:A1-_:ev-7_x".parse().unwrap();
    assert_eq!(uri.account_id().as_ref(), "A1-_");
    assert_eq!(uri.object_id().as_ref(), "ev-7_x");
}

/// draft-atwood-jmap-chat-calendars-00 L255: "A JMAP object URI ... of
/// type `principal`, in the form `jmap:principal:<accountId>:<principalId>`".
#[test]
fn principal_form_parses() {
    let uri: JmapUri = "jmap:principal:acct1:p9".parse().unwrap();
    assert_eq!(uri.data_type(), "principal");
    assert_eq!(uri.object_id().as_ref(), "p9");
}

/// draft-atwood-jmap-chat-tasks-00 L191: "A JMAP object URI ... of type
/// `task`, in the form `jmap:task:<accountId>:<taskId>`".
#[test]
fn task_form_carries_one_task_id() {
    let uri: JmapUri = "jmap:task:acct1:task3".parse().unwrap();
    assert_eq!(uri.data_type(), "task");
    assert_eq!(uri.account_id().as_ref(), "acct1");
    assert_eq!(uri.object_id().as_ref(), "task3");
    assert_eq!(uri.to_string(), "jmap:task:acct1:task3");
}

/// draft-atwood-jmap-object-uri-00 L71: "jmap-object-uri = "jmap:"
/// type-name ":" account-id ":" object-id" admits exactly one object-id,
/// and L96: "A receiver treats a `jmap` URI that matches no form it
/// implements as unresolvable." The retired five-segment task form
/// `jmap:task:<accountId>:<taskListId>:<taskId>` matches no form.
#[test]
fn segments_after_object_id_rejected() {
    for input in ["jmap:task:acct1:list2:task3", "jmap:calendarevent:a1:e1:"] {
        assert!(
            matches!(
                input.parse::<JmapUri>(),
                Err(JmapUriError::InvalidObjectId(_))
            ),
            "{input}"
        );
    }
}

/// draft-atwood-jmap-object-uri-00 L71-76: "type-name = 1*( %x61-7A /
/// DIGIT )", "account-id = 1*255id-char", "object-id = 1*255id-char".
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
        ("jmap://host/x", JmapUriError::InvalidDataType),
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

/// draft-atwood-jmap-object-uri-00 L92: "the scheme name is
/// case-insensitive ... The other components are compared
/// case-sensitively".
#[test]
fn scheme_is_case_insensitive_and_rendered_lowercase() {
    let uri: JmapUri = "JMAP:calendarevent:a1:e1".parse().unwrap();
    assert_eq!(uri.to_string(), "jmap:calendarevent:a1:e1");
    let upper: JmapUri = "jmap:calendarevent:A1:E1".parse().unwrap();
    assert_ne!(uri, upper);
}

/// The builder must not render a URI the parser would split differently:
/// draft-atwood-jmap-object-uri-00 L76 limits ids to the RFC 8620 §1.2
/// alphabet, which excludes ":".
#[test]
fn builder_validates_parts() {
    let uri = JmapUri::new("calendarevent", Id::from("a1"), Id::from("e1")).unwrap();
    assert_eq!(uri.to_string(), "jmap:calendarevent:a1:e1");
    assert!(matches!(
        JmapUri::new("task", Id::from("a:1"), Id::from("t1")),
        Err(JmapUriError::InvalidAccountId(Some(_)))
    ));
    assert!(matches!(
        JmapUri::new("task", Id::from("a1"), Id::from("l1:t1")),
        Err(JmapUriError::InvalidObjectId(_))
    ));
    assert_eq!(
        JmapUri::new("Task", Id::from("a1"), Id::from("t1")),
        Err(JmapUriError::InvalidDataType)
    );
}

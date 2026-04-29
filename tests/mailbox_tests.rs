mod common;

use jmap_mail_types::{Mailbox, MailboxRole};

#[test]
fn mailbox_inbox_deserializes_correctly() {
    let json = common::fixture("mailbox_inbox.json");
    let mb: Mailbox = serde_json::from_str(&json).expect("deserialize mailbox_inbox.json");
    assert_eq!(mb.name, "Inbox");
    assert_eq!(mb.role, Some(MailboxRole::Inbox));
    assert_eq!(mb.sort_order, 10);
    assert_eq!(mb.total_emails, 16307);
    assert!(mb.is_subscribed);
    assert!(mb.my_rights.may_read_items);
    assert!(!mb.my_rights.may_delete);
}

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn mailbox_inbox_roundtrip() {
    let json = common::fixture("mailbox_inbox.json");
    let mb: Mailbox = serde_json::from_str(&json).expect("deserialize");
    let serialized = serde_json::to_string(&mb).expect("serialize");
    let original: serde_json::Value = serde_json::from_str(&json).expect("parse original");
    let reserialized: serde_json::Value =
        serde_json::from_str(&serialized).expect("parse reserialized");
    assert_eq!(
        original, reserialized,
        "round-trip must produce identical JSON value"
    );
}

#[test]
fn mailbox_role_wire_names() {
    // Oracle: RFC 8621 §2 says roles are lowercase strings
    let cases = [
        (MailboxRole::Inbox, "\"inbox\""),
        (MailboxRole::Trash, "\"trash\""),
        (MailboxRole::Sent, "\"sent\""),
        (MailboxRole::Drafts, "\"drafts\""),
        (MailboxRole::Junk, "\"junk\""),
        (MailboxRole::Archive, "\"archive\""),
    ];
    for (role, expected) in cases {
        let s = serde_json::to_string(&role).expect("serialize role");
        assert_eq!(s, expected, "MailboxRole serialization mismatch");
    }
}

#[test]
fn mailbox_rights_all_nine_fields_present() {
    // Oracle: RFC 8621 §2 myRights has exactly 9 boolean fields
    let json = common::fixture("mailbox_inbox.json");
    let mb: Mailbox = serde_json::from_str(&json).expect("deserialize");
    let rights_json = serde_json::to_string(&mb.my_rights).expect("serialize rights");
    for field in &[
        "mayReadItems",
        "mayAddItems",
        "mayRemoveItems",
        "maySetSeen",
        "maySetKeywords",
        "mayCreateChild",
        "mayRename",
        "mayDelete",
        "maySubmit",
    ] {
        assert!(rights_json.contains(field), "Missing field: {}", field);
    }
}

#[test]
fn mailbox_trash_deserializes_correctly() {
    let json = common::fixture("mailbox_trash.json");
    let mb: Mailbox = serde_json::from_str(&json).expect("deserialize mailbox_trash.json");
    assert_eq!(mb.name, "Trash");
    assert_eq!(mb.role, Some(MailboxRole::Trash));
    assert_eq!(mb.total_emails, 42);
    assert!(!mb.is_subscribed);
}

#[test]
fn mailbox_role_unknown_does_not_error() {
    // Oracle: RFC 8621 §2 — unrecognized roles SHOULD be treated as if no role were set.
    // A future IANA-registered value must not hard-fail deserialization.
    let role: MailboxRole = serde_json::from_str("\"futurevalue\"")
        .expect("unknown role must not fail deserialization");
    assert_eq!(role, MailboxRole::Other);
}

#[test]
fn mailbox_role_display_matches_wire_names() {
    // Oracle: RFC 8621 §2 — roles are lowercase strings; Display must match wire name.
    assert_eq!(MailboxRole::Inbox.to_string(), "inbox");
    assert_eq!(MailboxRole::Trash.to_string(), "trash");
    assert_eq!(MailboxRole::Sent.to_string(), "sent");
    assert_eq!(MailboxRole::Drafts.to_string(), "drafts");
    assert_eq!(MailboxRole::Junk.to_string(), "junk");
    assert_eq!(MailboxRole::Archive.to_string(), "archive");
    assert_eq!(MailboxRole::Other.to_string(), "other");
}

#[test]
fn mailbox_with_unknown_role_deserializes() {
    // Inline JSON for a Mailbox carrying an unrecognized role string.
    let json = r#"{
        "id": "abc123",
        "name": "FutureBox",
        "role": "futurevalue",
        "sortOrder": 99,
        "totalEmails": 0,
        "unreadEmails": 0,
        "totalThreads": 0,
        "unreadThreads": 0,
        "myRights": {
            "mayReadItems": true,
            "mayAddItems": false,
            "mayRemoveItems": false,
            "maySetSeen": false,
            "maySetKeywords": false,
            "mayCreateChild": false,
            "mayRename": false,
            "mayDelete": false,
            "maySubmit": false
        },
        "isSubscribed": false
    }"#;
    let mb: Mailbox =
        serde_json::from_str(json).expect("Mailbox with unknown role must deserialize");
    assert_eq!(mb.role, Some(MailboxRole::Other));
}

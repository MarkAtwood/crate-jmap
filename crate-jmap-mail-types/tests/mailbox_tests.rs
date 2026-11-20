mod common;

use jmap_mail_types::{Mailbox, MailboxRights, MailboxRole};

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
    let mut original: serde_json::Value = serde_json::from_str(&json).expect("parse original");
    // RFC 8621 §2: parentId is "Id|null" (default: null), so it is
    // serialized even when the fixture omits it.
    original
        .as_object_mut()
        .unwrap()
        .entry("parentId")
        .or_insert(serde_json::Value::Null);
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
        (MailboxRole::Flagged, "\"flagged\""),
        (MailboxRole::Important, "\"important\""),
        (MailboxRole::All, "\"all\""),
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
    // A future IANA-registered value must not hard-fail deserialization and must round-trip.
    let role: MailboxRole = serde_json::from_str("\"futurevalue\"")
        .expect("unknown role must not fail deserialization");
    assert_eq!(role, MailboxRole::Other("futurevalue".to_owned()));
    // The original string must be preserved on re-serialization.
    let reserialized = serde_json::to_string(&role).expect("serialize");
    assert_eq!(reserialized, "\"futurevalue\"");
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
    assert_eq!(MailboxRole::Flagged.to_string(), "flagged");
    assert_eq!(MailboxRole::Important.to_string(), "important");
    assert_eq!(MailboxRole::All.to_string(), "all");
    assert_eq!(MailboxRole::Other("other".to_owned()).to_string(), "other");
    assert_eq!(
        MailboxRole::Other("futurevalue".to_owned()).to_string(),
        "futurevalue"
    );
}

#[test]
fn mailbox_with_unknown_role_deserializes() {
    // Inline JSON for a Mailbox carrying an unrecognized role string.
    let json = &common::fixture("mailbox_with_unknown_role_deserializes.json");
    let mb: Mailbox =
        serde_json::from_str(json).expect("Mailbox with unknown role must deserialize");
    assert_eq!(mb.role, Some(MailboxRole::Other("futurevalue".to_owned())));
}

#[test]
fn mailbox_new_constructor() {
    use jmap_mail_types::MailboxRights;
    let mb = Mailbox::new(
        jmap_types::Id::from("MB1"),
        "Inbox",
        10,
        16307,
        3,
        500,
        1,
        MailboxRights::default(),
        true,
    );
    assert_eq!(mb.id, jmap_types::Id::from("MB1"));
    assert_eq!(mb.name, "Inbox");
    assert_eq!(mb.sort_order, 10);
    assert_eq!(mb.total_emails, 16307);
    assert_eq!(mb.unread_emails, 3);
    assert_eq!(mb.total_threads, 500);
    assert_eq!(mb.unread_threads, 1);
    assert!(mb.is_subscribed);
    assert!(mb.parent_id.is_none());
    assert!(mb.role.is_none());
}

/// Oracle: RFC 8621 §2 `role: "String|null" (default: null)`, and RFC 8620
/// §5.1: "If null, all properties of the object are returned."
#[test]
fn mailbox_without_role_serializes_role_null() {
    let json = &common::fixture("mailbox_without_role_serializes_role_null.json");
    let mb: Mailbox = serde_json::from_str(json).expect("deserialize");
    let v = serde_json::to_value(&mb).expect("serialize");
    assert!(v["role"].is_null() && v.get("role").is_some(), "{v}");
}

/// draft-ietf-jmap-metadata-05 §4.1: the rights object of every data type with
/// `myRights` "gains the following property: *maySetMetadata*: Boolean", and
/// the right is only present when the metadata capability lists the type, so
/// it is optional on the wire. Oracle: hand-written JSON with the spec's wire name.
#[test]
fn mailbox_rights_may_set_metadata_typed_and_optional() {
    let with: serde_json::Value = serde_json::json!({ "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true, "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": false, "mayRename": false, "mayDelete": false, "maySubmit": false, "maySetMetadata": true });
    let r: MailboxRights = serde_json::from_value(with.clone()).unwrap();
    assert_eq!(r.may_set_metadata, Some(true));
    assert!(!r.extra.contains_key("maySetMetadata"));
    assert_eq!(serde_json::to_value(&r).unwrap(), with);

    let denied: serde_json::Value = serde_json::json!({ "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true, "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": false, "mayRename": false, "mayDelete": false, "maySubmit": false, "maySetMetadata": false });
    let r: MailboxRights = serde_json::from_value(denied.clone()).unwrap();
    assert_eq!(r.may_set_metadata, Some(false));
    assert_eq!(serde_json::to_value(&r).unwrap(), denied);

    let without: serde_json::Value = serde_json::json!({ "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true, "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": false, "mayRename": false, "mayDelete": false, "maySubmit": false });
    let r: MailboxRights = serde_json::from_value(without.clone()).unwrap();
    assert_eq!(r.may_set_metadata, None);
    assert_eq!(serde_json::to_value(&r).unwrap(), without);
}

// ── draft-ietf-jmap-mail-sharing-02 §2 ──────────────────────────────────
//
// "*shareWith*: Id[MailboxRights]|null (default: null)  This is a map
// configuring who the mailbox is shared with, or null if it is not shared
// with anyone. Each key in the map is the id of a Principal with whom the
// mailbox is shared. The value for each key is the set of access rights
// that Principal has for the mailbox."
//
// "*mayShare*: Boolean  The user may modify the shareWith property for
// this mailbox."

/// A shared Mailbox as the draft describes it: `shareWith` and `mayShare`
/// are typed, not left in `extra`, and the value round-trips.
#[test]
fn share_with_and_may_share_are_typed() {
    let raw = serde_json::json!({
        "id": "m1", "name": "Team", "parentId": null, "role": null,
        "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0,
        "totalThreads": 0, "unreadThreads": 0,
        "myRights": {
            "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
            "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
            "mayRename": true, "mayDelete": true, "maySubmit": true,
            "mayShare": true
        },
        "isSubscribed": true,
        "shareWith": {
            "P2": {
                "mayReadItems": true, "mayAddItems": false, "mayRemoveItems": false,
                "maySetSeen": true, "maySetKeywords": false, "mayCreateChild": false,
                "mayRename": false, "mayDelete": false, "maySubmit": false,
                "mayShare": false
            }
        }
    });
    let mb: Mailbox = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(mb.my_rights.may_share, Some(true));
    let share_with = mb.share_with.as_ref().unwrap();
    let p2 = &share_with[&jmap_types::Id::from("P2")];
    assert!(p2.may_read_items && p2.may_set_seen && !p2.may_delete);
    assert_eq!(p2.may_share, Some(false));
    assert!(mb.extra.is_empty() && mb.my_rights.extra.is_empty());
    assert_eq!(serde_json::to_value(&mb).unwrap(), raw);
}

/// `null`, "not shared with anyone", and an absent `shareWith` (an RFC
/// 8621 server without the extension) both decode as `None`; `None` is not
/// serialized. An absent `mayShare` is `None`.
#[test]
fn share_with_null_or_absent_is_none() {
    let mut raw = serde_json::json!({
        "id": "m1", "name": "Solo", "parentId": null, "role": null,
        "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0,
        "totalThreads": 0, "unreadThreads": 0,
        "myRights": {
            "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
            "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
            "mayRename": true, "mayDelete": true, "maySubmit": true
        },
        "isSubscribed": true
    });
    let absent: Mailbox = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(absent.share_with, None);
    assert_eq!(absent.my_rights.may_share, None);
    assert_eq!(serde_json::to_value(&absent).unwrap(), raw);
    raw["shareWith"] = serde_json::Value::Null;
    let null: Mailbox = serde_json::from_value(raw).unwrap();
    assert_eq!(null.share_with, None);
}

/// Workspace policy (AGENTS.md, wire maps are `BTreeMap`): `shareWith`
/// keys serialize sorted. Oracle: the hand-written sorted JSON below.
#[test]
fn share_with_serializes_sorted() {
    let rights = r#"{"mayReadItems":true,"mayAddItems":false,"mayRemoveItems":false,"maySetSeen":false,"maySetKeywords":false,"mayCreateChild":false,"mayRename":false,"mayDelete":false,"maySubmit":false}"#;
    let input = format!(r#"{{"zed":{rights},"amy":{rights}}}"#);
    let map: std::collections::BTreeMap<jmap_types::Id, MailboxRights> =
        serde_json::from_str(&input).unwrap();
    assert_eq!(
        serde_json::to_string(&map).unwrap(),
        format!(r#"{{"amy":{rights},"zed":{rights}}}"#)
    );
}

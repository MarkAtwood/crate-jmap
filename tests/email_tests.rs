mod common;

use jmap_mail_types::{Email, EmailBodyPart};
use jmap_types::Date;

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn email_body_part_text_roundtrip() {
    let json = common::fixture("email_body_part_text.json");
    let part: EmailBodyPart = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(part.part_id.as_deref(), Some("1"));
    assert_eq!(part.type_.as_deref(), Some("text/plain"));
    let serialized = serde_json::to_string(&part).expect("serialize");
    // Wire name is "type" not "type_"
    assert!(
        serialized.contains("\"type\""),
        "type_ must serialize as \"type\""
    );
    assert!(
        !serialized.contains("\"type_\""),
        "type_ must not appear in JSON"
    );
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn email_body_part_multipart_roundtrip() {
    let json = common::fixture("email_body_part_multipart.json");
    let part: EmailBodyPart = serde_json::from_str(&json).expect("deserialize");
    let sub = part.sub_parts.as_ref().expect("subParts should be present");
    assert_eq!(sub.len(), 2);
    assert_eq!(sub[0].part_id.as_deref(), Some("1"));
    assert_eq!(sub[1].disposition.as_deref(), Some("attachment"));
    let serialized = serde_json::to_string(&part).expect("serialize");
    assert!(
        serialized.contains("\"subParts\""),
        "subParts wire name must be correct"
    );
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn email_minimal_roundtrip() {
    let json = common::fixture("email_minimal.json");
    let email: Email = serde_json::from_str(&json).expect("deserialize");
    assert!(email.from.is_none());
    assert!(email.subject.is_none());
    let serialized = serde_json::to_string(&email).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn email_mailbox_ids_serializes_as_object() {
    let json = r#"{"id":"M1","blobId":"G1","threadId":"T1","mailboxIds":{"MB1":true,"MB2":true},"size":100,"receivedAt":"2024-01-01T00:00:00Z"}"#;
    let email: Email = serde_json::from_str(json).expect("deserialize");
    assert_eq!(email.mailbox_ids.len(), 2);
    let serialized = serde_json::to_string(&email).expect("serialize");
    // mailboxIds must be a JSON object, not an array
    let v: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert!(
        v["mailboxIds"].is_object(),
        "mailboxIds must serialize as JSON object"
    );
}

// Oracle: RFC 8621 §A.1 example — sentAt carries a +10:00 offset (non-UTC Date, not UTCDate).
#[test]
fn email_sent_at_accepts_non_utc_timezone() {
    let json = r#"{"id":"M1","blobId":"G1","threadId":"T1","mailboxIds":{"MB1":true},"size":100,"receivedAt":"2024-01-01T00:00:00Z","sentAt":"2018-07-10T11:03:11+10:00"}"#;
    let email: Email = serde_json::from_str(json).expect("deserialize");
    assert_eq!(
        email.sent_at.as_ref().map(Date::as_ref),
        Some("2018-07-10T11:03:11+10:00")
    );
    let serialized = serde_json::to_string(&email).expect("serialize");
    assert!(
        serialized.contains("2018-07-10T11:03:11+10:00"),
        "non-UTC sentAt must round-trip unchanged"
    );
}

#[test]
fn email_keywords_map_serializes_as_object() {
    let json = r#"{"id":"M1","blobId":"G1","threadId":"T1","mailboxIds":{"MB1":true},"keywords":{"$seen":true},"size":100,"receivedAt":"2024-01-01T00:00:00Z"}"#;
    let email: Email = serde_json::from_str(json).expect("deserialize");
    assert!(email.keywords.contains_key("$seen"));
    let serialized = serde_json::to_string(&email).expect("serialize");
    let v: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert!(
        v["keywords"].is_object(),
        "keywords must serialize as JSON object"
    );
    assert_eq!(v["keywords"]["$seen"], serde_json::Value::Bool(true));
}

#[test]
fn email_body_part_default() {
    let p = EmailBodyPart::default();
    assert!(p.part_id.is_none());
    assert!(p.type_.is_none());
    assert!(p.sub_parts.is_none());
}

#[test]
fn email_new_constructor() {
    use jmap_types::Id;
    let email: jmap_mail_types::Email = serde_json::from_str(
        r#"{
        "id": "M1",
        "blobId": "G1",
        "threadId": "T1",
        "mailboxIds": {"MB1": true},
        "size": 1234,
        "receivedAt": "2024-01-01T00:00:00Z"
    }"#,
    )
    .expect("deserialize email");
    assert_eq!(email.id, Id::from("M1"));
    assert!(email.from.is_none());
    assert!(email.keywords.is_empty());
    assert!(!email.has_attachment);
}

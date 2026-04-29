mod common;

use jmap_mail_types::{
    Address, Delivered, DeliveryStatus, Displayed, EmailSubmission, Envelope, UndoStatus,
};

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn address_with_params_roundtrip() {
    let json = common::fixture("address_with_params.json");
    let a: Address = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(a.email, "john@example.com");
    assert!(a.parameters.is_some());
    let serialized = serde_json::to_string(&a).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn address_no_params_omitted() {
    let json = common::fixture("address_no_params.json");
    let a: Address = serde_json::from_str(&json).expect("deserialize");
    assert!(a.parameters.is_none());
    let serialized = serde_json::to_string(&a).expect("serialize");
    assert!(
        !serialized.contains("\"parameters\""),
        "parameters must be absent when None"
    );
}

#[test]
fn envelope_roundtrip() {
    let json = common::fixture("envelope.json");
    let e: Envelope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(e.mail_from.email, "john@example.com");
    assert_eq!(e.rcpt_to.len(), 1);
    let serialized = serde_json::to_string(&e).expect("serialize");
    assert!(
        serialized.contains("\"mailFrom\""),
        "mailFrom wire name must be correct"
    );
    assert!(
        serialized.contains("\"rcptTo\""),
        "rcptTo wire name must be correct"
    );
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn email_submission_minimal_roundtrip() {
    let json = common::fixture("email_submission_minimal.json");
    let es: EmailSubmission = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(es.undo_status, UndoStatus::Final);
    assert!(es.envelope.is_none());
    assert!(es.delivery_status.is_none());
    let serialized = serde_json::to_string(&es).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn delivery_status_roundtrip() {
    let json = r#"{"smtpReply":"250 OK","delivered":"yes","displayed":"unknown"}"#;
    let ds: DeliveryStatus = serde_json::from_str(json).expect("deserialize");
    assert_eq!(ds.smtp_reply, "250 OK");
    assert_eq!(ds.delivered, Delivered::Yes);
    assert_eq!(ds.displayed, Displayed::Unknown);
    let serialized = serde_json::to_string(&ds).expect("serialize");
    assert!(serialized.contains("\"smtpReply\""));
    assert!(serialized.contains("\"delivered\""));
    assert!(serialized.contains("\"displayed\""));
}

#[test]
fn submission_enum_display_matches_wire_names() {
    // Oracle: RFC 8621 §7 — enum values are lowercase wire strings.
    assert_eq!(Delivered::Yes.to_string(), "yes");
    assert_eq!(Delivered::No.to_string(), "no");
    assert_eq!(Delivered::Queued.to_string(), "queued");
    assert_eq!(Delivered::Unknown.to_string(), "unknown");
    assert_eq!(Displayed::Yes.to_string(), "yes");
    assert_eq!(Displayed::Unknown.to_string(), "unknown");
    assert_eq!(UndoStatus::Pending.to_string(), "pending");
    assert_eq!(UndoStatus::Final.to_string(), "final");
    assert_eq!(UndoStatus::Canceled.to_string(), "canceled");
    assert_eq!(UndoStatus::Other.to_string(), "other");
}

#[test]
fn submission_wire_field_names() {
    let json = common::fixture("email_submission_minimal.json");
    let es: EmailSubmission = serde_json::from_str(&json).expect("deserialize");
    let serialized = serde_json::to_string(&es).expect("serialize");
    assert!(serialized.contains("\"identityId\""));
    assert!(serialized.contains("\"emailId\""));
    assert!(serialized.contains("\"threadId\""));
    assert!(serialized.contains("\"sendAt\""));
    assert!(serialized.contains("\"undoStatus\""));
    assert!(serialized.contains("\"dsnBlobIds\""));
    assert!(serialized.contains("\"mdnBlobIds\""));
}

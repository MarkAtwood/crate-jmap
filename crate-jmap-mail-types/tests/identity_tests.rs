mod common;

use jmap_mail_types::Identity;

// Oracle: RFC 8621 §6.4 example response — first identity object.
#[test]
fn identity_round_trips_from_rfc_example() {
    let json = &common::fixture("identity_round_trips_from_rfc_example.json");
    let identity: Identity = serde_json::from_str(json).expect("deserialize");
    assert_eq!(identity.id.as_ref(), "XD-3301-222-11_22AAz");
    assert_eq!(identity.name, "Joe Bloggs");
    assert_eq!(identity.email, "joe@example.com");
    assert_eq!(identity.reply_to, None);
    let bcc = identity.bcc.as_ref().expect("bcc present");
    assert_eq!(bcc.len(), 1);
    assert_eq!(bcc[0].email, "joe+archive@example.com");
    assert_eq!(identity.text_signature, "-- \nJoe Bloggs\nMaster of Email");
    assert!(!identity.may_delete);
}

// Oracle: RFC 8621 §6.4 example response — second identity object.
#[test]
fn identity_round_trips_minimal_rfc_example() {
    let json = &common::fixture("identity_round_trips_minimal_rfc_example.json");
    let identity: Identity = serde_json::from_str(json).expect("deserialize");
    assert_eq!(identity.id.as_ref(), "XD-9911312-11_22AAz");
    assert_eq!(identity.reply_to, None);
    assert_eq!(identity.bcc, None);
    assert_eq!(identity.text_signature, "");
    assert_eq!(identity.html_signature, "");
    assert!(identity.may_delete);
}

// Oracle: textSignature and htmlSignature are always present in serialized output
// (RFC §6 — they have defined defaults but must appear in responses).
#[test]
fn identity_serializes_signatures_always() {
    let identity: Identity = serde_json::from_str(&common::fixture(
        "identity_serializes_signatures_always.json",
    ))
    .expect("deserialize");
    let json = serde_json::to_string(&identity).expect("serialize");
    assert!(
        json.contains("\"textSignature\":\"\""),
        "textSignature must be present"
    );
    assert!(
        json.contains("\"htmlSignature\":\"\""),
        "htmlSignature must be present"
    );
}

// Oracle: null replyTo and bcc are omitted from serialized output
// (skip_serializing_if = "Option::is_none").
#[test]
fn identity_null_optional_fields_serialize_as_null() {
    let identity: Identity = serde_json::from_str(&common::fixture(
        "identity_null_optional_fields_serialize_as_null.json",
    ))
    .expect("deserialize");
    // RFC 8621 §6: replyTo and bcc are "EmailAddress[]|null" (default:
    // null), so an unset value serializes as null.
    let v = serde_json::to_value(&identity).expect("serialize");
    assert_eq!(v["replyTo"], serde_json::Value::Null);
    assert_eq!(v["bcc"], serde_json::Value::Null);
    assert!(v.as_object().unwrap().contains_key("replyTo"));
    assert!(v.as_object().unwrap().contains_key("bcc"));
}

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn identity_full_roundtrip() {
    let json = common::fixture("identity_full.json");
    let id: Identity = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(id.name, "John Doe");
    assert_eq!(id.email, "john@example.com");
    assert!(id.reply_to.is_some());
    assert!(id.bcc.is_some());
    assert!(!id.may_delete);
    let serialized = serde_json::to_string(&id).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn identity_minimal_roundtrip() {
    let json = common::fixture("identity_minimal.json");
    let id: Identity = serde_json::from_str(&json).expect("deserialize");
    assert!(id.reply_to.is_none());
    assert!(id.bcc.is_none());
    let serialized = serde_json::to_string(&id).expect("serialize");
    // RFC 8621 §6: replyTo and bcc default to null and are serialized
    assert!(
        serialized.contains("\"replyTo\":null"),
        "replyTo must be null when None"
    );
    assert!(
        serialized.contains("\"bcc\":null"),
        "bcc must be null when None"
    );
    // textSignature and htmlSignature MUST appear even when empty
    assert!(
        serialized.contains("\"textSignature\""),
        "textSignature must always be present"
    );
    assert!(
        serialized.contains("\"htmlSignature\""),
        "htmlSignature must always be present"
    );
}

#[test]
fn identity_wire_field_names() {
    // Oracle: RFC 8621 §6 — check camelCase wire names
    let json = common::fixture("identity_full.json");
    let id: Identity = serde_json::from_str(&json).expect("deserialize");
    let serialized = serde_json::to_string(&id).expect("serialize");
    assert!(serialized.contains("\"replyTo\""));
    assert!(serialized.contains("\"textSignature\""));
    assert!(serialized.contains("\"htmlSignature\""));
    assert!(serialized.contains("\"mayDelete\""));
}

#[test]
fn identity_new_constructor() {
    use jmap_types::Id;
    let id = Identity::new(Id::from("I1"), "alice@example.com", false);
    assert_eq!(id.id, Id::from("I1"));
    assert_eq!(id.email, "alice@example.com");
    assert_eq!(id.name, "");
    assert_eq!(id.text_signature, "");
    assert!(id.reply_to.is_none());
    assert!(!id.may_delete);
}

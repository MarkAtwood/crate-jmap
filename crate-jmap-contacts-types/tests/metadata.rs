//! draft-ietf-jmap-metadata-04 properties and filter conditions on
//! AddressBook and ContactCard (bd JMAP-s4kp.5).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt), with
//! hand-written JSON. The namespace and keys follow the -04 §6.6 example
//! ("metadata/acme.example.com/approvalStatus").

use jmap_contacts_types::{AddressBook, ContactCard, ContactCardFilterCondition};
use jmap_metadata_types::MetadataTextMatch;
use serde_json::json;

/// -04 §2 adds `metadata` and `privateMetadata` (objects keyed by
/// namespace) to each listed type; -04 §4.1 names AddressBook [RFC9610]
/// among the types with a myRights property. The values land in the typed
/// fields, not `extra`. -04 §3.2: "null is equivalent to {}", so `null`
/// decodes as absent; a non-object is rejected.
#[test]
fn address_book_metadata_properties() {
    let mut raw = json!({
        "id": "ab1", "name": "Work", "description": null, "sortOrder": 0,
        "isDefault": false, "isSubscribed": true, "shareWith": null,
        "myRights": {
            "mayRead": true, "mayWrite": true, "mayShare": true,
            "mayDelete": true, "maySetMetadata": true
        },
        "metadata": {"acme.example.com": {"department": "sales"}},
        "privateMetadata": {"acme.example.com": {"pinned": true}}
    });
    let book: AddressBook = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        book.metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["department"]),
        Some(&json!("sales"))
    );
    assert_eq!(
        book.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["pinned"]),
        Some(&json!(true))
    );
    assert!(book.extra.is_empty());
    let back = serde_json::to_value(&book).expect("encode");
    assert_eq!(back["metadata"], raw["metadata"]);
    assert_eq!(back["privateMetadata"], raw["privateMetadata"]);

    raw["metadata"] = serde_json::Value::Null;
    let book: AddressBook = serde_json::from_value(raw.clone()).expect("decode null");
    assert_eq!(book.metadata, None);
    assert!(
        serde_json::to_value(&book)
            .expect("encode")
            .get("metadata")
            .is_none()
    );
    raw["metadata"] = json!(["x"]);
    assert!(serde_json::from_value::<AddressBook>(raw).is_err());
}

/// -04 §2 on ContactCard, which -04 §4.1 names as a type that "takes the
/// right from its containers". A non-object `privateMetadata` is rejected.
#[test]
fn contact_card_metadata_properties() {
    let mut raw = json!({
        "id": "c1", "addressBookIds": {"ab1": true}, "uid": "u1",
        "metadata": {"acme.example.com": {"approvalStatus": "pending"}},
        "privateMetadata": {"acme.example.com": {"reminder": "call back"}}
    });
    let card: ContactCard = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        card.metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["approvalStatus"]),
        Some(&json!("pending"))
    );
    assert_eq!(
        card.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["reminder"]),
        Some(&json!("call back"))
    );
    assert!(card.extra.is_empty());
    let back = serde_json::to_value(&card).expect("encode");
    assert_eq!(back["metadata"], raw["metadata"]);
    assert_eq!(back["privateMetadata"], raw["privateMetadata"]);

    raw["privateMetadata"] = json!(7);
    assert!(serde_json::from_value::<ContactCard>(raw).is_err());
}

/// `metadata` and `privateMetadata` are typed wire fields now, so an
/// `extra` key of either name would serialize as a duplicate JSON object
/// key (RFC 8259 §4); `validate_extras` reports it.
#[test]
fn metadata_extra_keys_collide() {
    for field in ["metadata", "privateMetadata"] {
        let mut card = ContactCard::default();
        card.extra.insert(field.into(), json!({}));
        let err = card.validate_extras().expect_err(field);
        assert_eq!(err.keys(), &[field.to_owned()]);

        let mut book: AddressBook = serde_json::from_value(json!({
            "id": "ab1", "name": "Work", "isDefault": false, "isSubscribed": true,
            "myRights": {
                "mayRead": true, "mayWrite": true, "mayShare": true, "mayDelete": true
            }
        }))
        .expect("decode");
        book.extra.insert(field.into(), json!({}));
        let err = book.validate_extras().expect_err(field);
        assert_eq!(err.keys(), &[field.to_owned()]);
    }
}

/// The six wire names of -04 §3.6 ("metadataExists",
/// "privateMetadataExists", "metadataTextContains",
/// "privateMetadataTextContains", "metadataTextEquals",
/// "privateMetadataTextEquals") extend "the FilterCondition object for
/// Foo/query". Each decodes into its typed field and re-encodes under the
/// same name; a misspelt one is still rejected by `deny_unknown_fields`.
#[test]
fn metadata_condition_names_decode_and_typos_reject() {
    let m = json!({"path": "acme.example.com/approvalStatus", "value": "pending"});
    for (name, value) in [
        ("metadataExists", json!("acme.example.com")),
        ("privateMetadataExists", json!("acme.example.com")),
        ("metadataTextContains", m.clone()),
        ("privateMetadataTextContains", m.clone()),
        ("metadataTextEquals", m.clone()),
        ("privateMetadataTextEquals", m.clone()),
    ] {
        let wire = json!({ name: value });
        let c: ContactCardFilterCondition = serde_json::from_value(wire.clone()).expect(name);
        assert!(c.has_metadata_condition(), "{name}");
        assert_eq!(serde_json::to_value(&c).expect("encode"), wire, "{name}");
    }
    assert!(!ContactCardFilterCondition::default().has_metadata_condition());
    assert!(
        serde_json::from_value::<ContactCardFilterCondition>(json!({"metadataExist": "ns"}))
            .is_err()
    );
}

/// -04 §3.6: "A MetadataTextMatch object has the following fields:"
/// `path` and `value`. A leaf combining the RFC 9610 §3.3.1
/// `inAddressBook` with `metadataTextEquals` decodes both.
#[test]
fn metadata_text_condition_shape() {
    let c: ContactCardFilterCondition = serde_json::from_value(json!({
        "inAddressBook": "ab1",
        "metadataTextEquals": { "path": "acme.example.com/approvalStatus", "value": "pending" }
    }))
    .expect("decode");
    assert_eq!(c.in_address_book.as_ref().map(AsRef::as_ref), Some("ab1"));
    assert_eq!(
        c.metadata_text_equals,
        Some(MetadataTextMatch::new(
            "acme.example.com/approvalStatus",
            "pending"
        ))
    );
}

//! draft-ietf-jmap-metadata-04 properties and filter conditions on
//! Principal (bd JMAP-s4kp.7).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt), with
//! hand-written JSON; the Principal shape is the RFC 9670 §2 example.

use jmap_metadata_types::MetadataTextMatch;
use jmap_sharing_types::{Principal, PrincipalFilterCondition};
use serde_json::json;

/// -04 §2 adds `metadata` and `privateMetadata` (objects keyed by
/// namespace) to each listed type. The values land in the typed fields,
/// not `extra`, and round-trip. -04 §3.2: "null is equivalent to {}", so
/// `null` decodes as absent and is not re-emitted; a non-object is
/// rejected.
#[test]
fn principal_metadata_properties() {
    let mut raw = json!({
        "id": "P2342fnddd20", "type": "individual", "name": "Joe Bloggs",
        "description": null, "email": "joe@example.com",
        "timeZone": "America/New_York", "capabilities": {}, "accounts": null,
        "metadata": {"acme.example.com": {"badge": "B-17", "floor": 4}},
        "privateMetadata": {"acme.example.com": {"pinned": true}}
    });
    let p: Principal = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        p.metadata.as_ref().map(|m| &m["acme.example.com"]["badge"]),
        Some(&json!("B-17"))
    );
    assert_eq!(
        p.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["pinned"]),
        Some(&json!(true))
    );
    assert!(p.extra.is_empty());
    assert_eq!(serde_json::to_value(&p).expect("encode"), raw);

    raw["metadata"] = serde_json::Value::Null;
    let p: Principal = serde_json::from_value(raw.clone()).expect("decode null");
    assert_eq!(p.metadata, None);
    assert!(
        serde_json::to_value(&p)
            .expect("encode")
            .get("metadata")
            .is_none()
    );
    raw["privateMetadata"] = json!(7);
    assert!(serde_json::from_value::<Principal>(raw).is_err());
}

/// The six wire names of -04 §3.6 ("metadataExists",
/// "privateMetadataExists", "metadataTextContains",
/// "privateMetadataTextContains", "metadataTextEquals",
/// "privateMetadataTextEquals") extend "the FilterCondition object for
/// Foo/query". Each decodes into its typed field and re-encodes under the
/// same name; a misspelt one is still rejected by `deny_unknown_fields`.
#[test]
fn metadata_condition_names_decode_and_typos_reject() {
    let m = json!({"path": "acme.example.com/badge", "value": "B-17"});
    for (name, value) in [
        ("metadataExists", json!("acme.example.com")),
        ("privateMetadataExists", json!("acme.example.com")),
        ("metadataTextContains", m.clone()),
        ("privateMetadataTextContains", m.clone()),
        ("metadataTextEquals", m.clone()),
        ("privateMetadataTextEquals", m.clone()),
    ] {
        let wire = json!({ name: value });
        let c: PrincipalFilterCondition = serde_json::from_value(wire.clone()).expect(name);
        assert!(c.has_metadata_condition(), "{name}");
        assert_eq!(serde_json::to_value(&c).expect("encode"), wire, "{name}");
    }
    assert!(!PrincipalFilterCondition::default().has_metadata_condition());
    assert!(
        serde_json::from_value::<PrincipalFilterCondition>(json!({"metadataExist": "ns"})).is_err()
    );
}

/// -04 §3.6: "A MetadataTextMatch object has the following fields:"
/// `path` and `value`. A leaf combining the RFC 9670 §2.4.1 `type`
/// condition with `metadataTextEquals` decodes both.
#[test]
fn metadata_text_condition_shape() {
    let c: PrincipalFilterCondition = serde_json::from_value(json!({
        "type": "individual",
        "metadataTextEquals": { "path": "acme.example.com/badge", "value": "B-17" }
    }))
    .expect("decode");
    assert!(c.type_.is_some());
    assert_eq!(
        c.metadata_text_equals,
        Some(MetadataTextMatch::new("acme.example.com/badge", "B-17"))
    );
}

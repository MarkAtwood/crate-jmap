//! draft-ietf-jmap-metadata-04 properties and filter conditions on
//! FileNode (bd JMAP-s4kp.6).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt), with
//! hand-written JSON. The photography values follow the -04 §6.5 example
//! ("Attaching Photography Metadata to a FileNode").

use jmap_filenode_types::{FileNode, FileNodeFilterCondition};
use jmap_metadata_types::MetadataTextMatch;
use serde_json::json;

/// -04 §2 adds `metadata` and `privateMetadata` (objects keyed by
/// namespace) to each listed type; -04 §6.5 lists FileNode and sets
/// `metadata/photography`. The values land in the typed fields, not
/// `extra`, and round-trip. -04 §3.2: "null is equivalent to {}", so
/// `null` decodes as absent and is not re-emitted; a non-object is
/// rejected.
#[test]
fn file_node_metadata_properties() {
    let mut raw = json!({
        "id": "f1", "parentId": null, "@type": "File", "name": "IMG_0001.jpg",
        "blobId": "b1", "mediaType": "image/jpeg", "executable": false,
        "isSubscribed": true, "shareWith": null,
        "metadata": {"photography": {"cameraMake": "Canon", "iso": 400}},
        "privateMetadata": {"acme.example.com": {"pinned": true}}
    });
    let node: FileNode = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        node.metadata
            .as_ref()
            .map(|m| &m["photography"]["cameraMake"]),
        Some(&json!("Canon"))
    );
    assert_eq!(
        node.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["pinned"]),
        Some(&json!(true))
    );
    assert!(node.extra.is_empty());
    assert_eq!(serde_json::to_value(&node).expect("encode"), raw);

    raw["metadata"] = serde_json::Value::Null;
    let node: FileNode = serde_json::from_value(raw.clone()).expect("decode null");
    assert_eq!(node.metadata, None);
    assert!(
        serde_json::to_value(&node)
            .expect("encode")
            .get("metadata")
            .is_none()
    );
    raw["privateMetadata"] = json!(7);
    assert!(serde_json::from_value::<FileNode>(raw).is_err());
}

/// The six wire names of -04 §3.6 ("metadataExists",
/// "privateMetadataExists", "metadataTextContains",
/// "privateMetadataTextContains", "metadataTextEquals",
/// "privateMetadataTextEquals") extend "the FilterCondition object for
/// Foo/query". Each decodes into its typed field and re-encodes under the
/// same name; a misspelt one is still rejected by `deny_unknown_fields`.
#[test]
fn metadata_condition_names_decode_and_typos_reject() {
    let m = json!({"path": "photography/cameraMake", "value": "Canon"});
    for (name, value) in [
        ("metadataExists", json!("photography")),
        ("privateMetadataExists", json!("photography")),
        ("metadataTextContains", m.clone()),
        ("privateMetadataTextContains", m.clone()),
        ("metadataTextEquals", m.clone()),
        ("privateMetadataTextEquals", m.clone()),
    ] {
        let wire = json!({ name: value });
        let c: FileNodeFilterCondition = serde_json::from_value(wire.clone()).expect(name);
        assert!(c.has_metadata_condition(), "{name}");
        assert_eq!(serde_json::to_value(&c).expect("encode"), wire, "{name}");
    }
    assert!(!FileNodeFilterCondition::default().has_metadata_condition());
    assert!(
        serde_json::from_value::<FileNodeFilterCondition>(json!({"metadataExist": "ns"})).is_err()
    );
}

/// -04 §3.6: "A MetadataTextMatch object has the following fields:"
/// `path` and `value`. A leaf combining the draft-ietf-jmap-filenode
/// `nodeType` with `metadataTextEquals` decodes both.
#[test]
fn metadata_text_condition_shape() {
    let c: FileNodeFilterCondition = serde_json::from_value(json!({
        "nodeType": "File",
        "metadataTextEquals": { "path": "photography/cameraMake", "value": "Canon" }
    }))
    .expect("decode");
    assert!(c.node_type.is_some());
    assert_eq!(
        c.metadata_text_equals,
        Some(MetadataTextMatch::new("photography/cameraMake", "Canon"))
    );
}

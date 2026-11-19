//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-metadata-05 (the capability URI and the §6.1
//! capability example entry) and the crate's documented feature table.

use jmap_metadata::types::{DataTypeMetadataInfo, JMAP_METADATA_URI};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_METADATA_URI, "urn:ietf:params:jmap:metadata");
    let info: DataTypeMetadataInfo = serde_json::from_str(
        r#"{"namespaces": ["photography"],
        "supportsVendorNamespaces": false, "supportsPrivate": false, "maxDepth": 3}"#,
    )
    .unwrap();
    assert_eq!(info.max_depth, Some(3));
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // draft-ietf-jmap-metadata-05 §2: the two property names.
    assert_eq!(jmap_metadata::client::METADATA_PROPERTY, "metadata");
    assert_eq!(
        jmap_metadata::client::PRIVATE_METADATA_PROPERTY,
        "privateMetadata"
    );
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    use serde_json::json;
    // draft-ietf-jmap-metadata-05 §2.1: each nested object adds one level.
    assert_eq!(jmap_metadata::server::value_depth(&json!({})), 1);
    assert_eq!(
        jmap_metadata::server::value_depth(&json!({"a": {"b": 1}})),
        2
    );
}

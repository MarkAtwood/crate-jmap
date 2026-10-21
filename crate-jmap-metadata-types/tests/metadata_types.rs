//! Integration tests for jmap-metadata-types.
//!
//! All JSON fixtures are hand-written from draft-ietf-jmap-metadata-04 or
//! constructed directly from the spec field descriptions. No expected
//! value is derived from the code under test.
//!
//! Test name conventions:
//! - `*_draft_02_*` — pinned to a draft §N example, so a future spec
//!   revision can revise or replace the test alongside the wire-format
//!   change. The names date from -02; every pinned example is unchanged
//!   in -04.

use jmap_metadata_types::{
    is_registered_namespace, is_valid_namespace, is_vendor_namespace, DataTypeMetadataInfo,
    MetadataTextMatch, WithMetadata, JMAP_METADATA_URI,
};

// ---------------------------------------------------------------------------
// Capability URI
// ---------------------------------------------------------------------------

#[test]
fn capability_uri_matches_draft_02() {
    // Oracle: draft-ietf-jmap-metadata-04 §1.2.1, unchanged since -01.
    assert_eq!(JMAP_METADATA_URI, "urn:ietf:params:jmap:metadata");
}

// ---------------------------------------------------------------------------
// DataTypeMetadataInfo (§1.2.1)
// ---------------------------------------------------------------------------

#[test]
fn data_type_metadata_info_draft_02_section_6_1() {
    // Oracle: §6.1 example — the FileNode entry (registered namespace only).
    let json = r#"{
        "namespaces": ["photography"],
        "supportsVendorNamespaces": false,
        "supportsPrivate": false,
        "maxDepth": 3
    }"#;
    let info: DataTypeMetadataInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.namespaces, vec!["photography"]);
    assert!(!info.supports_vendor_namespaces);
    assert!(!info.supports_private);
    assert_eq!(info.max_depth, Some(3));
}

#[test]
fn data_type_metadata_info_draft_02_full_support() {
    // Oracle: hand-written from §1.2.1 field descriptions.
    // Vendor namespaces allowed, private metadata allowed, no depth limit.
    let json = r#"{
        "namespaces": ["photography", "workflow"],
        "supportsVendorNamespaces": true,
        "supportsPrivate": true,
        "maxDepth": null
    }"#;
    let info: DataTypeMetadataInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.namespaces, vec!["photography", "workflow"]);
    assert!(info.supports_vendor_namespaces);
    assert!(info.supports_private);
    assert_eq!(info.max_depth, None);
}

#[test]
fn data_type_metadata_info_defaults() {
    // Oracle: §1.2.1 says namespaces defaults to [], booleans default
    // to false. An empty object should deserialize with all defaults.
    let json = r#"{ "maxDepth": null }"#;
    let info: DataTypeMetadataInfo = serde_json::from_str(json).unwrap();
    assert!(info.namespaces.is_empty());
    assert!(!info.supports_vendor_namespaces);
    assert!(!info.supports_private);
    assert_eq!(info.max_depth, None);
}

#[test]
fn data_type_metadata_info_round_trip() {
    let json = r#"{
        "namespaces": ["photography"],
        "supportsVendorNamespaces": true,
        "supportsPrivate": false,
        "maxDepth": 5
    }"#;
    let info: DataTypeMetadataInfo = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_value(&info).unwrap();
    let reparsed: DataTypeMetadataInfo = serde_json::from_value(serialized).unwrap();
    assert_eq!(info, reparsed);
}

#[test]
fn data_type_metadata_info_max_depth_null_round_trips() {
    // maxDepth: null means no server-enforced limit.
    let json = r#"{
        "namespaces": [],
        "supportsVendorNamespaces": false,
        "supportsPrivate": false,
        "maxDepth": null
    }"#;
    let info: DataTypeMetadataInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.max_depth, None);
    let reser = serde_json::to_value(&info).unwrap();
    assert!(reser["maxDepth"].is_null());
}

// ---------------------------------------------------------------------------
// MetadataTextMatch (§3.6)
// ---------------------------------------------------------------------------

#[test]
fn metadata_text_match_draft_02_section_6_7() {
    // Oracle: §6.7 example — filtering by text match.
    let json = r#"{
        "path": "acme.example.com/memo",
        "value": "follow up"
    }"#;
    let m: MetadataTextMatch = serde_json::from_str(json).unwrap();
    assert_eq!(m.path, "acme.example.com/memo");
    assert_eq!(m.value, "follow up");
}

#[test]
fn metadata_text_match_round_trip() {
    let json = r#"{"path": "photography/album", "value": "sunset"}"#;
    let m: MetadataTextMatch = serde_json::from_str(json).unwrap();
    let reser = serde_json::to_value(&m).unwrap();
    let reparsed: MetadataTextMatch = serde_json::from_value(reser).unwrap();
    assert_eq!(m, reparsed);
}

#[test]
fn metadata_text_match_namespace_only_path() {
    // §3.6: path can be just a namespace (no key suffix).
    let json = r#"{"path": "photography", "value": "beach"}"#;
    let m: MetadataTextMatch = serde_json::from_str(json).unwrap();
    assert_eq!(m.path, "photography");
    assert_eq!(m.value, "beach");
}

// ---------------------------------------------------------------------------
// Namespace validation (§2.1)
// ---------------------------------------------------------------------------

#[test]
fn registered_namespace_accepts_valid() {
    assert!(is_registered_namespace("photography"));
    assert!(is_registered_namespace("my-namespace_v2"));
    assert!(is_registered_namespace("a"));
    assert!(is_registered_namespace("a1-b2_c3"));
}

#[test]
fn registered_namespace_rejects_invalid() {
    assert!(!is_registered_namespace(""));
    assert!(!is_registered_namespace("has.dot"));
    assert!(!is_registered_namespace("has space"));
    assert!(!is_registered_namespace("foo@bar"));
    assert!(!is_registered_namespace("A1-b2_c3"));
}

#[test]
fn vendor_namespace_accepts_valid() {
    assert!(is_vendor_namespace("acme.example.com"));
    assert!(is_vendor_namespace("a.b"));
}

#[test]
fn vendor_namespace_rejects_invalid() {
    assert!(!is_vendor_namespace("nodot"));
    assert!(!is_vendor_namespace(""));
    assert!(!is_vendor_namespace(".leading"));
    assert!(!is_vendor_namespace("trailing."));
    assert!(!is_vendor_namespace("a..b"));
    assert!(!is_vendor_namespace("a. b"));
    assert!(!is_vendor_namespace("Acme.example.com"));
}

#[test]
fn valid_namespace_accepts_both_kinds() {
    assert!(is_valid_namespace("photography"));
    assert!(is_valid_namespace("acme.example.com"));
}

#[test]
fn valid_namespace_rejects_invalid() {
    assert!(!is_valid_namespace(""));
    assert!(!is_valid_namespace("has space"));
    assert!(!is_valid_namespace(".leading"));
}

// ---------------------------------------------------------------------------
// WithMetadata (§3.6)
// ---------------------------------------------------------------------------

/// Stand-in for a per-type `FilterCondition` (`inMailbox` is the RFC 8621
/// §4.4.1 Email condition used in the §6.7 example).
#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EmailCondStub {
    #[serde(skip_serializing_if = "Option::is_none")]
    in_mailbox: Option<String>,
}

#[test]
fn with_metadata_draft_02_section_3_5_all_fields() {
    // Oracle: the six field names and value types listed in §3.6 ("The
    // extended FilterCondition fields are: metadataExists: String,
    // privateMetadataExists: String, metadataTextContains:
    // MetadataTextMatch, ..."), written by hand next to a base condition
    // key in the same object.
    let mut cond = WithMetadata::<EmailCondStub>::default();
    cond.base.in_mailbox = Some("MB-inbox".into());
    cond.metadata_exists = Some("photography".into());
    cond.private_metadata_exists = Some("acme.example.com/memo".into());
    cond.metadata_text_contains = Some(MetadataTextMatch::new("photography/album", "sun"));
    cond.private_metadata_text_contains =
        Some(MetadataTextMatch::new("acme.example.com/memo", "follow up"));
    cond.metadata_text_equals = Some(MetadataTextMatch::new("photography/album", "Sunset"));
    cond.private_metadata_text_equals = Some(MetadataTextMatch::new("acme.example.com/tag", "x"));
    let expected = serde_json::json!({
        "inMailbox": "MB-inbox",
        "metadataExists": "photography",
        "privateMetadataExists": "acme.example.com/memo",
        "metadataTextContains": {"path": "photography/album", "value": "sun"},
        "privateMetadataTextContains": {"path": "acme.example.com/memo", "value": "follow up"},
        "metadataTextEquals": {"path": "photography/album", "value": "Sunset"},
        "privateMetadataTextEquals": {"path": "acme.example.com/tag", "value": "x"}
    });
    assert_eq!(serde_json::to_value(&cond).unwrap(), expected);
}

#[test]
fn with_metadata_unset_fields_are_omitted() {
    // Oracle: §3.6 calls every added field "optional"; a condition with
    // only `metadataExists` set sends only that key (RFC 8620 §5.5: a
    // FilterCondition property that is absent places no constraint).
    let mut cond = WithMetadata::<EmailCondStub>::default();
    cond.metadata_exists = Some("photography".into());
    assert_eq!(
        serde_json::to_value(&cond).unwrap(),
        serde_json::json!({"metadataExists": "photography"})
    );
}

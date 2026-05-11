//! Integration tests for jmap-metadata-types.
//!
//! All JSON fixtures are hand-written from draft-ietf-jmap-metadata-01 or
//! constructed directly from the spec field descriptions. No expected
//! value is derived from the code under test.
//!
//! Test name conventions:
//! - `*_draft_01_*` — pinned to draft-ietf-jmap-metadata-01 §N example so a
//!   future spec revision can revise or replace the test alongside the
//!   wire-format change. All current tests are pinned to -01.
//! - `*_preserves_vendor_extras` / `*_round_trip_*` — workspace
//!   extras-preservation policy round-trip coverage (bd JMAP-lbdy).
//!
//! Structs are constructed exclusively via serde_json deserialization
//! because all public structs carry `#[non_exhaustive]`, which prevents
//! struct-literal construction outside the defining crate.

use jmap_metadata_types::{
    Annotation, ImapMetadata, Metadata, MetadataCapability, MetadataFilterCondition,
    MetadataProperty, WebDavMetadata, JMAP_METADATA_URI,
};
use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Capability URI / capability object
// ---------------------------------------------------------------------------

#[test]
fn capability_uri_matches_draft_01() {
    // Oracle: draft-ietf-jmap-metadata-01 §1.2.1, IANA registration §9.1.
    assert_eq!(JMAP_METADATA_URI, "urn:ietf:params:jmap:metadata");
}

#[test]
fn metadata_capability_draft_01_round_trip() {
    // Oracle: hand-written from §1.2.1 property descriptions.
    // dataTypes=null → "all data types"; maxDepth=null → "no nesting limit";
    // metadataTypes lists explicit @type values; maySetPrivate=true.
    let json = r#"{
        "dataTypes": null,
        "metadataTypes": ["Annotation", "ImapMetadata", "WebDavMetadata"],
        "maxDepth": null,
        "maySetPrivate": true
    }"#;
    let cap: MetadataCapability = serde_json::from_str(json).unwrap();
    assert_eq!(cap.data_types, None);
    assert_eq!(
        cap.metadata_types,
        vec![
            "Annotation".to_owned(),
            "ImapMetadata".to_owned(),
            "WebDavMetadata".to_owned(),
        ]
    );
    assert_eq!(cap.max_depth, None);
    assert_eq!(cap.may_set_private, Some(true));

    // Round-trip: required-and-nullable fields stay present as null.
    let back = serde_json::to_value(&cap).unwrap();
    let map = back.as_object().unwrap();
    assert!(map.contains_key("dataTypes"));
    assert!(map["dataTypes"].is_null());
    assert!(map.contains_key("maxDepth"));
    assert!(map["maxDepth"].is_null());
}

#[test]
fn metadata_capability_draft_01_scoped_types() {
    // Oracle: hand-written from §1.2.1.
    // dataTypes restricted to specific JMAP types; maxDepth=3 limits nesting.
    let json = r#"{
        "dataTypes": ["Email", "Mailbox", "ContactCard"],
        "metadataTypes": ["Annotation"],
        "maxDepth": 3,
        "maySetPrivate": false
    }"#;
    let cap: MetadataCapability = serde_json::from_str(json).unwrap();
    assert_eq!(
        cap.data_types,
        Some(vec![
            "Email".to_owned(),
            "Mailbox".to_owned(),
            "ContactCard".to_owned(),
        ])
    );
    assert_eq!(cap.max_depth, Some(3));
    assert_eq!(cap.may_set_private, Some(false));
}

#[test]
fn metadata_capability_may_set_private_absent_is_none() {
    // Oracle: §1.2.1 says default is true when absent.
    // The Rust type preserves "absent" as None so callers can detect it.
    let json = r#"{
        "dataTypes": null,
        "metadataTypes": ["Annotation"],
        "maxDepth": null
    }"#;
    let cap: MetadataCapability = serde_json::from_str(json).unwrap();
    assert_eq!(cap.may_set_private, None);
}

// ---------------------------------------------------------------------------
// Annotation
// ---------------------------------------------------------------------------

#[test]
fn annotation_draft_01_section_7_1_create_with_vendor_props() {
    // Oracle: §7.1 example, "Creating a Mailbox with Annotation".
    // Wire-bytes copied verbatim from the draft (whitespace normalised).
    let json = r##"{
        "@type": "Annotation",
        "relatedType": "Mailbox",
        "relatedId": "#new-mailbox",
        "isPrivate": true,
        "acme.example.com:color": "blue",
        "acme.example.com:priority": "high",
        "acme.example.com:project": {
            "@type": "acme.example.com:ProjectInfo",
            "projectId": "ALPHA-2024",
            "deadline": "2024-12-31",
            "team": "Engineering"
        }
    }"##;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let ann = match &meta {
        Metadata::Annotation(a) => a,
        _ => panic!("expected Annotation variant; got {}", meta.type_name()),
    };
    assert_eq!(ann.id, None);
    assert_eq!(ann.related_type, "Mailbox");
    // relatedId on create may carry a "#" creation reference; the wire
    // type is still Id (string), so the JMAP Id newtype accepts it.
    assert!(ann.related_id == *"#new-mailbox");
    assert_eq!(ann.is_private, Some(true));

    // Vendor-extension properties captured in `extra`.
    assert_eq!(
        ann.extra.get("acme.example.com:color"),
        Some(&serde_json::Value::String("blue".into())),
    );
    assert_eq!(
        ann.extra.get("acme.example.com:priority"),
        Some(&serde_json::Value::String("high".into())),
    );
    // Nested vendor object is preserved verbatim (the typed @type tag is
    // only consumed at the OUTER Metadata level; nested objects flow
    // through `extra` as-is).
    let project = ann.extra.get("acme.example.com:project").unwrap();
    assert_eq!(
        project["@type"],
        serde_json::Value::String("acme.example.com:ProjectInfo".into()),
    );
    assert_eq!(
        project["projectId"],
        serde_json::Value::String("ALPHA-2024".into()),
    );
}

#[test]
fn annotation_draft_01_section_7_2_partial_response_requires_related_type() {
    // Oracle: §7.2 example response demonstrates `metadataProperties`
    // filtering — the response omits `relatedType` because the
    // request did not list it. The `Annotation` struct treats
    // `relatedType` as mandatory per §2.2.1.3 ("Type: String
    // (mandatory)"), so the partial response cannot deserialise into
    // a `Metadata::Annotation` directly. This test pins that
    // behaviour: when the field is missing, deserialise MUST fail.
    //
    // Clients that issue queries with `metadataProperties` and need
    // to consume partial responses must use `serde_json::Value` or
    // a custom partial-Annotation struct — not this crate's
    // spec-faithful type.
    let json = r#"{
        "relatedId": "MB789",
        "@type": "Annotation",
        "acme.example.com:color": "blue",
        "acme.example.com:priority": "high"
    }"#;
    let r: Result<Metadata, _> = serde_json::from_str(json);
    assert!(
        r.is_err(),
        "partial §7.2 response missing relatedType must fail to deserialise into the spec-faithful type"
    );
    let err = r.unwrap_err().to_string();
    assert!(
        err.contains("relatedType"),
        "error should mention the missing field; got: {err}"
    );
}

#[test]
fn annotation_draft_01_section_7_5_atomic_create_response() {
    // Oracle: §7.5 successful response, Metadata/set "created" entry.
    let json = r#"{
        "id": "MD789",
        "@type": "Annotation",
        "relatedType": "Email",
        "relatedId": "EM456"
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let ann = match meta {
        Metadata::Annotation(a) => a,
        _ => panic!("expected Annotation variant"),
    };
    assert_eq!(ann.id.as_ref().map(AsRef::as_ref), Some("MD789"));
    assert_eq!(ann.related_type, "Email");
    assert!(ann.related_id == *"EM456");
    assert_eq!(ann.is_private, None);
    assert!(ann.extra.is_empty());
}

#[test]
fn annotation_round_trip_preserves_vendor_extras() {
    // Workspace extras-preservation policy: every public Deserialize
    // struct round-trips unknown fields losslessly.
    let original = r#"{
        "@type": "Annotation",
        "id": "MD42",
        "relatedType": "Email",
        "relatedId": "EM1",
        "isPrivate": false,
        "acme.example.com:workflowState": "approved",
        "acme.example.com:reviewer": "alice@example.com"
    }"#;

    let meta: Metadata = serde_json::from_str(original).unwrap();
    let round_tripped = serde_json::to_value(&meta).unwrap();
    let reparsed: Metadata = serde_json::from_value(round_tripped.clone()).unwrap();

    assert_eq!(meta, reparsed);

    // Independent oracle: the vendor keys survived as-is and the @type
    // tag is present at the outer level.
    let map = round_tripped.as_object().unwrap();
    assert_eq!(map["@type"], serde_json::Value::String("Annotation".into()));
    assert_eq!(
        map["acme.example.com:workflowState"],
        serde_json::Value::String("approved".into()),
    );
    assert_eq!(
        map["acme.example.com:reviewer"],
        serde_json::Value::String("alice@example.com".into()),
    );
}

#[test]
fn annotation_empty_extras_omitted_on_serialize() {
    // Workspace extras-preservation policy: empty Map serialises to
    // nothing (skip_serializing_if), keeping wire-bytes identical to
    // a spec example that has no vendor extras.
    let json = r#"{
        "@type": "Annotation",
        "id": "MD1",
        "relatedType": "Email",
        "relatedId": "EM1"
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let out = serde_json::to_value(&meta).unwrap();
    let map = out.as_object().unwrap();
    assert!(map.contains_key("@type"));
    assert!(map.contains_key("id"));
    assert!(map.contains_key("relatedType"));
    assert!(map.contains_key("relatedId"));
    // isPrivate was absent on input and is skip_serializing_if=is_none
    // on output.
    assert!(!map.contains_key("isPrivate"));
    // Map has exactly four keys: no stray "extra" object.
    assert_eq!(map.len(), 4);
}

// ---------------------------------------------------------------------------
// ImapMetadata
// ---------------------------------------------------------------------------

#[test]
fn imap_metadata_draft_01_section_2_2_2_private_namespace() {
    // Oracle: hand-written from §2.2.2.1 worked example.
    // isPrivate=true → keys map to /private/<key>.
    let json = r#"{
        "@type": "ImapMetadata",
        "id": "MD100",
        "relatedType": "Mailbox",
        "relatedId": "MB1",
        "isPrivate": true,
        "metadata": {
            "comment": "My notes",
            "vendor/acme.example/color": "blue"
        }
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let imap = match meta {
        Metadata::ImapMetadata(i) => i,
        _ => panic!("expected ImapMetadata variant"),
    };
    assert_eq!(imap.id.as_ref().map(AsRef::as_ref), Some("MD100"));
    assert_eq!(imap.related_type, "Mailbox");
    assert_eq!(imap.is_private, Some(true));
    assert_eq!(imap.metadata.get("comment"), Some(&"My notes".to_owned()));
    assert_eq!(
        imap.metadata.get("vendor/acme.example/color"),
        Some(&"blue".to_owned()),
    );
}

#[test]
fn imap_metadata_draft_01_shared_namespace() {
    // Oracle: hand-written from §2.2.2.1 second bullet.
    // isPrivate=false (or omitted) → keys map to /shared/<key>.
    let json = r#"{
        "@type": "ImapMetadata",
        "id": "MD101",
        "relatedType": "Mailbox",
        "relatedId": "MB1",
        "isPrivate": false,
        "metadata": {
            "comment": "Team mailbox"
        }
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let imap = match meta {
        Metadata::ImapMetadata(i) => i,
        _ => panic!("expected ImapMetadata variant"),
    };
    assert_eq!(imap.is_private, Some(false));
    assert_eq!(
        imap.metadata.get("comment"),
        Some(&"Team mailbox".to_owned()),
    );
}

#[test]
fn imap_metadata_empty_string_value_round_trips() {
    // Oracle: §2.2.2.1 explicitly permits empty-string values
    // ("Empty string values are permitted and represent IMAP metadata
    // entries that exist but have no value").
    let json = r#"{
        "@type": "ImapMetadata",
        "id": "MD200",
        "relatedType": "Mailbox",
        "relatedId": "MB1",
        "isPrivate": false,
        "metadata": {
            "comment": ""
        }
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let imap = match &meta {
        Metadata::ImapMetadata(i) => i,
        _ => panic!("expected ImapMetadata"),
    };
    assert_eq!(imap.metadata.get("comment"), Some(&"".to_owned()));

    // Round-trip preserves the empty-string entry.
    let reser = serde_json::to_value(&meta).unwrap();
    assert_eq!(
        reser["metadata"]["comment"],
        serde_json::Value::String("".into())
    );
}

#[test]
fn imap_metadata_preserves_vendor_extras() {
    // Workspace extras-preservation policy.
    let json = r#"{
        "@type": "ImapMetadata",
        "id": "MD300",
        "relatedType": "Mailbox",
        "relatedId": "MB1",
        "isPrivate": false,
        "metadata": { "comment": "shared" },
        "x-vendor-custom": "preserved"
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let imap = match meta {
        Metadata::ImapMetadata(i) => i,
        _ => panic!("expected ImapMetadata"),
    };
    assert_eq!(
        imap.extra.get("x-vendor-custom"),
        Some(&serde_json::Value::String("preserved".into())),
    );
}

// ---------------------------------------------------------------------------
// WebDavMetadata
// ---------------------------------------------------------------------------

#[test]
fn webdav_metadata_draft_01_section_2_2_3_expanded_name_keys() {
    // Oracle: hand-written from §2.2.3.1 worked example.
    let json = r#"{
        "@type": "WebDavMetadata",
        "id": "MD500",
        "relatedType": "FileNode",
        "relatedId": "F1",
        "isPrivate": false,
        "metadata": {
            "{http://example.com/ns}priority": "high",
            "{http://example.com/ns}reviewedBy": "alice@example.com",
            "{DAV:}displayname": "Project Documents",
            "{http://example.com/ns}complexdata": "<item><name>Test</name><value>123</value></item>"
        }
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let webdav = match meta {
        Metadata::WebDavMetadata(w) => w,
        _ => panic!("expected WebDavMetadata variant"),
    };
    assert_eq!(webdav.related_type, "FileNode");
    assert_eq!(
        webdav.metadata.get("{http://example.com/ns}priority"),
        Some(&"high".to_owned()),
    );
    assert_eq!(
        webdav.metadata.get("{DAV:}displayname"),
        Some(&"Project Documents".to_owned()),
    );
    // XML content survives verbatim.
    assert_eq!(
        webdav.metadata.get("{http://example.com/ns}complexdata"),
        Some(&"<item><name>Test</name><value>123</value></item>".to_owned()),
    );
}

#[test]
fn webdav_metadata_round_trip_preserves_keys_and_values() {
    let json = r#"{
        "@type": "WebDavMetadata",
        "id": "MD501",
        "relatedType": "Calendar",
        "relatedId": "CAL1",
        "isPrivate": false,
        "metadata": {
            "{DAV:}displayname": "Personal"
        }
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let reser: Metadata = serde_json::from_value(serde_json::to_value(&meta).unwrap()).unwrap();
    assert_eq!(meta, reser);
}

// ---------------------------------------------------------------------------
// Metadata enum dispatch
// ---------------------------------------------------------------------------

#[test]
fn metadata_tag_dispatches_on_at_type() {
    let cases: &[(&str, &str)] = &[
        (
            r#"{"@type":"Annotation","relatedType":"Email","relatedId":"EM1"}"#,
            "Annotation",
        ),
        (
            r#"{"@type":"ImapMetadata","relatedType":"Mailbox","relatedId":"MB1"}"#,
            "ImapMetadata",
        ),
        (
            r#"{"@type":"WebDavMetadata","relatedType":"FileNode","relatedId":"F1"}"#,
            "WebDavMetadata",
        ),
    ];
    for (json, expected) in cases {
        let meta: Metadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.type_name(), *expected, "failed for {json}");
    }
}

#[test]
fn metadata_unknown_tag_fails_to_deserialize() {
    // Future spec revisions may introduce new metadata @type values
    // (§2.1). Until this crate adds a new enum variant, an unknown
    // tag MUST fail to deserialize — silent-acceptance would lose the
    // tag string and prevent variant dispatch.
    let json = r#"{"@type":"FutureMetadataType","relatedType":"Email","relatedId":"EM1"}"#;
    let r: Result<Metadata, _> = serde_json::from_str(json);
    assert!(r.is_err(), "unknown @type tag must fail to deserialize");
}

#[test]
fn metadata_missing_tag_fails_to_deserialize() {
    // The @type discriminator is mandatory per §2.2.1.1.
    let json = r#"{"relatedType":"Email","relatedId":"EM1"}"#;
    let r: Result<Metadata, _> = serde_json::from_str(json);
    assert!(r.is_err(), "missing @type tag must fail to deserialize");
}

#[test]
fn metadata_common_accessors() {
    // Sanity-check the convenience accessors on the enum.
    let json = r#"{
        "@type": "Annotation",
        "id": "MDA",
        "relatedType": "Email",
        "relatedId": "EM1",
        "isPrivate": true
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.id().map(AsRef::as_ref), Some("MDA"));
    assert_eq!(meta.related_type(), "Email");
    assert!(meta.related_id() == &Into::<jmap_types::Id>::into("EM1"));
    assert!(meta.is_private());

    // is_private default when omitted on the wire.
    let json2 = r#"{"@type":"Annotation","relatedType":"Email","relatedId":"EM1"}"#;
    let meta2: Metadata = serde_json::from_str(json2).unwrap();
    assert!(!meta2.is_private());
}

// ---------------------------------------------------------------------------
// MetadataFilterCondition
// ---------------------------------------------------------------------------

#[test]
fn metadata_filter_draft_01_section_3_4_1_all_fields() {
    // Oracle: hand-written from §3.4.1 field-by-field listing.
    let json = r#"{
        "@type": ["Annotation"],
        "relatedType": "Email",
        "relatedIds": ["EM1", "EM2", "EM3"],
        "isPrivate": true,
        "textMatch": "approved"
    }"#;
    let filter: MetadataFilterCondition = serde_json::from_str(json).unwrap();
    assert_eq!(filter.type_names, Some(vec!["Annotation".to_owned()]));
    assert_eq!(filter.related_type, Some("Email".to_owned()));
    let ids = filter.related_ids.as_ref().unwrap();
    assert_eq!(ids.len(), 3);
    assert!(ids[0] == *"EM1");
    assert_eq!(filter.is_private, Some(true));
    assert_eq!(filter.text_match, Some("approved".to_owned()));
}

#[test]
fn metadata_filter_at_type_wire_name() {
    // Independent oracle: the wire field name MUST be "@type" exactly
    // (§3.4.1). Round-trip the type_names field and inspect the JSON
    // key directly.
    let json = r#"{"@type":["Annotation","ImapMetadata"]}"#;
    let filter: MetadataFilterCondition = serde_json::from_str(json).unwrap();
    let serialised = serde_json::to_value(&filter).unwrap();
    let map = serialised.as_object().unwrap();
    assert!(map.contains_key("@type"));
    assert!(!map.contains_key("typeNames"));
}

#[test]
fn metadata_filter_empty_is_default() {
    let filter: MetadataFilterCondition = serde_json::from_str("{}").unwrap();
    assert_eq!(filter, MetadataFilterCondition::default());
    // Empty filter serialises to empty object.
    let out = serde_json::to_value(&filter).unwrap();
    assert_eq!(out, serde_json::json!({}));
}

#[test]
fn metadata_filter_no_extras_field() {
    // Filter-algebra exclusion: per workspace policy, filter conditions
    // do NOT carry an `extra` flatten field. Unknown fields on the wire
    // serde-deserialise into the typed fields if they collide, or
    // (since we don't deny_unknown_fields) are silently dropped.
    //
    // Independent oracle: if we round-trip a filter with an unknown
    // vendor field, that field MUST NOT appear in the re-serialised
    // output (no extras to preserve it).
    let json = r#"{"@type":["Annotation"],"vendor:custom":"ignored"}"#;
    let filter: MetadataFilterCondition = serde_json::from_str(json).unwrap();
    let out = serde_json::to_value(&filter).unwrap();
    let map = out.as_object().unwrap();
    assert!(!map.contains_key("vendor:custom"));
    assert!(!map.contains_key("extra"));
    assert_eq!(map["@type"], serde_json::json!(["Annotation"]));
}

// ---------------------------------------------------------------------------
// JmapObject trait wiring
// ---------------------------------------------------------------------------

#[test]
fn metadata_jmap_object_type_name() {
    // The TYPE_NAME constant feeds the server's error messages and
    // capability dispatch. Verify it matches the IANA-registered name
    // (§9.2 "Metadata").
    assert_eq!(Metadata::TYPE_NAME, "Metadata");
}

#[test]
fn metadata_trait_associated_types_compile() {
    // Compile-time check that the trait wiring is correct. The body
    // is intentionally a no-op assertion: success here means
    // Metadata satisfies the GetObject, SetObject, and QueryObject
    // marker bounds.
    fn _assert_marker_bounds<T>()
    where
        T: GetObject + SetObject + QueryObject,
    {
    }
    _assert_marker_bounds::<Metadata>();
}

#[test]
fn metadata_property_selector_round_trip() {
    // The selector is internal Rust API (not on the wire), so the test
    // is identity-only.
    let cases = &[
        MetadataProperty::TypeName,
        MetadataProperty::Id,
        MetadataProperty::RelatedType,
        MetadataProperty::RelatedId,
        MetadataProperty::IsPrivate,
        MetadataProperty::Metadata,
        MetadataProperty::VendorProperty("acme.example.com:color".into()),
    ];
    for c in cases {
        let cloned = c.clone();
        assert_eq!(c, &cloned);
    }
}

// ---------------------------------------------------------------------------
// ImapMetadata + WebDavMetadata struct construction sanity
// ---------------------------------------------------------------------------

#[test]
fn imap_metadata_struct_round_trips_via_metadata_enum() {
    // Build the wire JSON for an ImapMetadata via the Metadata enum,
    // then verify the inner ImapMetadata struct fields agree.
    let json = r#"{
        "@type": "ImapMetadata",
        "id": "MD-IMAP-1",
        "relatedType": "Mailbox",
        "relatedId": "MB-99",
        "metadata": {}
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let inner: ImapMetadata = match meta.clone() {
        Metadata::ImapMetadata(i) => i,
        _ => panic!("expected ImapMetadata variant"),
    };
    assert_eq!(inner.id.as_ref().map(AsRef::as_ref), Some("MD-IMAP-1"));
    assert_eq!(inner.related_type, "Mailbox");
    assert!(inner.related_id == *"MB-99");
    assert!(inner.metadata.is_empty());

    let again = serde_json::to_value(&meta).unwrap();
    let map = again.as_object().unwrap();
    assert_eq!(
        map["@type"],
        serde_json::Value::String("ImapMetadata".into())
    );
}

#[test]
fn webdav_metadata_struct_round_trips_via_metadata_enum() {
    let json = r#"{
        "@type": "WebDavMetadata",
        "id": "MD-DAV-1",
        "relatedType": "FileNode",
        "relatedId": "F-99",
        "metadata": {}
    }"#;
    let meta: Metadata = serde_json::from_str(json).unwrap();
    let inner: WebDavMetadata = match meta.clone() {
        Metadata::WebDavMetadata(w) => w,
        _ => panic!("expected WebDavMetadata variant"),
    };
    assert_eq!(inner.id.as_ref().map(AsRef::as_ref), Some("MD-DAV-1"));
    assert_eq!(inner.related_type, "FileNode");
    assert!(inner.metadata.is_empty());

    let again = serde_json::to_value(&meta).unwrap();
    let map = again.as_object().unwrap();
    assert_eq!(
        map["@type"],
        serde_json::Value::String("WebDavMetadata".into())
    );
}

#[test]
fn standalone_annotation_struct_does_not_carry_at_type() {
    // When constructing an Annotation directly (not via the Metadata
    // enum), the @type tag is NOT serialised — only the Metadata enum
    // emits the tag. This mirrors how, in the spec, @type is a
    // discriminator carried by the enclosing Metadata object.
    let json = r#"{
        "id": "MD1",
        "relatedType": "Email",
        "relatedId": "EM1"
    }"#;
    let ann: Annotation = serde_json::from_str(json).unwrap();
    let out = serde_json::to_value(&ann).unwrap();
    let map = out.as_object().unwrap();
    assert!(!map.contains_key("@type"));
    assert_eq!(
        map["relatedType"],
        serde_json::Value::String("Email".into())
    );
}

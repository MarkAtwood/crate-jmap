//! Integration tests for jmap-did-types.
//!
//! Oracle: draft-atwood-jmap-did-00.md in jmap-chat-spec (HEAD 91672c9).
//! Line numbers below refer to that file. The JSON documents are copied
//! verbatim from the draft's examples, never produced by this crate.

mod common;

use jmap_did_types::{DidAccountCapability, DidCapability, KeyCustody, JMAP_DID_URI};
use serde_json::{json, Value};

/// L125-135: session-level capability example, verbatim.
fn session_example() -> String {
    common::fixture("session_example.json")
}

/// L155-170: account-level capability example, verbatim.
fn account_example() -> String {
    common::fixture("account_example.json")
}

#[test]
fn capability_uri_matches_draft() {
    // L111: "MUST advertise the `urn:ietf:params:jmap:did` capability".
    assert_eq!(JMAP_DID_URI, "urn:ietf:params:jmap:did");
}

#[test]
fn session_example_round_trips() {
    let doc: Value = serde_json::from_str(&session_example()).unwrap();
    let raw = doc["capabilities"]["urn:ietf:params:jmap:did"].clone();
    let cap: DidCapability = serde_json::from_value(raw.clone()).unwrap();
    // L117-118: `supportedMethods` (String[]), e.g. `"key"`, `"web"`.
    assert_eq!(cap.supported_methods, ["key", "web"]);
    // L121: "`null` if the server does not offer a verification endpoint."
    assert_eq!(cap.verification_endpoint, None);
    assert!(cap.extra.is_empty());
    assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
}

#[test]
fn verification_endpoint_url_round_trips() {
    // L120-121: `verificationEndpoint` (String|null): "A URL where
    // clients can submit a DID for server-side verification."
    let raw = json!({
        "supportedMethods": ["web"],
        "verificationEndpoint": "https://jmap.example.com/did/verify"
    });
    let cap: DidCapability = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        cap.verification_endpoint.as_deref(),
        Some("https://jmap.example.com/did/verify")
    );
    assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
}

#[test]
fn account_example_round_trips() {
    let doc: Value = serde_json::from_str(&account_example()).unwrap();
    let raw =
        doc["accounts"]["acct-001"]["accountCapabilities"]["urn:ietf:params:jmap:did"].clone();
    let cap: DidAccountCapability = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        cap.did,
        "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
    );
    assert_eq!(cap.did_document_uri, None);
    assert_eq!(cap.previous_did, None);
    assert_eq!(cap.key_custody, Some(KeyCustody::User));
    assert!(cap.extra.is_empty());
    assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
}

#[test]
fn key_custody_values_match_draft() {
    // L195: "`"server"` means the server, or a party operating on its
    // behalf, holds at least one of those private keys. `"user"` means
    // the server holds none of them. `null` means the server does not
    // disclose custody".
    let custody = |v: Value| {
        let mut raw = json!({
            "did": "did:web:example.com",
            "didDocumentUri": null,
            "previousDid": null
        });
        raw["keyCustody"] = v;
        serde_json::from_value::<DidAccountCapability>(raw)
            .unwrap()
            .key_custody
    };
    assert_eq!(custody(json!("server")), Some(KeyCustody::Server));
    assert_eq!(custody(json!("user")), Some(KeyCustody::User));
    assert_eq!(custody(Value::Null), None);
}

#[test]
fn unknown_key_custody_round_trips() {
    // L195: "clients and peers SHOULD NOT infer `"user"` from `null`" — an
    // unknown value is kept verbatim and is not `User` either.
    let raw = json!({
        "did": "did:web:example.com",
        "didDocumentUri": "https://example.com/.well-known/did.json",
        "previousDid": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
        "keyCustody": "escrow"
    });
    let cap: DidAccountCapability = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(cap.key_custody, Some(KeyCustody::Other("escrow".into())));
    assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
}

#[test]
fn nullable_fields_serialize_as_null() {
    // L120, L144, L147, L150: the four fields are typed `String|null`;
    // the draft examples (L131, L162-163) write `null`, not omit the key.
    let account: DidAccountCapability =
        serde_json::from_value(json!({"did": "did:web:example.com"})).unwrap();
    assert_eq!(
        serde_json::to_string(&account).unwrap(),
        common::fixture("nullable_fields_serialize_as_null.json")
    );
    let session: DidCapability =
        serde_json::from_value(json!({"supportedMethods": ["key"]})).unwrap();
    assert_eq!(
        serde_json::to_string(&session).unwrap(),
        common::fixture("nullable_fields_serialize_as_null_2.json")
    );
}

#[test]
fn vendor_extras_preserved() {
    // Workspace extras-preservation policy (AGENTS.md): unknown fields
    // survive deserialize and serialize.
    let raw = json!({
        "supportedMethods": ["key"],
        "verificationEndpoint": null,
        "vendor.example:plcDirectory": "https://plc.example"
    });
    let cap: DidCapability = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        cap.extra.get("vendor.example:plcDirectory"),
        Some(&json!("https://plc.example"))
    );
    assert_eq!(serde_json::to_value(&cap).unwrap(), raw);

    let raw = json!({
        "did": "did:web:example.com",
        "didDocumentUri": null,
        "previousDid": null,
        "keyCustody": null,
        "vendor.example:tier": 2
    });
    let cap: DidAccountCapability = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(cap.extra.get("vendor.example:tier"), Some(&json!(2)));
    assert_eq!(serde_json::to_value(&cap).unwrap(), raw);
}

// ---------------------------------------------------------------------
// Did (bd:JMAP-6itf.2). Oracles: W3C DID Core v1.0 §3.1 ABNF
// (https://www.w3.org/TR/did-core/#did-syntax), the did:web method spec
// (https://w3c-ccg.github.io/did-method-web/) examples, and
// draft-atwood-jmap-did-00.md L142 and L375.
//
// did                = "did:" method-name ":" method-specific-id
// method-name        = 1*method-char
// method-char        = %x61-7A / DIGIT
// method-specific-id = *( *idchar ":" ) 1*idchar
// idchar             = ALPHA / DIGIT / "." / "-" / "_" / pct-encoded
// pct-encoded        = "%" HEXDIG HEXDIG
// ---------------------------------------------------------------------

use jmap_did_types::{Did, DidError};

#[test]
fn did_core_and_draft_example_dids_parse() {
    for s in [
        // DID Core §1.1 Example 1.
        "did:example:123456789abcdefghi",
        // draft-atwood-jmap-did-00.md L164.
        "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
        // did:web spec §Method-Specific Identifier examples.
        "did:web:w3c-ccg.github.io",
        "did:web:w3c-ccg.github.io:user:alice",
        "did:web:example.com%3A3000",
        "did:web:example.com%3A3000:user:alice",
        // ABNF: `*idchar ":"` permits an empty segment before a colon.
        "did:example::a",
    ] {
        let did = Did::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(did.as_str(), s);
    }
}

#[test]
fn did_splits_method_and_method_specific_id() {
    // DID Core §3.1: "did:" method-name ":" method-specific-id.
    let did = Did::parse("did:web:w3c-ccg.github.io:user:alice").unwrap();
    assert_eq!(did.method(), "web");
    assert_eq!(did.method_specific_id(), "w3c-ccg.github.io:user:alice");
}

#[test]
fn invalid_dids_rejected() {
    // Each case violates one DID Core §3.1 ABNF rule.
    for (s, want) in [
        ("", DidError::MissingPrefix),
        ("urn:example:123", DidError::MissingPrefix),
        ("did:", DidError::InvalidMethodName),
        // method-name = 1*method-char
        ("did::123", DidError::InvalidMethodName),
        // method-char = %x61-7A / DIGIT (no uppercase, no "-")
        ("did:Example:123", DidError::InvalidMethodName),
        ("did:ex-ample:123", DidError::InvalidMethodName),
        // missing ":" method-specific-id
        ("did:example", DidError::InvalidMethodName),
        // method-specific-id ends in 1*idchar: not empty, no trailing ":"
        ("did:example:", DidError::InvalidMethodSpecificId),
        ("did:example:123:", DidError::InvalidMethodSpecificId),
        // idchar excludes "/", "?", "#", " " and "@"
        ("did:example:12/3", DidError::InvalidMethodSpecificId),
        ("did:example:123?x", DidError::InvalidMethodSpecificId),
        ("did:example:123#k", DidError::InvalidMethodSpecificId),
        ("did:example:1 23", DidError::InvalidMethodSpecificId),
        ("did:example:a@b", DidError::InvalidMethodSpecificId),
        // pct-encoded = "%" HEXDIG HEXDIG
        ("did:example:12%3", DidError::InvalidMethodSpecificId),
        ("did:example:12%zz", DidError::InvalidMethodSpecificId),
    ] {
        assert_eq!(Did::parse(s), Err(want), "{s:?}");
    }
}

#[test]
fn did_serde_is_a_json_string_and_rejects_invalid() {
    let raw = json!("did:example:123456789abcdefghi");
    let did: Did = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&did).unwrap(), raw);
    // draft-atwood-jmap-did-00.md L142: "The value MUST be a valid DID as
    // defined in {{DID-CORE}} Section 3.1."
    assert!(serde_json::from_value::<Did>(json!("did:example:")).is_err());
}

#[test]
fn web_domain_of_did_web_spec_examples() {
    // did:web spec: "A port MAY be included and the colon MUST be percent
    // encoded"; "Directories and subdirectories MAY optionally be
    // included, delimited by colons". Resolution maps
    // did:web:example.com%3A3000:user:alice to
    // https://example.com:3000/user/alice/did.json, so the host is
    // example.com.
    for (s, host) in [
        ("did:web:w3c-ccg.github.io", "w3c-ccg.github.io"),
        ("did:web:w3c-ccg.github.io:user:alice", "w3c-ccg.github.io"),
        ("did:web:example.com%3A3000", "example.com"),
        ("did:web:example.com%3A3000:user:alice", "example.com"),
        // RFC 3986 §2.1: hex digits in a percent-encoding are
        // case-insensitive.
        ("did:web:example.com%3a3000", "example.com"),
        // draft-atwood-jmap-did-00.md L375: "`did:web:examp1e.com` vs.
        // `did:web:example.com`"; the domain component is what differs.
        ("did:web:examp1e.com", "examp1e.com"),
        ("did:web:example.com", "example.com"),
    ] {
        assert_eq!(Did::parse(s).unwrap().web_domain(), Some(host), "{s}");
    }
}

#[test]
fn web_domain_none_for_non_web_or_non_domain() {
    for s in [
        // Not did:web.
        "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
        // did:web spec: "it MUST NOT include IP addresses".
        "did:web:192.0.2.1",
        "did:web:192.0.2.1%3A3000",
        // Empty host segment.
        "did:web::user:alice",
        "did:web:%3A3000",
        // A percent-escape other than the %3A port separator, or a port
        // that is not digits.
        "did:web:example.com%2Fevil",
        "did:web:example.com%3A",
        "did:web:example.com%3Ahttp",
    ] {
        assert_eq!(Did::parse(s).unwrap().web_domain(), None, "{s}");
    }
}

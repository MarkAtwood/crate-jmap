//! Integration tests for jmap-did-types.
//!
//! Oracle: draft-atwood-jmap-did-00.md in jmap-chat-spec (HEAD 91672c9).
//! Line numbers below refer to that file. The JSON documents are copied
//! verbatim from the draft's examples, never produced by this crate.

use jmap_did_types::{DidAccountCapability, DidCapability, KeyCustody, JMAP_DID_URI};
use serde_json::{json, Value};

/// L125-135: session-level capability example, verbatim.
const SESSION_EXAMPLE: &str = r#"{
  "capabilities": {
    "urn:ietf:params:jmap:core": {},
    "urn:ietf:params:jmap:did": {
      "supportedMethods": ["key", "web"],
      "verificationEndpoint": null
    }
  }
}"#;

/// L155-170: account-level capability example, verbatim.
const ACCOUNT_EXAMPLE: &str = r#"{
  "accounts": {
    "acct-001": {
      "accountCapabilities": {
        "urn:ietf:params:jmap:did": {
          "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
          "didDocumentUri": null,
          "previousDid": null,
          "keyCustody": "user"
        }
      }
    }
  }
}"#;

#[test]
fn capability_uri_matches_draft() {
    // L111: "MUST advertise the `urn:ietf:params:jmap:did` capability".
    assert_eq!(JMAP_DID_URI, "urn:ietf:params:jmap:did");
}

#[test]
fn session_example_round_trips() {
    let doc: Value = serde_json::from_str(SESSION_EXAMPLE).unwrap();
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
    let doc: Value = serde_json::from_str(ACCOUNT_EXAMPLE).unwrap();
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
    // L151: "`"server"` means the server, or a party operating on its
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
    // L151: "clients and peers MUST NOT infer `"user"` from `null`" — an
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
        r#"{"did":"did:web:example.com","didDocumentUri":null,"previousDid":null,"keyCustody":null}"#
    );
    let session: DidCapability =
        serde_json::from_value(json!({"supportedMethods": ["key"]})).unwrap();
    assert_eq!(
        serde_json::to_string(&session).unwrap(),
        r#"{"supportedMethods":["key"],"verificationEndpoint":null}"#
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

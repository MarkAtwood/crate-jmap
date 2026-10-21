//! Typed DID capability accessors on `Session` and `AccountInfo`
//! (draft-atwood-jmap-did-00 §Capability, jmap-chat-spec
//! draft-atwood-jmap-did-00.md L109-170).
//!
//! Oracle: the Session and account capability examples published in the
//! draft (L126-134 and L155-169), with the RFC 8620 §2 Session and
//! Account members those fragments omit filled in, plus the field
//! definitions quoted per test.

use jmap_base_client::{ClientError, Session};
use jmap_did_types::KeyCustody;
use serde_json::{json, Value};

/// Wraps the draft's `capabilities` and `accounts` fragments in the
/// remaining RFC 8620 §2 Session members, which the fragments omit.
fn session(capabilities: Value, accounts: Value) -> Session {
    serde_json::from_value(json!({
        "capabilities": capabilities,
        "accounts": accounts,
        "primaryAccounts": {},
        "username": "u@example.com",
        "apiUrl": "https://jmap.example.com/api/",
        "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
        "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
        "state": "s1"
    }))
    .expect("Session must deserialize")
}

/// draft L126-134 (session-level example) and L155-169 (account-level
/// example); `name`, `isPersonal` and `isReadOnly` are the RFC 8620 §2
/// Account members the draft fragment omits.
fn draft_example_session() -> Session {
    session(
        json!({
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:did": {
                "supportedMethods": ["key", "web"],
                "verificationEndpoint": null
            }
        }),
        json!({
            "acct-001": {
                "name": "acct-001@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:did": {
                        "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
                        "didDocumentUri": null,
                        "previousDid": null,
                        "keyCustody": "user"
                    }
                }
            }
        }),
    )
}

// L115: "The value of `capabilities["urn:ietf:params:jmap:did"]` is a JSON
// object"; L130-131: `"supportedMethods": ["key", "web"]`,
// `"verificationEndpoint": null`. L121: "`null` if the server does not
// offer a verification endpoint."
#[test]
fn session_did_capability_parses_draft_example() {
    let cap = draft_example_session()
        .did_capability()
        .expect("well-formed")
        .expect("advertised");
    assert_eq!(cap.supported_methods, ["key", "web"]);
    assert_eq!(cap.verification_endpoint, None);
}

// L139: "The value of `accountCapabilities["urn:ietf:params:jmap:did"]` is
// a JSON object"; values from the L155-169 example.
#[test]
fn account_did_capability_parses_draft_example() {
    let s = draft_example_session();
    let cap = s.accounts["acct-001"]
        .did_capability()
        .expect("well-formed")
        .expect("advertised");
    assert_eq!(
        cap.did,
        "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK"
    );
    assert_eq!(cap.did_document_uri, None);
    assert_eq!(cap.previous_did, None);
    // L195 (jmap-chat-spec 33702a5): "`"user"` means the server holds none of them."
    assert_eq!(cap.key_custody, Some(KeyCustody::User));
}

// L111: a server supporting the extension "MUST advertise the
// `urn:ietf:params:jmap:did` capability"; a server that does not support
// it advertises nothing, which is not an error.
#[test]
fn did_capability_absent_is_none() {
    let s = session(
        json!({ "urn:ietf:params:jmap:core": {} }),
        json!({
            "a1": {
                "name": "a1@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {}
            }
        }),
    );
    assert!(matches!(s.did_capability(), Ok(None)));
    assert!(matches!(s.accounts["a1"].did_capability(), Ok(None)));
}

// L118: "Servers MUST support at least one method."
#[test]
fn session_did_capability_empty_methods_is_parse_error() {
    let s = session(
        json!({
            "urn:ietf:params:jmap:did": {
                "supportedMethods": [],
                "verificationEndpoint": null
            }
        }),
        json!({}),
    );
    assert!(matches!(s.did_capability(), Err(ClientError::Parse(_))));
}

// L141-142: "`did` (String): The DID bound to this account." — required,
// so an account capability without it is malformed.
#[test]
fn account_did_capability_missing_did_is_parse_error() {
    let s = session(
        json!({}),
        json!({
            "a1": {
                "name": "a1@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:did": {
                        "didDocumentUri": null,
                        "previousDid": null,
                        "keyCustody": null
                    }
                }
            }
        }),
    );
    assert!(matches!(
        s.accounts["a1"].did_capability(),
        Err(ClientError::Parse(_))
    ));
}

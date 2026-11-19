//! The umbrella's re-export.
//!
//! Oracles: draft-atwood-jmap-did-00 (the capability URI and the account
//! capability) and the did:key test vector of the did:key Method v0.9
//! (an Ed25519 key, multicodec prefix `0xed`).

use jmap_did::types::{Did, DidAccountCapability, DidKey, JMAP_DID_URI, KeyCustody, KeyType};

#[test]
fn types_are_reexported() {
    assert_eq!(JMAP_DID_URI, "urn:ietf:params:jmap:did");
    let cap: DidAccountCapability = serde_json::from_str(
        r#"{"did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
            "didDocumentUri": null, "previousDid": null, "keyCustody": "user"}"#,
    )
    .unwrap();
    assert_eq!(cap.key_custody, Some(KeyCustody::User));
}

#[test]
fn did_key_parses_through_the_reexport() {
    let did = Did::parse("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK").unwrap();
    let key = DidKey::from_did(&did).unwrap();
    assert_eq!(key.key_type(), KeyType::Ed25519);
    assert_eq!(key.public_key().len(), 32);
}

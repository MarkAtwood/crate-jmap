//! The umbrella's re-export.
//!
//! Oracles: draft-atwood-jmap-cid-00 §3 (the capability URI), the SHA-256
//! digest of the empty string (FIPS 180-4), and its padded base64 form
//! (draft-ietf-jmap-blobext-03 §5, RFC 4648 §4).

use jmap_cid::types::{CidCapability, JMAP_CID_URI, Sha256};

/// SHA-256 of the empty octet string.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn capability_is_reexported() {
    assert_eq!(JMAP_CID_URI, "urn:ietf:params:jmap:cid");
    let cap: CidCapability = serde_json::from_str("{}").unwrap();
    assert_eq!(serde_json::to_string(&cap).unwrap(), "{}");
}

#[test]
fn sha256_is_reexported() {
    let digest: Sha256 = EMPTY_SHA256.parse().unwrap();
    assert_eq!(
        digest.to_base64(),
        "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
    );
}

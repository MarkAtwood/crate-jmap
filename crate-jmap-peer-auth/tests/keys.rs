//! `peerSigningKeys` and the DNSSEC precedence rule
//! (draft-atwood-jmap-chat-federation-00 §Key Publication, §DNSSEC Key
//! Binding).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jmap_peer_auth::ed25519_dalek::VerifyingKey;
use jmap_peer_auth::{usable_keys, DnssecAnswer, PeerSigningKey, Rejection};
use serde_json::{json, Value};

/// RFC 8037 Appendix A.3: the thumbprint of the Appendix A.1/A.2 key.
const A3_KID: &str = "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";
/// RFC 8037 Appendix A.2 public key `x`.
const A2_X: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";
/// RFC 9421 Appendix B.1.4 test-key-ed25519 `x`; its thumbprint is not a
/// kid this test knows, so it only appears with a mismatching `kid`.
const B14_X: &str = "JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs";

/// The `peerSigningKeys` entry from the draft's §Session Object
/// Extensions example.
fn draft_example() -> Value {
    json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
        "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
    })
}

fn key_from_x(x: &str) -> VerifyingKey {
    let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(x).unwrap().try_into().unwrap();
    VerifyingKey::from_bytes(&bytes).unwrap()
}

#[test]
fn kid_is_rfc8037_a3_thumbprint() {
    // RFC 8037 A.3: "which results in the base64url encoded JWK
    // Thumbprint representation of "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"".
    assert_eq!(PeerSigningKey::from(key_from_x(A2_X)).kid(), A3_KID);
}

#[test]
fn draft_example_is_usable_and_round_trips() {
    let key = PeerSigningKey::from_jwk(&draft_example()).expect("usable");
    assert_eq!(key.kid(), A3_KID);
    assert_eq!(key.verifying_key(), &key_from_x(A2_X));
    assert_eq!(key.to_jwk(), draft_example());
}

/// §Key Publication: "A key is usable only if its `kty` and `crv` are as
/// above and the receiving server, recomputing the thumbprint, obtains
/// the key's `kid`. The receiving server MUST ignore any other entry."
#[test]
fn unusable_entries_are_ignored() {
    let with = |name: &str, value: Value| {
        let mut jwk = draft_example();
        jwk[name] = value;
        jwk
    };
    let without = |name: &str| {
        let mut jwk = draft_example();
        jwk.as_object_mut().unwrap().remove(name);
        jwk
    };
    let cases = [
        ("kty EC", with("kty", json!("EC"))),
        ("crv X25519", with("crv", json!("X25519"))),
        ("crv Ed448", with("crv", json!("Ed448"))),
        ("no kty", without("kty")),
        ("no crv", without("crv")),
        ("no x", without("x")),
        ("no kid", without("kid")),
        // RFC 9421 B.1.4 names its key "test-key-ed25519", which is not
        // the key's thumbprint.
        (
            "kid not thumbprint",
            json!({"kty": "OKP", "crv": "Ed25519", "kid": "test-key-ed25519", "x": B14_X}),
        ),
        ("kid of another key", with("x", json!(B14_X))),
        ("x too short", with("x", json!("11qYAYKxCrfVS_7TyWQHOg"))),
        ("x padded", with("x", json!(format!("{A2_X}=")))),
        ("x not a string", with("x", json!(42))),
        ("not an object", json!([draft_example()])),
    ];
    for (what, jwk) in cases {
        assert_eq!(PeerSigningKey::from_jwk(&jwk), None, "{what}");
    }
}

/// §Key Publication: "A JWK in `peerSigningKeys` MUST NOT contain private
/// key members." The RFC 8037 Appendix A.1 private key is A.2 plus `d`.
#[test]
fn private_jwk_is_ignored() {
    let a1 = json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "d": "nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A",
        "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
        "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
    });
    assert_eq!(PeerSigningKey::from_jwk(&a1), None);
}

/// Two published keys: the draft example and an unusable entry.
fn published() -> Value {
    json!([draft_example(), {"kty": "RSA", "n": "AQAB", "e": "AQAB", "kid": "rsa"}])
}

fn kids(keys: &[PeerSigningKey]) -> Vec<&str> {
    keys.iter().map(PeerSigningKey::kid).collect()
}

#[test]
fn https_keys_alone_when_no_binding_applies() {
    // §DNSSEC Key Binding: "If the answer is a secure denial of existence,
    // is insecure, or contains no record of this form, the HTTPS-published
    // keys are used alone." "If the validation result is indeterminate
    // ..., or the lookup fails ..., the HTTPS-published keys are used
    // alone, as for an insecure answer."
    for answer in [
        DnssecAnswer::SecureDenial,
        DnssecAnswer::Insecure,
        DnssecAnswer::Indeterminate,
        DnssecAnswer::LookupFailed,
        DnssecAnswer::Secure(vec![]),
        DnssecAnswer::Secure(vec![
            "v=spf1 -all".to_owned(),
            "v=jmappeer1; kid=".to_owned(),
        ]),
    ] {
        let keys = usable_keys(&published(), &answer).unwrap();
        assert_eq!(kids(&keys), [A3_KID], "{answer:?}");
    }
}

#[test]
fn secure_records_restrict_keys() {
    // "If the answer is secure ... and contains at least one record of this
    // form, a key from `peerSigningKeys` is usable only if its recomputed
    // thumbprint equals the `kid` of one of those records."
    let listed = DnssecAnswer::Secure(vec![format!("v=jmappeer1; kid={A3_KID}")]);
    assert_eq!(kids(&usable_keys(&published(), &listed).unwrap()), [A3_KID]);

    let other = DnssecAnswer::Secure(vec!["v=jmappeer1; kid=some-other-kid".to_owned()]);
    assert!(usable_keys(&published(), &other).unwrap().is_empty());
}

#[test]
fn bogus_answer_rejects() {
    // "If the answer is bogus, the receiving server MUST NOT fall back to
    // the HTTPS-published keys and MUST reject the request."
    assert_eq!(
        usable_keys(&published(), &DnssecAnswer::Bogus),
        Err(Rejection::DnssecBogus)
    );
}

#[test]
fn non_array_peer_signing_keys_has_no_usable_keys() {
    assert!(usable_keys(&draft_example(), &DnssecAnswer::Insecure)
        .unwrap()
        .is_empty());
}

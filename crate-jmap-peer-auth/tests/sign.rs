//! Request signing (draft-atwood-jmap-chat-federation-00 §Request
//! Signing). The oracle for each signature is a signature base written
//! out by hand per RFC 9421 §2.5 and checked with ed25519-dalek directly.

use std::time::{Duration, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use http::Method;
use jmap_peer_auth::ed25519_dalek::{Signature, SigningKey};
use jmap_peer_auth::{sign, PeerDomain, SignError, SignatureHeaders};

/// RFC 8037 Appendix A.1 private key `d`; its thumbprint is the draft's
/// `kPrK_...` keyid example.
fn alice_key() -> SigningKey {
    let d = URL_SAFE_NO_PAD
        .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
        .unwrap();
    SigningKey::from_bytes(&d.try_into().unwrap())
}

const CREATED: u64 = 1_760_000_000;
const TARGET: &str = "https://bob.example/jmap/api/";
/// The draft's keyid example: "the signer's peer domain, the character
/// `#`, and the `kid` of the signing key".
const KEYID: &str = "alice.example#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";

fn sign_at(method: &Method, content: &[u8]) -> SignatureHeaders {
    let alice = PeerDomain::parse("alice.example").unwrap();
    sign(
        &alice_key(),
        &alice,
        method,
        TARGET,
        content,
        UNIX_EPOCH + Duration::from_secs(CREATED),
    )
    .unwrap()
}

/// Splits `peer=<params>` and checks `<params>` against the expected
/// inner list, returning the nonce it carries.
fn check_params(signature_input: &str, covered: &str) -> (String, String) {
    let params = signature_input.strip_prefix("peer=").expect("label peer");
    let nonce = params
        .split_once(";nonce=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(nonce, _)| nonce.to_owned())
        .expect("nonce parameter");
    let expected = format!(
        "({covered});created={CREATED};expires={};nonce=\"{nonce}\";keyid=\"{KEYID}\";alg=\"ed25519\";tag=\"jmap-chat-peer\"",
        CREATED + 300
    );
    assert_eq!(params, expected);
    (params.to_owned(), nonce)
}

fn check_signature(signature: &str, base: &str) {
    let bytes = signature
        .strip_prefix("peer=:")
        .and_then(|s| s.strip_suffix(':'))
        .map(|s| STANDARD.decode(s).unwrap())
        .expect("peer=:<base64>:");
    let signature = Signature::from_slice(&bytes).unwrap();
    alice_key()
        .verifying_key()
        .verify_strict(base.as_bytes(), &signature)
        .expect("signature verifies");
}

#[test]
fn post_with_content() {
    let out = sign_at(&Method::POST, br#"{"hello": "world"}"#);
    // "MUST, when the request has content, cover the `content-digest`
    // field ..., which MUST contain a `sha-256` digest of the content."
    // RFC 9530 Appendix D gives this digest for {"hello": "world"}.
    assert_eq!(
        out.content_digest.as_deref(),
        Some("sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:")
    );
    // "MUST cover the `@method` and `@target-uri` derived components";
    // "MUST include the `created` and `expires` parameters. `expires`
    // MUST NOT be more than 300 seconds after `created`."
    let (params, _) = check_params(
        &out.signature_input,
        r#""@method" "@target-uri" "content-digest""#,
    );
    let base = format!(
        "\"@method\": POST\n\"@target-uri\": {TARGET}\n\"content-digest\": sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:\n\"@signature-params\": {params}"
    );
    check_signature(&out.signature, &base);
}

#[test]
fn get_without_content() {
    let out = sign_at(&Method::GET, b"");
    assert_eq!(out.content_digest, None);
    let (params, _) = check_params(&out.signature_input, r#""@method" "@target-uri""#);
    check_signature(
        &out.signature,
        &format!("\"@method\": GET\n\"@target-uri\": {TARGET}\n\"@signature-params\": {params}"),
    );
}

/// "MUST include the `nonce` parameter, carrying at least 128 bits of
/// output from a cryptographically secure random number generator,
/// base64url-encoded. A signer MUST NOT reuse a nonce."
#[test]
fn nonce_is_128_bit_base64url_and_fresh() {
    let (_, first) = check_params(
        &sign_at(&Method::GET, b"").signature_input,
        r#""@method" "@target-uri""#,
    );
    let (_, second) = check_params(
        &sign_at(&Method::GET, b"").signature_input,
        r#""@method" "@target-uri""#,
    );
    assert!(URL_SAFE_NO_PAD.decode(&first).unwrap().len() * 8 >= 128);
    assert_ne!(first, second);
}

#[test]
fn relative_target_uri_is_refused() {
    let alice = PeerDomain::parse("alice.example").unwrap();
    let result = sign(
        &alice_key(),
        &alice,
        &Method::POST,
        "/jmap/api/",
        b"{}",
        UNIX_EPOCH + Duration::from_secs(CREATED),
    );
    assert!(matches!(result, Err(SignError::TargetUri)));
}

#[test]
fn pre_epoch_time_is_refused() {
    let alice = PeerDomain::parse("alice.example").unwrap();
    let result = sign(
        &alice_key(),
        &alice,
        &Method::GET,
        TARGET,
        b"",
        UNIX_EPOCH - Duration::from_secs(1),
    );
    assert!(matches!(result, Err(SignError::Clock)));
}

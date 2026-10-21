//! DID-keyed peer authentication (draft-atwood-jmap-chat-did-00
//! §Federation Authentication, jmap-chat-spec
//! draft-atwood-jmap-chat-did-00.md L213-227, §Resolution failure
//! handling L247, §Caching L233 at jmap-chat-spec 6bcd9d8).
//!
//! Oracles: the RFC 9421 Appendix B.1.4 `test-key-ed25519` key pair
//! signs every request; its RFC 7638 thumbprint and its did:key form were
//! computed outside this crate (Python `hashlib` and a hand-written
//! base58-btc encoder). Each request's `Signature-Input` text and RFC 9421
//! §2.5 signature base are written out here and signed with
//! ed25519-dalek directly. DID Documents are hand-written JSON.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use http::{HeaderMap, HeaderValue, Method};
use jmap_chat_server::{verify_did_signature, DidAuthRejection, DidDocument};
use jmap_did_types::Did;
use jmap_peer_auth::ed25519_dalek::{Signer as _, SigningKey};
use jmap_peer_auth::{Accepted, Rejection, Request, SignedRequest};
use serde_json::{json, Value};

/// RFC 9421 Appendix B.1.4 test-key-ed25519, JWK `d`.
const B14_D: &str = "n4Ni-HpISpVObnQMW0wOhCKROaIKqKtW_2ZYb2p9KcU";
/// RFC 9421 Appendix B.1.4 test-key-ed25519, JWK `x`.
const B14_X: &str = "JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs";
/// RFC 7638 thumbprint of test-key-ed25519 (SHA-256 over
/// `{"crv":"Ed25519","kty":"OKP","x":"<B14_X>"}`, base64url).
const B14_KID: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";
/// `did:key` of test-key-ed25519: `z` + base58-btc(0xed 0x01 || x).
const B14_DID_KEY: &str = "did:key:z6Mkh4LmfP1ev9MNPGr7JbEbtD6BD4fsu1duEj83PMCs3xHG";
/// RFC 8037 Appendix A.1 private key, a key that is not test-key-ed25519.
const OTHER_D: &str = "nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A";
/// RFC 8037 Appendix A.1 public key `x` of [`OTHER_D`].
const OTHER_X: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";

const TARGET: &str = "https://bob.example/jmap/api/";
const BODY: &[u8] = br#"{"hello": "world"}"#;
/// RFC 9530 Appendix D: sha-256 of {"hello": "world"}.
const DIGEST: &str = "sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:";
const NONCE: &str = "6QaXn8jf3mJwV1ce0bVb1A";
const CREATED: u64 = 1_760_000_000;

fn signing_key(d: &str) -> SigningKey {
    SigningKey::from_bytes(&URL_SAFE_NO_PAD.decode(d).unwrap().try_into().unwrap())
}

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(CREATED + 10)
}

/// The headers of a POST to [`TARGET`] carrying [`BODY`], with one
/// `jmap-chat-peer` signature by `key` whose `keyid` is
/// `alice.example#<B14_KID>`.
fn signed_headers(key: &SigningKey) -> HeaderMap {
    let params = format!(
        r#"("@method" "@target-uri" "content-digest");created={CREATED};expires={};nonce="{NONCE}";keyid="alice.example#{B14_KID}";alg="ed25519";tag="jmap-chat-peer""#,
        CREATED + 300
    );
    let base = format!(
        "\"@method\": POST\n\"@target-uri\": {TARGET}\n\"content-digest\": {DIGEST}\n\"@signature-params\": {params}"
    );
    let signature = STANDARD.encode(key.sign(base.as_bytes()).to_bytes());
    let mut headers = HeaderMap::new();
    headers.insert("content-digest", HeaderValue::from_static(DIGEST));
    headers.insert(
        "signature-input",
        HeaderValue::from_str(&format!("peer={params}")).unwrap(),
    );
    headers.insert(
        "signature",
        HeaderValue::from_str(&format!("peer=:{signature}:")).unwrap(),
    );
    headers
}

/// Verifies against a document that is not cached.
fn verify(
    headers: &HeaderMap,
    did: &str,
    document: Option<&Value>,
) -> Result<Accepted, DidAuthRejection> {
    verify_with(headers, did, document.map(DidDocument::Refreshed), false)
}

/// Verifies against `document`; `replayed` is what the replay store says
/// about the nonce.
fn verify_with(
    headers: &HeaderMap,
    did: &str,
    document: Option<DidDocument<'_>>,
    replayed: bool,
) -> Result<Accepted, DidAuthRejection> {
    let request = Request {
        method: &Method::POST,
        target_uri: TARGET,
        headers,
        content: BODY,
    };
    let signed = SignedRequest::parse(request).unwrap();
    let did: Did = did.parse().unwrap();
    verify_did_signature(&signed, &did, document, now(), |_, _| replayed)
}

/// A hand-written did:key DID Document for test-key-ed25519 (did:key
/// Method v0.9 Document Creation Algorithm, `Multikey` form).
fn did_key_document() -> Value {
    let multibase = B14_DID_KEY.strip_prefix("did:key:").unwrap();
    let vm = format!("{B14_DID_KEY}#{multibase}");
    json!({
        "@context": ["https://www.w3.org/ns/did/v1.1"],
        "id": B14_DID_KEY,
        "verificationMethod": [{
            "id": vm,
            "type": "Multikey",
            "controller": B14_DID_KEY,
            "publicKeyMultibase": multibase,
        }],
        "authentication": [vm],
    })
}

/// L221: "If verification succeeds, the receiving server MUST treat the
/// request as authenticated for the claimed DID". L227: "RFC 9421 ...
/// using an Ed25519 ... signing key selected from the DID Document's
/// `authentication` verification relationship."
#[test]
fn did_key_authentication_key_verifies() {
    let headers = signed_headers(&signing_key(B14_D));
    let accepted = verify(&headers, B14_DID_KEY, Some(&did_key_document())).unwrap();
    assert_eq!(accepted.peer_domain.to_string(), "alice.example");
    assert_eq!(accepted.nonce, NONCE);
}

/// W3C DID Core §3.2.2: "Relative DID URLs are often used to reference
/// verification methods"; the base URI "is the DID that is associated
/// with the DID subject". A `publicKeyJwk` method referenced as `#key-1`.
#[test]
fn did_web_relative_jwk_reference_verifies() {
    let document = json!({
        "id": "did:web:alice.example",
        "verificationMethod": [{
            "id": "#key-1",
            "type": "JsonWebKey",
            "controller": "did:web:alice.example",
            "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": B14_X},
        }],
        "authentication": ["#key-1"],
    });
    let headers = signed_headers(&signing_key(B14_D));
    assert!(verify(&headers, "did:web:alice.example", Some(&document)).is_ok());
}

/// W3C DID Core §5.3.1: "Each verification method MAY be embedded or
/// referenced."
#[test]
fn embedded_authentication_method_verifies() {
    let document = json!({
        "id": "did:web:alice.example",
        "authentication": [{
            "id": "did:web:alice.example#key-2",
            "type": "JsonWebKey",
            "controller": "did:web:alice.example",
            "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": B14_X},
        }],
    });
    let headers = signed_headers(&signing_key(B14_D));
    assert!(verify(&headers, "did:web:alice.example", Some(&document)).is_ok());
}

/// L227: the key is "selected from the DID Document's `authentication`
/// verification relationship"; a key listed only under `assertionMethod`
/// is not selected, and L220: "If verification fails, the receiving
/// server SHOULD reject the request".
#[test]
fn key_outside_authentication_is_rejected() {
    let mut document = did_key_document();
    let vm = document["authentication"].take();
    document["assertionMethod"] = vm;
    let headers = signed_headers(&signing_key(B14_D));
    assert_eq!(
        verify(&headers, B14_DID_KEY, Some(&document)),
        Err(DidAuthRejection::Signature(Rejection::UnknownKey))
    );
}

/// L219: the server "verifies a cryptographic signature attached to the
/// request against the selected verification method's public key"; L220:
/// on failure it "SHOULD reject the request". Signed by the RFC 8037 A.1
/// key while naming test-key-ed25519's `kid`.
#[test]
fn signature_by_another_key_is_rejected() {
    let headers = signed_headers(&signing_key(OTHER_D));
    assert_eq!(
        verify(&headers, B14_DID_KEY, Some(&did_key_document())),
        Err(DidAuthRejection::Signature(Rejection::BadSignature))
    );
}

/// L247: when resolution fails "and the server holds no usable cached
/// DID Document for that DID, the server SHOULD treat the DID as
/// unverified and SHOULD reject any authentication attempt that depends
/// on it."
#[test]
fn resolution_failure_is_rejected() {
    let headers = signed_headers(&signing_key(B14_D));
    assert_eq!(
        verify(&headers, B14_DID_KEY, None),
        Err(DidAuthRejection::Unresolved)
    );
}

/// W3C DID Core §7.1: "The value of id in the resolved DID document MUST
/// match the DID that was resolved."
#[test]
fn document_for_another_did_is_rejected() {
    let headers = signed_headers(&signing_key(B14_D));
    assert_eq!(
        verify(&headers, "did:web:alice.example", Some(&did_key_document())),
        Err(DidAuthRejection::DocumentIdMismatch)
    );
}

/// W3C DID Core §5.2.1 publicKeyJwk: "The map MUST NOT contain "d", or
/// any other members of the private information class". A method that
/// carries `d` is not used.
#[test]
fn jwk_with_private_member_is_ignored() {
    let document = json!({
        "id": "did:web:alice.example",
        "authentication": [{
            "id": "did:web:alice.example#key-1",
            "type": "JsonWebKey",
            "controller": "did:web:alice.example",
            "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": B14_X, "d": B14_D},
        }],
    });
    let headers = signed_headers(&signing_key(B14_D));
    assert_eq!(
        verify(&headers, "did:web:alice.example", Some(&document)),
        Err(DidAuthRejection::Signature(Rejection::UnknownKey))
    );
}

/// A hand-written did:web DID Document whose one `authentication` key has
/// JWK `x` value `x`.
fn did_web_document(x: &str) -> Value {
    json!({
        "id": "did:web:alice.example",
        "authentication": [{
            "id": "did:web:alice.example#key-1",
            "type": "JsonWebKey",
            "controller": "did:web:alice.example",
            "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": x},
        }],
    })
}

/// L233: "If the key that verified a request signature is not in the
/// `authentication` verification relationship of the cached DID Document
/// ..., the server MUST resolve the DID again, bypassing its cache,
/// before failing the verification. ... The rationale is that this
/// mismatch is the strongest available signal that the cached key
/// material has been rotated". The cached document holds the RFC 8037
/// A.1 key; the peer rotated to test-key-ed25519, which the refreshed
/// document holds.
#[test]
fn rotated_key_in_cached_document_requires_refresh() {
    let headers = signed_headers(&signing_key(B14_D));
    let cached = did_web_document(OTHER_X);
    assert_eq!(
        verify_with(
            &headers,
            "did:web:alice.example",
            Some(DidDocument::Cached(&cached)),
            false
        ),
        Err(DidAuthRejection::RefreshRequired)
    );
    let refreshed = did_web_document(B14_X);
    assert!(verify_with(
        &headers,
        "did:web:alice.example",
        Some(DidDocument::Refreshed(&refreshed)),
        false
    )
    .is_ok());
}

/// L233 makes an absent key the refresh trigger. Here the `kid` names
/// the cached document's key, so the key is present and a signature that
/// does not verify under it is a plain rejection, not a refresh.
#[test]
fn bad_signature_against_cached_document_is_rejected_without_refresh() {
    let headers = signed_headers(&signing_key(OTHER_D));
    let cached = did_web_document(B14_X);
    assert_eq!(
        verify_with(
            &headers,
            "did:web:alice.example",
            Some(DidDocument::Cached(&cached)),
            false
        ),
        Err(DidAuthRejection::Signature(Rejection::BadSignature))
    );
}

/// L247: servers "SHOULD NOT ... proceed with stale cached state when the
/// cache entry has been explicitly invalidated, including by the refresh
/// requirement above"; L220: "If verification fails, the receiving server
/// SHOULD reject the request". A refreshed document that still does not
/// verify is a plain rejection, not another refresh.
#[test]
fn bad_signature_against_refreshed_document_is_rejected() {
    let headers = signed_headers(&signing_key(OTHER_D));
    let refreshed = did_web_document(B14_X);
    assert_eq!(
        verify_with(
            &headers,
            "did:web:alice.example",
            Some(DidDocument::Refreshed(&refreshed)),
            false
        ),
        Err(DidAuthRejection::Signature(Rejection::BadSignature))
    );
}

/// L233 ties the refresh to a signing key that "is not in the
/// `authentication` verification relationship of the cached DID
/// Document"; a replayed nonce (federation-00 §Request Signing
/// check 5) fails whatever the document holds, so it is a plain
/// rejection.
#[test]
fn replay_against_cached_document_is_rejected_without_refresh() {
    let headers = signed_headers(&signing_key(B14_D));
    let cached = did_web_document(B14_X);
    assert_eq!(
        verify_with(
            &headers,
            "did:web:alice.example",
            Some(DidDocument::Cached(&cached)),
            true
        ),
        Err(DidAuthRejection::Signature(Rejection::Replayed))
    );
}

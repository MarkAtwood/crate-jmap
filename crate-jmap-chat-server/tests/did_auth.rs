//! DID-bound verification (draft-atwood-jmap-chat-did-00 §DID-Bound
//! Verification L215-223, §Caching L233, §Resolution failure handling
//! L237 at jmap-chat-spec 6bcd9d8).
//!
//! Oracles: the RFC 9421 Appendix B.1.4 `test-key-ed25519` key pair is
//! the peer domain's signing key; its RFC 7638 thumbprint and its did:key
//! form were computed outside this crate (Python `hashlib` and a
//! hand-written base58-btc encoder). The RFC 8037 Appendix A.1 key is a
//! second, unrelated Ed25519 key. The request's `Signature-Input` text
//! and RFC 9421 §2.5 signature base are written out here and signed with
//! ed25519-dalek directly. `peerSigningKeys` entries and DID Documents
//! are hand-written JSON.

use std::time::{Duration, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use http::{HeaderMap, HeaderValue, Method};
use jmap_chat_server::{DidAuthRejection, DidDocument, verify_did_binding};
use jmap_did_types::Did;
use jmap_peer_auth::ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use jmap_peer_auth::{PeerSigningKey, Request, SignedRequest};
use serde_json::{Value, json};

/// RFC 9421 Appendix B.1.4 test-key-ed25519, JWK `d`.
const B14_D: &str = "n4Ni-HpISpVObnQMW0wOhCKROaIKqKtW_2ZYb2p9KcU";
/// RFC 9421 Appendix B.1.4 test-key-ed25519, JWK `x`.
const B14_X: &str = "JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs";
/// RFC 7638 thumbprint of test-key-ed25519 (SHA-256 over
/// `{"crv":"Ed25519","kty":"OKP","x":"<B14_X>"}`, base64url).
const B14_KID: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";
/// `did:key` of test-key-ed25519: `z` + base58-btc(0xed 0x01 || x).
const B14_DID_KEY: &str = "did:key:z6Mkh4LmfP1ev9MNPGr7JbEbtD6BD4fsu1duEj83PMCs3xHG";
/// RFC 8037 Appendix A.1 public key `x`, a key that is not
/// test-key-ed25519.
const OTHER_X: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";
/// A hand-written host-less did:plc DID.
const PLC: &str = "did:plc:ewvi7nxzyoun6zhxrhs64oiz";

const TARGET: &str = "https://bob.example/jmap/api/";
const BODY: &[u8] = br#"{"hello": "world"}"#;
/// RFC 9530 Appendix D: sha-256 of {"hello": "world"}.
const DIGEST: &str = "sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:";
const NONCE: &str = "6QaXn8jf3mJwV1ce0bVb1A";
const CREATED: u64 = 1_760_000_000;

fn bytes32(b64url: &str) -> [u8; 32] {
    URL_SAFE_NO_PAD.decode(b64url).unwrap().try_into().unwrap()
}

fn verifying_key(x: &str) -> VerifyingKey {
    VerifyingKey::from_bytes(&bytes32(x)).unwrap()
}

fn check(did: &str, document: Option<DidDocument<'_>>, x: &str) -> Result<(), DidAuthRejection> {
    let did: Did = did.parse().unwrap();
    verify_did_binding(&did, document, &verifying_key(x))
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

/// A hand-written did:plc DID Document whose one `authentication` entry
/// is the embedded verification method `method`.
fn plc_embedded(method: Value) -> Value {
    json!({"id": PLC, "authentication": [method]})
}

/// An embedded `publicKeyJwk` verification method of [`PLC`].
fn plc_jwk_method(jwk: Value) -> Value {
    json!({
        "id": format!("{PLC}#key-1"),
        "type": "JsonWebKey",
        "controller": PLC,
        "publicKeyJwk": jwk,
    })
}

/// L215: "After the request signature has verified ({{JMAP-CHAT-FED}})";
/// L218: "It takes the Ed25519 public key that verified the request
/// signature: the key in the peer domain's `peerSigningKeys` whose `kid`
/// the signature's `keyid` names"; L219: the verification succeeds if the
/// `authentication` relationship holds a key "equal, octet for octet, to
/// that key". The request is signed with test-key-ed25519, published as a
/// hand-written `peerSigningKeys` JWK, and the DID is its did:key.
#[test]
fn key_that_verified_the_request_is_bound_to_its_did_key() {
    let params = format!(
        r#"("@method" "@target-uri" "content-digest");created={CREATED};expires={};nonce="{NONCE}";keyid="alice.example#{B14_KID}";alg="ed25519";tag="jmap-chat-peer""#,
        CREATED + 300
    );
    let base = format!(
        "\"@method\": POST\n\"@target-uri\": {TARGET}\n\"content-digest\": {DIGEST}\n\"@signature-params\": {params}"
    );
    let signature = SigningKey::from_bytes(&bytes32(B14_D)).sign(base.as_bytes());
    let mut headers = HeaderMap::new();
    headers.insert("content-digest", HeaderValue::from_static(DIGEST));
    headers.insert(
        "signature-input",
        HeaderValue::from_str(&format!("peer={params}")).unwrap(),
    );
    headers.insert(
        "signature",
        HeaderValue::from_str(&format!("peer=:{}:", STANDARD.encode(signature.to_bytes())))
            .unwrap(),
    );
    let request = Request {
        method: &Method::POST,
        target_uri: TARGET,
        headers: &headers,
        content: BODY,
    };
    let peer_signing_keys = [PeerSigningKey::from_jwk(
        &json!({"kty": "OKP", "crv": "Ed25519", "x": B14_X, "kid": B14_KID}),
    )
    .unwrap()];
    let accepted = SignedRequest::parse(request)
        .unwrap()
        .verify(
            &peer_signing_keys,
            UNIX_EPOCH + Duration::from_secs(CREATED + 10),
        )
        .unwrap();
    let did: Did = B14_DID_KEY.parse().unwrap();
    let document = did_key_document();
    assert_eq!(
        verify_did_binding(
            &did,
            Some(DidDocument::Refreshed(&document)),
            accepted.key.verifying_key()
        ),
        Ok(())
    );
}

/// L219 requires an `authentication` key equal to the key that verified
/// the request; L221: "If the verification fails ... the receiving server
/// rejects the request". L225: "A `did:key` DID Document contains only
/// the key the DID encodes", so a peer domain key that is not test-key-ed25519
/// is not bound to test-key-ed25519's did:key.
#[test]
fn other_key_is_not_bound_to_did_key() {
    let document = did_key_document();
    assert_eq!(
        check(
            B14_DID_KEY,
            Some(DidDocument::Refreshed(&document)),
            OTHER_X
        ),
        Err(DidAuthRejection::KeyNotBound)
    );
}

/// W3C DID Core §5.3.1: "Each verification method MAY be embedded or
/// referenced." An embedded `publicKeyJwk` method.
#[test]
fn embedded_jwk_method_binds() {
    let document = plc_embedded(plc_jwk_method(
        json!({"kty": "OKP", "crv": "Ed25519", "x": B14_X}),
    ));
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&document)), B14_X),
        Ok(())
    );
}

/// W3C DID Core §3.2.2: "Relative DID URLs are often used to reference
/// verification methods"; the base URI "is the DID that is associated
/// with the DID subject". A `Multikey` method referenced as `#atproto`.
#[test]
fn referenced_multikey_method_binds() {
    let multibase = B14_DID_KEY.strip_prefix("did:key:").unwrap();
    let document = json!({
        "id": PLC,
        "verificationMethod": [{
            "id": "#atproto",
            "type": "Multikey",
            "controller": PLC,
            "publicKeyMultibase": multibase,
        }],
        "authentication": ["#atproto"],
    });
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&document)), B14_X),
        Ok(())
    );
}

/// L219: the method's public key must be "an Ed25519 public key
/// ({{RFC8032}})". The same 32 octets published as an X25519 JWK
/// (RFC 8037 §2: `crv` "X25519") are not an Ed25519 key.
#[test]
fn same_octets_of_another_key_type_do_not_bind() {
    let document = plc_embedded(plc_jwk_method(
        json!({"kty": "OKP", "crv": "X25519", "x": B14_X}),
    ));
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&document)), B14_X),
        Err(DidAuthRejection::KeyNotBound)
    );
}

/// L219 names the `authentication` verification relationship; a key
/// listed only under `assertionMethod` does not bind.
#[test]
fn key_outside_authentication_does_not_bind() {
    let mut document = did_key_document();
    let vm = document["authentication"].take();
    document["assertionMethod"] = vm;
    assert_eq!(
        check(B14_DID_KEY, Some(DidDocument::Refreshed(&document)), B14_X),
        Err(DidAuthRejection::KeyNotBound)
    );
}

/// W3C DID Core §5.2.1 publicKeyJwk: "The map MUST NOT contain "d", or
/// any other members of the private information class". A method that
/// carries `d` is not used.
#[test]
fn jwk_with_private_member_is_ignored() {
    let document = plc_embedded(plc_jwk_method(
        json!({"kty": "OKP", "crv": "Ed25519", "x": B14_X, "d": B14_D}),
    ));
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&document)), B14_X),
        Err(DidAuthRejection::KeyNotBound)
    );
}

/// L221: "If the verification fails, including because the DID cannot be
/// resolved ({{resolution}}) ... the receiving server rejects the
/// request." L237: with no usable cached document "the DID-bound
/// verification fails".
#[test]
fn resolution_failure_is_rejected() {
    assert_eq!(
        check(B14_DID_KEY, None, B14_X),
        Err(DidAuthRejection::Unresolved)
    );
}

/// W3C DID Core §7.1: "The value of id in the resolved DID document MUST
/// match the DID that was resolved."
#[test]
fn document_for_another_did_is_rejected() {
    let document = did_key_document();
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&document)), B14_X),
        Err(DidAuthRejection::DocumentIdMismatch)
    );
}

/// L233: "If the key that verified a request signature is not in the
/// `authentication` verification relationship of the cached DID Document
/// ..., the server MUST resolve the DID again, bypassing its cache,
/// before failing the verification." The cached document holds the
/// RFC 8037 A.1 key; the peer rotated to test-key-ed25519, which the
/// refreshed document holds.
#[test]
fn key_absent_from_cached_document_requires_refresh() {
    let jwk = |x| json!({"kty": "OKP", "crv": "Ed25519", "x": x});
    let cached = plc_embedded(plc_jwk_method(jwk(OTHER_X)));
    assert_eq!(
        check(PLC, Some(DidDocument::Cached(&cached)), B14_X),
        Err(DidAuthRejection::RefreshRequired)
    );
    let refreshed = plc_embedded(plc_jwk_method(jwk(B14_X)));
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&refreshed)), B14_X),
        Ok(())
    );
}

/// L233 makes only an absent key the refresh trigger: a cached document
/// that holds the key binds it without a refresh.
#[test]
fn key_present_in_cached_document_binds() {
    let cached = did_key_document();
    assert_eq!(
        check(B14_DID_KEY, Some(DidDocument::Cached(&cached)), B14_X),
        Ok(())
    );
}

/// L237: servers "SHOULD NOT ... proceed with stale cached state when the
/// cache entry has been explicitly invalidated, including by the refresh
/// requirement above"; L221: on failure the server "rejects the request".
/// A refreshed document that still lacks the key is a plain rejection,
/// not another refresh.
#[test]
fn key_absent_from_refreshed_document_is_rejected() {
    let refreshed = plc_embedded(plc_jwk_method(
        json!({"kty": "OKP", "crv": "Ed25519", "x": OTHER_X}),
    ));
    assert_eq!(
        check(PLC, Some(DidDocument::Refreshed(&refreshed)), B14_X),
        Err(DidAuthRejection::KeyNotBound)
    );
}

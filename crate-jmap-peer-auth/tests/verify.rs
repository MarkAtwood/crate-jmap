//! Request verification (draft-atwood-jmap-chat-federation-00 §Request
//! Signing, checks 1 to 6). Each request is built by hand: the
//! `Signature-Input` text and the RFC 9421 §2.5 signature base are
//! written out in the test and signed with ed25519-dalek directly.

use std::cell::RefCell;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use http::{HeaderMap, HeaderValue, Method};
use jmap_peer_auth::ed25519_dalek::{Signer as _, SigningKey};
use jmap_peer_auth::{Accepted, PeerDomain, PeerSigningKey, Rejection, Request, SignedRequest};
use serde_json::json;

const KEYID: &str = "alice.example#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";
const TARGET: &str = "https://bob.example/jmap/api/";
const BODY: &[u8] = br#"{"hello": "world"}"#;
/// RFC 9530 Appendix D: sha-256 of {"hello": "world"}.
const DIGEST: &str = "sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:";
const NONCE: &str = "6QaXn8jf3mJwV1ce0bVb1A";
const CREATED: i64 = 1_760_000_000;

/// RFC 8037 Appendix A.1 private key.
fn alice_key() -> SigningKey {
    let d = URL_SAFE_NO_PAD
        .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
        .unwrap();
    SigningKey::from_bytes(&d.try_into().unwrap())
}

/// alice.example's usable keys: the draft's `peerSigningKeys` example.
fn alice_keys() -> Vec<PeerSigningKey> {
    vec![PeerSigningKey::from_jwk(&json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
        "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
    }))
    .unwrap()]
}

fn at(secs: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs as u64)
}

/// One hand-built request.
struct Case {
    method: Method,
    body: &'static [u8],
    /// The `Signature-Input` members as (label, inner-list text).
    inputs: Vec<(&'static str, String)>,
    /// The signature base lines before `"@signature-params"`, as signed.
    signed_lines: String,
    content_digest: Option<&'static str>,
    /// The target URI the receiver sees.
    target: &'static str,
}

fn params(covered: &str, created: i64, expires: i64, tail: &str) -> String {
    format!(
        "({covered});created={created};expires={expires};nonce=\"{NONCE}\";keyid=\"{KEYID}\"{tail}"
    )
}

const TAIL: &str = r#";alg="ed25519";tag="jmap-chat-peer""#;
const POST_COVERED: &str = r#""@method" "@target-uri" "content-digest""#;

impl Case {
    /// A valid signed `Peer/*` POST.
    fn post() -> Self {
        Self::post_with(params(POST_COVERED, CREATED, CREATED + 300, TAIL))
    }

    fn post_with(inner_list: String) -> Self {
        Case {
            method: Method::POST,
            body: BODY,
            inputs: vec![("sig1", inner_list)],
            signed_lines: format!(
                "\"@method\": POST\n\"@target-uri\": {TARGET}\n\"content-digest\": {DIGEST}\n"
            ),
            content_digest: Some(DIGEST),
            target: TARGET,
        }
    }

    /// A valid signed session GET.
    fn get() -> Self {
        Case {
            method: Method::GET,
            body: b"",
            inputs: vec![(
                "sig1",
                params(r#""@method" "@target-uri""#, CREATED, CREATED + 300, TAIL),
            )],
            signed_lines: format!("\"@method\": GET\n\"@target-uri\": {TARGET}\n"),
            content_digest: None,
            target: TARGET,
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let mut inputs = Vec::new();
        let mut signatures = Vec::new();
        for (label, inner_list) in &self.inputs {
            let base = format!("{}\"@signature-params\": {inner_list}", self.signed_lines);
            let signature = alice_key().sign(base.as_bytes()).to_bytes();
            inputs.push(format!("{label}={inner_list}"));
            signatures.push(format!("{label}=:{}:", STANDARD.encode(signature)));
        }
        if !inputs.is_empty() {
            headers.insert(
                "signature-input",
                HeaderValue::from_str(&inputs.join(", ")).unwrap(),
            );
            headers.insert(
                "signature",
                HeaderValue::from_str(&signatures.join(", ")).unwrap(),
            );
        }
        if let Some(digest) = self.content_digest {
            headers.insert("content-digest", HeaderValue::from_static(digest));
        }
        headers
    }

    fn verify_with(
        &self,
        headers: &HeaderMap,
        now: i64,
        seen: bool,
    ) -> Result<Accepted, Rejection> {
        let request = Request {
            method: &self.method,
            target_uri: self.target,
            headers,
            content: self.body,
        };
        SignedRequest::parse(request)?.verify(&alice_keys(), at(now), |_, _| seen)
    }

    fn verify(&self, now: i64) -> Result<Accepted, Rejection> {
        self.verify_with(&self.headers(), now, false)
    }
}

#[test]
fn valid_requests_are_accepted() {
    // "The peer domain named in a verified `keyid` is the request's
    // authenticated peer domain."
    for case in [Case::post(), Case::get()] {
        let accepted = case.verify(CREATED).unwrap();
        assert_eq!(
            accepted.peer_domain,
            PeerDomain::parse("alice.example").unwrap()
        );
        assert_eq!(accepted.nonce, NONCE);
        // "MUST remember each accepted (peer domain, `nonce`) pair at least
        // until 60 seconds after that signature's `expires`."
        assert_eq!(accepted.forget_after, at(CREATED + 300 + 60));
        assert_eq!(accepted.created, at(CREATED));
    }
}

#[test]
fn parse_exposes_keyid_parts() {
    let case = Case::post();
    let headers = case.headers();
    let request = Request {
        method: &case.method,
        target_uri: TARGET,
        headers: &headers,
        content: BODY,
    };
    let signed = SignedRequest::parse(request).unwrap();
    assert_eq!(signed.peer_domain().as_str(), "alice.example");
    assert_eq!(signed.kid(), "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
}

/// "A receiving server MUST reject ... a request that lacks such a
/// signature".
#[test]
fn check1_missing_signature() {
    let mut case = Case::post();
    case.inputs.clear();
    assert_eq!(case.verify(CREATED), Err(Rejection::NoSignature));

    // A signature with another tag is not "such a signature".
    let case = Case::post_with(params(
        POST_COVERED,
        CREATED,
        CREATED + 300,
        r#";alg="ed25519";tag="other-app""#,
    ));
    assert_eq!(case.verify(CREATED), Err(Rejection::NoSignature));
    let case = Case::post_with(params(
        POST_COVERED,
        CREATED,
        CREATED + 300,
        r#";alg="ed25519""#,
    ));
    assert_eq!(case.verify(CREATED), Err(Rejection::NoSignature));
}

/// Check 1: "The request carries exactly one signature tagged
/// `"jmap-chat-peer"`".
#[test]
fn check1_exactly_one_tagged_signature() {
    let mut case = Case::post();
    case.inputs
        .push(("sig2", params(POST_COVERED, CREATED, CREATED + 300, TAIL)));
    assert_eq!(case.verify(CREATED), Err(Rejection::MultipleSignatures));

    // Signatures with other tags alongside are fine.
    let mut case = Case::post();
    case.inputs.insert(
        0,
        (
            "other",
            params(POST_COVERED, CREATED, CREATED + 300, r#";tag="other-app""#),
        ),
    );
    assert!(case.verify(CREATED).is_ok());
}

/// Check 1: "its `alg` is `ed25519`"; the signature "MUST include the
/// `alg` parameter with that value".
#[test]
fn check1_alg_ed25519() {
    for tail in [
        r#";alg="rsa-pss-sha512";tag="jmap-chat-peer""#,
        r#";tag="jmap-chat-peer""#,
    ] {
        let case = Case::post_with(params(POST_COVERED, CREATED, CREATED + 300, tail));
        assert_eq!(
            case.verify(CREATED),
            Err(Rejection::WrongAlgorithm),
            "{tail}"
        );
    }
}

/// Check 1: "it covers the components listed above": `@method`,
/// `@target-uri`, and `content-digest` "when the request has content".
#[test]
fn check1_covered_components() {
    let cases = [
        (r#""@target-uri" "content-digest""#, "@method"),
        (r#""@method" "content-digest""#, "@target-uri"),
        (r#""@method" "@target-uri""#, "content-digest"),
    ];
    for (covered, missing) in cases {
        let case = Case::post_with(params(covered, CREATED, CREATED + 300, TAIL));
        assert_eq!(
            case.verify(CREATED),
            Err(Rejection::MissingComponent(missing)),
            "{covered}"
        );
    }
}

/// Check 2: "The `keyid` parses as a peer domain and a `kid`".
#[test]
fn check2_keyid_must_parse() {
    for keyid in [
        "alice.example",
        "alice.example#",
        "#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
        "alice example#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
        // An IPv4 literal is not a peer domain (§Terminology).
        "192.0.2.1#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
        "127.0.0.1#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
        "10.0.0.1#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
    ] {
        let inner = params(POST_COVERED, CREATED, CREATED + 300, TAIL).replace(KEYID, keyid);
        assert_eq!(
            Case::post_with(inner).verify(CREATED),
            Err(Rejection::InvalidKeyId),
            "{keyid}"
        );
    }
}

/// Check 2: "the `kid` names a usable key for that peer domain".
#[test]
fn check2_kid_must_name_usable_key() {
    let inner = params(POST_COVERED, CREATED, CREATED + 300, TAIL).replace(
        "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
        "test-key-ed25519",
    );
    assert_eq!(
        Case::post_with(inner).verify(CREATED),
        Err(Rejection::UnknownKey)
    );

    let case = Case::post();
    let headers = case.headers();
    let request = Request {
        method: &case.method,
        target_uri: TARGET,
        headers: &headers,
        content: BODY,
    };
    let result = SignedRequest::parse(request)
        .unwrap()
        .verify(&[], at(CREATED), |_, _| false);
    assert_eq!(result, Err(Rejection::UnknownKey));
}

/// Check 3: "The signature verifies under that key". Covering
/// `@target-uri` "binds the signature to the receiving server, so a
/// signature made for one peer cannot be replayed to another."
#[test]
fn check3_signature_must_verify() {
    let mut replayed = Case::post();
    replayed.target = "https://carol.example/jmap/api/";
    assert_eq!(replayed.verify(CREATED), Err(Rejection::BadSignature));

    let mut wrong_method = Case::post();
    wrong_method.method = Method::PUT;
    assert_eq!(wrong_method.verify(CREATED), Err(Rejection::BadSignature));

    let case = Case::post();
    let mut headers = case.headers();
    let tampered = headers["signature-input"]
        .to_str()
        .unwrap()
        .replace("created=1760000000", "created=1760000001");
    headers.insert("signature-input", HeaderValue::from_str(&tampered).unwrap());
    assert_eq!(
        case.verify_with(&headers, CREATED, false),
        Err(Rejection::BadSignature)
    );

    let mut headers = case.headers();
    headers.insert("signature", HeaderValue::from_static("sig1=:AAAA:"));
    assert_eq!(
        case.verify_with(&headers, CREATED, false),
        Err(Rejection::BadSignature)
    );
}

/// Check 4: "`created` is no more than 60 seconds later than the
/// receiver's current time, `expires` is no more than 300 seconds after
/// `created`, and the receiver's current time is no more than 60 seconds
/// after `expires`."
#[test]
fn check4_clock_window() {
    let case = Case::post();
    assert!(case.verify(CREATED - 60).is_ok());
    assert_eq!(case.verify(CREATED - 61), Err(Rejection::CreatedInFuture));
    assert!(case.verify(CREATED + 300 + 60).is_ok());
    assert_eq!(case.verify(CREATED + 300 + 61), Err(Rejection::Expired));

    let too_long = Case::post_with(params(POST_COVERED, CREATED, CREATED + 301, TAIL));
    assert_eq!(too_long.verify(CREATED), Err(Rejection::LifetimeTooLong));
}

/// Check 5: "The receiver has not accepted a request carrying the same
/// peer domain and `nonce` before."
#[test]
fn check5_replay() {
    let case = Case::post();
    let headers = case.headers();
    assert_eq!(
        case.verify_with(&headers, CREATED, true),
        Err(Rejection::Replayed)
    );

    let asked = RefCell::new(None);
    let request = Request {
        method: &case.method,
        target_uri: TARGET,
        headers: &headers,
        content: BODY,
    };
    SignedRequest::parse(request)
        .unwrap()
        .verify(&alice_keys(), at(CREATED), |domain, nonce| {
            asked.replace(Some((domain.to_string(), nonce.to_owned())));
            false
        })
        .unwrap();
    assert_eq!(
        asked.into_inner(),
        Some(("alice.example".to_owned(), NONCE.to_owned()))
    );
}

/// Check 6: "For a request with content, the `content-digest` field
/// contains a `sha-256` digest and that digest matches the content."
#[test]
fn check6_content_digest() {
    // The signature covers the digest, so a body swapped under an intact
    // signature is caught by the digest.
    let mut swapped = Case::post();
    swapped.body = br#"{"hello": "mallory"}"#;
    assert_eq!(
        swapped.verify(CREATED),
        Err(Rejection::ContentDigestMismatch)
    );

    // RFC 9530 Appendix D sha-512 of the same content: no sha-256 member.
    const SHA512: &str = "sha-512=:WZDPaVn/7XgHaAy8pmojAkGWoRx2UFChF41A2svX+TaPm+AbwAgBWnrIiYllu7BNNyealdVLvRwEmTHWXvJwew==:";
    let mut sha512_only = Case::post();
    sha512_only.content_digest = Some(SHA512);
    sha512_only.signed_lines =
        format!("\"@method\": POST\n\"@target-uri\": {TARGET}\n\"content-digest\": {SHA512}\n");
    assert_eq!(
        sha512_only.verify(CREATED),
        Err(Rejection::ContentDigestMismatch)
    );

    let mut absent = Case::post();
    absent.content_digest = None;
    assert_eq!(
        absent.verify(CREATED),
        Err(Rejection::ContentDigestMismatch)
    );
}

#[test]
fn malformed_signature_fields() {
    let without = |param: &str| {
        let inner = params(POST_COVERED, CREATED, CREATED + 300, TAIL);
        let start = inner.find(&format!(";{param}=")).unwrap();
        let end = inner[start + 1..]
            .find(';')
            .map_or(inner.len(), |i| start + 1 + i);
        format!("{}{}", &inner[..start], &inner[end..])
    };
    for param in ["created", "expires", "nonce", "keyid"] {
        assert!(
            matches!(
                Case::post_with(without(param)).verify(CREATED),
                Err(Rejection::Malformed(_))
            ),
            "{param}"
        );
    }

    let case = Case::post();
    let mut headers = case.headers();
    headers.remove("signature");
    assert!(matches!(
        case.verify_with(&headers, CREATED, false),
        Err(Rejection::Malformed(_))
    ));
    headers.insert(
        "signature-input",
        HeaderValue::from_static("sig1=(\"@method\""),
    );
    assert!(matches!(
        case.verify_with(&headers, CREATED, false),
        Err(Rejection::Malformed(_))
    ));

    let repeated = params(
        r#""@method" "@method" "@target-uri" "content-digest""#,
        CREATED,
        CREATED + 300,
        TAIL,
    );
    assert!(matches!(
        Case::post_with(repeated).verify(CREATED),
        Err(Rejection::Malformed(_))
    ));
}

/// Covering more than the required components is allowed by RFC 9421;
/// the receiver derives each one.
#[test]
fn extra_components() {
    let covered = r#""@method" "@authority" "@path" "@target-uri" "content-digest" "content-type""#;
    let mut case = Case::post_with(params(covered, CREATED, CREATED + 300, TAIL));
    case.signed_lines = format!(
        "\"@method\": POST\n\"@authority\": bob.example\n\"@path\": /jmap/api/\n\"@target-uri\": {TARGET}\n\"content-digest\": {DIGEST}\n\"content-type\": application/json\n"
    );
    let mut headers = case.headers();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    assert!(case.verify_with(&headers, CREATED, false).is_ok());

    headers.remove("content-type");
    assert_eq!(
        case.verify_with(&headers, CREATED, false),
        Err(Rejection::UnderivableComponent("content-type".to_owned()))
    );

    let unsupported = params(
        r#""@method" "@target-uri" "content-digest" "@query-param";name="a""#,
        CREATED,
        CREATED + 300,
        TAIL,
    );
    assert_eq!(
        Case::post_with(unsupported).verify(CREATED),
        Err(Rejection::UnderivableComponent("@query-param".to_owned()))
    );
}

//! RFC 8292 VAPID header tests.
//!
//! Oracles: the RFC 8292 §2.4 example token (header and claims segments)
//! and the RFC 8291 §5 example sender key pair (the `k` parameter).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_web_push::p256::ecdsa::signature::Verifier as _;
use jmap_web_push::p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use jmap_web_push::url::Url;
use jmap_web_push::{Vapid, VapidError};

/// RFC 8291 §5, "Sender: private key".
const SENDER_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
/// RFC 8291 §5, "Sender: public key".
const SENDER_PUBLIC: &str =
    "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";

/// RFC 8292 §2.4, Figure 1: the `t` and `k` parameters.
const RFC8292_T: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiJ9.eyJhdWQiOiJodHRwczovL3B1c2guZXhhbXBsZS5uZXQiLCJleHAiOjE0NTM1MjM3NjgsInN1YiI6Im1haWx0bzpwdXNoQGV4YW1wbGUuY29tIn0.i3CYb7t4xfxCDquptFOepC9GAu_HLGkMlMuCGSK2rpiUfnK9ojFwDXb1JrErtmysazNjjvW2L9OkSSHzvoD1oA";
const RFC8292_K: &str =
    "BA1Hxzyi1RUM1b5wjxsn7nGxAszw2u61m164i3MrAIxHF6YK5h4SDYic-dRuU_RCPCfA5aq9ojSwk5Y2EmClBPs";

fn b64(s: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(s).unwrap()
}

fn vapid() -> Vapid {
    let key = SigningKey::from_slice(&b64(SENDER_PRIVATE)).unwrap();
    Vapid::new(key, "mailto:push@example.com")
}

/// Splits `vapid t=<jwt>, k=<key>` (RFC 8292 §3).
fn params(header: &str) -> (&str, &str) {
    let rest = header
        .strip_prefix("vapid t=")
        .expect("vapid scheme, t first");
    rest.split_once(", k=").expect("k after t")
}

/// RFC 7515 §5.2 / RFC 7518 §3.4: the ES256 signature is the 64-octet
/// r || s over `<header>.<claims>`.
fn verifies(token: &str, k: &str) -> bool {
    let (input, sig) = token.rsplit_once('.').unwrap();
    let key = VerifyingKey::from_sec1_bytes(&b64(k)).unwrap();
    let sig = Signature::from_slice(&b64(sig)).unwrap();
    key.verify(input.as_bytes(), &sig).is_ok()
}

/// The check `verifies` makes accepts the RFC's own example token, so it
/// is a valid oracle for ours.
#[test]
fn rfc8292_example_token_verifies() {
    assert!(verifies(RFC8292_T, RFC8292_K));
}

/// RFC 8292 §2.4 Figure 2: JWT header `{ "typ": "JWT", "alg": "ES256" }`,
/// body `{ "aud": "https://push.example.net", "exp": 1453523768, "sub":
/// "mailto:push@example.com" }`. The same inputs give the same two
/// segments; the signature differs (another key) and verifies against
/// `k`.
#[test]
fn token_matches_rfc8292_example() {
    let url = Url::parse("https://push.example.net/p/JzLQ3raZJfFBR0aqvOMsLrt54w4rJUsV").unwrap();
    let header = vapid().authorization(&url, 1453523768).unwrap();
    let (t, k) = params(&header);

    let (rfc_segments, _) = RFC8292_T.rsplit_once('.').unwrap();
    let (segments, _) = t.rsplit_once('.').unwrap();
    assert_eq!(segments, rfc_segments);
    assert!(verifies(t, k));
}

/// RFC 8292 §3.2: "The "k" parameter includes an ECDSA public key
/// [FIPS186] in uncompressed form [X9.62] that is encoded using base64url
/// encoding".
#[test]
fn k_is_the_uncompressed_public_key() {
    let url = Url::parse("https://push.example.net/p/x").unwrap();
    let header = vapid().authorization(&url, 1453523768).unwrap();
    assert_eq!(params(&header).1, SENDER_PUBLIC);
    assert_eq!(vapid().public_key(), SENDER_PUBLIC);
}

/// RFC 8292 §2: "An "aud" (Audience) claim in the token MUST include the
/// Unicode serialization of the origin (Section 6.1 of [RFC6454]) of the
/// push resource URL." RFC 6454 §6.1 keeps a port that is not the
/// scheme's default.
#[test]
fn aud_is_the_origin_with_port() {
    let claims = |url: &str| {
        let header = vapid().authorization(&Url::parse(url).unwrap(), 1).unwrap();
        let claims = params(&header).0.split('.').nth(1).unwrap().to_owned();
        serde_json::from_slice::<serde_json::Value>(&b64(&claims)).unwrap()
    };
    assert_eq!(
        claims("https://push.example.net:8443/p/x?y=z")["aud"],
        "https://push.example.net:8443"
    );
    assert_eq!(
        claims("https://push.example.net:443/p/x")["aud"],
        "https://push.example.net"
    );
}

/// A URL with no tuple origin has no `aud`.
#[test]
fn opaque_origin_is_an_error() {
    let url = Url::parse("mailto:push@example.com").unwrap();
    assert!(matches!(
        vapid().authorization(&url, 1),
        Err(VapidError::OpaqueOrigin)
    ));
}

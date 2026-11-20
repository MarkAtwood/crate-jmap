//! Test helpers: throwaway keys generated per test run and JWT checks done
//! with crates other than the ones the code under test signs with (`rsa`
//! for RS256, ring for ES256).

#![allow(dead_code)]

use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jmap_platform_push::Message;
use rsa::pkcs8::{EncodePrivateKey as _, LineEnding};
use rsa::sha2::Sha256;
use rsa::{Generate as _, PrivateKey as RsaPrivateKey, PublicKey as RsaPublicKey};
use serde_json::{Value, json};
use wiremock::Request;

/// A throwaway RSA key (the `rsa` default size, 3072 bits), made once per
/// test binary: its PKCS #8 PEM and its public key.
pub fn rsa_key() -> &'static (String, RsaPublicKey) {
    static KEY: OnceLock<(String, RsaPublicKey)> = OnceLock::new();
    KEY.get_or_init(|| {
        let key = RsaPrivateKey::generate();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
        (pem, key.to_public_key())
    })
}

/// A throwaway P-256 key from ring: its PKCS #8 PEM and its uncompressed
/// public point.
pub fn ec_key() -> (String, Vec<u8>) {
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    (
        pem("PRIVATE KEY", pkcs8.as_ref()),
        pair.public_key().as_ref().to_vec(),
    )
}

fn pem(label: &str, der: &[u8]) -> String {
    let b64 = STANDARD.encode(der);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

fn jwt_parts(jwt: &str) -> (Value, Value, Vec<u8>, String) {
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3, "JWS compact form has three parts");
    let header = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    let claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
    (header, claims, sig, format!("{}.{}", parts[0], parts[1]))
}

/// Verifies an RS256 JWT (RFC 7518 §3.3) with `rsa`; returns header and
/// claims.
pub fn verify_rs256(jwt: &str, key: &RsaPublicKey) -> (Value, Value) {
    let (header, claims, sig, input) = jwt_parts(jwt);
    key.pkcs1v15_verify::<Sha256>(input.as_bytes(), &sig)
        .expect("RS256 signature verifies");
    (header, claims)
}

/// Verifies an ES256 JWT (RFC 7518 §3.4: R || S, 64 octets) with ring;
/// returns header and claims.
pub fn verify_es256(jwt: &str, public: &[u8]) -> (Value, Value) {
    let (header, claims, sig, input) = jwt_parts(jwt);
    assert_eq!(sig.len(), 64);
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, public)
        .verify(input.as_bytes(), &sig)
        .expect("ES256 signature verifies");
    (header, claims)
}

/// A `StateChange` push (RFC 8620 §7.1 example shape) with a one-hour TTL.
pub fn state_change() -> Message {
    Message::new(
        &json!({ "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } }),
        Duration::from_secs(3600),
    )
    .unwrap()
}

pub fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers.get(name).map(|v| v.to_str().unwrap())
}

/// The decoded `application/x-www-form-urlencoded` body of `req`.
pub fn form(req: &Request) -> Vec<(String, String)> {
    url::form_urlencoded::parse(&req.body)
        .into_owned()
        .collect()
}

pub fn form_value(req: &Request, key: &str) -> Option<String> {
    form(req)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// An OAuth token endpoint answer (RFC 6749 §5.1).
pub fn token_response(token: &str) -> Value {
    json!({ "access_token": token, "token_type": "Bearer", "expires_in": 3600 })
}

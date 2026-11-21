//! LiveKit access tokens against an independent oracle.
//!
//! Oracles: "Tokens & grants",
//! <https://docs.livekit.io/frontends/reference/tokens-grants/> (claim
//! names, the example join-token payload, `roomRecord` "Permission to use
//! Egress service"), and HS256 values computed outside this crate with
//! Python's standard library (`hmac`, `hashlib`, `base64`): the
//! `DOC_TOKEN` signs the documented example payload, and the expected
//! token of `signs_the_documented_claims` was checked there (and with
//! `openssl dgst -sha256 -hmac`) to carry an HMAC-SHA256 over its own
//! signing input.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_vtc_livekit::token::{ApiKey, TokenError, VideoGrant};
use serde_json::{Value, json};

const SECRET: &str = "oracle-secret-0123456789abcdef";

/// The documented example payload (`exp` 1621657263, `iss`
/// `APIMmxiL8rquKztZEoZJV9Fb`, `nbf` 1619065263, `sub` `myidentity`,
/// `video` `{"room": "myroom", "roomJoin": true}`, `metadata` `""`),
/// signed with [`SECRET`] by the Python oracle.
const DOC_TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjE2MjE2NTcyNjMsImlzcyI6IkFQSU1teGlMOHJxdUt6dFpFb1pKVjlGYiIsIm5iZiI6MTYxOTA2NTI2Mywic3ViIjoibXlpZGVudGl0eSIsInZpZGVvIjp7InJvb20iOiJteXJvb20iLCJyb29tSm9pbiI6dHJ1ZX0sIm1ldGFkYXRhIjoiIn0.XFGwQoiXTG0XLaxBUQKCBNfNevaUYgfyNTIDgDPA4bg";

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[test]
fn verifies_the_documented_example() {
    let key = ApiKey::new("APIMmxiL8rquKztZEoZJV9Fb", SECRET);
    let claims = key.verify(DOC_TOKEN, at(1_620_000_000)).unwrap();
    assert_eq!(claims.issuer, "APIMmxiL8rquKztZEoZJV9Fb");
    assert_eq!(claims.subject.as_deref(), Some("myidentity"));
    assert_eq!(claims.not_before, Some(at(1_619_065_263)));
    assert_eq!(claims.expires_at, at(1_621_657_263));
    assert_eq!(claims.video, Some(VideoGrant::room_join("myroom")));
    // Outside its validity window (plus the minute of leeway).
    assert_eq!(
        key.verify(DOC_TOKEN, at(1_621_657_263 + 61)),
        Err(TokenError::Expired)
    );
    assert_eq!(
        key.verify(DOC_TOKEN, at(1_619_065_263 - 61)),
        Err(TokenError::NotYetValid)
    );
    // Another secret, another key id.
    assert_eq!(
        ApiKey::new("APIMmxiL8rquKztZEoZJV9Fb", "x").verify(DOC_TOKEN, at(1_620_000_000)),
        Err(TokenError::Signature)
    );
    assert_eq!(
        ApiKey::new("APIother", SECRET).verify(DOC_TOKEN, at(1_620_000_000)),
        Err(TokenError::Issuer)
    );
}

/// The token the Egress client sends: `iss` the key, `iat`/`nbf` now,
/// `exp` later, `video.roomRecord` and nothing else.
#[test]
fn signs_the_documented_claims() {
    let key = ApiKey::new("APIegress", SECRET);
    let token = key.sign(
        &key.claims(at(1_700_000_000), Duration::from_secs(600))
            .with_video(VideoGrant::room_record()),
    );
    let parts: Vec<&str> = token.split('.').collect();
    let json =
        |s: &str| -> Value { serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).unwrap()).unwrap() };
    assert_eq!(json(parts[0]), json!({"alg": "HS256", "typ": "JWT"}));
    assert_eq!(
        json(parts[1]),
        json!({
            "iss": "APIegress",
            "iat": 1_700_000_000,
            "nbf": 1_700_000_000,
            "exp": 1_700_000_600,
            "video": {"roomRecord": true}
        })
    );
    assert_eq!(token, EXPECTED_EGRESS_TOKEN);
}

/// [`signs_the_documented_claims`]'s token; the Python oracle and
/// `openssl dgst -sha256 -hmac` both confirmed its signature is
/// HMAC-SHA256 with [`SECRET`] over its first two segments.
const EXPECTED_EGRESS_TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjE3MDAwMDA2MDAsImlhdCI6MTcwMDAwMDAwMCwiaXNzIjoiQVBJZWdyZXNzIiwibmJmIjoxNzAwMDAwMDAwLCJ2aWRlbyI6eyJyb29tUmVjb3JkIjp0cnVlfX0.JAOHE7hqtqie8wo90gmdc_6oe4F7JdDDtFu2RzRGp6Q";

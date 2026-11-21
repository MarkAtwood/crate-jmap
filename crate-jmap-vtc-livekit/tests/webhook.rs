//! Webhook verification against an independent oracle.
//!
//! LiveKit publishes no signed example webhook, so the token below was
//! made outside this crate, as livekit/protocol `webhook/url_notifier.go`
//! makes one: the body is protojson (lowerCamelCase names, `int64` as
//! strings, default values left out); the token is an HS256 access token
//! of the API key (`iss`), valid for 5 minutes, with `sha256` the standard
//! base64 SHA-256 of the body ("The token includes a sha256 hash of the
//! payload", <https://docs.livekit.io/home/server/webhooks/>). Computed
//! with Python's `hmac`, `hashlib` and `base64`; the checksum also with
//! `openssl dgst -sha256 -binary | base64`.

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jmap_vtc_livekit::egress::EgressStatus;
use jmap_vtc_livekit::token::{ApiKey, TokenError};
use jmap_vtc_livekit::webhook::{WebhookError, WebhookEventKind, WebhookReceiver};

const SECRET: &str = "oracle-secret-0123456789abcdef";

const BODY: &str = r#"{"event":"egress_ended","egressInfo":{"egressId":"EG_wh1","roomId":"RM_1","roomName":"vtc-C1","status":"EGRESS_COMPLETE","startedAt":"1700000000000000000","endedAt":"1700000065000000000","updatedAt":"1700000065000000000","fileResults":[{"filename":"jmap-vtc/recordings/R1.mp4","startedAt":"1700000000000000000","endedAt":"1700000065000000000","duration":"65400000000","size":"7340032","location":"https://bucket.s3.amazonaws.com/jmap-vtc/recordings/R1.mp4"}]},"id":"EV_3kq8sVQ2p7xH","createdAt":"1700000066"}"#;

/// `{"iss":"APIwebhook","nbf":1700000066,"iat":1700000066,
/// "exp":1700000366,"sha256":"QEiYGIOP6xZgT39quv5+7cDlTgoasZbiTsEoFTdmpjA="}`
/// signed with [`SECRET`].
const TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJBUEl3ZWJob29rIiwibmJmIjoxNzAwMDAwMDY2LCJpYXQiOjE3MDAwMDAwNjYsImV4cCI6MTcwMDAwMDM2Niwic2hhMjU2IjoiUUVpWUdJT1A2eFpnVDM5cXV2NSs3Y0RsVGdvYXNaYmlUc0VvRlRkbXBqQT0ifQ.IgkMo-jwkmZbBu4QJRHtaQqdEl8wxmQp4VVOYAR9oy4";

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn receiver() -> WebhookReceiver {
    WebhookReceiver::new(ApiKey::new("APIwebhook", SECRET))
}

#[test]
fn receives_a_signed_egress_ended() {
    let event = receiver()
        .receive(BODY.as_bytes(), TOKEN, at(1_700_000_070))
        .unwrap();
    assert_eq!(event.event, WebhookEventKind::EgressEnded);
    assert_eq!(event.id, "EV_3kq8sVQ2p7xH");
    assert_eq!(event.created_at, 1_700_000_066);
    let info = event.egress_info.expect("egressInfo");
    assert_eq!(info.egress_id, "EG_wh1");
    assert_eq!(info.status, EgressStatus::Complete);
    let file = &info.file_results[0];
    assert_eq!(file.duration, 65_400_000_000);
    assert_eq!(file.size, 7_340_032);
    assert_eq!(
        file.location,
        "https://bucket.s3.amazonaws.com/jmap-vtc/recordings/R1.mp4"
    );
}

#[test]
fn refusals() {
    let rx = receiver();
    let now = at(1_700_000_070);
    // One changed byte in the body.
    let tampered = BODY.replace("7340032", "7340033");
    assert!(matches!(
        rx.verify(tampered.as_bytes(), TOKEN, now),
        Err(WebhookError::Checksum)
    ));
    // Expired: exp + one minute of leeway has passed.
    assert!(matches!(
        rx.verify(BODY.as_bytes(), TOKEN, at(1_700_000_366 + 61)),
        Err(WebhookError::Token(TokenError::Expired))
    ));
    // The right key id with the wrong secret.
    assert!(matches!(
        WebhookReceiver::new(ApiKey::new("APIwebhook", "wrong")).verify(
            BODY.as_bytes(),
            TOKEN,
            now
        ),
        Err(WebhookError::Token(TokenError::Signature))
    ));
    // A key this receiver does not hold.
    assert!(matches!(
        WebhookReceiver::new(ApiKey::new("APIother", SECRET)).verify(BODY.as_bytes(), TOKEN, now),
        Err(WebhookError::UnknownKey)
    ));
    assert!(matches!(
        rx.verify(BODY.as_bytes(), "not-a-jwt", now),
        Err(WebhookError::Token(TokenError::Malformed))
    ));
}

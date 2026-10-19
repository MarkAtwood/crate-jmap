//! The reference `MemoryBackend` implements `PushSubscriptionBackend`
//! (bd:JMAP-nbk3.4): verification codes are fixed-length and carry at least
//! 128 bits, and the per-user subscription cap returns `overQuota`.
//!
//! Oracle: RFC 8620 §7.2 text (~/PROJECT/IETF/specs/rfc8620.txt); each test
//! quotes the sentence it enforces.

use std::collections::HashSet;

use jmap_chat_server::memory::MemoryBackend;
use jmap_server::{BackendSetError, PushSubscriptionBackend, SetErrorType};
use jmap_types::PushSubscription;
use serde_json::json;

/// A create request as the `PushSubscription/set` handler passes it to the
/// backend: no `id`, no `verificationCode`.
fn new_subscription(n: usize) -> PushSubscription {
    serde_json::from_value(json!({
        "deviceClientId": format!("device-{n}"),
        "url": format!("https://push.example.com/endpoint/{n}"),
        "types": null,
    }))
    .expect("valid PushSubscription")
}

/// RFC 8620 §7.2.2 (rfc8620.txt:3784-3786), on `PushVerification`'s
/// `verificationCode`: "This MUST contain sufficient entropy to avoid the
/// client being able to guess the code via brute force." The trait contract
/// makes that a fixed-length code carrying at least 128 bits from a CSPRNG;
/// the reference backend encodes 16 random bytes as base64url, so each code
/// is at least 22 base64url characters (22 * 6 = 132 bits >= 128).
///
/// Each code is destroyed after it is read, so the per-user cap never stops
/// the loop.
#[tokio::test]
async fn verification_codes_are_fixed_length_and_distinct() {
    const N: usize = 256;
    let backend = MemoryBackend::new();
    let mut codes = HashSet::with_capacity(N);
    let mut lengths = HashSet::new();
    for n in 0..N {
        let stored = backend
            .create_push_subscription(&(), new_subscription(n))
            .await
            .expect("create under the cap succeeds");
        let code = stored.expected_code.trim_end_matches('=');
        assert!(
            code.len() >= 22,
            "code of {} base64url chars carries fewer than 128 bits",
            code.len()
        );
        assert!(
            code.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "code is not base64url"
        );
        lengths.insert(stored.expected_code.len());
        codes.insert(stored.expected_code.clone());
        let id = stored.subscription.id.expect("backend assigns an id");
        backend
            .destroy_push_subscription(&(), &id)
            .await
            .expect("destroy of own subscription succeeds");
    }
    assert_eq!(codes.len(), N, "verification codes repeat");
    assert_eq!(lengths.len(), 1, "verification codes vary in length");
}

/// RFC 8620 §8.6 (rfc8620.txt:4172-4175): "The server MUST limit the number
/// of push subscriptions any one user may have to ensure the user cannot
/// cause the server to send a large number of push notifications at once".
/// The trait contract maps reaching the cap to `overQuota`.
///
/// The cap's value is the backend's choice; 1000 creates bounds the search.
#[tokio::test]
async fn create_over_cap_returns_over_quota() {
    const BOUND: usize = 1000;
    let backend = MemoryBackend::new();
    for n in 0..BOUND {
        match backend
            .create_push_subscription(&(), new_subscription(n))
            .await
        {
            Ok(_) => continue,
            Err(BackendSetError::SetError(se)) => {
                assert_eq!(se.error_type, SetErrorType::OverQuota);
                assert!(n > 0, "cap rejects the first subscription");
                let listed = backend
                    .list_push_subscriptions(&())
                    .await
                    .expect("list succeeds");
                assert_eq!(listed.len(), n, "rejected create was stored");
                return;
            }
            Err(BackendSetError::Other(e)) => panic!("storage error: {e}"),
            Err(e) => panic!("unexpected backend error: {e:?}"),
        }
    }
    panic!("{BOUND} creates for one user never hit the subscription cap");
}

//! `freshness_lifetime` against RFC 9111 (HTTP Caching).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::{HeaderMap, HeaderValue};
use jmap_peer_auth_io::freshness_lifetime;

fn headers(fields: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in fields {
        map.append(*name, HeaderValue::from_static(value));
    }
    map
}

/// 1994-11-06T08:49:37Z, the RFC 9110 §5.6.7 example date.
fn received() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(784_111_777)
}

/// RFC 9111 §4.2.1: "If the max-age response directive (Section 5.2.2.1)
/// is present, use its value".
#[test]
fn max_age_is_the_lifetime() {
    let h = headers(&[("cache-control", "public, max-age=600")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::from_secs(600));
}

/// RFC 9111 §5.2: "Cache directives are identified by a token, to be
/// compared case-insensitively"; §5.2.2.1 max-age: "a cache ... SHOULD
/// accept the quoted-string form".
#[test]
fn directive_names_are_case_insensitive_and_quoted_values_accepted() {
    let h = headers(&[("cache-control", "Max-Age=\"120\"")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::from_secs(120));
}

/// RFC 9111 §4.2.1: "If the max-age response directive ... is present,
/// use its value, or If the Expires response header field (Section 5.3)
/// is present, use its value minus the value of the Date response header
/// field". max-age comes first.
#[test]
fn max_age_takes_precedence_over_expires() {
    let h = headers(&[
        ("cache-control", "max-age=60"),
        ("date", "Sun, 06 Nov 1994 08:49:37 GMT"),
        ("expires", "Sun, 06 Nov 1994 09:49:37 GMT"),
    ]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::from_secs(60));
}

/// RFC 9111 §4.2.1: Expires "minus the value of the Date response header
/// field": 09:49:37 - 08:49:37 = 3600 seconds.
#[test]
fn expires_minus_date() {
    let h = headers(&[
        ("date", "Sun, 06 Nov 1994 08:49:37 GMT"),
        ("expires", "Sun, 06 Nov 1994 09:49:37 GMT"),
    ]);
    assert_eq!(
        freshness_lifetime(&h, received()),
        Duration::from_secs(3600)
    );
}

/// RFC 9110 §6.6.1: a recipient of a response without Date "MUST record
/// the time it was received" and use it as the Date.
#[test]
fn expires_without_date_counts_from_receipt() {
    let h = headers(&[("expires", "Sun, 06 Nov 1994 08:59:37 GMT")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::from_secs(600));
}

/// RFC 9111 §5.3: "A cache recipient MUST interpret invalid date
/// formats, especially the value "0", as representing a time in the past
/// (i.e., "already expired")."
#[test]
fn invalid_expires_is_already_expired() {
    let h = headers(&[("date", "Sun, 06 Nov 1994 08:49:37 GMT"), ("expires", "0")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::ZERO);
}

/// RFC 9111 §4.2.1: "caches are encouraged to consider responses that
/// have invalid freshness information (e.g., a max-age directive with
/// non-integer content) to be stale."
#[test]
fn invalid_max_age_is_stale() {
    let h = headers(&[("cache-control", "max-age=soon")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::ZERO);
}

/// RFC 9111 §5.2.2.5 no-store: "a cache MUST NOT store any part of
/// either the immediate request or the response"; §5.2.2.4 no-cache:
/// "the response MUST NOT be used to satisfy any other request without
/// forwarding it for validation". RFC 9111 §4.2.1: "If directives
/// conflict ..., the most restrictive directive should be honored."
#[test]
fn no_store_and_no_cache_are_never_fresh() {
    let h = headers(&[("cache-control", "max-age=600, no-store")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::ZERO);
    let h = headers(&[
        ("cache-control", "max-age=600"),
        ("cache-control", "no-cache"),
    ]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::ZERO);
}

/// RFC 9111 §4.2: "response_is_fresh = (freshness_lifetime >
/// current_age)", and §5.1 Age is "the sender's estimate of the time
/// since the response was generated or successfully validated at the
/// origin server". 600 - 100 = 500 seconds remain.
#[test]
fn age_is_subtracted() {
    let h = headers(&[("cache-control", "max-age=600"), ("age", "100")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::from_secs(500));
    let h = headers(&[("cache-control", "max-age=600"), ("age", "900")]);
    assert_eq!(freshness_lifetime(&h, received()), Duration::ZERO);
}

/// No explicit expiration time: this crate does not compute a §4.2.2
/// heuristic lifetime, so the response is not reused.
#[test]
fn no_explicit_freshness_is_zero() {
    assert_eq!(
        freshness_lifetime(&HeaderMap::new(), received()),
        Duration::ZERO
    );
}

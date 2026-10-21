//! `PeerKeys` and `authenticate` against draft-atwood-jmap-chat-federation-00
//! §Key Publication, §DNSSEC Key Binding and §Request Signing, with a
//! stub fetcher, a stub DNSSEC resolver and an injected clock.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use http::{HeaderMap, HeaderValue, Method};
use jmap_peer_auth::ed25519_dalek::SigningKey;
use jmap_peer_auth::{sign, DnssecAnswer, PeerDomain, PeerSigningKey, Rejection, Request};
use jmap_peer_auth_io::{
    authenticate, DnsAnswer, DnssecLookup, Error, Fetch, FetchError, MemoryReplayStore, PeerKeys,
    Response,
};
use serde_json::{json, Value};

/// RFC 8037 Appendix A.2 public key; Appendix A.3 gives its thumbprint,
/// which the draft also uses as its example `kid`.
const X_A: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";
const KID_A: &str = "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";
/// RFC 8032 §7.1 TEST 2 public key; its RFC 7638 thumbprint, computed
/// with Python hashlib over `{"crv":"Ed25519","kty":"OKP","x":"..."}`.
const X_B: &str = "PUAXw-hDiVqStwqnTRt-vJyYLM8uxJaMwM1V8Sr0Zgw";
const KID_B: &str = "FtIu-VbGrfe_KB6CH7GNwODB72MNxj_ml11dEvO-7kk";

fn jwk(x: &str, kid: &str) -> Value {
    json!({"kty": "OKP", "crv": "Ed25519", "x": x, "kid": kid})
}

fn session(keys: &[Value]) -> Vec<u8> {
    json!({
        "capabilities": {},
        "accounts": {},
        "primaryAccounts": {},
        "username": "",
        "ownerUserId": "alice@alice.example",
        "peerSigningKeys": keys,
    })
    .to_string()
    .into_bytes()
}

fn response(cache_control: &'static str, body: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static(cache_control));
    Response { headers, body }
}

/// A fetcher that serves whatever the test sets and records each URL.
#[derive(Default)]
struct StubFetch {
    next: Mutex<Option<Result<Response, u16>>>,
    urls: Mutex<Vec<String>>,
}

impl StubFetch {
    fn serve(&self, response: Response) {
        *self.next.lock().unwrap() = Some(Ok(response));
    }
    fn fail(&self, status: u16) {
        *self.next.lock().unwrap() = Some(Err(status));
    }
    fn calls(&self) -> usize {
        self.urls.lock().unwrap().len()
    }
}

/// The fetcher handed to `PeerKeys`, sharing the test's `StubFetch`.
struct SharedFetch(Arc<StubFetch>);

impl Fetch for SharedFetch {
    async fn get(&self, url: &str) -> Result<Response, FetchError> {
        self.0.urls.lock().unwrap().push(url.to_owned());
        // Suspend once, as a real fetch would, so concurrent callers
        // overlap.
        tokio::task::yield_now().await;
        let next = self.0.next.lock().unwrap().clone();
        match next {
            Some(Ok(response)) => Ok(response),
            Some(Err(status)) => Err(FetchError::Status(status)),
            None => Err(FetchError::Status(404)),
        }
    }
}

/// A DNSSEC resolver that answers whatever the test sets and records
/// each queried name.
struct StubDns {
    answer: Mutex<DnsAnswer>,
    names: Mutex<Vec<String>>,
}

impl StubDns {
    fn new(answer: DnssecAnswer, ttl: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(DnsAnswer { answer, ttl }),
            names: Mutex::new(Vec::new()),
        })
    }
    fn calls(&self) -> usize {
        self.names.lock().unwrap().len()
    }
}

/// The resolver handed to `PeerKeys`, sharing the test's `StubDns`.
struct SharedDns(Arc<StubDns>);

impl DnssecLookup for SharedDns {
    async fn txt(&self, name: &str) -> DnsAnswer {
        self.0.names.lock().unwrap().push(name.to_owned());
        self.0.answer.lock().unwrap().clone()
    }
}

/// A clock the test moves forward.
#[derive(Clone)]
struct Clock(Arc<Mutex<Instant>>);

impl Clock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }
    fn advance(&self, secs: u64) {
        *self.0.lock().unwrap() += Duration::from_secs(secs);
    }
}

struct Fixture {
    fetch: Arc<StubFetch>,
    dns: Arc<StubDns>,
    clock: Clock,
    keys: PeerKeys<SharedFetch, SharedDns>,
}

fn fixture(dns: DnssecAnswer, ttl: Option<Duration>) -> Fixture {
    let fetch = Arc::new(StubFetch::default());
    let dns = StubDns::new(dns, ttl);
    let clock = Clock::new();
    let now = clock.clone();
    let keys = PeerKeys::new(SharedFetch(Arc::clone(&fetch)), SharedDns(Arc::clone(&dns)))
        .with_clock(move || *now.0.lock().unwrap());
    Fixture {
        fetch,
        dns,
        clock,
        keys,
    }
}

fn alice() -> PeerDomain {
    "alice.example".parse().unwrap()
}

fn kids(set: &jmap_peer_auth_io::PeerKeySet) -> Vec<&str> {
    set.keys.iter().map(|k| k.kid()).collect()
}

fn is_unknown_key(result: Result<jmap_peer_auth_io::PeerKeySet, Error>) -> bool {
    matches!(result, Err(Error::Rejected(Rejection::UnknownKey)))
}

/// §Key Publication: the receiving server "fetches
/// `https://<peer-domain>/.well-known/jmap`"; §DNSSEC Key Binding: the
/// TXT record is "at the name `_jmap-peer.<peer-domain>`".
#[tokio::test]
async fn fetches_the_well_known_session_and_queries_the_txt_name() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    let set = f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(kids(&set), [KID_A]);
    assert_eq!(set.owner_user_id.as_deref(), Some("alice@alice.example"));
    assert_eq!(
        *f.fetch.urls.lock().unwrap(),
        ["https://alice.example/.well-known/jmap"]
    );
    assert_eq!(*f.dns.names.lock().unwrap(), ["_jmap-peer.alice.example."]);
}

/// §Key Publication: "The receiving server MAY cache the keys of a peer
/// domain for the HTTP freshness lifetime of the response".
#[tokio::test]
async fn keys_are_cached_for_the_freshness_lifetime() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    f.clock.advance(599);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 1);
    f.clock.advance(1);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 2);
}

/// §Key Publication: "but for no longer than one hour".
#[tokio::test]
async fn keys_are_cached_for_at_most_one_hour() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=86400", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    f.clock.advance(3599);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 1);
    f.clock.advance(1);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 2);
}

/// §DNSSEC Key Binding: "A receiving server MUST NOT cache the DNS
/// answer for longer than its TTL." The keys filtered by that answer
/// expire with it. §Key Publication: "It MUST NOT refetch the keys of one
/// peer domain, or repeat that query, more often than once every 60
/// seconds", so between the TTL and 60 seconds the request is rejected.
#[tokio::test]
async fn keys_are_cached_for_at_most_the_dns_ttl() {
    let f = fixture(DnssecAnswer::Insecure, Some(Duration::from_secs(30)));
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    f.clock.advance(29);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.dns.calls(), 1);
    f.clock.advance(1);
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_A).await));
    assert_eq!(f.dns.calls(), 1);
    f.clock.advance(30);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.dns.calls(), 2);
}

/// A response that may not be stored is not reused, and §Key
/// Publication: "It MUST NOT refetch the keys of one peer domain ...
/// more often than once every 60 seconds; a request that arrives within
/// that interval and names a `kid` that is not among the usable keys is
/// rejected." With nothing cached, no key is usable.
#[tokio::test]
async fn a_no_store_response_is_not_reused() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("no-store", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_A).await));
    assert_eq!(f.fetch.calls(), 1);
    f.clock.advance(60);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 2);
}

/// The finding's reflection case: a peer domain whose `/.well-known/jmap`
/// carries no `Cache-Control` is fetched once per 60 seconds however many
/// requests name it with unknown `kid`s. §Key Publication: "It MUST NOT
/// refetch the keys of one peer domain, or repeat that query, more often
/// than once every 60 seconds".
#[tokio::test]
async fn forged_kids_cannot_force_refetches_of_a_zero_lifetime_peer() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch.serve(Response {
        headers: HeaderMap::new(),
        body: session(&[jwk(X_A, KID_A)]),
    });
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    for i in 0..20 {
        f.clock.advance(2);
        let kid = format!("forged-{i}");
        assert!(is_unknown_key(f.keys.keys_for(&alice(), &kid).await));
    }
    assert_eq!(f.fetch.calls(), 1);
    assert_eq!(f.dns.calls(), 1);
}

/// §Key Publication: "When a request names a `kid` that is not among the
/// usable keys of its peer domain ..., the receiving server MUST fetch
/// the keys again, bypassing its cache, before rejecting the request; if
/// it uses the DNSSEC binding ..., it also queries the `_jmap-peer` name
/// again". A rotated-in key is found by that refetch.
#[tokio::test]
async fn an_unknown_kid_forces_a_refetch_and_a_new_dns_query() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=3600", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    f.clock.advance(60);
    f.fetch.serve(response(
        "max-age=3600",
        session(&[jwk(X_A, KID_A), jwk(X_B, KID_B)]),
    ));
    let set = f.keys.keys_for(&alice(), KID_B).await.unwrap();
    assert_eq!(kids(&set), [KID_A, KID_B]);
    assert_eq!(f.fetch.calls(), 2);
    assert_eq!(f.dns.calls(), 2);
}

/// §Key Publication: "It MUST NOT refetch the keys of one peer domain,
/// or repeat that query, more often than once every 60 seconds; a
/// request that arrives within that interval and names a `kid` that is
/// not among the usable keys is rejected."
#[tokio::test]
async fn unknown_kid_refetches_are_limited_to_one_per_60_seconds() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=3600", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    f.clock.advance(59);
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_B).await));
    assert_eq!(f.fetch.calls(), 1);
    assert_eq!(f.dns.calls(), 1);
    f.clock.advance(1);
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_B).await));
    assert_eq!(f.fetch.calls(), 2);
    f.clock.advance(30);
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_B).await));
    assert_eq!(f.fetch.calls(), 2);
    assert_eq!(f.dns.calls(), 2);
    // The known key still verifies from the cache meanwhile.
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 2);
}

/// A failed fetch leaves no usable keys, so every `kid` is unknown and
/// the same 60-second limit applies before the next attempt.
#[tokio::test]
async fn a_failed_fetch_is_retried_after_60_seconds() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch.fail(503);
    assert!(matches!(
        f.keys.keys_for(&alice(), KID_A).await,
        Err(Error::Fetch(FetchError::Status(503)))
    ));
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    f.clock.advance(59);
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_A).await));
    assert_eq!(f.fetch.calls(), 1);
    f.clock.advance(1);
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
    assert_eq!(f.fetch.calls(), 2);
}

#[tokio::test]
async fn a_body_that_is_not_a_json_object_is_an_invalid_session() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch.serve(response("max-age=600", b"[]".to_vec()));
    assert!(matches!(
        f.keys.keys_for(&alice(), KID_A).await,
        Err(Error::InvalidSession)
    ));
}

/// §DNSSEC Key Binding: "If the answer is secure ... and contains at
/// least one record of this form, a key from `peerSigningKeys` is usable
/// only if its recomputed thumbprint equals the `kid` of one of those
/// records."
#[tokio::test]
async fn a_secure_answer_restricts_the_keys_to_the_listed_kids() {
    let records = vec![format!("v=jmappeer1; kid={KID_B}")];
    let f = fixture(DnssecAnswer::Secure(records), None);
    f.fetch.serve(response(
        "max-age=600",
        session(&[jwk(X_A, KID_A), jwk(X_B, KID_B)]),
    ));
    let set = f.keys.keys_for(&alice(), KID_B).await.unwrap();
    assert_eq!(kids(&set), [KID_B]);
    assert!(is_unknown_key(f.keys.keys_for(&alice(), KID_A).await));
}

/// "If the answer is secure ... and contains at least one record of this
/// form": a secure answer with no such record falls under "contains no
/// record of this form, the HTTPS-published keys are used alone."
#[tokio::test]
async fn a_secure_answer_without_jmappeer_records_uses_the_https_keys() {
    let f = fixture(DnssecAnswer::Secure(vec!["v=spf1 -all".to_owned()]), None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    f.keys.keys_for(&alice(), KID_A).await.unwrap();
}

/// "If the answer is a secure denial of existence, is insecure, or
/// contains no record of this form, the HTTPS-published keys are used
/// alone." and "If the validation result is indeterminate ..., or the
/// lookup fails ..., the HTTPS-published keys are used alone".
#[tokio::test]
async fn other_answers_use_the_https_keys_alone() {
    for answer in [
        DnssecAnswer::SecureDenial,
        DnssecAnswer::Insecure,
        DnssecAnswer::Indeterminate,
        DnssecAnswer::LookupFailed,
    ] {
        let f = fixture(answer.clone(), None);
        f.fetch
            .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
        let set = f.keys.keys_for(&alice(), KID_A).await.unwrap();
        assert_eq!(kids(&set), [KID_A], "{answer:?}");
    }
}

/// "If the answer is bogus, the receiving server MUST NOT fall back to
/// the HTTPS-published keys and MUST reject the request." The cached
/// bogus result keeps rejecting within the refetch interval.
#[tokio::test]
async fn a_bogus_answer_rejects() {
    let f = fixture(DnssecAnswer::Bogus, None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    for _ in 0..2 {
        assert!(matches!(
            f.keys.keys_for(&alice(), KID_A).await,
            Err(Error::Rejected(Rejection::DnssecBogus))
        ));
    }
    assert_eq!(f.fetch.calls(), 1);
}

/// RFC 8032 §7.1 TEST 1 SECRET KEY, the key of RFC 8037 Appendix A.1
/// (whose public key is `X_A`).
fn signing_key_a() -> SigningKey {
    SigningKey::from_bytes(&[
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ])
}

/// End to end: a request signed by alice.example's key authenticates as
/// alice.example with the published `ownerUserId`; the same request a
/// second time is a replay (check 5); an unsigned request is rejected.
#[tokio::test]
async fn authenticate_accepts_once_then_rejects_the_replay() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    let now = UNIX_EPOCH + Duration::from_secs(1_760_000_000);
    let replay = MemoryReplayStore::started_at(now);
    let uri = "https://bob.example/jmap/api/";
    let body = br#"{"using":[]}"#;
    let out = sign(&signing_key_a(), &alice(), &Method::POST, uri, body, now).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-digest",
        HeaderValue::from_str(out.content_digest.as_deref().unwrap()).unwrap(),
    );
    headers.insert(
        "signature-input",
        HeaderValue::from_str(&out.signature_input).unwrap(),
    );
    headers.insert("signature", HeaderValue::from_str(&out.signature).unwrap());
    let request = || Request {
        method: &Method::POST,
        target_uri: uri,
        headers: &headers,
        content: body,
    };

    let peer = authenticate(request(), now, &f.keys, &replay)
        .await
        .unwrap();
    assert_eq!(peer.peer_domain, alice());
    assert_eq!(peer.owner_user_id.as_deref(), Some("alice@alice.example"));
    // draft-atwood-jmap-chat-did-00 §Federation Authentication step 2:
    // the published key whose `kid` the `keyid` names.
    assert_eq!(
        peer.key,
        PeerSigningKey::from_jwk(&jwk(X_A, KID_A)).unwrap()
    );

    let again = authenticate(request(), now + Duration::from_secs(1), &f.keys, &replay).await;
    assert!(matches!(again, Err(Error::Rejected(Rejection::Replayed))));

    let empty = HeaderMap::new();
    let unsigned = Request {
        method: &Method::POST,
        target_uri: uri,
        headers: &empty,
        content: body,
    };
    let result = authenticate(unsigned, now, &f.keys, &replay).await;
    assert!(matches!(
        result,
        Err(Error::Rejected(Rejection::NoSignature))
    ));
}

/// §Replay: "A receiving server whose nonce cache is lost, for example on
/// restart, SHOULD reject requests whose `created` precedes the restart,
/// since it can no longer detect their replay." A store started one
/// second after `created` rejects the request; one started at `created`
/// accepts it.
#[tokio::test]
async fn authenticate_rejects_requests_created_before_the_restart() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    let created = UNIX_EPOCH + Duration::from_secs(1_760_000_000);
    let uri = "https://bob.example/jmap/api/";
    let body = br#"{"using":[]}"#;
    let out = sign(
        &signing_key_a(),
        &alice(),
        &Method::POST,
        uri,
        body,
        created,
    )
    .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-digest",
        HeaderValue::from_str(out.content_digest.as_deref().unwrap()).unwrap(),
    );
    headers.insert(
        "signature-input",
        HeaderValue::from_str(&out.signature_input).unwrap(),
    );
    headers.insert("signature", HeaderValue::from_str(&out.signature).unwrap());
    let request = || Request {
        method: &Method::POST,
        target_uri: uri,
        headers: &headers,
        content: body,
    };
    let now = created + Duration::from_secs(2);

    let restarted = MemoryReplayStore::started_at(created + Duration::from_secs(1));
    let result = authenticate(request(), now, &f.keys, &restarted).await;
    assert!(matches!(
        result,
        Err(Error::Rejected(Rejection::PrecedesRestart))
    ));

    let running = MemoryReplayStore::started_at(created);
    let peer = authenticate(request(), now, &f.keys, &running)
        .await
        .unwrap();
    assert_eq!(peer.peer_domain, alice());
}

/// Two overlapping lookups for one peer domain share one fetch: the
/// second waits for the first instead of fetching too.
#[tokio::test]
async fn overlapping_lookups_share_one_fetch() {
    let f = fixture(DnssecAnswer::Insecure, None);
    f.fetch
        .serve(response("max-age=600", session(&[jwk(X_A, KID_A)])));
    let domain = alice();
    let (a, b) = tokio::join!(
        f.keys.keys_for(&domain, KID_A),
        f.keys.keys_for(&domain, KID_A)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(f.fetch.calls(), 1);
}

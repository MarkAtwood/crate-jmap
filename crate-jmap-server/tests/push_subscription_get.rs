//! `PushSubscription/get` (RFC 8620 §7.2.1) through the public
//! [`register_push_subscription_handlers`] entry point (bd:JMAP-nbk3.2).
//!
//! Oracle: the request and response JSON of the RFC 8620 §7.2.3 example
//! (rfc8620.txt:3817-3841) and the normative sentences of §7.2.1
//! (rfc8620.txt:3729-3746). Expected values are copied from the RFC, not
//! produced by the code under test.

use std::sync::{Arc, Mutex};

use jmap_server::{
    register_push_subscription_handlers, BackendChangesError, BackendQueryError, BackendSetError,
    ChangesResult, Dispatcher, GetObject, Id, JmapBackend, JmapObject, JmapRequest,
    PushSubscriptionBackend, QueryChangesResult, QueryObject, QueryResult, State,
    StoredPushSubscription,
};
use jmap_types::PushSubscription;
use serde_json::{json, Value};

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

/// Stores `(owner, subscription)` pairs. The caller context is the id of the
/// credentials that made the request.
#[derive(Default)]
struct MockBackend {
    subs: Mutex<Vec<(Id, StoredPushSubscription)>>,
}

impl MockBackend {
    fn insert(&self, owner: &str, sub: Value, expected_code: &str) {
        let subscription: PushSubscription =
            serde_json::from_value(sub).expect("test: subscription JSON must deserialize");
        self.subs.lock().expect("test: mutex poisoned").push((
            Id::from(owner),
            StoredPushSubscription {
                subscription,
                expected_code: expected_code.to_owned(),
            },
        ));
    }
}

/// The account-scoped methods panic: PushSubscription/get takes no
/// `accountId`, so the handler must not reach them. A panic in a handler
/// task surfaces as a `serverFail` response, which fails the assertions.
impl JmapBackend for MockBackend {
    type Error = MockError;
    type CallerCtx = Id;

    async fn account_exists(&self, _caller: &Id, _account_id: &Id) -> Result<bool, MockError> {
        unreachable!("PushSubscription/get is not account-scoped")
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        unreachable!("PushSubscription/get is not account-scoped")
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
    ) -> Result<State, MockError> {
        unreachable!("PushSubscription/get returns no state")
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
        _since_state: &State,
        _max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        unreachable!("not exercised")
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _limit: Option<u64>,
        _position: i64,
        _anchor: Option<&Id>,
        _anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        unreachable!("not exercised")
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
        _since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        unreachable!("not exercised")
    }
}

impl PushSubscriptionBackend for MockBackend {
    async fn list_push_subscriptions(
        &self,
        caller: &Id,
    ) -> Result<Vec<StoredPushSubscription>, MockError> {
        Ok(self
            .subs
            .lock()
            .expect("test: mutex poisoned")
            .iter()
            .filter(|(owner, _)| owner == caller)
            .map(|(_, s)| s.clone())
            .collect())
    }

    async fn create_push_subscription(
        &self,
        _caller: &Id,
        _new: PushSubscription,
    ) -> Result<StoredPushSubscription, BackendSetError<MockError>> {
        unreachable!("PushSubscription/get does not write")
    }

    async fn update_push_subscription(
        &self,
        _caller: &Id,
        _sub: PushSubscription,
    ) -> Result<PushSubscription, BackendSetError<MockError>> {
        unreachable!("PushSubscription/get does not write")
    }

    async fn destroy_push_subscription(
        &self,
        _caller: &Id,
        _id: &Id,
    ) -> Result<(), BackendSetError<MockError>> {
        unreachable!("PushSubscription/get does not write")
    }
}

const PRIVATE_URL_1: &str = "https://push.example.com/private-url-canary-1";
const PRIVATE_URL_2: &str = "https://push.example.com/private-url-canary-2";
const PRIVATE_P256DH: &str = "p256dh-canary";
const PRIVATE_AUTH: &str = "auth-canary";

/// The two subscriptions of the RFC 8620 §7.2.3 example response
/// (rfc8620.txt:3828-3840), stored with the `url` and `keys` the RFC
/// example omits from the response, all owned by `owner`.
fn backend_with_rfc_example(owner: &str) -> MockBackend {
    let backend = MockBackend::default();
    backend.insert(
        owner,
        json!({
            "id": "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1",
            "deviceClientId": "b37ff8001ca0",
            "url": PRIVATE_URL_1,
            "keys": { "p256dh": PRIVATE_P256DH, "auth": PRIVATE_AUTH },
            "verificationCode": "b210ef734fe5f439c1ca386421359f7b",
            "expires": "2018-07-31T00:13:21Z",
            "types": ["Todo"]
        }),
        "b210ef734fe5f439c1ca386421359f7b",
    );
    backend.insert(
        owner,
        json!({
            "id": "f2d0aab5-e976-4e8b-ad4b-b380a5b987e4",
            "deviceClientId": "X8980fc",
            "url": PRIVATE_URL_2,
            "verificationCode": "f3d4618a9ae15c8b7f5582533786d531",
            "expires": "2018-07-12T05:55:00Z",
            "types": ["Mailbox", "Email", "EmailDelivery"]
        }),
        "f3d4618a9ae15c8b7f5582533786d531",
    );
    backend
}

/// Dispatch one `PushSubscription/get` call as `caller` and return the
/// single method response as `(name, args, callId)`.
async fn get(backend: MockBackend, caller: &str, args: Value) -> (String, Value, String) {
    let mut dispatcher = Dispatcher::new();
    register_push_subscription_handlers(&mut dispatcher, Arc::new(backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core"],
        "methodCalls": [["PushSubscription/get", args, "0"]]
    }))
    .expect("test: request JSON must deserialize");
    let mut response = dispatcher
        .dispatch(request, Id::from(caller), State::from("s0"))
        .await;
    assert_eq!(response.method_responses.len(), 1, "one call, one response");
    response.method_responses.remove(0).into()
}

/// RFC 8620 §7.2.1 (rfc8620.txt:3731-3735): "Standard /get method as
/// described in Section 5.1, except it does *not* take or return an
/// "accountId" argument, as push subscriptions are not tied to specific
/// accounts. It also does *not* return a "state" argument. The "ids"
/// argument may be null to fetch all at once."
///
/// RFC 8620 §7.2.1 (rfc8620.txt:3742-3745): "As the "url" and "keys"
/// properties may contain data that is private to a particular device, the
/// values for these properties MUST NOT be returned. If the "properties"
/// argument is null or omitted, the server MUST default to all properties
/// excluding these two."
///
/// Request and expected response are the RFC 8620 §7.2.3 example
/// (rfc8620.txt:3821-3841), verbatim.
#[tokio::test]
async fn get_null_ids_matches_rfc_example_without_account_id_state_url_or_keys() {
    let (name, args, call_id) = get(
        backend_with_rfc_example("credA"),
        "credA",
        json!({ "ids": null }),
    )
    .await;

    assert_eq!(name, "PushSubscription/get", "got error response: {args}");
    assert_eq!(call_id, "0");
    assert_eq!(
        args,
        json!({
            "list": [{
                "id": "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1",
                "deviceClientId": "b37ff8001ca0",
                "verificationCode": "b210ef734fe5f439c1ca386421359f7b",
                "expires": "2018-07-31T00:13:21Z",
                "types": [ "Todo" ]
            }, {
                "id": "f2d0aab5-e976-4e8b-ad4b-b380a5b987e4",
                "deviceClientId": "X8980fc",
                "verificationCode": "f3d4618a9ae15c8b7f5582533786d531",
                "expires": "2018-07-12T05:55:00Z",
                "types": [ "Mailbox", "Email", "EmailDelivery" ]
            }],
            "notFound": []
        })
    );
}

/// RFC 8620 §7.2.1 (rfc8620.txt:3742-3746): "As the "url" and "keys"
/// properties may contain data that is private to a particular device, the
/// values for these properties MUST NOT be returned. [...] If one of them is
/// explicitly requested, the method call MUST be rejected with a
/// "forbidden" error."
#[tokio::test]
async fn get_explicitly_requesting_url_or_keys_is_forbidden() {
    for properties in [json!(["id", "url"]), json!(["keys"])] {
        let (name, args, call_id) = get(
            backend_with_rfc_example("credA"),
            "credA",
            json!({ "ids": null, "properties": properties }),
        )
        .await;

        assert_eq!(name, "error", "properties {properties} must be rejected");
        assert_eq!(args["type"], "forbidden", "properties {properties}");
        assert_eq!(call_id, "0");
        let wire = args.to_string();
        for secret in [PRIVATE_URL_1, PRIVATE_URL_2, PRIVATE_P256DH, PRIVATE_AUTH] {
            assert!(!wire.contains(secret), "{secret} leaked in {wire}");
        }
    }
}

/// RFC 8620 §7.2.1 (rfc8620.txt:3737-3739): "The server MUST only return
/// push subscriptions that were created using the same authentication
/// credentials as for this "PushSubscription/get" request."
#[tokio::test]
async fn get_returns_only_subscriptions_of_the_requesting_credentials() {
    let a_id = "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1";
    let b_id = "c0ffee00-0000-4000-8000-00000000000b";

    let backend = backend_with_rfc_example("credA");
    backend.insert(
        "credB",
        json!({
            "id": b_id,
            "deviceClientId": "a889-ffea-910",
            "url": "https://example.com/push/?device=X8980fc&client=12c6d086",
            "types": null
        }),
        "0123456789abcdef0123456789abcdef",
    );

    // Fetch-all as B: only B's subscription.
    let (name, args, _) = get(backend, "credB", json!({ "ids": null })).await;
    assert_eq!(name, "PushSubscription/get", "got error response: {args}");
    let ids: Vec<&str> = args["list"]
        .as_array()
        .expect("list must be an array")
        .iter()
        .map(|s| s["id"].as_str().expect("id must be a string"))
        .collect();
    assert_eq!(ids, vec![b_id]);

    // Fetch A's subscription by id as B: not found, not returned.
    let backend = backend_with_rfc_example("credA");
    let (name, args, _) = get(backend, "credB", json!({ "ids": [a_id] })).await;
    assert_eq!(name, "PushSubscription/get", "got error response: {args}");
    assert_eq!(args["list"], json!([]));
    assert_eq!(args["notFound"], json!([a_id]));
}

/// RFC 8620 §5.1 (rfc8620.txt:1648-1651): "If an identical id is included
/// more than once in the request, the server MUST only include it once in
/// either the "list" or the "notFound" argument of the response."
#[tokio::test]
async fn get_duplicate_ids_are_returned_once() {
    let found_id = "e50b2c1d-9553-41a3-b0a7-a7d26b599ee1";
    let missing_id = "c0ffee00-0000-4000-8000-00000000000b";

    let (name, args, _) = get(
        backend_with_rfc_example("credA"),
        "credA",
        json!({ "ids": [found_id, missing_id, found_id, missing_id] }),
    )
    .await;

    assert_eq!(name, "PushSubscription/get", "got error response: {args}");
    let ids: Vec<&str> = args["list"]
        .as_array()
        .expect("list must be an array")
        .iter()
        .map(|s| s["id"].as_str().expect("id must be a string"))
        .collect();
    assert_eq!(ids, vec![found_id]);
    assert_eq!(args["notFound"], json!([missing_id]));
}

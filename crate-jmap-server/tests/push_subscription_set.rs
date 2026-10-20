//! `PushSubscription/set` (RFC 8620 §7.2.2) through the public
//! [`register_push_subscription_handlers`] entry point, and the delivery
//! gates [`is_deliverable`] and [`state_change_for`] (bd:JMAP-nbk3.3).
//!
//! Oracle: the request and response JSON of the RFC 8620 §7.2.3 example
//! (rfc8620.txt:3849-3947) and the normative sentences of §7.2 and §7.2.2
//! (rfc8620.txt:3570-3798). Expected values are copied from the RFC or
//! computed from wall-clock time and the RFC's 7-day bound, not produced by
//! the code under test.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use jmap_server::{
    is_deliverable, register_push_subscription_handlers, state_change_for, BackendChangesError,
    BackendQueryError, BackendSetError, ChangesResult, Dispatcher, GetObject, Id, JmapBackend,
    JmapObject, JmapRequest, PushSubscriptionBackend, QueryChangesResult, QueryObject, QueryResult,
    SetError, SetErrorType, State, StoredPushSubscription, UTCDate,
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

/// The id and verification code the RFC 8620 §7.2.3 example server assigns
/// (rfc8620.txt:3877, 3891).
const RFC_ID: &str = "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60";
const RFC_CODE: &str = "da1f097b11ca17f06424e30bf02bfa67";
const RFC_URL: &str = "https://example.com/push/?device=X8980fc&client=12c6d086";

const SEVEN_DAYS_SECS: i64 = 7 * 86_400;
const FORTY_EIGHT_HOURS_SECS: i64 = 48 * 3_600;

/// Stores `(owner, subscription)` pairs. The caller context is the id of the
/// credentials that made the request. Creates assign [`RFC_ID`] and
/// [`RFC_CODE`], like the RFC example server.
#[derive(Default)]
struct MockBackend {
    subs: Mutex<Vec<(Id, StoredPushSubscription)>>,
    /// Backend policy: reject creates whose `keys` is null (chinwag's
    /// policy; RFC 8620 permits null keys).
    reject_keyless: bool,
    /// Make `list_push_subscriptions` fail, as a transient storage error.
    fail_list: bool,
}

impl MockBackend {
    fn stored(&self, id: &str) -> Option<StoredPushSubscription> {
        self.subs
            .lock()
            .expect("test: mutex poisoned")
            .iter()
            .find(|(_, s)| s.subscription.id.as_ref() == Some(&Id::from(id)))
            .map(|(_, s)| s.clone())
    }

    fn len(&self) -> usize {
        self.subs.lock().expect("test: mutex poisoned").len()
    }

    /// Store the RFC example subscription as `owner`, unverified, with no
    /// expiry.
    fn insert_rfc_subscription(&self, owner: &str) {
        let subscription: PushSubscription = serde_json::from_value(json!({
            "id": RFC_ID,
            "deviceClientId": "a889-ffea-910",
            "url": RFC_URL,
            "keys": null,
            "verificationCode": null,
            "expires": null,
            "types": null
        }))
        .expect("test: subscription JSON must deserialize");
        self.subs.lock().expect("test: mutex poisoned").push((
            Id::from(owner),
            StoredPushSubscription {
                subscription,
                expected_code: RFC_CODE.to_owned(),
            },
        ));
    }
}

/// The account-scoped methods panic: PushSubscription/set takes no
/// `accountId`, so the handler must not reach them. A panic in a handler
/// task surfaces as a `serverFail` response, which fails the assertions.
impl JmapBackend for MockBackend {
    type Error = MockError;
    type CallerCtx = Id;

    async fn account_exists(&self, _caller: &Id, _account_id: &Id) -> Result<bool, MockError> {
        unreachable!("PushSubscription/set is not account-scoped")
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        unreachable!("PushSubscription/set is not account-scoped")
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
        _account_id: &Id,
    ) -> Result<State, MockError> {
        unreachable!("PushSubscription/set returns no state")
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
        if self.fail_list {
            return Err(MockError);
        }
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
        caller: &Id,
        mut new: PushSubscription,
    ) -> Result<StoredPushSubscription, BackendSetError<MockError>> {
        if self.reject_keyless && new.keys.is_none() {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties).with_properties(["keys"]),
            ));
        }
        new.id = Some(Id::from(RFC_ID));
        let stored = StoredPushSubscription {
            subscription: new,
            expected_code: RFC_CODE.to_owned(),
        };
        self.subs
            .lock()
            .expect("test: mutex poisoned")
            .push((caller.clone(), stored.clone()));
        Ok(stored)
    }

    async fn update_push_subscription(
        &self,
        caller: &Id,
        sub: PushSubscription,
    ) -> Result<PushSubscription, BackendSetError<MockError>> {
        let mut subs = self.subs.lock().expect("test: mutex poisoned");
        let slot = subs
            .iter_mut()
            .find(|(owner, s)| owner == caller && s.subscription.id == sub.id)
            .ok_or(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )))?;
        slot.1.subscription = sub.clone();
        Ok(sub)
    }

    async fn destroy_push_subscription(
        &self,
        caller: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MockError>> {
        let mut subs = self.subs.lock().expect("test: mutex poisoned");
        let before = subs.len();
        subs.retain(|(owner, s)| !(owner == caller && s.subscription.id.as_ref() == Some(id)));
        if subs.len() == before {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }
        Ok(())
    }
}

fn now_secs() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test: clock before 1970")
            .as_secs(),
    )
    .expect("test: time fits i64")
}

/// Dispatch one `PushSubscription/set` call as `caller` and return the
/// single method response as `(name, args, callId)`.
async fn set(backend: &Arc<MockBackend>, caller: &str, args: Value) -> (String, Value, String) {
    let mut dispatcher = Dispatcher::new();
    register_push_subscription_handlers(&mut dispatcher, Arc::clone(backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core"],
        "methodCalls": [["PushSubscription/set", args, "0"]]
    }))
    .expect("test: request JSON must deserialize");
    let mut response = dispatcher
        .dispatch(request, Id::from(caller), State::from("s0"))
        .await;
    assert_eq!(response.method_responses.len(), 1, "one call, one response");
    response.method_responses.remove(0).into()
}

fn epoch_of(value: &Value) -> i64 {
    let s = value.as_str().expect("expires must be a string");
    UTCDate::from(s)
        .to_epoch_seconds()
        .unwrap_or_else(|e| panic!("expires {s} must be a seconds-precision UTCDate: {e}"))
}

fn assert_clamped_to_seven_days(expires: &Value, before: i64, after: i64) {
    let secs = epoch_of(expires);
    assert!(
        secs >= before + FORTY_EIGHT_HOURS_SECS,
        "the maximum expiry MUST be at least 48 hours in the future, got {expires}"
    );
    assert!(
        (before + SEVEN_DAYS_SECS..=after + SEVEN_DAYS_SECS).contains(&secs),
        "expires {expires} must be clamped to now + 7 days"
    );
}

/// RFC 8620 §7.2.2 (rfc8620.txt:3761-3764): "Standard /set method as
/// described in Section 5.3, except it does *not* take or return an
/// "accountId" argument, as push subscriptions are not tied to specific
/// accounts. It also does *not* take an "ifInState" argument or return
/// "oldState" or "newState" arguments."
///
/// RFC 8620 §7.2.1 (rfc8620.txt:3711-3715): "the server SHOULD set an expiry
/// time for the push subscription if none is given and limit the expiry
/// time if set too far in the future. This maximum expiry time MUST be at
/// least 48 hours in the future and SHOULD be at least 7 days in the
/// future."
///
/// Request is the RFC 8620 §7.2.3 create (rfc8620.txt:3849-3857) with a
/// far-future `expires`; the RFC response (rfc8620.txt:3874-3882) echoes the
/// server-limited expiry in "created".
#[tokio::test]
async fn create_returns_id_and_clamped_expires_without_account_id_or_state() {
    let backend = Arc::new(MockBackend::default());
    let before = now_secs();
    let (name, args, call_id) = set(
        &backend,
        "credA",
        json!({
            "create": {
                "4f29": {
                    "deviceClientId": "a889-ffea-910",
                    "url": RFC_URL,
                    "expires": "2999-01-01T00:00:00Z",
                    "types": null
                }
            }
        }),
    )
    .await;
    let after = now_secs();

    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_eq!(call_id, "0");
    let created = &args["created"]["4f29"];
    assert_eq!(created["id"], RFC_ID, "created: {args}");
    assert_clamped_to_seven_days(&created["expires"], before, after);
    // RFC 8620 §7.2.3 (rfc8620.txt:3876-3879): the response for this keyless
    // create carries "keys": null. RFC 8620 §5.3 (rfc8620.txt:2034-2039):
    // "created" holds the properties "that were not sent by the client".
    assert_eq!(created.get("keys"), Some(&Value::Null), "created: {args}");
    for key in ["deviceClientId", "url", "types"] {
        assert!(
            created.get(key).is_none(),
            "{key} was sent by the client: {args}"
        );
    }
    for key in ["accountId", "oldState", "newState"] {
        assert!(
            args.get(key).is_none(),
            "{key} must not be returned: {args}"
        );
    }

    let stored = backend.stored(RFC_ID).expect("subscription must be stored");
    let stored_expires = stored
        .subscription
        .expires
        .expect("stored expires must be set");
    assert_eq!(
        json!(stored_expires),
        created["expires"],
        "created echoes the stored expiry"
    );
}

/// RFC 8620 §7.2 (rfc8620.txt:3620-3621): "An absolute URL where the JMAP
/// server will POST the data for the push message. This MUST begin with
/// "https://"."
#[tokio::test]
async fn create_with_non_https_url_is_invalid_properties() {
    let backend = Arc::new(MockBackend::default());
    let (name, args, _) = set(
        &backend,
        "credA",
        json!({
            "create": {
                "k1": {
                    "deviceClientId": "a889-ffea-910",
                    "url": "http://example.com/push/",
                    "types": null
                }
            }
        }),
    )
    .await;

    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_eq!(
        args["notCreated"]["k1"]["type"], "invalidProperties",
        "{args}"
    );
    assert_eq!(
        args["notCreated"]["k1"]["properties"],
        json!(["url"]),
        "{args}"
    );
    assert_eq!(backend.len(), 0, "a rejected create must not reach storage");
}

/// RFC 8620 §7.2 (rfc8620.txt:3649): "This MUST be null (or omitted) when
/// the subscription is created."
#[tokio::test]
async fn create_with_verification_code_is_invalid_properties() {
    let backend = Arc::new(MockBackend::default());
    let (name, args, _) = set(
        &backend,
        "credA",
        json!({
            "create": {
                "k1": {
                    "deviceClientId": "a889-ffea-910",
                    "url": RFC_URL,
                    "verificationCode": RFC_CODE,
                    "types": null
                }
            }
        }),
    )
    .await;

    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_eq!(
        args["notCreated"]["k1"]["type"], "invalidProperties",
        "{args}"
    );
    assert_eq!(
        args["notCreated"]["k1"]["properties"],
        json!(["verificationCode"]),
        "{args}"
    );
    assert_eq!(backend.len(), 0, "a rejected create must not reach storage");
}

/// RFC 8620 §7.2 (rfc8620.txt:3623): "keys: "Object|null" (immutable)" —
/// null keys are permitted by the protocol, so rejecting them is backend
/// policy. The backend's SetError must reach "notCreated" verbatim.
#[tokio::test]
async fn backend_policy_rejection_of_keyless_create_passes_through_verbatim() {
    let backend = Arc::new(MockBackend {
        reject_keyless: true,
        ..MockBackend::default()
    });
    let (name, args, _) = set(
        &backend,
        "credA",
        json!({
            "create": {
                "4f29": {
                    "deviceClientId": "a889-ffea-910",
                    "url": RFC_URL,
                    "types": null
                }
            }
        }),
    )
    .await;

    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_eq!(
        args["notCreated"]["4f29"],
        json!({ "type": "invalidProperties", "properties": ["keys"] })
    );
    assert_eq!(backend.len(), 0);
}

/// RFC 8620 §7.2 (rfc8620.txt:3593, 3597, 3618, 3623): id, deviceClientId,
/// url and keys are each marked "(immutable)".
///
/// RFC 8620 §7.2.2 (rfc8620.txt:3766-3768): "The "url" and "keys" properties
/// are immutable; if the client wishes to change these, it must destroy the
/// current push subscription and create a new one."
#[tokio::test]
async fn update_of_immutable_property_is_invalid_properties() {
    let cases = [
        ("id", json!("c0ffee00-0000-4000-8000-00000000000b")),
        ("deviceClientId", json!("other-device")),
        ("url", json!("https://attacker.example/push")),
        (
            "keys",
            json!({ "p256dh": "p256dh-canary", "auth": "auth-canary" }),
        ),
    ];
    for (property, value) in cases {
        let backend = Arc::new(MockBackend::default());
        backend.insert_rfc_subscription("credA");
        let (name, args, _) = set(
            &backend,
            "credA",
            json!({ "update": { RFC_ID: { property: value } } }),
        )
        .await;

        assert_eq!(name, "PushSubscription/set", "got error response: {args}");
        assert_eq!(
            args["notUpdated"][RFC_ID]["type"], "invalidProperties",
            "{property}: {args}"
        );
        assert_eq!(
            args["notUpdated"][RFC_ID]["properties"],
            json!([property]),
            "{property}: {args}"
        );
        let stored = backend.stored(RFC_ID).expect("subscription must remain");
        assert_eq!(
            stored.subscription.url.as_deref(),
            Some(RFC_URL),
            "{property}"
        );
        assert_eq!(stored.subscription.keys, None, "{property}");
        assert_eq!(stored.subscription.device_client_id, "a889-ffea-910");
    }
}

/// RFC 8620 §7.2.2 (rfc8620.txt:3788-3792): "The client MUST update the push
/// subscription with the correct verification code before the server makes
/// any further requests to the subscription's URL. Attempts to update the
/// subscription with an invalid verification code MUST be rejected by the
/// server with an "invalidProperties" SetError."
#[tokio::test]
async fn update_with_wrong_verification_code_is_invalid_properties() {
    let backend = Arc::new(MockBackend::default());
    backend.insert_rfc_subscription("credA");
    let (name, args, _) = set(
        &backend,
        "credA",
        json!({
            "update": {
                RFC_ID: { "verificationCode": "00000000000000000000000000000000" }
            }
        }),
    )
    .await;

    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_eq!(
        args["notUpdated"][RFC_ID]["type"], "invalidProperties",
        "{args}"
    );
    assert_eq!(
        args["notUpdated"][RFC_ID]["properties"],
        json!(["verificationCode"]),
        "{args}"
    );
    let stored = backend.stored(RFC_ID).expect("subscription must remain");
    assert_eq!(stored.subscription.verification_code, None);
    assert!(
        !is_deliverable(&stored, now_secs()),
        "an unverified subscription must not be deliverable"
    );
}

/// RFC 8620 §7.2.3 (rfc8620.txt:3899-3908): the client updates the
/// subscription with the code from the PushVerification object, and "The
/// server confirms the update was successful and will now make requests to
/// the registered URL when the state changes."
///
/// RFC 8620 §7.2.2 (rfc8620.txt:3794-3798): "The client may update the
/// "expires" property to extend (or, less commonly, shorten) the lifetime
/// of a push subscription. The server MAY modify the proposed new expiry
/// time to enforce server-defined limits. Extending the lifetime does not
/// require the subscription to be verified again."
///
/// RFC 8620 §7.2.3 (rfc8620.txt:3938-3947): "The server extends the expiry
/// time, but only again to its maximum" and returns it in "updated".
#[tokio::test]
async fn verify_then_extend_expires_keeps_verification_and_clamps() {
    let backend = Arc::new(MockBackend::default());
    backend.insert_rfc_subscription("credA");

    let (name, args, _) = set(
        &backend,
        "credA",
        json!({ "update": { RFC_ID: { "verificationCode": RFC_CODE } } }),
    )
    .await;
    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert!(
        args["updated"].get(RFC_ID).is_some(),
        "correct code must be accepted: {args}"
    );
    let stored = backend.stored(RFC_ID).expect("subscription must remain");
    assert_eq!(
        stored.subscription.verification_code.as_deref(),
        Some(RFC_CODE)
    );
    assert!(is_deliverable(&stored, now_secs()), "verified, no expiry");

    let before = now_secs();
    let (name, args, _) = set(
        &backend,
        "credA",
        json!({ "update": { RFC_ID: { "expires": "2999-01-01T00:00:00Z" } } }),
    )
    .await;
    let after = now_secs();
    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_clamped_to_seven_days(&args["updated"][RFC_ID]["expires"], before, after);
    for key in ["accountId", "oldState", "newState"] {
        assert!(
            args.get(key).is_none(),
            "{key} must not be returned: {args}"
        );
    }

    let stored = backend.stored(RFC_ID).expect("subscription must remain");
    assert_eq!(
        stored.subscription.verification_code.as_deref(),
        Some(RFC_CODE),
        "extending the lifetime must not reset verification"
    );
    assert!(
        is_deliverable(&stored, now_secs()),
        "extending the lifetime does not require re-verification"
    );
}

/// RFC 8620 §5.3 destroy through the §7.2.2 standard /set: the destroyed
/// subscription is reported in "destroyed" and removed from storage.
#[tokio::test]
async fn destroy_removes_subscription() {
    let backend = Arc::new(MockBackend::default());
    backend.insert_rfc_subscription("credA");
    let (name, args, _) = set(&backend, "credA", json!({ "destroy": [RFC_ID] })).await;

    assert_eq!(name, "PushSubscription/set", "got error response: {args}");
    assert_eq!(args["destroyed"], json!([RFC_ID]), "{args}");
    assert!(backend.stored(RFC_ID).is_none());
}

/// RFC 8620 §3.6.2 (rfc8620.txt:1158-1160): "With the exception of when
/// the "serverPartialFail" error is returned, the externally visible state
/// of the server MUST NOT have changed if an error is returned at the method
/// level." A storage failure while reading subscriptions for the update
/// must not leave the same call's create committed.
#[tokio::test]
async fn list_failure_returns_server_fail_without_committing_create() {
    let backend = Arc::new(MockBackend {
        fail_list: true,
        ..MockBackend::default()
    });
    let (name, args, _) = set(
        &backend,
        "credA",
        json!({
            "create": {
                "4f29": {
                    "deviceClientId": "a889-ffea-910",
                    "url": RFC_URL,
                    "types": null
                }
            },
            "update": { RFC_ID: { "expires": null } }
        }),
    )
    .await;

    assert_eq!(name, "error", "got: {args}");
    assert_eq!(args["type"], "serverFail", "{args}");
    assert_eq!(backend.len(), 0, "no subscription may be stored");
}

fn stored_sub(verification_code: Option<&str>, expires: Option<&str>) -> StoredPushSubscription {
    StoredPushSubscription {
        subscription: serde_json::from_value(json!({
            "id": RFC_ID,
            "deviceClientId": "a889-ffea-910",
            "url": RFC_URL,
            "verificationCode": verification_code,
            "expires": expires,
            "types": null
        }))
        .expect("test: subscription JSON must deserialize"),
        expected_code: RFC_CODE.to_owned(),
    }
}

/// RFC 8620 §7.2 (rfc8620.txt:3578-3581): "The JMAP server MUST NOT make
/// any further requests to the URL until the client receives the push and
/// updates the subscription with the correct verification code."
///
/// RFC 8620 §7.2 (rfc8620.txt:3656-3658): "The time this push subscription
/// expires. If specified, the JMAP server MUST NOT make further requests to
/// this resource after this time."
///
/// 2018-07-13T02:14:29Z is 1531448069 seconds after the epoch (the RFC
/// example expiry, rfc8620.txt:3879).
#[test]
fn is_deliverable_requires_verified_and_unexpired() {
    let expires = Some("2018-07-13T02:14:29Z");
    let expires_secs = 1_531_448_069;

    assert!(!is_deliverable(&stored_sub(None, None), expires_secs));
    assert!(!is_deliverable(
        &stored_sub(None, expires),
        expires_secs - 1000
    ));
    assert!(!is_deliverable(
        &stored_sub(Some("00000000000000000000000000000000"), expires),
        expires_secs - 1000
    ));
    assert!(is_deliverable(
        &stored_sub(Some(RFC_CODE), None),
        expires_secs
    ));
    assert!(is_deliverable(
        &stored_sub(Some(RFC_CODE), expires),
        expires_secs - 1000
    ));
    assert!(!is_deliverable(
        &stored_sub(Some(RFC_CODE), expires),
        expires_secs + 1000
    ));
}

fn type_state(pairs: &[(&str, &str)]) -> HashMap<String, State> {
    pairs
        .iter()
        .map(|(t, s)| ((*t).to_owned(), State::from(*s)))
        .collect()
}

/// RFC 8620 §7.2 (rfc8620.txt:3667-3672): "A list of types the client is
/// interested in [...]. A StateChange notification will only be sent if the
/// data for one of these types changes. Other types are omitted from the
/// TypeState object. If null, changes will be pushed for all types."
#[test]
fn state_change_for_filters_by_types() {
    let account = Id::from("u1");
    let changed: HashMap<Id, HashMap<String, State>> = HashMap::from([(
        account.clone(),
        type_state(&[("Mailbox", "m1"), ("Email", "e1")]),
    )]);

    let mut sub = stored_sub(Some(RFC_CODE), None).subscription;
    sub.types = Some(vec!["Mailbox".to_owned()]);
    let sc = state_change_for(&sub, &changed).expect("Mailbox changed: must notify");
    assert_eq!(
        sc.changed,
        HashMap::from([(account.clone(), type_state(&[("Mailbox", "m1")]))])
    );

    let email_only: HashMap<Id, HashMap<String, State>> =
        HashMap::from([(account.clone(), type_state(&[("Email", "e2")]))]);
    assert!(
        state_change_for(&sub, &email_only).is_none(),
        "no subscribed type changed: no notification"
    );

    sub.types = None;
    let sc = state_change_for(&sub, &changed).expect("null types: all changes pushed");
    assert_eq!(sc.changed, changed);
}

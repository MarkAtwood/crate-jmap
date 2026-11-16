//! The PushSubscription extension seam (bd JMAP-r44n.6;
//! docs/design/emailpush.md §5.1): the `using` gate on extension
//! properties, the echo of extension properties a backend normalizes, and
//! backend validation errors.
//!
//! Oracles: draft-ietf-jmap-emailpush-05 §4 ("To get or set the emailPush
//! property, the client MUST include" its capability in `using`;
//! "(default: null)"), RFC 8620 §5.1 ("If an invalid property is
//! requested, the call MUST be rejected with an "invalidArguments" error")
//! and §5.3 (`created` holds properties "set to a default by the server";
//! `updated` holds "any property that changed in a way *not* explicitly
//! requested by the PatchObject"). The mock below stands in for an
//! EmailPush backend; its normalization (filling `{"v": 1}`) is the
//! mock's, not the draft's.

use std::sync::Arc;
use std::sync::Mutex;

use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, Dispatcher, GetObject,
    Id, JmapBackend, JmapObject, JmapRequest, PushSubscriptionBackend, QueryChangesParams,
    QueryChangesResult, QueryObject, QueryParams, QueryResult, SetError, SetErrorType, State,
    StoredPushSubscription, register_push_subscription_handlers,
};
use jmap_types::PushSubscription;
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const EMAILPUSH: &str = "urn:ietf:params:jmap:emailpush";

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

/// Gates `emailPush` on the emailpush capability. Normalizes an
/// `emailPush` object by adding `"v": 1` when absent, rejects the value
/// `"bad"` with `invalidProperties`, and stores other extension keys as
/// sent.
#[derive(Default)]
struct MockBackend {
    subs: Mutex<Vec<StoredPushSubscription>>,
}

impl MockBackend {
    fn normalize(sub: &mut PushSubscription) -> Result<(), BackendSetError<MockError>> {
        match sub.extra.get_mut("emailPush") {
            Some(Value::String(s)) if s == "bad" => Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties).with_properties(["emailPush"]),
            ))),
            Some(Value::Object(config)) => {
                config.entry("v").or_insert(json!(1));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn seed(&self, id: &str, extra: Value) {
        let mut sub = json!({
            "id": id, "deviceClientId": "d", "url": "https://example.com/p",
            "keys": null, "verificationCode": null, "expires": null, "types": null
        });
        sub.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.subs.lock().unwrap().push(StoredPushSubscription {
            subscription: serde_json::from_value(sub).unwrap(),
            expected_code: "code".to_owned(),
        });
    }

    fn stored(&self, id: &str) -> Option<PushSubscription> {
        self.subs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.subscription.id.as_ref().map(AsRef::as_ref) == Some(id))
            .map(|s| s.subscription.clone())
    }
}

impl JmapBackend for MockBackend {
    type Error = MockError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), _account_id: &Id) -> Result<bool, MockError> {
        unreachable!("PushSubscription methods are not account-scoped")
    }
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        unreachable!()
    }
    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
    ) -> Result<State, MockError> {
        unreachable!()
    }
    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        unreachable!()
    }
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        unreachable!()
    }
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        unreachable!()
    }
}

impl PushSubscriptionBackend for MockBackend {
    const PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES: &'static [(&'static str, &'static str)] =
        &[("emailPush", EMAILPUSH)];

    async fn list_push_subscriptions(
        &self,
        _caller: &(),
    ) -> Result<Vec<StoredPushSubscription>, MockError> {
        Ok(self.subs.lock().unwrap().clone())
    }

    async fn create_push_subscription(
        &self,
        _caller: &(),
        mut new: PushSubscription,
    ) -> Result<StoredPushSubscription, BackendSetError<MockError>> {
        Self::normalize(&mut new)?;
        let mut subs = self.subs.lock().unwrap();
        new.id = Some(Id::from(format!("P{}", subs.len() + 1)));
        let stored = StoredPushSubscription {
            subscription: new,
            expected_code: "code".to_owned(),
        };
        subs.push(stored.clone());
        Ok(stored)
    }

    async fn update_push_subscription(
        &self,
        _caller: &(),
        mut sub: PushSubscription,
    ) -> Result<PushSubscription, BackendSetError<MockError>> {
        Self::normalize(&mut sub)?;
        let mut subs = self.subs.lock().unwrap();
        let slot = subs
            .iter_mut()
            .find(|s| s.subscription.id == sub.id)
            .ok_or_else(|| {
                BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
            })?;
        slot.subscription = sub.clone();
        Ok(sub)
    }

    async fn destroy_push_subscription(
        &self,
        _caller: &(),
        _id: &Id,
    ) -> Result<(), BackendSetError<MockError>> {
        unreachable!()
    }
}

/// Run one call with `using`; the response name and arguments.
async fn call(b: &Arc<MockBackend>, using: &[&str], method: &str, args: Value) -> (String, Value) {
    let mut d = Dispatcher::new();
    register_push_subscription_handlers(&mut d, Arc::clone(b));
    let req: JmapRequest =
        serde_json::from_value(json!({"using": using, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

fn create(extra: Value) -> Value {
    let mut sub = json!({"deviceClientId": "d", "url": "https://example.com/p"});
    sub.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({"create": {"c": sub}})
}

#[tokio::test]
async fn get_without_the_capability_hides_the_property() {
    let b = Arc::new(MockBackend::default());
    b.seed("P1", json!({"emailPush": {"v": 1}}));

    let (name, r) = call(&b, &[CORE], "PushSubscription/get", json!({})).await;
    assert_eq!(name, "PushSubscription/get", "{r}");
    assert!(r["list"][0].get("emailPush").is_none(), "{r}");

    let (name, r) = call(
        &b,
        &[CORE],
        "PushSubscription/get",
        json!({"properties": ["emailPush"]}),
    )
    .await;
    assert_eq!(
        (name.as_str(), &r["type"]),
        ("error", &json!("invalidArguments")),
        "{r}"
    );
}

#[tokio::test]
async fn get_with_the_capability_shows_the_property_or_null() {
    let b = Arc::new(MockBackend::default());
    b.seed("P1", json!({"emailPush": {"v": 1}}));
    b.seed("P2", json!({}));
    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/get",
        json!({"properties": ["emailPush"]}),
    )
    .await;
    assert_eq!(
        r["list"],
        json!([{"id": "P1", "emailPush": {"v": 1}}, {"id": "P2", "emailPush": null}])
    );
}

#[tokio::test]
async fn set_without_the_capability_rejects_the_property() {
    let b = Arc::new(MockBackend::default());
    let (_, r) = call(
        &b,
        &[CORE],
        "PushSubscription/set",
        create(json!({"emailPush": {}})),
    )
    .await;
    assert_eq!(
        r["notCreated"]["c"],
        json!({"type": "invalidProperties", "properties": ["emailPush"]})
    );
    assert!(b.stored("P1").is_none(), "nothing stored");

    b.seed("P1", json!({}));
    let (_, r) = call(
        &b,
        &[CORE],
        "PushSubscription/set",
        json!({"update": {"P1": {"emailPush/A": true}}}),
    )
    .await;
    assert_eq!(
        r["notUpdated"]["P1"],
        json!({"type": "invalidProperties", "properties": ["emailPush"]})
    );
    // The gate comes before the other checks: even an unknown id.
    let (_, r) = call(
        &b,
        &[CORE],
        "PushSubscription/set",
        json!({"update": {"P9": {"emailPush": null}}}),
    )
    .await;
    assert_eq!(
        r["notUpdated"]["P9"]["properties"],
        json!(["emailPush"]),
        "{r}"
    );
}

#[tokio::test]
async fn created_shows_an_absent_gated_property_as_null() {
    let b = Arc::new(MockBackend::default());
    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        create(json!({})),
    )
    .await;
    // Present and null, not absent.
    assert_eq!(
        r["created"]["c"].get("emailPush"),
        Some(&Value::Null),
        "{r}"
    );

    // Without the capability it is not a property at all.
    let (_, r) = call(&b, &[CORE], "PushSubscription/set", create(json!({}))).await;
    assert!(r["created"]["c"].get("emailPush").is_none(), "{r}");
}

#[tokio::test]
async fn backend_validation_errors_reach_the_response() {
    let b = Arc::new(MockBackend::default());
    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        create(json!({"emailPush": "bad"})),
    )
    .await;
    assert_eq!(
        r["notCreated"]["c"],
        json!({"type": "invalidProperties", "properties": ["emailPush"]})
    );

    b.seed("P1", json!({}));
    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        json!({"update": {"P1": {"emailPush": "bad"}}}),
    )
    .await;
    assert_eq!(
        r["notUpdated"]["P1"],
        json!({"type": "invalidProperties", "properties": ["emailPush"]})
    );
}

/// A value the backend normalized is echoed; one stored as sent is not.
#[tokio::test]
async fn normalized_extension_values_are_echoed() {
    let b = Arc::new(MockBackend::default());
    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        create(json!({"emailPush": {}, "other": 7})),
    )
    .await;
    assert_eq!(r["created"]["c"]["emailPush"], json!({"v": 1}), "{r}");
    assert!(
        r["created"]["c"].get("other").is_none(),
        "stored as sent: {r}"
    );

    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        create(json!({"emailPush": {"v": 1}})),
    )
    .await;
    assert!(r["created"]["c"].get("emailPush").is_none(), "{r}");

    b.seed("U", json!({"emailPush": {"v": 2}}));
    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        json!({"update": {"U": {"emailPush": {"w": 3}}}}),
    )
    .await;
    assert_eq!(
        r["updated"]["U"],
        json!({"emailPush": {"v": 1, "w": 3}}),
        "{r}"
    );
    assert_eq!(
        b.stored("U").unwrap().extra["emailPush"],
        json!({"v": 1, "w": 3})
    );

    let (_, r) = call(
        &b,
        &[CORE, EMAILPUSH],
        "PushSubscription/set",
        json!({"update": {"U": {"emailPush/w": 4}}}),
    )
    .await;
    assert!(r["updated"]["U"].is_null(), "patched as stored: {r}");
}

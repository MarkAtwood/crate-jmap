//! Conditional `PushSubscription/set` through
//! `register_push_subscription_conditional` (bd JMAP-y1yu.5;
//! docs/design/conditional.md §5.1 and §9 emailpush paragraph).
//!
//! Oracles: draft-ietf-jmap-conditional-02 §3.1 and §9.1 (a precondition
//! the caller may not read is `forbidden`; `url` and `keys` are never
//! readable, RFC 8620 §7.2.1), §4.2 (atomic failure), and RFC 8620 §7.2.2
//! (PushSubscription/set takes no `ifInState`).

use std::sync::{Arc, Mutex};

use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, BeginSetError, CallContext,
    ChangesResult, ClosureHandler, CommitFuture, ConditionalBackend, Dispatcher, GetObject,
    HandlerFuture, HandlerOutput, Id, JmapBackend, JmapHandler, JmapObject, JmapRequest,
    PushSubscriptionBackend, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, RollbackFuture, SetError, SetErrorType, SetMode, SetScope, SetScopeRequest, State,
    StoredPushSubscription, register_push_subscription_conditional,
    register_push_subscription_handlers,
};
use jmap_types::PushSubscription;
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:conditional",
];

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

type Subs = Arc<Mutex<Vec<StoredPushSubscription>>>;

/// Rejects a create whose `deviceClientId` is `"bad"`, standing in for
/// extension validation in `create_push_subscription` (decision 16).
#[derive(Default)]
struct MockBackend {
    subs: Subs,
}

impl MockBackend {
    fn seed(&self, id: &str, device: &str) {
        self.subs.lock().unwrap().push(StoredPushSubscription {
            subscription: serde_json::from_value(json!({
                "id": id, "deviceClientId": device, "url": "https://example.com/p",
                "keys": {"p256dh": "k", "auth": "a"}, "verificationCode": null,
                "expires": null, "types": ["Mailbox"]
            }))
            .unwrap(),
            expected_code: "code".to_owned(),
        });
    }

    fn ids(&self) -> Vec<String> {
        self.subs
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.subscription.id.as_ref().unwrap().as_ref().to_owned())
            .collect()
    }
}

/// A scope that restores the store on rollback or drop (atomic only).
struct SnapshotScope {
    subs: Subs,
    snapshot: Option<Vec<StoredPushSubscription>>,
}

impl SetScope for SnapshotScope {
    fn commit(mut self: Box<Self>) -> CommitFuture {
        self.snapshot = None;
        Box::pin(async { Ok(()) })
    }

    fn rollback(self: Box<Self>) -> RollbackFuture {
        Box::pin(async {})
    }
}

impl Drop for SnapshotScope {
    fn drop(&mut self) {
        if let Some(before) = self.snapshot.take() {
            *self.subs.lock().unwrap() = before;
        }
    }
}

impl ConditionalBackend for MockBackend {
    async fn begin_set(
        &self,
        _caller: &(),
        request: SetScopeRequest<'_>,
    ) -> Result<Box<dyn SetScope>, BeginSetError<MockError>> {
        assert!(
            request.account_id.is_none(),
            "PushSubscription has no account"
        );
        let snapshot = (request.mode == SetMode::Atomic).then(|| self.subs.lock().unwrap().clone());
        Ok(Box::new(SnapshotScope {
            subs: Arc::clone(&self.subs),
            snapshot,
        }))
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
        if new.device_client_id == "bad" {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties).with_properties(["deviceClientId"]),
            )));
        }
        let mut subs = self.subs.lock().unwrap();
        new.id = Some(Id::from(format!("N{}", subs.len() + 1)));
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
        sub: PushSubscription,
    ) -> Result<PushSubscription, BackendSetError<MockError>> {
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
        id: &Id,
    ) -> Result<(), BackendSetError<MockError>> {
        self.subs
            .lock()
            .unwrap()
            .retain(|s| s.subscription.id.as_ref() != Some(id));
        Ok(())
    }
}

fn dispatcher(b: &Arc<MockBackend>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_push_subscription_handlers(&mut d, Arc::clone(b));
    register_push_subscription_conditional(&mut d, Arc::clone(b)).unwrap();
    d
}

async fn set(d: &Dispatcher<()>, args: Value) -> (String, Value) {
    let req: JmapRequest = serde_json::from_value(
        json!({"using": USING, "methodCalls": [["PushSubscription/set", args, "0"]]}),
    )
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// §9.1: `url` and `keys` are never readable, so a precondition on either
/// is `forbidden`, whatever the stored value, and nothing changes.
#[tokio::test]
async fn precondition_on_url_or_keys_is_forbidden() {
    let b = Arc::new(MockBackend::default());
    b.seed("P1", "phone");
    let d = dispatcher(&b);
    for pre in [
        json!({"P1": {"keys": {"p256dh": "k", "auth": "a"}}}),
        json!({"P1": {"url": "https://example.com/p"}}),
        json!({"P1": {"keys/auth": "a"}}),
    ] {
        let (name, r) = set(&d, json!({"ifUnchangedBy": pre.clone(), "destroy": ["P1"]})).await;
        assert_eq!(
            (name.as_str(), &r["type"]),
            ("error", &json!("forbidden")),
            "{pre}: {r}"
        );
    }
    assert_eq!(b.ids(), ["P1"]);
}

#[tokio::test]
async fn precondition_on_a_readable_property() {
    let b = Arc::new(MockBackend::default());
    b.seed("P1", "phone");
    let d = dispatcher(&b);
    let destroy_if = |device: &str| json!({"ifUnchangedBy": {"P1": {"deviceClientId": device}}, "destroy": ["P1"]});
    let (name, r) = set(&d, destroy_if("laptop")).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["P1"]}))
    );
    assert_eq!(b.ids(), ["P1"]);

    let (name, r) = set(&d, destroy_if("phone")).await;
    assert_eq!(name, "PushSubscription/set", "{r}");
    assert_eq!(r["destroyed"], json!(["P1"]));
}

/// RFC 8620 §7.2.2: no `ifInState`, so a stale one is ignored even on the
/// conditional path.
#[tokio::test]
async fn if_in_state_stays_ignored() {
    let b = Arc::new(MockBackend::default());
    b.seed("P1", "phone");
    let d = dispatcher(&b);
    let (name, r) = set(
        &d,
        json!({"ifInState": "stale", "ifUnchangedBy": {"P1": {}}, "destroy": ["P1"]}),
    )
    .await;
    assert_eq!(name, "PushSubscription/set", "{r}");
    assert_eq!(r["destroyed"], json!(["P1"]));
}

/// §4.2: a SetError from the backend's create (where extension validation
/// runs) fails the atomic method, and the other create is not stored.
#[tokio::test]
async fn atomic_backend_rejection_stores_nothing() {
    let b = Arc::new(MockBackend::default());
    let d = dispatcher(&b);
    let (name, r) = set(
        &d,
        json!({"atomic": true, "create": {
            "c1": {"deviceClientId": "good", "url": "https://example.com/1"},
            "c2": {"deviceClientId": "bad", "url": "https://example.com/2"}
        }}),
    )
    .await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "atomicFailure", "notCreated": {
                "c2": {"type": "invalidProperties", "properties": ["deviceClientId"]}
            }})
        )
    );
    assert!(b.ids().is_empty(), "c1 rolled back");
}

#[test]
fn ready_once_every_set_is_opted_in() {
    let b = Arc::new(MockBackend::default());
    let mut d = Dispatcher::new();
    register_push_subscription_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps(), ["PushSubscription/set"]);
    register_push_subscription_conditional(&mut d, Arc::clone(&b)).unwrap();
    assert!(d.conditional_ready());

    // Another /set that has not opted in keeps the server from advertising.
    let probe: Arc<dyn JmapHandler<()>> = Arc::new(ClosureHandler::new(
        Arc::new(()),
        |_: Arc<()>, _: CallContext<()>| {
            Box::pin(async { Ok(HandlerOutput::new(json!({}))) }) as HandlerFuture
        },
    ));
    d.register("Probe/set", Arc::clone(&probe));
    d.register("Probe/get", probe);
    assert_eq!(d.conditional_gaps(), ["Probe/set"]);
    assert!(!d.conditional_ready());

    // Before the handlers there is nothing to attach to.
    assert!(register_push_subscription_conditional(&mut Dispatcher::new(), b).is_err());
}

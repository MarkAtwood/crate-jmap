//! draft-ietf-jmap-conditional-02 through the dispatcher: every
//! `jmap-server` dispatcher case in docs/design/conditional.md §10, with
//! probe `Probe/get` and `Probe/set` handlers over a tiny store.
//!
//! The store's scopes snapshot it at `begin_set` and restore the snapshot
//! on rollback (or drop), so a test can see that a rejected method left
//! nothing behind. Expected values are hand-written from the draft's
//! rules, quoted at each test.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use jmap_server::{
    BackendChangesError, BackendQueryError, BeginSetError, CallContext, ChangesParams,
    ChangesResult, CommitError, CommitFuture, ConditionalBackend, DeferredFailure, Dispatcher,
    GetObject, HandlerFuture, HandlerOutput, JmapBackend, JmapHandler, JmapObject,
    QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult, RollbackFuture,
    SetMode, SetObject, SetOp, SetScope, SetScopeRequest, SetSupport, current_set_scope,
};
use jmap_types::{Id, JmapError, JmapRequest, SetError, SetErrorType, State};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const COND: &str = "urn:ietf:params:jmap:conditional";

// ---- the probe type and store ----------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct Probe;

impl JmapObject for Probe {
    const TYPE_NAME: &'static str = "Probe";
    type Property = ();
}

impl SetObject for Probe {
    type Patch = Value;
    const SERVER_SET: &'static [&'static str] = &["id", "size"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["name", "participants"];
    const IMMUTABLE: &'static [&'static str] = &[];
}

#[derive(Default)]
struct Store {
    objects: BTreeMap<String, Map<String, Value>>,
    state: u64,
    next: u64,
    log: Vec<String>,
    /// Objects Probe/get does not return (invisible to the caller).
    invisible: BTreeSet<String>,
    /// Probe/get's `unreadable` marks.
    unreadable: BTreeMap<Id, BTreeSet<String>>,
    /// `hidden_properties` answers.
    hidden: BTreeMap<Id, Vec<String>>,
    refuse_atomic: bool,
    /// Commit rejects the first object created in the scope.
    reject_create_at_commit: bool,
    /// Commit rejects a restore of this id.
    reject_restore_at_commit: Option<Id>,
}

type Shared = Arc<Mutex<Store>>;

fn seeded() -> Shared {
    let mut store = Store::default();
    for o in [
        json!({"id": "p1", "name": "a", "size": 13, "participants": {"x": {"name": "X"}}}),
        json!({"id": "p2", "name": "b"}),
    ] {
        let o = o.as_object().unwrap().clone();
        store
            .objects
            .insert(o["id"].as_str().unwrap().to_owned(), o);
    }
    Arc::new(Mutex::new(store))
}

fn log(store: &Shared) -> Vec<String> {
    store.lock().unwrap().log.clone()
}

// ---- backend ---------------------------------------------------------------

#[derive(Debug)]
struct StubError;

impl std::fmt::Display for StubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stub")
    }
}

impl std::error::Error for StubError {}

struct Backend(Shared);

impl JmapBackend for Backend {
    type Error = StubError;
    type CallerCtx = ();

    async fn account_exists(&self, _: &(), _: &Id) -> Result<bool, StubError> {
        unreachable!("the probe handlers do not call the backend")
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: Option<&[Id]>,
        _: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), StubError> {
        unreachable!("the probe handlers do not call the backend")
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
    ) -> Result<State, StubError> {
        unreachable!("the probe handlers do not call the backend")
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<StubError>> {
        unreachable!("the probe handlers do not call the backend")
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<StubError>> {
        unreachable!("the probe handlers do not call the backend")
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<StubError>> {
        unreachable!("the probe handlers do not call the backend")
    }
}

impl ConditionalBackend for Backend {
    async fn begin_set(
        &self,
        _: &(),
        request: SetScopeRequest<'_>,
    ) -> Result<Box<dyn SetScope>, BeginSetError<StubError>> {
        let mut store = self.0.lock().unwrap();
        if store.refuse_atomic && request.mode == SetMode::Atomic {
            return Err(BeginSetError::CannotApplyAtomically);
        }
        store.log.push(format!("begin {:?}", request.mode));
        Ok(Box::new(Scope {
            store: Arc::clone(&self.0),
            snapshot: Some((store.objects.clone(), store.state, store.next)),
        }))
    }

    async fn hidden_properties(
        &self,
        _: &(),
        _: Option<&Id>,
        _: &str,
        ids: &[Id],
    ) -> Result<BTreeMap<Id, Vec<String>>, StubError> {
        let store = self.0.lock().unwrap();
        Ok(ids
            .iter()
            .filter_map(|id| Some((id.clone(), store.hidden.get(id)?.clone())))
            .collect())
    }
}

type Snapshot = (BTreeMap<String, Map<String, Value>>, u64, u64);

struct Scope {
    store: Shared,
    /// `None` once ended.
    snapshot: Option<Snapshot>,
}

impl Scope {
    fn restore(&mut self, event: &str) {
        let mut store = self.store.lock().unwrap();
        if let Some((objects, state, next)) = self.snapshot.take() {
            store.objects = objects;
            store.state = state;
            store.next = next;
        }
        store.log.push(event.to_owned());
    }
}

impl SetScope for Scope {
    fn commit(mut self: Box<Self>) -> CommitFuture {
        let (rejected, before) = {
            let store = self.store.lock().unwrap();
            let before = self
                .snapshot
                .as_ref()
                .map(|s| s.0.clone())
                .unwrap_or_default();
            let mut rejected = Vec::new();
            if store.reject_create_at_commit
                && let Some(new) = store.objects.keys().find(|k| !before.contains_key(*k))
            {
                rejected.push(DeferredFailure::new(
                    SetOp::Create(Id::from(new.as_str())),
                    SetError::new(SetErrorType::from("alreadyExists")),
                ));
            }
            if let Some(id) = &store.reject_restore_at_commit {
                rejected.push(DeferredFailure::new(
                    SetOp::Restore(id.clone()),
                    SetError::new(SetErrorType::NotFound),
                ));
            }
            (rejected, before)
        };
        let _ = before;
        if rejected.is_empty() {
            self.snapshot = None;
            self.store.lock().unwrap().log.push("commit".to_owned());
            Box::pin(async { Ok(()) })
        } else {
            self.restore("rejected");
            Box::pin(async { Err(CommitError::Rejected(rejected)) })
        }
    }

    fn rollback(mut self: Box<Self>) -> RollbackFuture {
        self.restore("rollback");
        Box::pin(async {})
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.snapshot.is_some() {
            self.restore("rollback on drop");
        }
    }
}

// ---- handlers ---------------------------------------------------------------

struct GetHandler(Shared);

impl JmapHandler<()> for GetHandler {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let store = Arc::clone(&self.0);
        Box::pin(async move {
            let mut store = store.lock().unwrap();
            let properties: Option<Vec<String>> =
                serde_json::from_value(cx.args["properties"].clone()).ok();
            store.log.push(format!("get {properties:?}"));
            let ids: Vec<String> = serde_json::from_value(cx.args["ids"].clone()).unwrap();
            let (mut list, mut not_found) = (Vec::new(), Vec::new());
            for id in ids {
                match store.objects.get(&id) {
                    Some(o) if !store.invisible.contains(&id) => {
                        let mut o = o.clone();
                        if let Some(props) = &properties {
                            o.retain(|k, _| k == "id" || props.contains(k));
                        }
                        list.push(Value::Object(o));
                    }
                    _ => not_found.push(id),
                }
            }
            let primary = json!({
                "accountId": "a1",
                "state": store.state.to_string(),
                "list": list,
                "notFound": not_found
            });
            Ok(HandlerOutput::new(primary).with_unreadable(store.unreadable.clone()))
        })
    }
}

struct SetHandler(Shared);

impl JmapHandler<()> for SetHandler {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let store = Arc::clone(&self.0);
        Box::pin(async move {
            let args = cx.args.as_object().unwrap().clone();
            let mut keys: Vec<&String> = args.keys().collect();
            keys.sort();
            let in_scope = current_set_scope().is_some();
            let mut s = store.lock().unwrap();
            s.log.push(format!(
                "set atomic={} scope={in_scope} args={keys:?}",
                cx.atomic
            ));
            if args.get("fail").is_some() {
                return Err(JmapError::invalid_arguments("probe failure"));
            }
            let old_state = s.state.to_string();
            let (mut created, mut not_created) = (Map::new(), Map::new());
            let (mut updated, mut not_updated) = (Map::new(), Map::new());
            let mut not_restored = Map::new();
            let mut changed = false;
            for (cid, o) in args
                .get("create")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if o["name"] == "bad" {
                    not_created.insert(
                        cid.clone(),
                        json!({"type": "invalidProperties", "properties": ["name"]}),
                    );
                    continue;
                }
                s.next += 1;
                let id = format!("n{}", s.next);
                let mut o = o.as_object().unwrap().clone();
                o.insert("id".to_owned(), json!(id));
                s.objects.insert(id.clone(), o);
                created.insert(cid.clone(), json!({"id": id}));
                changed = true;
            }
            for (id, patch) in args
                .get("update")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                match s.objects.get_mut(id) {
                    Some(o) => {
                        for (k, v) in patch.as_object().unwrap() {
                            if v.is_null() {
                                o.remove(k);
                            } else {
                                o.insert(k.clone(), v.clone());
                            }
                        }
                        updated.insert(id.clone(), Value::Null);
                        changed = true;
                    }
                    None => {
                        not_updated.insert(id.clone(), json!({"type": "notFound"}));
                    }
                }
            }
            for id in args
                .get("restore")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                not_restored.insert(id.as_str().unwrap().to_owned(), json!({"type": "notFound"}));
            }
            if changed {
                s.state += 1;
            }
            let mut primary = json!({
                "accountId": "a1",
                "oldState": old_state,
                "newState": s.state.to_string(),
                "created": created,
                "updated": updated,
                "notCreated": not_created,
                "notUpdated": not_updated
            });
            if !not_restored.is_empty() {
                primary["notRestored"] = Value::Object(not_restored);
            }
            // Panic after writing, without poisoning the store's lock.
            drop(s);
            assert!(args.get("panic").is_none(), "probe panic after writing");
            Ok(HandlerOutput::new(primary))
        })
    }
}

// ---- harness -----------------------------------------------------------------

fn dispatcher(store: &Shared, with_support: bool) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    d.register("Probe/get", Arc::new(GetHandler(Arc::clone(store))));
    d.register("Probe/set", Arc::new(SetHandler(Arc::clone(store))));
    if with_support {
        d.attach_set_support(
            "Probe/set",
            SetSupport::new::<Probe, _>(Arc::new(Backend(Arc::clone(store)))),
        )
        .unwrap();
    }
    d
}

async fn send(d: &Dispatcher<()>, conditional: bool, calls: Value) -> Vec<Value> {
    let mut using = vec![CORE];
    if conditional {
        using.push(COND);
    }
    let req: JmapRequest =
        serde_json::from_value(json!({"using": using, "methodCalls": calls})).unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    resp["methodResponses"].as_array().unwrap().clone()
}

/// One Probe/set with `args`; returns `[name, arguments]`.
async fn set(d: &Dispatcher<()>, conditional: bool, args: Value) -> (String, Value) {
    let mut args = args;
    args["accountId"] = json!("a1");
    let r = send(d, conditional, json!([["Probe/set", args, "c"]])).await;
    (r[0][0].as_str().unwrap().to_owned(), r[0][1].clone())
}

fn name_of(store: &Shared, id: &str) -> Value {
    store.lock().unwrap().objects[id]["name"].clone()
}

fn state(store: &Shared) -> u64 {
    store.lock().unwrap().state
}

fn sets(store: &Shared) -> usize {
    log(store).iter().filter(|l| l.starts_with("set ")).count()
}

fn assert_error(r: &(String, Value), expected: Value) {
    assert_eq!(r.0, "error", "{r:?}");
    let mut got = r.1.clone();
    got.as_object_mut().unwrap().remove("description");
    assert_eq!(got, expected, "{r:?}");
}

// ---- §3.1 rules ----------------------------------------------------------------

/// Rule 1: "Preconditions are evaluated against the state of each object
/// as it exists at the start of the method"; §3.2/§3.3 on failure.
#[tokio::test]
async fn precondition_passes_then_fails() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let args = json!({
        "ifUnchangedBy": {"p1": {"name": "a"}},
        "update": {"p1": {"name": "z"}}
    });
    let ok = set(&d, true, args.clone()).await;
    assert_eq!(ok.0, "Probe/set");
    assert_eq!(name_of(&store, "p1"), "z");
    assert_eq!(state(&store), 1);
    assert!(log(&store).contains(&"commit".to_owned()));

    let calls = sets(&store);
    let r = set(&d, true, args).await;
    assert_error(&r, json!({"type": "stateMismatch", "failed": ["p1"]}));
    assert_eq!(sets(&store), calls, "handler not called");
    assert_eq!(name_of(&store, "p1"), "z");
    assert_eq!(state(&store), 1, "§3.2: state string unchanged");
}

/// Rule 2: `#c1` from an earlier call stands for its id; a failed or
/// unknown creation is a nonexistent object (`null` holds, `{}` fails with
/// no `failed`); a creation in the same call does not exist yet.
#[tokio::test]
async fn creation_id_keys() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = send(
        &d,
        true,
        json!([
            ["Probe/set", {"accountId": "a1", "create": {"c1": {"name": "new"}, "c3": {"name": "bad"}}}, "0"],
            ["Probe/set", {"accountId": "a1", "ifUnchangedBy": {"#c1": {"name": "new"}}}, "1"],
            ["Probe/set", {"accountId": "a1", "ifUnchangedBy": {"#c1": {"name": "old"}}}, "2"],
            ["Probe/set", {"accountId": "a1", "ifUnchangedBy": {"#c3": null, "#nope": null}}, "3"],
            ["Probe/set", {"accountId": "a1", "ifUnchangedBy": {"#c3": {}}}, "4"],
            ["Probe/set", {"accountId": "a1", "ifUnchangedBy": {"#c2": null}, "create": {"c2": {"name": "x"}}}, "5"]
        ]),
    )
    .await;
    assert_eq!(r[1][0], "Probe/set", "{r:?}");
    assert_eq!(r[2][0], "error");
    assert_eq!(r[2][1]["type"], "stateMismatch");
    assert_eq!(
        r[2][1]["failed"],
        json!(["n1"]),
        "failed names the resolved id"
    );
    assert_eq!(r[3][0], "Probe/set", "{r:?}");
    assert_eq!(r[4][0], "error");
    assert_eq!(
        r[4][1],
        json!({"type": "stateMismatch"}),
        "no failed for an unresolved key"
    );
    assert_eq!(r[5][0], "Probe/set", "{r:?}");
}

/// Rule 3: preconditions "MAY reference any property", server-set ones too.
#[tokio::test]
async fn server_set_property() {
    let store = seeded();
    let d = dispatcher(&store, true);
    assert_eq!(
        set(&d, true, json!({"ifUnchangedBy": {"p1": {"size": 13}}}))
            .await
            .0,
        "Probe/set"
    );
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"size": 14}}})).await;
    assert_error(&r, json!({"type": "stateMismatch", "failed": ["p1"]}));
}

/// Rule 4: invalid pointers and malformed preconditions are
/// `invalidArguments`; a pointer whose parent is missing "is never
/// satisfied, whatever the value, including null".
#[tokio::test]
async fn malformed_and_inapplicable_pointers() {
    let store = seeded();
    let d = dispatcher(&store, true);
    for bad in [
        json!({"p1": {"participants": {}, "participants/x/name": "X"}}),
        json!({"p1": {"bogus": 1}}),
        json!({"p1": {"na~2me": "a"}}),
        json!({"not an id!": {}}),
        json!({"p1": "a"}),
        json!(["p1"]),
    ] {
        let r = set(&d, true, json!({"ifUnchangedBy": bad})).await;
        assert_eq!(r.1["type"], "invalidArguments", "{bad}: {r:?}");
    }
    let r = set(
        &d,
        true,
        json!({"ifUnchangedBy": {"p2": {"participants/p1/name": null}}}),
    )
    .await;
    assert_error(&r, json!({"type": "stateMismatch", "failed": ["p2"]}));
    let r = set(
        &d,
        true,
        json!({"ifUnchangedBy": {"p1": {"participants/x/name": "X", "participants/y": null}}}),
    )
    .await;
    assert_eq!(r.0, "Probe/set", "{r:?}");
    // `null` asserts the member is absent: a present one fails.
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"name": null}}})).await;
    assert_error(&r, json!({"type": "stateMismatch", "failed": ["p1"]}));
    let r = set(&d, true, json!({"ifUnchangedBy": {"p2": {"size": null}}})).await;
    assert_eq!(r.0, "Probe/set", "{r:?}");
    // `null` and `{}` preconditions.
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {}, "p9": null}})).await;
    assert_eq!(r.0, "Probe/set", "{r:?}");
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": null, "p9": {}}})).await;
    assert_error(&r, json!({"type": "stateMismatch", "failed": ["p1", "p9"]}));
}

/// Rule 5: "the server checks ifInState first"; its failure carries no
/// `failed`, and the preconditions are not evaluated.
#[tokio::test]
async fn if_in_state_first() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({"ifInState": "7", "ifUnchangedBy": {"p1": {"name": "zz"}}}),
    )
    .await;
    assert_error(&r, json!({"type": "stateMismatch"}));
    let r = set(
        &d,
        true,
        json!({"ifInState": "0", "ifUnchangedBy": {"p1": {"name": "a"}}}),
    )
    .await;
    assert_eq!(r.0, "Probe/set", "{r:?}");
}

/// The /get call asks for the pointers' top-level names plus `id`.
#[tokio::test]
async fn get_properties_requested() {
    let store = seeded();
    let d = dispatcher(&store, true);
    set(
        &d,
        true,
        json!({"ifUnchangedBy": {"p1": {"participants/x/name": "X", "size": 13}}}),
    )
    .await;
    assert!(
        log(&store).contains(&r#"get Some(["id", "participants", "size"])"#.to_owned()),
        "{:?}",
        log(&store)
    );
}

// ---- §9.1 ---------------------------------------------------------------------

#[tokio::test]
async fn invisible_unreadable_and_guarded() {
    let store = seeded();
    let d = dispatcher(&store, true);

    // An invisible object is a nonexistent one: stateMismatch, and `null`
    // holds.
    store.lock().unwrap().invisible.insert("p1".to_owned());
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"name": "a"}}})).await;
    assert_error(&r, json!({"type": "stateMismatch", "failed": ["p1"]}));
    assert_eq!(
        set(&d, true, json!({"ifUnchangedBy": {"p1": null}}))
            .await
            .0,
        "Probe/set"
    );
    store.lock().unwrap().invisible.clear();

    // An unreadable property is forbidden even when the value matches.
    store
        .lock()
        .unwrap()
        .unreadable
        .insert(Id::from("p1"), BTreeSet::from(["name".to_owned()]));
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"name": "a"}}})).await;
    assert_error(&r, json!({"type": "forbidden"}));
    store.lock().unwrap().unreadable.clear();

    // So is one the backend hides.
    store
        .lock()
        .unwrap()
        .hidden
        .insert(Id::from("p1"), vec!["size".to_owned()]);
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"size": 13}}})).await;
    assert_error(&r, json!({"type": "forbidden"}));
    // Unknown names still come first.
    let r = set(
        &d,
        true,
        json!({"ifUnchangedBy": {"p1": {"size": 13, "bogus": 1}}}),
    )
    .await;
    assert_eq!(r.1["type"], "invalidArguments");
    store.lock().unwrap().hidden.clear();

    // An extension property Probe/get leaves out for this caller, named in
    // no property list, is still a property: hidden, it is forbidden, not
    // an unknown name (a Task `chatId` the caller may not view).
    store
        .lock()
        .unwrap()
        .hidden
        .insert(Id::from("p1"), vec!["secret".to_owned()]);
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"secret": "x"}}})).await;
    assert_error(&r, json!({"type": "forbidden"}));
    store.lock().unwrap().hidden.clear();
    store
        .lock()
        .unwrap()
        .unreadable
        .insert(Id::from("p1"), BTreeSet::from(["secret".to_owned()]));
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"secret/x": 1}}})).await;
    assert_error(&r, json!({"type": "forbidden"}));
    store.lock().unwrap().unreadable.clear();
    // Unmarked, the same name is unknown.
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {"secret": "x"}}})).await;
    assert_eq!(r.1["type"], "invalidArguments");

    // A guard refusing Probe/get refuses the preconditions.
    let mut guarded = dispatcher(&store, false);
    guarded.set_guard(|method, _| {
        if method == "Probe/get" {
            Err(JmapError::forbidden())
        } else {
            Ok(())
        }
    });
    guarded
        .attach_set_support(
            "Probe/set",
            SetSupport::new::<Probe, _>(Arc::new(Backend(Arc::clone(&store)))),
        )
        .unwrap();
    let r = set(&guarded, true, json!({"ifUnchangedBy": {"p1": {}}})).await;
    assert_error(&r, json!({"type": "forbidden"}));
}

// ---- atomic (§4) -----------------------------------------------------------------

#[tokio::test]
async fn atomic_argument_values() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = set(&d, true, json!({"atomic": "true"})).await;
    assert_eq!(r.1["type"], "invalidArguments");
    // null is the default: the plain path, no scope.
    let r = set(&d, true, json!({"atomic": null})).await;
    assert_eq!(r.0, "Probe/set");
    assert_eq!(
        log(&store),
        [r#"set atomic=false scope=false args=["accountId"]"#]
    );
}

/// §4.3: refused atomicity is `cannotApplyAtomically`, with no change.
#[tokio::test]
async fn backend_refusing_atomic() {
    let store = seeded();
    store.lock().unwrap().refuse_atomic = true;
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({"atomic": true, "update": {"p1": {"name": "z"}}}),
    )
    .await;
    assert_error(&r, json!({"type": "cannotApplyAtomically"}));
    assert_eq!(sets(&store), 0, "handler never called");
    assert_eq!(name_of(&store, "p1"), "a");
}

/// §4.2: one SetError rejects the whole method with `atomicFailure`
/// naming only the failing id; the successful create is rolled back.
#[tokio::test]
async fn atomic_set_error_rolls_back() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({
            "atomic": true,
            "create": {"c1": {"name": "ok"}, "c2": {"name": "bad"}},
            "update": {"p1": {"name": "z"}}
        }),
    )
    .await;
    assert_error(
        &r,
        json!({
            "type": "atomicFailure",
            "notCreated": {"c2": {"type": "invalidProperties", "properties": ["name"]}}
        }),
    );
    let s = store.lock().unwrap();
    assert_eq!(s.objects.len(), 2, "c1 rolled back");
    assert_eq!(s.objects["p1"]["name"], "a");
    assert_eq!(s.state, 0);
    assert!(
        s.log
            .iter()
            .any(|l| l.starts_with("set atomic=true scope=true"))
    );
    assert_eq!(s.log.last().unwrap(), "rollback");
}

/// Decision 6(b): a handler's `notRestored` under `atomic` is a failure.
#[tokio::test]
async fn atomic_not_restored_rolls_back() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({"atomic": true, "update": {"p1": {"name": "z"}}, "restore": ["p7"]}),
    )
    .await;
    assert_error(
        &r,
        json!({"type": "atomicFailure", "notRestored": {"p7": {"type": "notFound"}}}),
    );
    assert_eq!(name_of(&store, "p1"), "a");
}

/// A commit-time rejection of a create is reported under its creation id;
/// a rejected restore under `notRestored`.
#[tokio::test]
async fn rejected_at_commit() {
    let store = seeded();
    store.lock().unwrap().reject_create_at_commit = true;
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({"atomic": true, "create": {"c1": {"name": "n"}}}),
    )
    .await;
    assert_error(
        &r,
        json!({"type": "atomicFailure", "notCreated": {"c1": {"type": "alreadyExists"}}}),
    );
    assert_eq!(store.lock().unwrap().objects.len(), 2);

    let store = seeded();
    store.lock().unwrap().reject_restore_at_commit = Some(Id::from("p1"));
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({"atomic": true, "update": {"p2": {"name": "q"}}}),
    )
    .await;
    assert_error(
        &r,
        json!({"type": "atomicFailure", "notRestored": {"p1": {"type": "notFound"}}}),
    );
    assert_eq!(name_of(&store, "p2"), "b");
}

/// An atomic method that succeeds commits, and its creations reach later
/// calls.
#[tokio::test]
async fn atomic_success_commits() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = send(
        &d,
        true,
        json!([
            ["Probe/set", {"accountId": "a1", "atomic": true, "create": {"c1": {"name": "n"}}}, "0"],
            ["Probe/set", {"accountId": "a1", "ifUnchangedBy": {"#c1": {"name": "n"}}}, "1"]
        ]),
    )
    .await;
    assert_eq!(r[0][1]["created"]["c1"]["id"], "n1");
    assert_eq!(r[1][0], "Probe/set", "{r:?}");
    assert_eq!(log(&store).iter().filter(|l| *l == "commit").count(), 2);
}

/// A handler error rolls back an isolated scope too.
#[tokio::test]
async fn handler_error_rolls_back() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {}}, "fail": true})).await;
    assert_eq!(r.1["type"], "invalidArguments");
    assert_eq!(log(&store).last().unwrap(), "rollback");
}

/// A panic inside the scope drops it, which rolls back.
#[tokio::test]
async fn panic_rolls_back_via_drop() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = set(
        &d,
        true,
        json!({"atomic": true, "update": {"p1": {"name": "z"}}, "panic": true}),
    )
    .await;
    assert_eq!(r.1["type"], "serverFail");
    assert_eq!(name_of(&store, "p1"), "a");
    assert_eq!(log(&store).last().unwrap(), "rollback on drop");
}

// ---- decisions 1 and 2 -------------------------------------------------------------

/// Decision 2: without the URI, a non-default `atomic` or `ifUnchangedBy`
/// is `invalidArguments` and the handler is not called; the defaults are
/// removed before the handler sees the arguments. The same with or without
/// support attached.
#[tokio::test]
async fn without_the_uri() {
    for with_support in [true, false] {
        let store = seeded();
        let d = dispatcher(&store, with_support);
        for bad in [json!({"atomic": true}), json!({"ifUnchangedBy": {"x": {}}})] {
            let r = set(&d, false, bad.clone()).await;
            assert_eq!(r.1["type"], "invalidArguments", "{bad}");
        }
        assert_eq!(sets(&store), 0, "handler never called");
        for ok in [
            json!({"atomic": false}),
            json!({"atomic": null}),
            json!({"ifUnchangedBy": {}}),
            json!({"ifUnchangedBy": null}),
        ] {
            let r = set(&d, false, ok.clone()).await;
            assert_eq!(r.0, "Probe/set", "{ok}");
        }
        assert_eq!(
            log(&store),
            vec![r#"set atomic=false scope=false args=["accountId"]"#; 4],
            "with_support={with_support}"
        );
    }
}

/// Decision 1: with the URI but no support, conditional arguments are
/// `invalidArguments`; a request without them runs as before.
#[tokio::test]
async fn uri_without_support() {
    let store = seeded();
    let d = dispatcher(&store, false);
    assert!(!d.conditional_ready());
    assert_eq!(d.conditional_gaps(), ["Probe/set"]);
    let r = set(&d, true, json!({"ifUnchangedBy": {"p1": {}}})).await;
    assert_eq!(r.1["type"], "invalidArguments");
    let r = set(&d, true, json!({"atomic": true})).await;
    assert_eq!(r.1["type"], "invalidArguments");
    let r = set(&d, true, json!({"update": {"p1": {"name": "z"}}})).await;
    assert_eq!(r.0, "Probe/set");
    assert_eq!(name_of(&store, "p1"), "z");

    let ready = dispatcher(&store, true);
    assert!(ready.conditional_ready());
}

/// Only Foo/set calls are inspected: conditional arguments on another
/// method reach its handler untouched.
#[tokio::test]
async fn other_methods_untouched() {
    let store = seeded();
    let d = dispatcher(&store, true);
    let r = send(
        &d,
        false,
        json!([["Probe/get", {"ids": ["p2"], "atomic": true}, "0"]]),
    )
    .await;
    assert_eq!(r[0][0], "Probe/get");
}

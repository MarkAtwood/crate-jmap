//! The object-history handlers and restore stage against a mock backend
//! (bd JMAP-tna1.3.3).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 ("If includeReplaced and
//! includeDestroyed are both false, the server ignores historyAfter,
//! historyBefore, historySkip, and historyLimit"; "maxObjectsInGet ...
//! apply to the number of distinct ids"; same-id entries "MUST be ordered
//! by objectHistory.version (ascending)"), §3.1 (objectHistory "is
//! returned if and only if includeReplaced or includeDestroyed is true,
//! regardless of whether it is listed in properties"; an unsupported type
//! "MUST reject the call with an "invalidArguments" error"), §4
//! (destroyedAfter/destroyedBefore "is supplied and onlyDestroyed is false"
//! ⇒ invalidArguments), §5.1 (`restore` shapes, "An empty PatchObject is
//! equivalent to null", restores run "before any create, update, or
//! destroy", `restored`/`notRestored` "null if none"), §7.3/7.4 (reserved
//! `objectHistory`), the -04 editor's copy (queryChanges arguments;
//! restores count toward maxObjectsInSet; same-type restores ordered
//! referenced-first; a patch may repeat a server-set value but not change
//! it) and docs/design/DECISIONS.md decision 18 (no URI ⇒
//! invalidArguments).

use std::sync::{Arc, Mutex};

use jmap_object_history_types::{
    HistoryEntry, HistoryObject, JMAP_OBJECT_HISTORY_URI, ObjectHistoryAccountCapability,
    ObjectHistoryInfo,
};
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, CallContext, ChangesParams,
    ChangesResult, ClosureHandler, DestroyedQueryChangesParams, DestroyedQueryParams, Dispatcher,
    GetObject, HandlerFuture, HistoryGetParams, HistoryGetResult, JmapBackend, JmapHandler,
    JmapObject, ObjectHistoryBackend, ParentRestoreHooks, QueryChangesParams, QueryChangesResult,
    QueryObject, QueryParams, QueryResult, Restored, SetObject, append_restore_results,
    apply_restore_patch, enforce_max_objects_in_set_with_restore, extract_account_id,
    handle_get_with_history, handle_query_changes_with_history, handle_query_with_history,
    names_object_history, object_history_set_error, parse_restore_argument, run_restore_phase,
};
use jmap_types::{Id, JmapError, PatchObject, SetError, SetErrorType, State, UTCDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const URI: &str = JMAP_OBJECT_HISTORY_URI;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Note {
    id: Id,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

impl JmapObject for Note {
    const TYPE_NAME: &'static str = "Note";
    type Property = ();
}
impl GetObject for Note {}
impl HistoryObject for Note {}
impl QueryObject for Note {
    type Filter = Value;
    type Comparator = Value;
}
impl SetObject for Note {
    type Patch = Value;
    const SERVER_SET: &'static [&'static str] = &["id", "size"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["title", "parentId", "kind"];
    const IMMUTABLE: &'static [&'static str] = &["kind"];
}

#[derive(Debug)]
struct StubError;
impl std::fmt::Display for StubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SECRET backend detail")
    }
}
impl std::error::Error for StubError {}

#[derive(Default)]
struct Inner {
    live: Map<String, Value>,
    /// id → final version.
    destroyed: Map<String, Value>,
    /// What the backend was asked, in order.
    log: Vec<String>,
}

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<Inner>>);

fn from<O: serde::de::DeserializeOwned>(v: &Value) -> O {
    serde_json::from_value(v.clone()).unwrap()
}

impl Mock {
    fn new() -> Self {
        let m = Mock::default();
        {
            let mut i = m.0.lock().unwrap();
            i.live
                .insert("n1".into(), json!({"id": "n1", "title": "now", "size": 3}));
            i.destroyed.insert(
                "d1".into(),
                json!({"id": "d1", "title": "gone", "size": 4, "kind": "a"}),
            );
            i.destroyed.insert(
                "parent".into(),
                json!({"id": "parent", "title": "p", "parentId": null}),
            );
            i.destroyed.insert(
                "child".into(),
                json!({"id": "child", "title": "c", "parentId": "parent"}),
            );
        }
        m
    }

    fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().log.clone()
    }

    fn push(&self, s: impl Into<String>) {
        self.0.lock().unwrap().log.push(s.into());
    }
}

impl JmapBackend for Mock {
    type Error = StubError;
    type CallerCtx = ();

    async fn account_exists(&self, _: &(), a: &Id) -> Result<bool, StubError> {
        Ok(matches!(a.as_ref(), "A1" | "A2"))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        ids: Option<&[Id]>,
        _: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), StubError> {
        self.push("get_objects");
        let i = self.0.lock().unwrap();
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for id in ids.map(<[Id]>::to_vec).unwrap_or_default() {
            match i.live.get(id.as_ref()) {
                Some(v) => found.push(from(v)),
                None => missing.push(id),
            }
        }
        Ok((found, missing))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
    ) -> Result<State, StubError> {
        Ok(State::from("s1"))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<StubError>> {
        unreachable!()
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<StubError>> {
        self.push("query_objects");
        Ok(QueryResult::new(
            vec![Id::from("n1")],
            0,
            None,
            State::from("live1"),
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        p: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<StubError>> {
        self.push(format!("query_changes {}", p.since_query_state.as_ref()));
        if p.since_query_state.as_ref().starts_with("dq") {
            return Err(BackendChangesError::CannotCalculate);
        }
        Ok(QueryChangesResult::new(
            p.since_query_state.clone(),
            State::from("live2"),
            None,
            vec![],
            vec![],
        ))
    }

    fn max_objects_in_get(&self, _: &(), _: &Id) -> u64 {
        2
    }

    fn max_objects_in_set(&self, _: &(), _: &Id) -> u64 {
        6
    }
}

/// The window, encoded in a destroyed query's state (decision 25).
fn dq_state(after: Option<&UTCDate>) -> String {
    format!("dq:{}", after.map_or("", |d| d.as_ref()))
}

impl ObjectHistoryBackend for Mock {
    fn object_history_capability(&self, _: &(), a: &Id) -> Option<ObjectHistoryAccountCapability> {
        let types = if a.as_ref() == "A1" {
            json!(["Note"])
        } else {
            json!([])
        };
        Some(from(
            &json!({"maxHistoryDuration": null, "supportedTypes": types}),
        ))
    }

    async fn get_object_history<O: HistoryObject>(
        &self,
        _: &(),
        _: &Id,
        p: HistoryGetParams<'_>,
    ) -> Result<HistoryGetResult<O>, BackendQueryError<StubError>> {
        self.push(format!("get_object_history {:?}", p.properties));
        let i = self.0.lock().unwrap();
        let mut entries = Vec::new();
        let mut not_found = Vec::new();
        for id in p.ids.map(<[Id]>::to_vec).unwrap_or_default() {
            if let Some(v) = i.live.get(id.as_ref()) {
                // Returned newest first, so the handler's sort is tested.
                entries.push(HistoryEntry::new(from(v), ObjectHistoryInfo::live(2, true)));
                if p.arguments.include_replaced {
                    entries.push(HistoryEntry::new(
                        from(&json!({"id": id, "title": "before"})),
                        ObjectHistoryInfo::replaced(
                            1,
                            UTCDate::from("2026-01-01T00:00:00Z"),
                            false,
                            None,
                        ),
                    ));
                }
            } else if let (true, Some(v)) =
                (p.arguments.include_destroyed, i.destroyed.get(id.as_ref()))
            {
                entries.push(HistoryEntry::new(
                    from(v),
                    ObjectHistoryInfo::replaced(
                        1,
                        UTCDate::from("2026-02-01T00:00:00Z"),
                        false,
                        None,
                    ),
                ));
            } else {
                not_found.push(id);
            }
        }
        Ok(HistoryGetResult::new(entries, not_found))
    }

    async fn query_destroyed<O: HistoryObject + QueryObject>(
        &self,
        _: &(),
        _: &Id,
        p: DestroyedQueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<StubError>> {
        self.push("query_destroyed");
        let ids = self
            .0
            .lock()
            .unwrap()
            .destroyed
            .keys()
            .map(|k| Id::from(k.as_str()))
            .collect();
        Ok(QueryResult::new(
            ids,
            0,
            None,
            State::from(dq_state(p.destroyed_after)),
            true,
        ))
    }

    async fn query_destroyed_changes<O: HistoryObject + QueryObject>(
        &self,
        _: &(),
        _: &Id,
        p: DestroyedQueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<StubError>> {
        self.push("query_destroyed_changes");
        let since = p.query_changes.since_query_state;
        if since.as_ref() != dq_state(p.destroyed_after) {
            return Err(BackendChangesError::CannotCalculate);
        }
        Ok(QueryChangesResult::new(
            since.clone(),
            State::from("dq2"),
            None,
            vec![Id::from("d1")],
            vec![],
        ))
    }

    async fn restore_object<O: HistoryObject + SetObject>(
        &self,
        _: &(),
        _: &Id,
        id: &Id,
        patch: Option<PatchObject>,
    ) -> Result<Restored<O>, BackendSetError<StubError>> {
        self.push(format!("restore {}", id.as_ref()));
        if id.as_ref() == "boom" {
            return Err(BackendSetError::Other(StubError));
        }
        let mut i = self.0.lock().unwrap();
        let Some(Value::Object(fin)) = i.destroyed.remove(id.as_ref()) else {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        };
        let baseline =
            apply_restore_patch::<O>(&fin, patch.as_ref()).map_err(BackendSetError::SetError)?;
        let mut stored = baseline.clone();
        // A mutable server-set value the restore recomputes.
        if stored.contains_key("size") {
            stored.insert("size".into(), json!(99));
        }
        i.live
            .insert(id.as_ref().to_owned(), Value::Object(stored.clone()));
        Ok(Restored::new(from(&Value::Object(stored)), baseline))
    }
}

fn using() -> Vec<String> {
    vec!["urn:ietf:params:jmap:core".into(), URI.into()]
}

fn err_type(r: Result<impl std::fmt::Debug, JmapError>) -> String {
    match r {
        Err(e) => serde_json::to_value(&e).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned(),
        Ok(v) => panic!("expected an error, got {v:?}"),
    }
}

async fn get(m: &Mock, args: Value, using: &[String]) -> Result<Value, JmapError> {
    handle_get_with_history::<Note, Mock>(m, &(), args, using)
        .await
        .map(|o| o.primary)
}

// ---- Foo/get --------------------------------------------------------------

#[tokio::test]
async fn without_flags_history_arguments_are_ignored_and_object_history_not_returned() {
    let m = Mock::new();
    let out = get(
        &m,
        json!({"accountId": "A1", "ids": ["n1"], "historyLimit": "x", "historySkip": -3,
               "includeReplaced": false, "properties": ["title", "objectHistory"]}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(out["list"], json!([{"id": "n1", "title": "now"}]));
    assert_eq!(m.log(), ["get_objects"]);
}

#[tokio::test]
async fn flags_return_object_history_whatever_properties_says() {
    let m = Mock::new();
    let out = get(
        &m,
        json!({"accountId": "A1", "ids": ["n1", "d1", "zz"], "includeReplaced": true,
               "includeDestroyed": true, "properties": ["title"]}),
        &using(),
    )
    .await;
    // maxObjectsInGet is 2: three ids are too many.
    assert_eq!(err_type(out), "requestTooLarge");

    let out = get(
        &m,
        json!({"accountId": "A1", "ids": ["n1", "d1"], "includeReplaced": true,
               "includeDestroyed": true, "properties": ["title"]}),
        &using(),
    )
    .await
    .unwrap();
    // Three entries for two ids; n1's entries ascend by version.
    let list = out["list"].as_array().unwrap();
    let got: Vec<(&str, u64)> = list
        .iter()
        .map(|e| {
            (
                e["id"].as_str().unwrap(),
                e["objectHistory"]["version"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(got, [("n1", 1), ("n1", 2), ("d1", 1)]);
    assert_eq!(
        list[1],
        json!({"id": "n1", "title": "now", "objectHistory":
               {"version": 2, "replaced": null, "hasOlder": true, "expires": null}})
    );
    assert_eq!(out["notFound"], json!([]));
    assert!(
        m.log()
            .contains(&r#"get_object_history Some(["title"])"#.to_owned())
    );
}

#[tokio::test]
async fn unsupported_type_or_missing_uri_is_invalid_arguments() {
    let m = Mock::new();
    let a2 = json!({"accountId": "A2", "ids": ["n1"], "includeDestroyed": true});
    assert_eq!(err_type(get(&m, a2, &using()).await), "invalidArguments");
    let no_uri = json!({"accountId": "A1", "ids": ["n1"], "includeDestroyed": true});
    assert_eq!(err_type(get(&m, no_uri, &[]).await), "invalidArguments");
    let not_bool = json!({"accountId": "A1", "ids": ["n1"], "includeReplaced": 1});
    assert_eq!(
        err_type(get(&m, not_bool, &using()).await),
        "invalidArguments"
    );
    let bad_date = json!({"accountId": "A1", "ids": ["n1"], "includeReplaced": true,
                          "historyBefore": "yesterday"});
    assert_eq!(
        err_type(get(&m, bad_date, &using()).await),
        "invalidArguments"
    );
}

// ---- Foo/query and Foo/queryChanges -------------------------------------

async fn query(m: &Mock, args: Value, using: &[String]) -> Result<Value, JmapError> {
    handle_query_with_history::<Note, Mock>(m, &(), args, using)
        .await
        .map(|o| o.primary)
}

async fn query_changes(m: &Mock, args: Value) -> Result<Value, JmapError> {
    handle_query_changes_with_history::<Note, Mock>(m, &(), args, &using())
        .await
        .map(|o| o.primary)
}

#[tokio::test]
async fn destroyed_query_arguments() {
    let m = Mock::new();
    let out = query(
        &m,
        json!({"accountId": "A1", "destroyedAfter": null}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(out["queryState"], "live1");
    let lone = json!({"accountId": "A1", "destroyedAfter": "2026-01-01T00:00:00Z"});
    assert_eq!(
        err_type(query(&m, lone, &using()).await),
        "invalidArguments"
    );
    let no_uri = json!({"accountId": "A1", "onlyDestroyed": true});
    assert_eq!(err_type(query(&m, no_uri, &[]).await), "invalidArguments");
    let a2 = json!({"accountId": "A2", "onlyDestroyed": true});
    assert_eq!(err_type(query(&m, a2, &using()).await), "invalidArguments");
    let out = query(
        &m,
        json!({"accountId": "A1", "onlyDestroyed": true,
               "destroyedAfter": "2026-01-01T00:00:00Z"}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(out["queryState"], "dq:2026-01-01T00:00:00Z");
    assert_eq!(out["ids"], json!(["child", "d1", "parent"]));
    assert_eq!(m.log(), ["query_objects", "query_destroyed"]);
}

/// -04 §4: queryChanges takes the Foo/query values; a state of the other
/// kind or another window cannot be calculated.
#[tokio::test]
async fn query_changes_routes_by_only_destroyed() {
    let m = Mock::new();
    let window = "2026-01-01T00:00:00Z";
    let out = query_changes(
        &m,
        json!({"accountId": "A1", "sinceQueryState": format!("dq:{window}"),
               "onlyDestroyed": true, "destroyedAfter": window}),
    )
    .await
    .unwrap();
    assert_eq!(out["removed"], json!(["d1"]));
    let other_window = json!({"accountId": "A1", "sinceQueryState": format!("dq:{window}"),
                              "onlyDestroyed": true});
    assert_eq!(
        err_type(query_changes(&m, other_window).await),
        "cannotCalculateChanges"
    );
    let live_state = json!({"accountId": "A1", "sinceQueryState": "live1", "onlyDestroyed": true});
    assert_eq!(
        err_type(query_changes(&m, live_state).await),
        "cannotCalculateChanges"
    );
    let out = query_changes(&m, json!({"accountId": "A1", "sinceQueryState": "live1"}))
        .await
        .unwrap();
    assert_eq!(out["newQueryState"], "live2");
    let destroyed_state = json!({"accountId": "A1", "sinceQueryState": "dq:"});
    assert_eq!(
        err_type(query_changes(&m, destroyed_state).await),
        "cannotCalculateChanges"
    );
}

// ---- Foo/set restore ---------------------------------------------------

/// A minimal history-aware Note/set: restore, then update (which records
/// whether the id was live), then the envelope.
async fn set(m: &Mock, args: Value, using: &[String]) -> Result<Value, JmapError> {
    let (account, mut args) = extract_account_id(args)?;
    let restore = parse_restore_argument(&mut args, using)?;
    enforce_max_objects_in_set_with_restore(
        &args,
        restore.len(),
        m.max_objects_in_set(&(), &account),
    )?;
    let old_state = m.get_state::<Note>(&(), &account).await.unwrap();
    let acc =
        run_restore_phase::<Note, Mock>(m, &(), &account, restore, &ParentRestoreHooks("parentId"))
            .await?;
    let mut not_created = Map::new();
    for (k, v) in args
        .get("create")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
    {
        if names_object_history(v.as_object().unwrap()) {
            not_created.insert(k, serde_json::to_value(object_history_set_error()).unwrap());
        }
    }
    let mut updated = Map::new();
    for k in args
        .get("update")
        .and_then(Value::as_object)
        .map(|u| u.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let live = m.0.lock().unwrap().live.contains_key(&k);
        m.push(format!("update {k} live={live}"));
        if live {
            updated.insert(k, Value::Null);
        }
    }
    let mut out = json!({"accountId": account, "oldState": old_state, "newState": "s2",
                         "updated": updated, "notCreated": not_created});
    if using.iter().any(|u| u == URI) {
        append_restore_results(&mut out, &acc);
    }
    Ok(out)
}

#[tokio::test]
async fn restore_entry_shapes() {
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "restore": {
            "d1": {},
            "n1": null,
            "bad id!": null,
            "x": 7,
            "y": {"objectHistory": {}},
            "z": {"a/b": 1, "a": 2}
        }}),
        &using(),
    )
    .await
    .unwrap();
    // {} ≡ null; the backend recomputed `size`, so restored names it.
    assert_eq!(out["restored"], json!({"d1": {"size": 99}}));
    let nr = &out["notRestored"];
    assert_eq!(nr["n1"]["type"], "notFound", "a current id: {out}");
    assert_eq!(nr["bad id!"]["type"], "notFound");
    assert_eq!(nr["x"]["type"], "invalidPatch");
    assert_eq!(nr["y"]["type"], "invalidProperties");
    assert_eq!(nr["y"]["properties"], json!(["objectHistory"]));
    assert_eq!(nr["z"]["type"], "invalidPatch");
}

#[tokio::test]
async fn restore_runs_before_update_and_old_state_is_read_first() {
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "restore": {"d1": null}, "update": {"d1": {"title": "t"}}}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(out["oldState"], "s1");
    assert_eq!(out["updated"], json!({"d1": null}));
    assert_eq!(m.log(), ["restore d1", "update d1 live=true"]);
    assert_eq!(out["notRestored"], Value::Null);
}

#[tokio::test]
async fn restored_and_not_restored_are_null_when_empty_and_absent_without_uri() {
    let m = Mock::new();
    let out = set(&m, json!({"accountId": "A1", "restore": {}}), &using())
        .await
        .unwrap();
    assert_eq!(
        (&out["restored"], &out["notRestored"]),
        (&Value::Null, &Value::Null)
    );
    assert!(out.as_object().unwrap().contains_key("restored"));
    let out = set(&m, json!({"accountId": "A1", "restore": {}}), &[])
        .await
        .unwrap();
    assert!(!out.as_object().unwrap().contains_key("restored"));
    let no_uri = json!({"accountId": "A1", "restore": {"d1": null}});
    assert_eq!(err_type(set(&m, no_uri, &[]).await), "invalidArguments");
    let not_object = json!({"accountId": "A1", "restore": ["d1"]});
    assert_eq!(
        err_type(set(&m, not_object, &using()).await),
        "invalidArguments"
    );
    let a2 = json!({"accountId": "A2", "restore": {"d1": null}});
    assert_eq!(err_type(set(&m, a2, &using()).await), "invalidArguments");
    assert!(m.log().is_empty(), "{:?}", m.log());
}

/// -04 §5.1: restores count toward maxObjectsInSet (6 here).
#[tokio::test]
async fn restores_count_toward_max_objects_in_set() {
    let m = Mock::new();
    let over = json!({"accountId": "A1", "restore": {"d1": null, "child": null, "parent": null},
                      "update": {"n1": {}, "q": {}}, "destroy": ["n1", "q"]});
    assert_eq!(err_type(set(&m, over, &using()).await), "requestTooLarge");
    let at = json!({"accountId": "A1", "restore": {"d1": null, "child": null, "parent": null},
                    "update": {"n1": {}, "q": {}}, "destroy": ["n1"]});
    assert!(set(&m, at, &using()).await.is_ok());
}

/// -04 §5.1: a patch may repeat a server-set or immutable value, not
/// change it.
#[tokio::test]
async fn patch_may_repeat_but_not_change_server_set_values() {
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "restore": {"d1": {"size": 5, "kind": "b", "title": "x"}}}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(
        out["notRestored"]["d1"]["properties"],
        json!(["kind", "size"])
    );
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "restore": {"d1": {"id": "d1", "size": 4, "kind": "a", "title": "x"}}}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(out["restored"]["d1"], json!({"size": 99}));
}

/// -04 §5.1: "an object is restored before any restore that refers to
/// it", whatever the key order.
#[tokio::test]
async fn same_type_references_restore_first() {
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "restore": {"child": null, "parent": null}}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(out["restored"], json!({"child": null, "parent": null}));
    assert_eq!(
        m.log()
            .iter()
            .filter(|l| l.starts_with("restore"))
            .collect::<Vec<_>>(),
        ["restore parent", "restore child"]
    );
}

#[tokio::test]
async fn backend_failure_is_a_redacted_server_fail() {
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "restore": {"boom": null}}),
        &using(),
    )
    .await
    .unwrap();
    let e = &out["notRestored"]["boom"];
    assert_eq!(e["type"], "serverFail");
    assert!(!e.to_string().contains("SECRET"), "{e}");
}

#[tokio::test]
async fn object_history_in_create_is_invalid_properties() {
    let m = Mock::new();
    let out = set(
        &m,
        json!({"accountId": "A1", "create": {"c": {"title": "t", "objectHistory": {}}}}),
        &using(),
    )
    .await
    .unwrap();
    assert_eq!(
        out["notCreated"]["c"]["properties"],
        json!(["objectHistory"])
    );
}

// ---- Dispatcher marks -------------------------------------------------

fn noop() -> Arc<dyn JmapHandler<()>> {
    Arc::new(ClosureHandler::new(
        Arc::new(()),
        |_: Arc<()>, _: CallContext<()>| {
            Box::pin(async { Ok(jmap_server::HandlerOutput::new(json!({}))) }) as HandlerFuture
        },
    ))
}

#[test]
fn object_history_gaps_flag_plain_registrations() {
    let mut d: Dispatcher<()> = Dispatcher::new();
    d.register("Note/get", noop());
    d.register("Note/changes", noop());
    d.register_history_aware("Note/set", noop());
    assert_eq!(d.object_history_gaps(&["Note"]), ["Note/get"]);
    d.register_history_aware("Note/get", noop());
    assert!(d.object_history_gaps(&["Note"]).is_empty());
    d.register("Note/set", noop());
    assert_eq!(d.object_history_gaps(&["Note"]), ["Note/set"]);
    assert!(d.object_history_gaps(&["Other"]).is_empty());
}

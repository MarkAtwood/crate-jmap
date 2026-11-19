//! `DelegateJmapReads`: a wrapper keeps its own `JmapAccounts` and sends
//! the five reads to the backend it wraps (chat-backend-split phase 2,
//! docs/design/chat-backend-split.md §5 question 3, bd:JMAP-fmi6).
//!
//! Oracle: the contract in the `DelegateJmapReads` docs. The wrapper's
//! `account_exists` decides which accounts exist (`accountNotFound` per
//! RFC 8620 §3.6.2 otherwise); every read the standard handlers make
//! reaches the inner backend with the caller and account unchanged.
//! Expected values are written out by hand.

use std::sync::Mutex;

use jmap_server::{
    BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, DelegateJmapReads,
    GetObject, Id, JmapAccounts, JmapBackend, JmapObject, JmapReads, QueryChangesParams,
    QueryChangesResult, QueryObject, QueryParams, QueryResult, State, handle_changes, handle_get,
};
use jmap_types::MethodErrorType;
use serde_json::{Value, json};

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

#[derive(serde::Serialize, serde::Deserialize)]
struct Foo {
    id: String,
}

impl JmapObject for Foo {
    const TYPE_NAME: &'static str = "Foo";
    type Property = ();
}

impl GetObject for Foo {}

/// Knows account `inner-only` and answers every read, recording each one
/// as `(method, caller, account)`.
#[derive(Default)]
struct Inner {
    reads: Mutex<Vec<(&'static str, String, String)>>,
}

impl Inner {
    fn record(&self, method: &'static str, caller: &str, account: &Id) {
        self.reads
            .lock()
            .unwrap()
            .push((method, caller.to_owned(), account.to_string()));
    }
}

impl JmapAccounts for Inner {
    type Error = MockError;
    type CallerCtx = String;

    async fn account_exists(&self, _: &String, account_id: &Id) -> Result<bool, MockError> {
        Ok(account_id.as_ref() == "inner-only")
    }
}

impl JmapReads for Inner {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        _: Option<&[Id]>,
        _: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        self.record("get_objects", caller, account_id);
        let object = serde_json::from_value(json!({ "id": "f1" })).unwrap();
        Ok((vec![object], vec![Id::from("missing")]))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<State, MockError> {
        self.record("get_state", caller, account_id);
        Ok(State::from("s7"))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        self.record("get_changes", caller, account_id);
        assert_eq!(params.since_state.as_ref(), "s6");
        Ok(ChangesResult::new(
            vec![Id::from("f2")],
            vec![Id::from("f1")],
            vec![],
            false,
            State::from("s7"),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &String,
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        unreachable!("not exercised")
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &String,
        _: &Id,
        _: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        unreachable!("not exercised")
    }
}

/// Overrides `account_exists` (only `outer`) and delegates the reads.
#[derive(Default)]
struct Wrapper {
    inner: Inner,
}

impl JmapAccounts for Wrapper {
    type Error = MockError;
    type CallerCtx = String;

    async fn account_exists(&self, _: &String, account_id: &Id) -> Result<bool, MockError> {
        Ok(account_id.as_ref() == "outer")
    }
}

impl DelegateJmapReads for Wrapper {
    type Target = Inner;

    fn jmap_reads(&self) -> &Inner {
        &self.inner
    }
}

/// Compile-time: the two halves make a `JmapBackend`.
fn assert_backend<B: JmapBackend>() {}

#[test]
fn wrapper_is_a_jmap_backend() {
    assert_backend::<Wrapper>();
}

#[tokio::test]
async fn get_uses_the_wrappers_accounts_and_the_inner_reads() {
    let w = Wrapper::default();
    let caller = "alice".to_owned();

    let out = handle_get::<Foo, _>(&w, &caller, json!({ "accountId": "outer", "ids": null }))
        .await
        .expect("Foo/get on the wrapper's account");
    let primary: Value = out.primary;
    assert_eq!(primary["state"], "s7");
    assert_eq!(primary["list"], json!([{ "id": "f1" }]));
    assert_eq!(primary["notFound"], json!(["missing"]));

    let reads = w.inner.reads.lock().unwrap().clone();
    assert!(reads.contains(&("get_objects", "alice".into(), "outer".into())));
    assert!(reads.contains(&("get_state", "alice".into(), "outer".into())));
}

#[tokio::test]
async fn an_account_only_the_inner_backend_knows_is_not_found() {
    let w = Wrapper::default();
    let err = handle_get::<Foo, _>(
        &w,
        &"alice".to_owned(),
        json!({ "accountId": "inner-only", "ids": null }),
    )
    .await
    .expect_err("the wrapper's account_exists decides");
    assert_eq!(err.error_type, MethodErrorType::AccountNotFound);
    assert!(
        w.inner.reads.lock().unwrap().is_empty(),
        "no read reached the inner backend"
    );
}

#[tokio::test]
async fn changes_go_to_the_inner_backend() {
    let w = Wrapper::default();
    let out = handle_changes::<Foo, _>(
        &w,
        &"bob".to_owned(),
        json!({ "accountId": "outer", "sinceState": "s6" }),
        &[],
    )
    .await
    .expect("Foo/changes");
    assert_eq!(
        out.primary,
        json!({
            "accountId": "outer",
            "oldState": "s6",
            "newState": "s7",
            "hasMoreChanges": false,
            "created": ["f2"],
            "updated": ["f1"],
            "destroyed": [],
            "updatedProperties": null
        })
    );
    let reads = w.inner.reads.lock().unwrap().clone();
    assert_eq!(reads, vec![("get_changes", "bob".into(), "outer".into())]);
}

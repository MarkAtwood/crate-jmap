//! JMAP FileNode extension method handlers (draft-ietf-jmap-filenode-13).
//!
//! # Usage
//!
//! Implement [`FileNodeBackend`] for your storage layer, then call
//! [`register_filenode_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_filenode_server::{FileNodeBackend, register_filenode_handlers};
//! # use jmap_server::{Dispatcher, JmapBackend};
//! # fn example<B: FileNodeBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_filenode_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`FileNodeBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod filenode;
mod helpers;
/// In-memory reference implementation of [`FileNodeBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, FileNodeBackend, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};
pub use filenode::{
    handle_filenode_changes, handle_filenode_copy, handle_filenode_get, handle_filenode_query,
    handle_filenode_query_changes, handle_filenode_set,
};

/// Capability URI for `urn:ietf:params:jmap:filenode`.
pub use jmap_filenode_types::JMAP_FILENODE_URI;

// ---------------------------------------------------------------------------
// register_filenode_handlers — main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP FileNode method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler
/// closure.  Pass the same `Arc<B>` to both this function and any
/// application-level code that needs the backend directly.
///
/// After this call the dispatcher handles:
/// `FileNode/get`, `FileNode/changes`, `FileNode/set`,
/// `FileNode/copy`, `FileNode/query`, `FileNode/queryChanges`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
pub fn register_filenode_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: FileNodeBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx). `$ctx` is the per-request caller context
    // (`B::CallerCtx`) forwarded by the dispatcher; closures pass `&ctx` to the
    // inner `handle_*` fn.
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                Box::new(
                    move |$b: Arc<B>, $ci: String, $a: serde_json::Value, $ctx: B::CallerCtx| {
                        Box::pin(async move { $body }) as HandlerFuture
                    },
                ),
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("FileNode/get", backend, |b, _ci, a, ctx| {
        handle_filenode_get(&*b, &ctx, a).await
    });
    reg!("FileNode/changes", backend, |b, _ci, a, ctx| {
        handle_filenode_changes(&*b, &ctx, a).await
    });
    reg!("FileNode/set", backend, |b, _ci, a, ctx| {
        handle_filenode_set(&*b, &ctx, a).await
    });
    reg!("FileNode/copy", backend, |b, _ci, a, ctx| {
        handle_filenode_copy(&*b, &ctx, a).await
    });
    reg!("FileNode/query", backend, |b, _ci, a, ctx| {
        handle_filenode_query(&*b, &ctx, a).await
    });
    reg!("FileNode/queryChanges", backend, |b, _ci, a, ctx| {
        handle_filenode_query_changes(&*b, &ctx, a).await
    });
}

pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend
// ---------------------------------------------------------------------------

#[cfg(test)]
#[deny(clippy::await_holding_lock)]
pub(crate) mod test_support {
    //! In-memory mock backend for unit tests.
    //!
    //! Provides a minimal `FileNodeBackend` backed by `HashMap`s.
    //! Not suitable for production use.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use jmap_filenode_types::FileNode;
    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_types::{Id, State};

    use crate::backend::FileNodeBackend;

    /// Minimal error type for the mock backend.
    #[derive(Debug)]
    pub struct MockError(pub String);

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock error: {}", self.0)
        }
    }

    impl std::error::Error for MockError {}

    /// Shared mutable inner state of the mock backend.
    struct MockInner {
        /// Known account IDs.
        accounts: HashMap<String, ()>,
        /// node_id → ancestor chain (immediate parent first, up to root).
        ancestors: HashMap<String, Vec<FileNode>>,
        /// node_id → all descendant IDs (children, grandchildren, etc.).
        descendants: HashMap<String, Vec<String>>,
        /// (parent_id_opt, name_lowercase) → existing sibling node_id.
        siblings: HashMap<(Option<String>, String), String>,
        /// parent_id → direct child IDs (for depth-query tests).
        /// Key is `Option<String>` where None means "children of root".
        children: HashMap<Option<String>, Vec<String>>,
        /// When set, `query_objects` returns `Err(MockError(msg))` once
        /// the call counter has reached `after_calls`.  Used to verify
        /// silent-error regressions in `query_subtree` and
        /// `handle_filenode_query`.
        query_objects_err: Option<QueryObjectsErrConfig>,
        /// Running count of `query_objects` invocations on this backend.
        query_objects_calls: u64,
        /// When `Some(msg)`, `get_ancestors` returns `Err(MockError(msg))`.
        /// Used to verify error propagation through
        /// `handle_filenode_get`'s fetchParents path.
        get_ancestors_err: Option<String>,
        /// Pre-seeded FileNode list returned by `get_objects::<FileNode>`.
        /// Stored as the JSON shape so the generic round-trip in
        /// `get_objects` can decode it back into `O`.
        get_objects_nodes: Vec<serde_json::Value>,
        /// When set, `create_object` overrides the `name` field on the
        /// returned object with this string (regardless of the supplied
        /// name). Used to test that the handler enforces the rename
        /// MUST when a backend does not echo the supplied name.
        create_object_override_name: Option<String>,
    }

    /// Configuration for the `query_objects` failure injection.
    struct QueryObjectsErrConfig {
        msg: String,
        /// Number of calls to allow through before failing. `0` means the
        /// very next call fails.
        after_calls: u64,
    }

    /// In-memory mock backend for testing.
    #[derive(Clone)]
    pub struct MockBackend {
        inner: Arc<Mutex<MockInner>>,
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(MockInner {
                    accounts: HashMap::new(),
                    ancestors: HashMap::new(),
                    descendants: HashMap::new(),
                    siblings: HashMap::new(),
                    children: HashMap::new(),
                    query_objects_err: None,
                    query_objects_calls: 0,
                    get_ancestors_err: None,
                    get_objects_nodes: Vec::new(),
                    create_object_override_name: None,
                })),
            }
        }

        /// Cause `create_object` to override the `name` field of the
        /// returned object with the supplied string, simulating a
        /// backend that does not echo the supplied object verbatim.
        /// Used to test the handler's enforcement of the rename MUST
        /// (draft-ietf-jmap-filenode-13 §3.2.3 lines 572-575).
        pub fn set_create_object_override_name(&self, name: &str) {
            self.inner.lock().unwrap().create_object_override_name = Some(name.to_owned());
        }

        /// Cause `get_ancestors` to return `Err(MockError(msg))`. Used to
        /// exercise error propagation through `handle_filenode_get`'s
        /// fetchParents path.
        pub fn set_get_ancestors_err(&self, msg: &str) {
            self.inner.lock().unwrap().get_ancestors_err = Some(msg.to_owned());
        }

        /// Add a pre-seeded FileNode (as a JSON value) to the list returned
        /// by `get_objects::<FileNode>`. The value is round-tripped through
        /// serde into the generic `O`, so the caller is responsible for
        /// shape correctness.
        ///
        /// Used to exercise paths in handlers (such as
        /// `handle_filenode_get`'s fetchParents branch) that depend on
        /// the response list being non-empty.
        pub fn add_get_objects_node(&self, node: serde_json::Value) {
            self.inner.lock().unwrap().get_objects_nodes.push(node);
        }

        /// Cause `query_objects` to return `Err(MockError(msg))` immediately
        /// on the next call. Used to exercise error propagation through
        /// `query_subtree` and `handle_filenode_query`.
        pub fn set_query_objects_err(&self, msg: &str) {
            self.inner.lock().unwrap().query_objects_err = Some(QueryObjectsErrConfig {
                msg: msg.to_owned(),
                after_calls: 0,
            });
        }

        /// Cause `query_objects` to succeed on the first `after_calls`
        /// invocations and return `Err(MockError(msg))` on the next.
        /// Used to test the depth-expansion path of `handle_filenode_query`
        /// (level-0 succeeds, the first per-level recursion fails).
        pub fn set_query_objects_err_after(&self, msg: &str, after_calls: u64) {
            self.inner.lock().unwrap().query_objects_err = Some(QueryObjectsErrConfig {
                msg: msg.to_owned(),
                after_calls,
            });
        }

        pub fn new_with_account(account_id: &str) -> Self {
            let b = Self::new();
            b.inner
                .lock()
                .unwrap()
                .accounts
                .insert(account_id.to_owned(), ());
            b
        }

        /// Declare the descendant IDs for `node_id`.
        ///
        /// Used by tests to set up cycle detection (if proposed new parent is
        /// in `descendant_ids`) and `nodeHasChildren` checks (non-empty list).
        pub fn set_descendants(&self, node_id: &str, descendant_ids: &[&str]) {
            self.inner.lock().unwrap().descendants.insert(
                node_id.to_owned(),
                descendant_ids.iter().map(|s| s.to_string()).collect(),
            );
        }

        /// Pre-register ancestor nodes for a set of node ids (used by fetchParents tests).
        ///
        /// Each id in `node_ids` will return `ancestors` from `get_ancestors`.
        #[allow(dead_code)]
        pub fn set_ancestors(&self, node_ids: &[&str], ancestors: Vec<FileNode>) {
            let mut guard = self.inner.lock().unwrap();
            for node_id in node_ids {
                guard
                    .ancestors
                    .insert(node_id.to_string(), ancestors.clone());
            }
        }

        /// Register a sibling mapping: a node with `name` under `parent_id`
        /// (None = root) already exists with id `existing_id`.
        ///
        /// Used by `find_sibling_by_name` to simulate collision detection.
        /// Both case-sensitive and case-insensitive lookups use the lowercase key.
        pub fn set_sibling(&self, parent_id: Option<&str>, name: &str, existing_id: &str) {
            let mut guard = self.inner.lock().unwrap();
            // Store with lowercase name so both sensitive/insensitive lookups work.
            let key = (parent_id.map(|s| s.to_owned()), name.to_lowercase());
            guard.siblings.insert(key, existing_id.to_owned());
        }

        /// Register direct children of a parent node for depth-query tests.
        ///
        /// When `query_objects::<FileNode>` is called with a `parentId` filter
        /// matching this parent, the registered child IDs are returned.
        ///
        /// Use `parent_id=None` for root-level children.
        pub fn set_children(&self, parent_id: Option<&str>, child_ids: &[&str]) {
            let mut guard = self.inner.lock().unwrap();
            guard.children.insert(
                parent_id.map(|s| s.to_owned()),
                child_ids.iter().map(|s| s.to_string()).collect(),
            );
        }
    }

    impl JmapBackend for MockBackend {
        type Error = MockError;
        type CallerCtx = ();

        async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .accounts
                .contains_key(account_id.as_ref()))
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            // Pre-seeded nodes are returned for any type whose JSON
            // shape decodes (the test adds them shaped as FileNodes).
            // Tests that don't seed get the empty response.
            let snapshot = self.inner.lock().unwrap().get_objects_nodes.clone();
            let mut out: Vec<O> = Vec::with_capacity(snapshot.len());
            for v in snapshot {
                if let Ok(obj) = serde_json::from_value::<O>(v) {
                    out.push(obj);
                }
            }
            Ok((out, vec![]))
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            Ok(State::from("0"))
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _since_state: &State,
            _max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            Ok(ChangesResult::new(
                vec![],
                vec![],
                vec![],
                false,
                State::from("0"),
            ))
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _limit: Option<u64>,
            _position: i64,
        ) -> Result<QueryResult, Self::Error> {
            // Optional failure injection for tests covering silent-error
            // regressions through callers of `query_objects`.
            {
                let mut guard = self.inner.lock().unwrap();
                let call_index = guard.query_objects_calls;
                guard.query_objects_calls = guard.query_objects_calls.saturating_add(1);
                if let Some(cfg) = guard.query_objects_err.as_ref() {
                    if call_index >= cfg.after_calls {
                        let msg = cfg.msg.clone();
                        drop(guard);
                        return Err(MockError(msg));
                    }
                }
            }
            // Support parentId filter for depth-query tests.  Try to interpret the
            // filter as a FileNodeFilterCondition via serde round-trip.  If the cast
            // fails or the filter has no parentId, return empty (default).
            if let Some(filter_ref) = filter {
                if let Ok(json_val) = serde_json::to_value(filter_ref) {
                    if let Ok(fc) = serde_json::from_value::<
                        jmap_filenode_types::FileNodeFilterCondition,
                    >(json_val)
                    {
                        let parent_key = fc.parent_id.as_ref().map(|id| id.as_ref().to_owned());
                        let guard = self.inner.lock().unwrap();
                        if let Some(child_ids) = guard.children.get(&parent_key) {
                            let ids: Vec<Id> =
                                child_ids.iter().map(|s| Id::from(s.as_str())).collect();
                            let total = ids.len() as u64;
                            return Ok(QueryResult::new(
                                ids,
                                0,
                                Some(total),
                                State::from("0"),
                                false,
                            ));
                        }
                    }
                }
            }
            Ok(QueryResult::new(
                vec![],
                0,
                Some(0),
                State::from("0"),
                false,
            ))
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            since_query_state: &State,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _max_changes: Option<u64>,
            _up_to_id: Option<&Id>,
            _collapse_threads: bool,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            Ok(QueryChangesResult::new(
                since_query_state.clone(),
                State::from("0"),
                Some(0),
                vec![],
                vec![],
            ))
        }
    }

    impl FileNodeBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            // If the test has armed a name override, round-trip the
            // object through serde JSON, swap the `name` field, and
            // round-trip back. Simulates a backend that normalises
            // names rather than echoing the supplied value.
            let override_name = self
                .inner
                .lock()
                .unwrap()
                .create_object_override_name
                .clone();
            if let Some(name) = override_name {
                if let Ok(mut v) = serde_json::to_value(&obj) {
                    if let serde_json::Value::Object(ref mut m) = v {
                        m.insert("name".to_owned(), serde_json::Value::String(name));
                    }
                    if let Ok(reshaped) = serde_json::from_value::<O>(v) {
                        return Ok((Id::from("mock-id-1"), reshaped));
                    }
                }
            }
            Ok((Id::from("mock-id-1"), obj))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _id: &Id,
            _patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::Forbidden,
            )))
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )))
        }

        fn supports_type<O: JmapObject>(&self) -> bool {
            true
        }

        async fn get_ancestors(
            &self,
            _caller: &(),
            _account_id: &Id,
            ids: &[Id],
        ) -> Result<Vec<FileNode>, Self::Error> {
            if let Some(first_id) = ids.first() {
                let guard = self.inner.lock().unwrap();
                if let Some(msg) = guard.get_ancestors_err.as_ref() {
                    let msg = msg.clone();
                    drop(guard);
                    return Err(MockError(msg));
                }
                Ok(guard
                    .ancestors
                    .get(first_id.as_ref())
                    .cloned()
                    .unwrap_or_default())
            } else {
                Ok(vec![])
            }
        }

        async fn get_descendant_ids(
            &self,
            _caller: &(),
            _account_id: &Id,
            id: &Id,
        ) -> Result<Vec<Id>, Self::Error> {
            let guard = self.inner.lock().unwrap();
            Ok(guard
                .descendants
                .get(id.as_ref())
                .map(|v| v.iter().map(|s| Id::from(s.as_str())).collect())
                .unwrap_or_default())
        }

        async fn blob_exists(&self, _caller: &(), _account_id: &Id, _blob_id: &Id) -> bool {
            // Mock always reports blobs as existing.
            true
        }

        async fn find_sibling_by_name(
            &self,
            _caller: &(),
            _account_id: &Id,
            parent_id: Option<&Id>,
            name: &str,
            case_insensitive: bool,
        ) -> Result<Option<Id>, Self::Error> {
            let guard = self.inner.lock().unwrap();
            let key_name = if case_insensitive {
                name.to_lowercase()
            } else {
                name.to_owned()
            };
            let key = (parent_id.map(|id| id.as_ref().to_owned()), key_name);
            Ok(guard.siblings.get(&key).map(|s| Id::from(s.as_str())))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jmap_server::{Dispatcher, JmapRequest, State};
    use serde_json::json;

    use super::*;
    use crate::test_support::MockBackend;

    fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
        JmapRequest::new(
            vec![jmap_filenode_types::JMAP_FILENODE_URI.into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    /// Oracle: register_filenode_handlers registers all 6 JMAP FileNode methods.
    ///
    /// Verification: each method returns a non-`unknownMethod` response when
    /// dispatched with a valid account.
    #[tokio::test]
    async fn registers_all_6_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_filenode_handlers(&mut dispatcher, Arc::clone(&backend));

        let methods = [
            ("FileNode/get", json!({"accountId": "acc1", "ids": null})),
            (
                "FileNode/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            ("FileNode/set", json!({"accountId": "acc1", "destroy": []})),
            (
                "FileNode/copy",
                json!({
                    "fromAccountId": "acc1",
                    "accountId": "acc1",
                    "create": {}
                }),
            ),
            (
                "FileNode/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "FileNode/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
        ];

        for (method, args) in methods {
            let req = single_call(method, args, "c0");
            let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
            assert_eq!(
                resp.method_responses.len(),
                1,
                "{method}: expected 1 response"
            );
            let (_, resp_args, _) = &resp.method_responses[0];
            assert_ne!(
                resp_args["type"], "unknownMethod",
                "{method}: must not be unknownMethod — is it registered?"
            );
        }
    }

    /// Oracle: FileNode/set empty destroy returns valid empty set response.
    #[tokio::test]
    async fn set_empty_destroy_is_valid() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_filenode_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "FileNode/set",
            json!({"accountId": "acc1", "destroy": []}),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be an error response: {args}"
        );
        assert_eq!(args["accountId"], "acc1");
    }
}

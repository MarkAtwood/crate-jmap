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
//! # use jmap_server::Dispatcher;
//! # fn example<B: FileNodeBackend + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_filenode_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod filenode;
mod helpers;

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
/// **Caller context `C` is not forwarded to handlers.** Each handler closure
/// receives only `(Arc<B>, call_id, args)`; the `caller: C` value from the
/// dispatcher is discarded.  If per-request auth context is needed, register
/// handlers individually via [`Dispatcher::register`] with a closure that
/// captures `ctx`. This matches the convention in `jmap-chat-server` and
/// `jmap-sharing-server`.
pub fn register_filenode_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: FileNodeBackend + 'static,
    C: Clone + Send + 'static,
{
    // Helper: register one method with a closure taking (Arc<B>, call_id, args).
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<C>> = Arc::new(ClosureHandler {
                backend: backend_arc,
                call_fn: Box::new(move |$b: Arc<B>, $ci: String, $a: serde_json::Value| {
                    Box::pin(async move { $body }) as HandlerFuture
                }),
            });
            dispatcher.register($method, h);
        }};
    }

    reg!("FileNode/get", backend, |b, _ci, a| {
        handle_filenode_get(&*b, a).await
    });
    reg!("FileNode/changes", backend, |b, _ci, a| {
        handle_filenode_changes(&*b, a).await
    });
    reg!("FileNode/set", backend, |b, _ci, a| {
        handle_filenode_set(&*b, a).await
    });
    reg!("FileNode/copy", backend, |b, _ci, a| {
        handle_filenode_copy(&*b, a).await
    });
    reg!("FileNode/query", backend, |b, _ci, a| {
        handle_filenode_query(&*b, a).await
    });
    reg!("FileNode/queryChanges", backend, |b, _ci, a| {
        handle_filenode_query_changes(&*b, a).await
    });
}

pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    //! In-memory mock backend for unit tests.
    //!
    //! Provides a minimal `FileNodeBackend` backed by `HashMap`s.
    //! Not suitable for production use.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

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
        /// Nodes that report `node_has_children = true`.
        has_children: HashMap<String, bool>,
        /// (node_id, proposed_parent_id) pairs that would create a cycle.
        cycles: HashMap<(String, String), bool>,
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
                    has_children: HashMap::new(),
                    cycles: HashMap::new(),
                })),
            }
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

        /// Mark `node_id` as having (or not having) children.
        pub fn set_has_children(&self, node_id: &str, has: bool) {
            self.inner
                .lock()
                .unwrap()
                .has_children
                .insert(node_id.to_owned(), has);
        }

        /// Declare that moving `node_id` under `new_parent_id` would (or
        /// would not) create a cycle.
        pub fn set_cycle_pair(&self, node_id: &str, new_parent_id: &str, cycles: bool) {
            self.inner
                .lock()
                .unwrap()
                .cycles
                .insert((node_id.to_owned(), new_parent_id.to_owned()), cycles);
        }
    }

    impl JmapBackend for MockBackend {
        type Error = MockError;

        async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .accounts
                .contains_key(account_id.as_ref()))
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            account_id: &Id,
            ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            // Only FileNode is stored; for other types return empty.
            let _ = (account_id, ids);
            Ok((vec![], vec![]))
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            Ok(State::from("0"))
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
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
            _account_id: &Id,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _limit: Option<u64>,
            _position: i64,
        ) -> Result<QueryResult, Self::Error> {
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
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            Ok((Id::from("mock-id-1"), obj))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
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

        async fn would_create_cycle(
            &self,
            _account_id: &Id,
            node_id: &Id,
            new_parent_id: &Id,
        ) -> bool {
            let guard = self.inner.lock().unwrap();
            *guard
                .cycles
                .get(&(
                    node_id.as_ref().to_owned(),
                    new_parent_id.as_ref().to_owned(),
                ))
                .unwrap_or(&false)
        }

        async fn node_has_children(&self, _account_id: &Id, node_id: &Id) -> bool {
            let guard = self.inner.lock().unwrap();
            *guard.has_children.get(node_id.as_ref()).unwrap_or(&false)
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

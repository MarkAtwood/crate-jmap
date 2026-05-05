//! JMAP Sharing extension method handlers (RFC 9670).
//!
//! # Usage
//!
//! Implement [`SharingBackend`] for your storage layer, then call
//! [`register_sharing_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_sharing_server::{SharingBackend, register_sharing_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: SharingBackend + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_sharing_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
mod helpers;
pub mod notification;
pub mod principal;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    SharingBackend,
};
pub use notification::{
    handle_share_notification_changes, handle_share_notification_get,
    handle_share_notification_query, handle_share_notification_query_changes,
    handle_share_notification_set,
};
pub use principal::{
    handle_principal_changes, handle_principal_get, handle_principal_query,
    handle_principal_query_changes, handle_principal_set,
};

/// Capability URI for `urn:ietf:params:jmap:principals`.
pub const CAPABILITY_PRINCIPALS: &str = "urn:ietf:params:jmap:principals";

/// Capability URI for `urn:ietf:params:jmap:principals:owner`.
pub const CAPABILITY_PRINCIPALS_OWNER: &str = "urn:ietf:params:jmap:principals:owner";

// ---------------------------------------------------------------------------
// register_sharing_handlers — the main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all RFC 9670 JMAP Sharing method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler.
/// Pass the same `Arc<B>` to both this function and any application-level code
/// that needs to access the backend.
///
/// After this call, the dispatcher handles:
/// `Principal/get`, `Principal/changes`, `Principal/set`,
/// `Principal/query`, `Principal/queryChanges`,
/// `ShareNotification/get`, `ShareNotification/changes`,
/// `ShareNotification/set`, `ShareNotification/query`,
/// `ShareNotification/queryChanges`.
///
/// **Caller context `C` is not forwarded to handlers.** Each handler closure
/// receives only `(Arc<B>, call_id, args)`; the `caller: C` value from the
/// dispatcher is discarded. If per-request auth context is needed, register
/// handlers individually via [`Dispatcher::register`] with a closure that
/// uses `ctx`. This matches the convention in `jmap-chat-server` and
/// `jmap-mail-server`.
pub fn register_sharing_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: SharingBackend + 'static,
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

    // Principal
    reg!("Principal/get", backend, |b, _ci, a| {
        handle_principal_get(&*b, a).await
    });
    reg!("Principal/changes", backend, |b, _ci, a| {
        handle_principal_changes(&*b, a).await
    });
    reg!("Principal/set", backend, |b, _ci, a| {
        handle_principal_set(&*b, a).await
    });
    reg!("Principal/query", backend, |b, _ci, a| {
        handle_principal_query(&*b, a).await
    });
    reg!("Principal/queryChanges", backend, |b, _ci, a| {
        handle_principal_query_changes(&*b, a).await
    });

    // ShareNotification
    reg!("ShareNotification/get", backend, |b, _ci, a| {
        handle_share_notification_get(&*b, a).await
    });
    reg!("ShareNotification/changes", backend, |b, _ci, a| {
        handle_share_notification_changes(&*b, a).await
    });
    reg!("ShareNotification/set", backend, |b, _ci, a| {
        handle_share_notification_set(&*b, a).await
    });
    reg!("ShareNotification/query", backend, |b, _ci, a| {
        handle_share_notification_query(&*b, a).await
    });
    reg!("ShareNotification/queryChanges", backend, |b, _ci, a| {
        handle_share_notification_query_changes(&*b, a).await
    });
}

pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend used by inline tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    //! In-memory mock backend for unit tests.
    //!
    //! Provides a minimal `SharingBackend` implementation backed by
    //! `HashMap`s. Not suitable for production use.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_sharing_types::{Principal, ShareNotification};
    use jmap_types::{Id, State};

    use crate::backend::SharingBackend;

    /// Minimal error type for the mock backend.
    #[derive(Debug)]
    pub struct MockError(pub String);

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock error: {}", self.0)
        }
    }

    impl std::error::Error for MockError {}

    /// In-memory state for one account.
    #[derive(Default, Clone)]
    struct AccountState {
        principals: HashMap<Id, Principal>,
        notifications: HashMap<Id, ShareNotification>,
        principal_state: u64,
        notification_state: u64,
    }

    /// In-memory mock backend for testing.
    #[derive(Clone)]
    pub struct MockBackend {
        /// Known accounts and their state. The outer `Arc<Mutex<…>>` allows
        /// the mock to be shared across threads (required by `JmapBackend: Sync`).
        state: Arc<Mutex<HashMap<String, AccountState>>>,
    }

    impl MockBackend {
        /// Create a backend with no accounts registered.
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        /// Create a backend with the given account already registered.
        pub fn new_with_account(account_id: &str) -> Self {
            let b = Self::new();
            b.state
                .lock()
                .unwrap()
                .insert(account_id.to_owned(), AccountState::default());
            b
        }

        /// Pre-populate a `ShareNotification` in the given account.
        pub fn add_notification(&mut self, account_id: &str, notif_id: &str) {
            use jmap_sharing_types::ShareNotification;
            // Use serde_json deserialization — types are #[non_exhaustive] and
            // cannot be constructed with struct literals outside the defining crate.
            let notif: ShareNotification = serde_json::from_value(serde_json::json!({
                "id": notif_id,
                "created": "2024-01-01T00:00:00Z",
                "changedBy": { "name": "Test", "email": null, "principalId": null },
                "objectType": "Mailbox",
                "objectAccountId": "acc1",
                "objectId": "mb1",
                "oldRights": null,
                "newRights": null,
                "name": "Test Notification"
            }))
            .expect("test fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard
                .entry(account_id.to_owned())
                .or_insert_with(AccountState::default);
            acct.notifications.insert(Id::from(notif_id), notif);
        }
    }

    impl JmapBackend for MockBackend {
        type Error = MockError;

        async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
            Ok(self.state.lock().unwrap().contains_key(account_id.as_ref()))
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            account_id: &Id,
            ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            // This mock returns an empty list for simplicity.
            // Tests that need objects use add_notification / add_principal.
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

    impl SharingBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            // This mock returns the object as-is with a generated id string.
            // INVARIANT GAP: the returned `obj` retains whatever placeholder id
            // was passed in by the caller — it is NOT patched to `"mock-id-1"`.
            // This violates the SharingBackend::create_object invariant (the
            // returned O must have its `id` field set to the server-assigned Id).
            // Use MemoryBackend for tests that require a correct create response.
            Ok((Id::from("mock-id-1"), obj))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _account_id: &Id,
            _id: &Id,
            _patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            // Always return forbidden for updates to test the handler behavior.
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::Forbidden,
            )))
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            account_id: &Id,
            id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            // For ShareNotification, check the in-memory map.
            let mut guard = self.state.lock().unwrap();
            if let Some(acct) = guard.get_mut(account_id.as_ref()) {
                if acct.notifications.remove(id).is_some() {
                    acct.notification_state += 1;
                    return Ok(());
                }
                if acct.principals.remove(id).is_some() {
                    acct.principal_state += 1;
                    return Ok(());
                }
            }
            Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )))
        }

        fn supports_type<O: JmapObject>(&self) -> bool {
            true
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

    // Helper: build a minimal JmapRequest with one method call.
    fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
        JmapRequest::new(
            vec!["urn:ietf:params:jmap:principals".into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    /// Oracle: register_sharing_handlers registers all 10 RFC 9670 methods.
    ///
    /// Verification: each method name returns a non-error response when
    /// dispatched with a valid account (not `unknownMethod`).
    #[tokio::test]
    async fn registers_all_10_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

        let methods = [
            ("Principal/get", json!({"accountId": "acc1", "ids": null})),
            (
                "Principal/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            ("Principal/set", json!({"accountId": "acc1", "destroy": []})),
            (
                "Principal/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "Principal/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
            (
                "ShareNotification/get",
                json!({"accountId": "acc1", "ids": null}),
            ),
            (
                "ShareNotification/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "ShareNotification/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "ShareNotification/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "ShareNotification/queryChanges",
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

    /// Oracle: RFC 9670 §3.3 — ShareNotification/set with only destroy:[] returns
    /// a valid empty /set response (no errors, no created/updated/destroyed).
    #[tokio::test]
    async fn share_notification_set_empty_destroy_is_valid() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "ShareNotification/set",
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

    /// Oracle: RFC 9670 §3.3 — ShareNotification/set with create entries → notCreated
    /// contains `forbidden` for every create entry; no top-level error.
    #[tokio::test]
    async fn share_notification_set_create_returns_forbidden_via_dispatcher() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "ShareNotification/set",
            json!({
                "accountId": "acc1",
                "create": {
                    "c1": {
                        "id": "x", "created": "2024-01-01T00:00:00Z",
                        "changedBy": { "name": "A", "email": null, "principalId": null },
                        "objectType": "Mailbox", "objectAccountId": "a",
                        "objectId": "m1", "oldRights": null, "newRights": null,
                        "name": "N"
                    }
                }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert_eq!(
            args["notCreated"]["c1"]["type"], "forbidden",
            "create must be forbidden: {args}"
        );
    }
}

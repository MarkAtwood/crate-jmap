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
//! # use jmap_server::{Dispatcher, JmapBackend};
//! # fn example<B: SharingBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_sharing_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`SharingBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
mod helpers;
/// In-memory reference implementation of [`SharingBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
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
pub use jmap_sharing_types::JMAP_PRINCIPALS_URI;

/// Capability URI for `urn:ietf:params:jmap:principals:owner`.
pub use jmap_sharing_types::JMAP_PRINCIPALS_OWNER_URI;

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
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
pub fn register_sharing_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: SharingBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx).
    //
    // `$ci` is the call_id string (echoed back to the client). The sharing
    // handlers do not generate onSuccess* side-effect invocations, so all
    // sites bind it as `_ci`.
    //
    // `$ctx` is the per-request caller context (`B::CallerCtx`) forwarded
    // by the dispatcher. Closures pass `&ctx` to the inner `handle_*` fn.
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

    // Principal
    reg!("Principal/get", backend, |b, _ci, a, ctx| {
        handle_principal_get(&*b, &ctx, a).await
    });
    reg!("Principal/changes", backend, |b, _ci, a, ctx| {
        handle_principal_changes(&*b, &ctx, a).await
    });
    reg!("Principal/set", backend, |b, _ci, a, ctx| {
        handle_principal_set(&*b, &ctx, a).await
    });
    reg!("Principal/query", backend, |b, _ci, a, ctx| {
        handle_principal_query(&*b, &ctx, a).await
    });
    reg!("Principal/queryChanges", backend, |b, _ci, a, ctx| {
        handle_principal_query_changes(&*b, &ctx, a).await
    });

    // ShareNotification
    reg!("ShareNotification/get", backend, |b, _ci, a, ctx| {
        handle_share_notification_get(&*b, &ctx, a).await
    });
    reg!("ShareNotification/changes", backend, |b, _ci, a, ctx| {
        handle_share_notification_changes(&*b, &ctx, a).await
    });
    reg!("ShareNotification/set", backend, |b, _ci, a, ctx| {
        handle_share_notification_set(&*b, &ctx, a).await
    });
    reg!("ShareNotification/query", backend, |b, _ci, a, ctx| {
        handle_share_notification_query(&*b, &ctx, a).await
    });
    reg!(
        "ShareNotification/queryChanges",
        backend,
        |b, _ci, a, ctx| handle_share_notification_query_changes(&*b, &ctx, a).await
    );
}

pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// test_support — in-memory mock backend used by inline tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[deny(clippy::await_holding_lock)]
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
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.notifications.insert(Id::from(notif_id), notif);
        }
    }

    impl JmapBackend for MockBackend {
        type Error = MockError;
        type CallerCtx = ();

        async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
            Ok(self.state.lock().unwrap().contains_key(account_id.as_ref()))
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
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

    impl SharingBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
            // Mint a fixed test id ("mock-id-1") AND patch it into the
            // returned object, per the SharingBackend::create_object
            // invariant. Done via JSON round-trip because `SetObject` does
            // not expose a typed `set_id` API — every concrete impl is
            // `Deserialize`/`Serialize` so this is well-defined.
            // Previously this fixture intentionally retained the caller's
            // placeholder id and admitted the violation in a comment; the
            // gap is closed per bd:JMAP-3t94.17 so the in-tree fixture
            // matches the contract every downstream backend is expected
            // to honor.
            let server_id = Id::from("mock-id-1");
            let mut val = serde_json::to_value(&obj)
                .map_err(|e| BackendSetError::Other(MockError(format!("serialize: {e}"))))?;
            if let serde_json::Value::Object(ref mut m) = val {
                m.insert(
                    "id".to_owned(),
                    serde_json::Value::String(server_id.as_ref().to_owned()),
                );
            }
            let stored_obj: O = O::deserialize(&val).map_err(|e| {
                BackendSetError::Other(MockError(format!("deserialize after create: {e}")))
            })?;
            Ok((server_id, stored_obj))
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
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
            _caller: &(),
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

    /// Oracle: `SharingBackend::create_object` invariant — MockBackend
    /// returns an `O` whose `id` field equals the tuple's server-assigned
    /// Id. Regression guard for bd:JMAP-3t94.17 (the MockBackend's previous
    /// "INVARIANT GAP" admission).
    #[tokio::test]
    async fn mock_backend_create_object_returned_o_carries_server_id() {
        use crate::test_support::MockBackend;
        use jmap_sharing_types::Principal;

        let backend = MockBackend::new_with_account("acc1");
        let principal: Principal = serde_json::from_value(json!({
            "type": "individual",
            "name": "Probe",
            "id": "client-placeholder",
            "description": null,
            "email": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": null
        }))
        .expect("must deserialize");

        let (new_id, stored_obj) =
            <MockBackend as crate::SharingBackend>::create_object::<Principal>(
                &backend,
                &(),
                &jmap_types::Id::from("acc1"),
                "c1",
                principal,
            )
            .await
            .expect("create must succeed");

        // Serialize stored_obj to JSON and read the id field — Principal's
        // id field is the source of truth the handler ships to the client.
        let val = serde_json::to_value(&stored_obj).expect("serialize");
        let id_field = val
            .get("id")
            .and_then(|v| v.as_str())
            .expect("id field must be present");
        assert_eq!(
            id_field,
            new_id.as_ref(),
            "returned O.id MUST equal the server-assigned tuple Id \
             (SharingBackend::create_object invariant). Got id={id_field:?}, \
             tuple Id={:?}",
            new_id.as_ref()
        );
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

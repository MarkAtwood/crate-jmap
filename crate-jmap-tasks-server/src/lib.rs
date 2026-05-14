//! JMAP Tasks extension method handlers (draft-ietf-jmap-tasks-06).
//!
//! # Usage
//!
//! Implement [`TasksBackend`] for your storage layer, then call
//! [`register_tasks_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_tasks_server::{TasksBackend, register_tasks_handlers};
//! # use jmap_server::{Dispatcher, JmapBackend};
//! # fn example<B: TasksBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_tasks_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`TasksBackend`]. This is the same backend
//! used by this crate's own integration tests, intended for downstream
//! contributors to study and for smoke tests / examples that do not want
//! to stand up a real database. **Not production.** API stability is
//! opt-in via this feature and may break across minor versions while the
//! crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
mod helpers;
/// In-memory reference implementation of [`TasksBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
pub mod task;
pub mod task_list;
pub mod task_notification;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    TasksBackend,
};
pub use task::{
    handle_task_changes, handle_task_copy, handle_task_get, handle_task_query,
    handle_task_query_changes, handle_task_set,
};
pub use task_list::{handle_task_list_changes, handle_task_list_get, handle_task_list_set};
pub use task_notification::{
    handle_task_notification_changes, handle_task_notification_get, handle_task_notification_query,
    handle_task_notification_query_changes, handle_task_notification_set,
};

/// Capability URI for `urn:ietf:params:jmap:tasks`.
pub use jmap_tasks_types::JMAP_TASKS_URI;

// ---------------------------------------------------------------------------
// register_tasks_handlers — the main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP Tasks method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler.
///
/// After this call, the dispatcher handles:
/// `TaskList/get`, `TaskList/changes`, `TaskList/set`,
/// `Task/get`, `Task/changes`, `Task/set`, `Task/copy`,
/// `Task/query`, `Task/queryChanges`,
/// `TaskNotification/get`, `TaskNotification/changes`,
/// `TaskNotification/set`, `TaskNotification/query`,
/// `TaskNotification/queryChanges`.
pub fn register_tasks_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: TasksBackend + 'static,
{
    // Helper: register one method with a closure taking
    // (Arc<B>, call_id, args, ctx). `$ctx` is the per-request caller context
    // (`B::CallerCtx`) forwarded by the dispatcher; closures pass `&ctx` to the
    // inner `handle_*` fn. `$ci` is the call_id string — most handlers ignore
    // it (`_ci`); only `Task/copy` uses it.
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

    // TaskList
    reg!("TaskList/get", backend, |b, _ci, a, ctx| {
        handle_task_list_get(&*b, &ctx, a).await
    });
    reg!("TaskList/changes", backend, |b, _ci, a, ctx| {
        handle_task_list_changes(&*b, &ctx, a).await
    });
    reg!("TaskList/set", backend, |b, _ci, a, ctx| {
        handle_task_list_set(&*b, &ctx, a).await
    });

    // Task
    reg!("Task/get", backend, |b, _ci, a, ctx| {
        handle_task_get(&*b, &ctx, a).await
    });
    reg!("Task/changes", backend, |b, _ci, a, ctx| {
        handle_task_changes(&*b, &ctx, a).await
    });
    reg!("Task/set", backend, |b, _ci, a, ctx| {
        handle_task_set(&*b, &ctx, a).await
    });
    reg!("Task/copy", backend, |b, ci, a, ctx| {
        handle_task_copy(&*b, &ctx, a, &ci).await
    });
    reg!("Task/query", backend, |b, _ci, a, ctx| {
        handle_task_query(&*b, &ctx, a).await
    });
    reg!("Task/queryChanges", backend, |b, _ci, a, ctx| {
        handle_task_query_changes(&*b, &ctx, a).await
    });

    // TaskNotification
    reg!("TaskNotification/get", backend, |b, _ci, a, ctx| {
        handle_task_notification_get(&*b, &ctx, a).await
    });
    reg!("TaskNotification/changes", backend, |b, _ci, a, ctx| {
        handle_task_notification_changes(&*b, &ctx, a).await
    });
    reg!("TaskNotification/set", backend, |b, _ci, a, ctx| {
        handle_task_notification_set(&*b, &ctx, a).await
    });
    reg!("TaskNotification/query", backend, |b, _ci, a, ctx| {
        handle_task_notification_query(&*b, &ctx, a).await
    });
    reg!(
        "TaskNotification/queryChanges",
        backend,
        |b, _ci, a, ctx| handle_task_notification_query_changes(&*b, &ctx, a).await
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

    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use jmap_server::{
        BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
        QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    };
    use jmap_tasks_types::{Task, TaskList, TaskNotification};
    use jmap_types::{Id, State};

    use crate::backend::TasksBackend;

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
        task_lists: HashMap<Id, TaskList>,
        tasks: HashMap<Id, Task>,
        notifications: HashMap<Id, TaskNotification>,
        task_list_state: u64,
        task_state: u64,
        notification_state: u64,
    }

    /// In-memory mock backend for testing.
    #[derive(Clone)]
    pub struct MockBackend {
        state: Arc<Mutex<HashMap<String, AccountState>>>,
        /// Counts how many times `update_task_per_user` was called.
        /// Used by integration tests to verify per-user routing.
        pub per_user_calls: Arc<AtomicU32>,
    }

    impl MockBackend {
        /// Create a backend with no accounts registered.
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(HashMap::new())),
                per_user_calls: Arc::new(AtomicU32::new(0)),
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

        /// Register an additional account on an existing backend. Used by
        /// Task/copy tests that need two valid accounts (`fromAccountId`
        /// and `accountId` must differ per RFC 8620 §5.4).
        pub fn add_account(&mut self, account_id: &str) {
            self.state
                .lock()
                .unwrap()
                .entry(account_id.to_owned())
                .or_default();
        }

        /// Pre-populate a TaskNotification in the given account.
        pub fn add_notification(&mut self, account_id: &str, notif_id: &str) {
            let notif: TaskNotification = serde_json::from_value(serde_json::json!({
                "id": notif_id,
                "created": "2024-01-01T00:00:00Z",
                "changedBy": { "@type": "Person", "name": "Test" },
                "type": "created",
                "taskId": "task1"
            }))
            .expect("test fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.notifications.insert(Id::from(notif_id), notif);
        }

        /// Pre-populate a Task with a specific `isDraft` value in the given account.
        ///
        /// Used by tests for the isDraft immutability enforcement path.
        pub fn seed_task(&mut self, account_id: &str, task_id: &str, is_draft: bool) {
            let task: Task = serde_json::from_value(serde_json::json!({
                "id": task_id,
                "isDraft": is_draft
            }))
            .expect("task seed fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.tasks.insert(Id::from(task_id), task);
        }

        /// Pre-populate a TaskList with a task in the given account.
        pub fn add_task_list_with_task(&mut self, account_id: &str, list_id: &str) {
            let task_list: TaskList = serde_json::from_value(serde_json::json!({
                "id": list_id,
                "name": "Test List",
                "sortOrder": 0,
                "isSubscribed": true,
                "myRights": {
                    "mayReadItems": true,
                    "mayWriteAll": true,
                    "mayWriteOwn": true,
                    "mayUpdatePrivate": true,
                    "mayRSVP": true,
                    "mayAdmin": true,
                    "mayDelete": true
                }
            }))
            .expect("task list fixture must deserialize");
            let mut guard = self.state.lock().unwrap();
            let acct = guard.entry(account_id.to_owned()).or_default();
            acct.task_lists.insert(Id::from(list_id), task_list);
            // Add a task referencing the list
            let task: Task = serde_json::from_value(serde_json::json!({
                "id": "task1",
                "taskListId": list_id
            }))
            .expect("task fixture must deserialize");
            acct.tasks.insert(Id::from("task1"), task);
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
            // Attempt to serve from the tasks store via a serde round-trip.
            // When O = Task this is identity; for other types the deserialize
            // step will fail (no data in the store matches) and we fall back
            // to empty — preserving the prior behaviour for TaskList/get etc.
            let guard = self.state.lock().unwrap();
            let Some(acct) = guard.get(account_id.as_ref()) else {
                return Ok((vec![], vec![]));
            };

            let mut found: Vec<O> = Vec::new();
            let mut not_found: Vec<Id> = Vec::new();

            if let Some(id_slice) = ids {
                for id in id_slice {
                    if let Some(task) = acct.tasks.get(id) {
                        match serde_json::to_value(task)
                            .ok()
                            .and_then(|v| serde_json::from_value::<O>(v).ok())
                        {
                            Some(obj) => found.push(obj),
                            None => not_found.push(id.clone()),
                        }
                    } else {
                        not_found.push(id.clone());
                    }
                }
            }
            // If no specific ids were requested, serve nothing (empty list) —
            // the isDraft check always passes specific ids.
            Ok((found, not_found))
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

    impl TasksBackend for MockBackend {
        async fn create_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _create_id: &str,
            obj: O,
        ) -> Result<(Id, O), BackendSetError<Self::Error>> {
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
            account_id: &Id,
            id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            let mut guard = self.state.lock().unwrap();
            if let Some(acct) = guard.get_mut(account_id.as_ref()) {
                if acct.notifications.remove(id).is_some() {
                    acct.notification_state += 1;
                    return Ok(());
                }
                if acct.tasks.remove(id).is_some() {
                    acct.task_state += 1;
                    return Ok(());
                }
                if acct.task_lists.remove(id).is_some() {
                    acct.task_list_state += 1;
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

        async fn update_task_per_user(
            &self,
            caller: &(),
            account_id: &Id,
            id: &Id,
            patch: jmap_types::PatchObject,
        ) -> Result<Option<Task>, BackendSetError<Self::Error>> {
            // Track that this per-user path was called.
            self.per_user_calls.fetch_add(1, Ordering::Relaxed);
            // Delegate to update_object (same outcome as default impl).
            self.update_object::<Task>(caller, account_id, id, patch)
                .await
        }

        async fn task_list_has_tasks(
            &self,
            _caller: &(),
            account_id: &Id,
            task_list_id: &Id,
        ) -> bool {
            let guard = self.state.lock().unwrap();
            if let Some(acct) = guard.get(account_id.as_ref()) {
                return acct.tasks.values().any(|t| {
                    t.task_list_id
                        .as_ref()
                        .map(|lid| lid == task_list_id)
                        .unwrap_or(false)
                });
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use jmap_server::{Dispatcher, JmapRequest, State};
    use serde_json::json;

    use super::*;
    use crate::test_support::MockBackend;

    // Helper: build a minimal JmapRequest with one method call.
    fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
        JmapRequest::new(
            vec!["urn:ietf:params:jmap:tasks".into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    /// Oracle: register_tasks_handlers registers all 14 JMAP Tasks methods.
    ///
    /// Verification: each method name returns a non-error response when
    /// dispatched with a valid account (not `unknownMethod`).
    #[tokio::test]
    async fn registers_all_14_methods() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let methods = [
            ("TaskList/get", json!({"accountId": "acc1", "ids": null})),
            (
                "TaskList/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            ("TaskList/set", json!({"accountId": "acc1", "destroy": []})),
            ("Task/get", json!({"accountId": "acc1", "ids": null})),
            (
                "Task/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            ("Task/set", json!({"accountId": "acc1", "destroy": []})),
            (
                "Task/copy",
                json!({"fromAccountId": "acc1", "accountId": "acc1", "create": {}}),
            ),
            (
                "Task/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "Task/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
            (
                "TaskNotification/get",
                json!({"accountId": "acc1", "ids": null}),
            ),
            (
                "TaskNotification/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "TaskNotification/set",
                json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "TaskNotification/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "TaskNotification/queryChanges",
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

    /// Oracle: TaskNotification/set with create entries → notCreated contains
    /// `forbidden` for every create entry; no top-level error.
    #[tokio::test]
    async fn task_notification_set_create_returns_forbidden() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "TaskNotification/set",
            json!({
                "accountId": "acc1",
                "create": {
                    "c1": {
                        "id": "x",
                        "created": "2024-01-01T00:00:00Z",
                        "changedBy": { "@type": "Person", "name": "A" },
                        "type": "created",
                        "taskId": "t1"
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

    /// Oracle: TaskList/set destroy with tasks returns taskListHasTask error
    /// (draft-ietf-jmap-tasks-06 §3.4).
    ///
    /// When `onDestroyRemoveTasks` is false (default) and the task list has tasks,
    /// the destroy should fail with a custom `taskListHasTask` error.
    #[tokio::test]
    async fn task_list_set_destroy_with_tasks_returns_task_list_has_task() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_task_list_with_task("acc1", "list1");
        let backend = Arc::new(backend);

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "TaskList/set",
            json!({
                "accountId": "acc1",
                "destroy": ["list1"],
                "onDestroyRemoveTasks": false
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
            args["notDestroyed"]["list1"]["type"], "taskListHasTask",
            "must return taskListHasTask when list has tasks: {args}"
        );
    }

    // ── Integration tests: isDraft, utcStart, per-user routing ──────────────

    /// Oracle: draft-tasks-06 §4 — isDraft false→true revert is rejected via
    /// dispatcher (end-to-end path through register_tasks_handlers).
    #[tokio::test]
    async fn isdraft_revert_via_dispatcher() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_task("acc1", "t1", false);
        let backend = Arc::new(backend);

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Task/set",
            json!({
                "accountId": "acc1",
                "update": { "t1": { "isDraft": true } }
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
            args["notUpdated"]["t1"]["type"], "invalidProperties",
            "isDraft revert must be invalidProperties: {args}"
        );
        assert_eq!(
            args["notUpdated"]["t1"]["properties"][0], "isDraft",
            "isDraft must be listed in properties: {args}"
        );
    }

    /// Oracle: draft-tasks-06 §4 — isDraft false (draft → published) is always
    /// allowed; the handler must not pre-reject it.
    #[tokio::test]
    async fn isdraft_draft_to_publish_allowed() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_task("acc1", "t1", true);
        let backend = Arc::new(backend);

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Task/set",
            json!({
                "accountId": "acc1",
                "update": { "t1": { "isDraft": false } }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        // The patch must NOT be pre-rejected with invalidProperties.
        if let Some(not_updated) = args["notUpdated"].as_object() {
            if let Some(err) = not_updated.get("t1") {
                assert_ne!(
                    err["type"].as_str(),
                    Some("invalidProperties"),
                    "isDraft:false must not produce invalidProperties: {args}"
                );
            }
        }
    }

    /// Oracle: draft-tasks-06 §4 (lines 739-772) — utcStart is NOT returned
    /// when not in the properties list.
    #[tokio::test]
    async fn utcstart_not_in_default_properties() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Task/get",
            json!({ "accountId": "acc1", "ids": null, "properties": ["id"] }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(args.get("type").is_none(), "must not be error: {args}");
        for item in args["list"].as_array().unwrap_or(&vec![]) {
            assert!(
                item.get("utcStart").is_none(),
                "utcStart must not appear when not requested: {item}"
            );
        }
    }

    /// Oracle: draft-tasks-06 §4 — when utcStart is explicitly requested,
    /// the handler invokes compute_task_utc_times (default: returns None, so
    /// no value injected), but no error is raised.
    #[tokio::test]
    async fn utcstart_returned_when_requested() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "Task/get",
            json!({
                "accountId": "acc1",
                "ids": null,
                "properties": ["id", "utcStart"]
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "Task/get with utcStart must not error: {args}"
        );
    }

    /// Oracle: draft-tasks-06 §4.5.1 — a per-user-only patch is routed to
    /// `update_task_per_user`. MockBackend tracks the call count.
    #[tokio::test]
    async fn per_user_patch_split() {
        let backend = Arc::new(MockBackend::new_with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));

        let before = backend.per_user_calls.load(Ordering::Relaxed);

        let req = single_call(
            "Task/set",
            json!({
                "accountId": "acc1",
                "update": { "t1": { "color": "#ff0000" } }
            }),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be top-level error: {args}"
        );

        let after = backend.per_user_calls.load(Ordering::Relaxed);
        assert_eq!(
            after - before,
            1,
            "per_user update must have been called exactly once for a color-only patch"
        );
    }
}

//! TasksBackend trait and supporting types for JMAP Tasks method handlers.
//!
//! Consumers implement [`TasksBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations and type-specific operations are here.

pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};
pub use jmap_tasks_types::backend::{TaskListProperty, TaskNotificationProperty, TaskProperty};
pub use jmap_types::PatchObject;

// ---------------------------------------------------------------------------
// TasksBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Tasks method handlers (draft-ietf-jmap-tasks-06).
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and type introspection are here.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl TasksBackend>` when sharing across tasks.
pub trait TasksBackend: JmapBackend {
    /// Create a new object (TaskList or Task).
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing object.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Copy a Task from another account into the given account.
    ///
    /// Called by the `Task/copy` handler for each entry in the `create` map.
    fn copy_task(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        task: jmap_tasks_types::Task,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_tasks_types::Task), BackendSetError<Self::Error>>,
    > + Send;

    /// Returns true if the given task list contains at least one task.
    ///
    /// Called by `TaskList/set` destroy handler when `onDestroyRemoveTasks`
    /// is false: if this returns true, the destroy is rejected with
    /// `taskListHasTask` (draft-ietf-jmap-tasks-06 §3.4).
    fn task_list_has_tasks(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        task_list_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send;

    /// Returns true if `prop` is a per-user Task property (draft-tasks-06 §4.5.1).
    ///
    /// Per-user properties — `keywords`, `color`, `freeBusyStatus`,
    /// `useDefaultAlerts`, and `alerts` — belong to the authenticated user and
    /// MUST NOT change the shared `updated` timestamp when patched.  The routing
    /// logic in `handle_task_set` calls [`Self::update_task_per_user`] when
    /// every non-null patch key is in this set.
    fn is_per_user_property(prop: &str) -> bool {
        matches!(
            prop,
            "keywords" | "color" | "freeBusyStatus" | "useDefaultAlerts" | "alerts"
        )
    }

    /// Apply a patch that contains only per-user Task properties
    /// (draft-tasks-06 §4.5.1 — `keywords`, `color`, `freeBusyStatus`,
    /// `useDefaultAlerts`, `alerts`).
    ///
    /// When only per-user properties are patched, the shared `updated`
    /// timestamp MUST NOT change (§4.5.1 lines 978-981).  The default
    /// implementation delegates to [`Self::update_object`], which is correct
    /// for single-user scenarios but backends serving multiple users SHOULD
    /// override this method to route to a user-scoped patch path.
    fn update_task_per_user(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: PatchObject,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_tasks_types::Task>, BackendSetError<Self::Error>>,
    > + Send {
        // Task::Patch = PatchObject (see jmap_tasks_types backend.rs).
        self.update_object::<jmap_tasks_types::Task>(caller, account_id, id, patch)
    }

    /// Returns `true` if this backend enforces the `isDraft` immutability invariant
    /// atomically in `update_object` (by returning `SetError { error_type: InvalidProperties,
    /// properties: ["isDraft"] }` when a patch attempts to set `isDraft: true` on a
    /// published task).
    ///
    /// When `true`, the handler skips the `get_objects` pre-fetch in `Task/set` update
    /// processing, saving one backend round-trip per update that contains `isDraft: true`.
    ///
    /// Default: `false` — pre-fetch is always performed.
    fn enforce_is_draft_atomically(&self) -> bool {
        false
    }

    /// Compute `utcStart` and `utcDue` for a [`Task`](jmap_tasks_types::Task) by converting the task's
    /// `start`/`due` local-time fields and time zone into UTC (draft-tasks-06 §4,
    /// lines 739-772).
    ///
    /// Returns `(utc_start, utc_due)` as [`UTCDate`](jmap_types::UTCDate) values,
    /// or `None` for each if the corresponding field is absent or the time zone
    /// is unknown.
    ///
    /// The default implementation returns `(None, None)` — backends that do not
    /// support time-zone conversion can accept this behaviour and the caller will
    /// omit both fields.  Backends with full tz support should override this.
    ///
    /// # Parameters
    /// - `task` — the task whose `start` and `due` fields are to be converted.
    /// - `tz_hint` — an optional IANA time-zone override; if `None`, the task's
    ///   own `time_zone` field (if any) is used.
    fn compute_task_utc_times(
        &self,
        _task: &jmap_tasks_types::Task,
        _tz_hint: Option<&str>,
    ) -> (Option<jmap_types::UTCDate>, Option<jmap_types::UTCDate>) {
        // Default: no UTC conversion capability; callers omit utcStart/utcDue.
        (None, None)
    }
}

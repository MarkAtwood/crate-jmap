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
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
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
    /// `taskListHasTasks`.
    fn task_list_has_tasks(
        &self,
        account_id: &jmap_types::Id,
        task_list_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send;
}

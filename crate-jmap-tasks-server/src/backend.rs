//! TasksBackend trait and supporting types for JMAP Tasks method handlers.
//!
//! Consumers implement [`TasksBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations and type-specific operations are here.

use jmap_server::{BackendSetError, JmapBackend, JmapObject, SetObject};
use jmap_types::PatchObject;

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
    ///
    /// # Authorisation contract
    ///
    /// This method is the canonical enforcement point for caller authorisation
    /// per workspace AGENTS.md "Caller identity (foundation seam)" and per-crate
    /// AGENTS.md "Permission enforcement: backend canonical". Implementors MUST
    /// verify that the principal identified by
    /// [`JmapBackend::principal_id(caller)`](JmapBackend::principal_id) has write
    /// access to the target account / TaskList / Task before performing the
    /// mutation, and MUST return
    /// [`BackendSetError::SetError`] carrying
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. Handlers in this crate do NO
    /// permission checking; any handler-layer pre-check is defense-in-depth
    /// only and the backend MUST re-verify atomically with the mutation. A
    /// backend that trusts the handler is a bug. A backend whose
    /// `principal_id` returns `None` permits every write (fail-open,
    /// bd:JMAP-bghk) and CANNOT correctly implement TaskList ACLs or
    /// per-user scoping.
    ///
    /// # Errors
    ///
    /// Reject a create that RFC 8620 §5.3 defines an error for with
    /// [`BackendSetError::SetError`] carrying the matching
    /// [`SetErrorType`](jmap_server::SetErrorType); the handler places it verbatim in `notCreated`.
    /// For create these are `forbidden`, `overQuota`, `tooLarge`,
    /// `rateLimit` and `invalidProperties`. An `invalidProperties` error
    /// SHOULD name in `properties` "*all* the properties that were
    /// invalid", for example `taskListId` when a Task names a TaskList that
    /// does not exist.
    ///
    /// Return [`BackendSetError::Other`] only for unexpected storage
    /// failures. The handler reports it as `serverFail` without the
    /// error's `Display` text, which tells the client to retry; a
    /// deterministic rejection returned as `Other` invites retries that
    /// cannot succeed.
    ///
    /// The `/set` handlers in this crate reject an unknown `account_id`
    /// with the method-level `accountNotFound` error before calling the
    /// backend.
    ///
    /// # State and change log
    ///
    /// RFC 8620 §5.1 defines the state string over "*all* the data of
    /// this type in the account": "If the data changes, this string MUST
    /// change." Every successful `create_object`,
    /// [`update_object`](Self::update_object) and
    /// [`destroy_object`](Self::destroy_object) MUST advance
    /// [`get_state`](JmapBackend::get_state) for `O` in `account_id` and
    /// record the id as created, updated or destroyed so
    /// [`get_changes`](JmapBackend::get_changes) reports it.
    ///
    /// The write, the state advance and the change record MUST be atomic
    /// with respect to other callers: the reference `MemoryBackend` does
    /// all three under one mutex acquisition, and a database backend uses
    /// one transaction. A call that returns `Err` MUST leave the stored
    /// data, the state and the change log unchanged.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        build: F,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
    + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&jmap_types::Id) -> O + Send;

    /// Store `obj`, a record read from `from_account_id`, as a new record in
    /// `to_account_id`, for `/copy` (RFC 8620 §5.4).
    ///
    /// Called by `Task/copy` after the handler fetched the Task from
    /// `from_account_id` and applied the request's overrides, including any
    /// `metadata` and `privateMetadata` it supplies
    /// (draft-ietf-jmap-metadata-04 §3.4). `create` is the request's create
    /// object for this copy (RFC 8620 §5.4). -04 §3.4 says its `metadata`
    /// and `privateMetadata` follow "the same rules as a create in
    /// Foo/set", so the backend validates them as
    /// [`create_object`](Self::create_object) does (-04 §3.2 rules 1 to 4,
    /// `maxDepth` only on these supplied values) and leaves the copied
    /// values unvalidated ("Copied values are not revalidated against the
    /// destination account"). Every other rule of
    /// [`create_object`](Self::create_object) applies to the copy. The
    /// backend assigns the new `id`, stores the record in `to_account_id`
    /// and records the create in the state and change log as
    /// [`create_object`](Self::create_object) does. Returns the new id and
    /// the stored record.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller read access to `obj` in
    /// `from_account_id` and write access to `to_account_id` before storing the
    /// copy, and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object): a
    /// [`BackendSetError::SetError`] is reported for that copy.
    fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        obj: O,
        create: &serde_json::Value,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
    + Send;

    /// Apply a partial update (patch) to an existing object.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// applying the patch and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Patch format
    ///
    /// `patch` is an RFC 8620 §5.3 PatchObject, not an RFC 7396 JSON
    /// Merge Patch: "The keys are a path in JSON Pointer format
    /// \[RFC6901\], with an implicit leading "/"". So `{"a/b": null}`
    /// removes member `b` of property `a`; it does not touch a top-level
    /// property literally named `a/b`. A `null` value sets the
    /// property's default if it has one, otherwise removes it. A pointer
    /// into an array, a pointer whose parent does not exist, or two keys
    /// where one pointer is a prefix of the other "MUST be rejected with
    /// an "invalidPatch" error".
    ///
    /// Implementors SHOULD apply `patch` with
    /// [`jmap_server::apply_patch_object`], which enforces these rules
    /// all-or-nothing, and map any [`jmap_server::PatchPathError`] to
    /// [`jmap_server::SetErrorType::InvalidPatch`], as the reference
    /// `MemoryBackend` does.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object), plus these RFC 8620
    /// §5.3 rules for update:
    ///
    /// - "If an id given cannot be found, the update or destroy MUST be
    ///   rejected with a "notFound" set error."
    /// - A patch that breaks the rules under "Patch format" is
    ///   `invalidPatch`.
    /// - A patched record that is invalid is `invalidProperties`. That
    ///   includes changing a server-set property, though "it is not an
    ///   error to include a server-set property in an update as long as
    ///   the value is identical to the current value on the server."
    /// - A Task patch whose result has `isDraft: true` when the stored
    ///   value is not `true` (false, or absent after a `null` patch) is
    ///   `invalidProperties` naming `isDraft`: draft-tasks-06 §4 "This may
    ///   only be set to true upon creation." The `Task/set` handler does
    ///   not pre-check this; the backend is the only enforcement point, in
    ///   the same critical section that applies the patch.
    ///
    /// # Atomicity
    ///
    /// RFC 8620 §5.3: the server "MUST NOT only commit part of an update
    /// to a single record". Apply the whole patch and validate the result
    /// before writing it, so that on any error the stored record is
    /// unchanged. State and change log follow
    /// [`create_object`](Self::create_object) ("State and change log"); an
    /// `Ok(None)` return still advances the state.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// destroying it and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    ///
    /// # Errors
    ///
    /// As for [`create_object`](Self::create_object), plus these RFC 8620
    /// §5.3 rules for destroy: an unknown id, including one already
    /// destroyed, is `notFound`, and a destroy that "would violate an ACL
    /// or other permissions policy" is `forbidden`.
    ///
    /// # Cascades
    ///
    /// The handlers perform the cross-record effects the client chooses,
    /// through explicit backend calls, and the backend does not repeat
    /// them. `TaskList/set` `onDestroyRemoveTasks` does not yet destroy
    /// the list's Tasks (bd:JMAP-ji1v).
    ///
    /// If the backend itself changes another record because of the
    /// destroy, it MUST advance that record type's state and record the
    /// change in the same atomic step, so `/changes` on that type sees it.
    /// State and change log for `O` follow
    /// [`create_object`](Self::create_object) ("State and change log").
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    ///
    /// # Per-account semantics
    ///
    /// Capabilities are per account (RFC 8620 §1.6.2, §2
    /// `accountCapabilities`), so the answer may vary with `caller` and
    /// `account_id`. Backends that support every type in this trait for
    /// every account can ignore both arguments and return `true`.
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;

    /// Returns true if the given task list contains at least one task.
    ///
    /// Called by `TaskList/set` destroy handler when `onDestroyRemoveTasks`
    /// is false: if this returns `Ok(true)`, the destroy is rejected with
    /// `taskListHasTask` (draft-ietf-jmap-tasks-06 §3.4).
    ///
    /// # Three-way result
    ///
    /// The return type is `Result<bool, Self::Error>` to distinguish
    /// three states that callers actually need to tell apart
    /// (mirrors [`CalendarsBackend::calendar_has_events`]):
    ///
    /// - `Ok(true)` — the task list is definitely non-empty. The
    ///   handler rejects the destroy with `taskListHasTask`.
    /// - `Ok(false)` — the task list is definitely empty. The handler
    ///   forwards to `destroy_object`.
    /// - `Err(_)` — connectivity/transient failure. The handler maps
    ///   this to `serverFail` so the client knows to retry. Returning
    ///   `Ok(false)` for a transient backend failure is a bug: the
    ///   destroy proceeds and any tasks that DID exist become silently
    ///   orphaned.
    ///
    /// [`CalendarsBackend::calendar_has_events`]: https://docs.rs/jmap-calendars-server/latest/jmap_calendars_server/trait.CalendarsBackend.html#tymethod.calendar_has_events
    ///
    /// # Authorisation contract
    ///
    /// This is a read-side probe that informs a destroy decision the caller is
    /// already authorised to attempt; implementors MAY assume the caller has at
    /// least read access to the target TaskList (the surrounding `/set` flow
    /// re-validates write access in [`Self::destroy_object`]). Backends that
    /// scope task visibility per-principal MUST filter by the caller's effective
    /// access — a counter that includes tasks the caller cannot see leaks
    /// existence information.
    fn task_list_has_tasks(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        task_list_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Apply a patch that contains only per-user Task properties
    /// (draft-tasks-06 §4.5.1 — `keywords`, `color`, `freeBusyStatus`,
    /// `useDefaultAlerts`, `alerts`).
    ///
    /// The handler routes here when every patch key's leading segment
    /// satisfies [`jmap_tasks_types::is_per_user_task_property`]; that
    /// classification is fixed by the spec and is not a backend hook.
    ///
    /// When only per-user properties are patched, the shared `updated`
    /// timestamp MUST NOT change (§4.5.1 (per-user updated paragraph)).
    /// There is no default: delegating to [`Self::update_object`] is correct
    /// only for a single-user backend whose `update_object` leaves `updated`
    /// alone. A backend serving multiple users stores the patch in a
    /// user-scoped path.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify that the caller may modify per-user properties
    /// on the target Task and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`](jmap_server::SetErrorType::Forbidden) otherwise. That is the TaskList's
    /// `mayUpdatePrivate` right, which draft-ietf-jmap-tasks-06 §3 defines as:
    /// "The user may modify the following properties on all tasks in the task
    /// list, even if they would not otherwise have permission to modify that
    /// task." The write-access rule of [`Self::update_object`] does NOT apply
    /// here. See [`Self::create_object`] for the workspace-canonical rationale
    /// on backend-as-enforcement-point.
    fn update_task_per_user(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: PatchObject,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_tasks_types::Task>, BackendSetError<Self::Error>>,
    > + Send;

    /// Compute `utcStart` and `utcDue` for a [`Task`](jmap_tasks_types::Task) by converting the task's
    /// `start`/`due` local-time fields and time zone into UTC (draft-tasks-06 §4,
    /// utcStart/utcDue paragraphs).
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
    /// - `caller` — the caller context. The default implementation ignores it,
    ///   but backends MAY use [`JmapBackend::principal_id`] to resolve the
    ///   caller's preferred default time zone when `tz_hint` is `None`.
    /// - `account_id` — the target account. The default implementation ignores
    ///   it, but backends MAY use it to look up an account-scoped tz database
    ///   or per-account default time zone.
    /// - `task` — the task whose `start` and `due` fields are to be converted.
    /// - `tz_hint` — an optional IANA time-zone override; if `None`, the task's
    ///   own `time_zone` field (if any) is used.
    ///
    /// Naming and signature mirror the canonical
    /// [`CalendarsBackend::compute_utc_times`](https://docs.rs/jmap-calendars-server/latest/jmap_calendars_server/trait.CalendarsBackend.html#method.compute_utc_times)
    /// (bd:JMAP-ops7.25).
    ///
    /// [`JmapBackend::principal_id`]: jmap_server::JmapBackend::principal_id
    fn compute_utc_times(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _task: &jmap_tasks_types::Task,
        _tz_hint: Option<&str>,
    ) -> impl std::future::Future<
        Output = (Option<jmap_types::UTCDate>, Option<jmap_types::UTCDate>),
    > + Send {
        // Default: no UTC conversion capability; callers omit utcStart/utcDue.
        async { (None, None) }
    }
}

//! TasksBackend trait and supporting types for JMAP Tasks method handlers.
//!
//! Consumers implement [`TasksBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations and type-specific operations are here.

// ── Re-exports from jmap-server (the foundation crate) ────────────────────
//
// Every TasksBackend implementor needs these names. They are re-exported
// here so a consumer can write `use jmap_tasks_server::backend::*;` and get
// the full vocabulary in one import.

/// JMAP wire-format SetError; emitted by [`TasksBackend`] write methods
/// wrapped as [`BackendSetError::SetError`] and serialised into
/// `notCreated` / `notUpdated` / `notDestroyed` maps by the `/set` handlers
/// (RFC 8620 §5.3).
pub use jmap_server::SetError;

/// The standard wire-format SetError type tag (RFC 8620 §5.3 +
/// extension-specific types like `taskListHasTask` from
/// draft-ietf-jmap-tasks-06 §3.4).
pub use jmap_server::SetErrorType;

/// Backend-side wrapper around [`SetError`]; the trait's write methods
/// return `Result<…, BackendSetError<Self::Error>>`. The handler unwraps
/// `BackendSetError::SetError` into the per-target SetError map and
/// folds `BackendSetError::Other(e)` into a top-level `serverFail`.
pub use jmap_server::BackendSetError;

/// Backend-side error wrapper for `/changes` paths (RFC 8620 §5.2);
/// distinguished from [`BackendSetError`] because the `cannotCalculateChanges`
/// recovery is a `/changes`-specific contract.
pub use jmap_server::BackendChangesError;

/// Result of [`JmapBackend::get_changes`] (RFC 8620 §5.2). Carries
/// `created`, `updated`, `destroyed`, the `hasMoreChanges` flag, and the
/// new state token.
pub use jmap_server::ChangesResult;

/// Result of [`JmapBackend::query_objects`] (RFC 8620 §5.5). Carries the
/// id list, anchor position, total count (when requested), and state.
pub use jmap_server::QueryResult;

/// Result of [`JmapBackend::query_changes`] (RFC 8620 §5.6). Carries
/// `added`, `removed`, optional total count, and the new state.
pub use jmap_server::QueryChangesResult;

/// One entry in the `added` array of a [`QueryChangesResult`]: a typed
/// `(index, id)` pair.
pub use jmap_server::AddedItem;

/// Foundation `JmapBackend` supertrait. [`TasksBackend`] extends this with
/// Task-specific methods; consumers must implement both.
pub use jmap_server::JmapBackend;

/// Marker trait identifying a JMAP object type (RFC 8620 §5). Used as a
/// bound on `get_state` / `get_changes` / `query_objects`.
pub use jmap_server::JmapObject;

/// Marker trait identifying a "gettable" JMAP object type. Adds the
/// property-projection contract on top of [`JmapObject`].
pub use jmap_server::GetObject;

/// Marker trait identifying a "settable" JMAP object type. Adds the
/// `Patch` associated type on top of [`JmapObject`].
pub use jmap_server::SetObject;

/// Marker trait identifying a "queryable" JMAP object type. Adds the
/// `Filter` and `Comparator` associated types on top of [`JmapObject`].
pub use jmap_server::QueryObject;

// ── Property selector enums from jmap-tasks-types ────────────────────────
//
// These let backends interpret the `properties` argument on /get without
// reparsing string literals.

/// `TaskList` property selector enum (draft-ietf-jmap-tasks-06 §3); used
/// in [`JmapBackend::get_objects::<TaskList>`] property projection.
pub use jmap_tasks_types::backend::TaskListProperty;

/// `Task` property selector enum (draft-ietf-jmap-tasks-06 §4); used in
/// [`JmapBackend::get_objects::<Task>`] property projection.
pub use jmap_tasks_types::backend::TaskProperty;

/// `TaskNotification` property selector enum
/// (draft-ietf-jmap-tasks-06 §5).
pub use jmap_tasks_types::backend::TaskNotificationProperty;

// ── Wire types from jmap-types ───────────────────────────────────────────

/// JMAP wire-format `PatchObject` (RFC 8620 §5.3) — a JSON-Pointer-keyed
/// map of property paths to new values, used as the patch payload for
/// `update_object` and `update_task_per_user`.
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
    /// [`SetErrorType::Forbidden`] otherwise. Handlers in this crate do NO
    /// permission checking; any handler-layer pre-check is defense-in-depth
    /// only and the backend MUST re-verify atomically with the mutation. A
    /// backend that trusts the handler is a bug; a backend that returns `None`
    /// from `principal_id` is signalling "single-user posture" and CANNOT
    /// correctly implement TaskList ACLs or per-user scoping.
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
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller write-access to the target object before
    /// applying the patch and MUST return [`BackendSetError::SetError`] with
    /// [`SetErrorType::Forbidden`] otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
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
    /// [`SetErrorType::Forbidden`] otherwise. See [`Self::create_object`] for
    /// the workspace-canonical rationale; the same rule applies here.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;

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
    /// timestamp MUST NOT change (§4.5.1 (per-user updated paragraph)).  The default
    /// implementation delegates to [`Self::update_object`], which is correct
    /// for single-user scenarios but backends serving multiple users SHOULD
    /// override this method to route to a user-scoped patch path.
    ///
    /// # Authorisation contract
    ///
    /// Implementors MUST verify caller read access to the target Task (per-user
    /// properties are still scoped to the user, but only users who can see the
    /// Task can have per-user state on it) and MUST return
    /// [`BackendSetError::SetError`] with [`SetErrorType::Forbidden`] otherwise.
    /// The shared-property write-access rule from [`Self::update_object`] does
    /// NOT apply here — a user with only read access to the shared Task may
    /// still mutate their own per-user state. See [`Self::create_object`] for
    /// the workspace-canonical rationale on backend-as-enforcement-point.
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

    /// Declares which side of the draft-tasks-06 §4 (`isDraft` paragraph)
    /// immutability invariant enforcement contract this backend honours.
    ///
    /// This is a **correctness-handover contract**, not a performance flag.
    /// The invariant is: once a Task transitions to `isDraft: false`, the
    /// value MUST NOT be updated back to `true`. Exactly one of the handler
    /// or the backend is responsible for rejecting such a revert with
    /// [`SetErrorType::InvalidProperties`] (`properties: ["isDraft"]`); this
    /// method tells the handler which side owns enforcement on this backend.
    ///
    /// # Returning `false` (default)
    ///
    /// The handler in `Task/set` performs a `get_objects` pre-fetch on every
    /// update that contains `isDraft: true`, inspects the current value, and
    /// rejects the revert before calling [`Self::update_object`]. The backend
    /// is then free to apply the patch verbatim — it will never receive a
    /// spec-violating patch.
    ///
    /// # Returning `true`
    ///
    /// Implementor commits to **atomically rejecting** the
    /// `isDraft: false → true` revert inside [`Self::update_object`] (and
    /// [`Self::update_task_per_user`] if applicable), in the same critical
    /// section as the read-and-write that applies the patch. The handler
    /// skips its pre-fetch and forwards the raw patch.
    ///
    /// **Returning `true` without actually performing the atomic re-check is
    /// a spec violation that silently corrupts data.** The pre-fetch
    /// fast-path is removed under the assumption the backend will catch
    /// the revert; if it does not, draft-tasks-06 §4 immutability is broken
    /// with no client-visible signal. Workspace test-integrity rules
    /// (`AGENTS.md` "Permission enforcement: backend canonical") make the
    /// backend the canonical enforcement point regardless of which value
    /// is returned here — a `true` return value is opting OUT of the
    /// handler's defense-in-depth pre-check, not opting INTO enforcement
    /// the handler would otherwise do alone.
    ///
    /// # Performance is a side effect
    ///
    /// The handler's pre-fetch costs one extra `get_objects` round-trip per
    /// update that contains `isDraft: true`. Returning `true` eliminates
    /// that round-trip. This is real, but the reason to return `true` is
    /// that the backend genuinely enforces the invariant atomically — the
    /// round-trip saving alone is not a sufficient reason to flip the flag.
    ///
    /// # Reference impl
    ///
    /// The `memory` feature's `MemoryBackend` returns `true` and enforces
    /// the revert atomically in [`Self::update_object`] under the same lock
    /// that applies the patch. See `memory.rs` for the shape a production
    /// backend should mirror.
    ///
    /// # Default
    ///
    /// `false` — pre-fetch is always performed. Safe default for backends
    /// that have not yet wired atomic isDraft re-checking.
    fn enforce_is_draft_atomically(&self) -> bool {
        false
    }

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
    ) -> impl std::future::Future<Output = (Option<jmap_types::UTCDate>, Option<jmap_types::UTCDate>)>
           + Send {
        // Default: no UTC conversion capability; callers omit utcStart/utcDue.
        async { (None, None) }
    }
}

//! Shared test infrastructure.
//!
//! The in-memory backend used by these tests lives in the crate itself
//! as the public reference implementation
//! [`jmap_tasks_server::memory::MemoryBackend`]. This module re-exports
//! it under the historical `common::*` path so tests can use
//! `use common::MemoryBackend;` and match the cookie-cutter pattern
//! established by sibling server crates.
//!
//! It also keeps the test-only [`FaultyBackend`] fault-injection wrapper,
//! mirroring the canonical `jmap-mail-server` harness: it forces chosen
//! write operations to return `BackendSetError::Other(MemoryError)` so
//! tests can assert the per-id `serverFail` redaction contract.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]

pub use jmap_tasks_server::memory::{MemoryBackend, MemoryError};

use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesParams, ChangesResult,
    GetObject, JmapAccounts, JmapBackend, JmapObject, JmapReads, QueryChangesParams,
    QueryChangesResult, QueryObject, QueryParams, QueryResult, SetObject,
};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// FaultyBackend — injects BackendSetError::Other on demand
// ---------------------------------------------------------------------------

/// Canary literal embedded in every injected `BackendSetError::Other`
/// payload. Tests assert this string does NOT appear in the wire-format
/// `/set` response, proving the per-id `serverFail` path redacts
/// backend-error Display text. Mirrors the canonical
/// `jmap-mail-server` `tests/common/mod.rs` harness (bd:JMAP-ops7.28).
pub const FAULTY_BACKEND_CANARY: &str = "BACKEND-CANARY-LEAK-DO-NOT-WIRE-7f3a2";

/// A thin wrapper around [`MemoryBackend`] that can inject
/// `BackendSetError::Other` for specific `(type_name, operation)` pairs,
/// where operation is `"create"`, `"update"` or `"destroy"`.
///
/// Call [`FaultyBackend::inject`] before the operation under test. The
/// first matching call returns an error whose text contains
/// [`FAULTY_BACKEND_CANARY`]; the fault is then cleared so later calls go
/// to the inner backend. Every other method delegates to `inner`.
pub struct FaultyBackend {
    pub inner: MemoryBackend,
    failures:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(&'static str, &'static str)>>>,
}

impl FaultyBackend {
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            failures: Default::default(),
        }
    }

    /// Schedule a `BackendSetError::Other` for the next call to `op` on
    /// `type_name`. Injecting the same pair twice queues only one fault.
    pub fn inject(&self, type_name: &'static str, op: &'static str) {
        self.failures.lock().unwrap().insert((type_name, op));
    }

    /// Remove and return a previously-injected fault (fire-once).
    fn take_fault(&self, type_name: &'static str, op: &'static str) -> bool {
        self.failures.lock().unwrap().remove(&(type_name, op))
    }

    /// The injected error for `op`, carrying the canary in its Display text.
    fn fault(op: &str) -> BackendSetError<MemoryError> {
        BackendSetError::Other(MemoryError::new(format!(
            "injected {op} error {FAULTY_BACKEND_CANARY}"
        )))
    }
}

impl JmapAccounts for FaultyBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(&(), account_id).await
    }
}

impl JmapReads for FaultyBackend {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(&(), account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(&(), account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner.get_changes::<O>(&(), account_id, params).await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner.query_objects::<O>(&(), account_id, params).await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner.query_changes::<O>(&(), account_id, params).await
    }
}

impl jmap_tasks_server::TasksBackend for FaultyBackend {
    async fn create_object<O, F>(
        &self,
        _caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        if self.take_fault(O::TYPE_NAME, "create") {
            return Err(Self::fault("create"));
        }
        self.inner
            .create_object(&(), account_id, create_id, build)
            .await
    }

    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
        create: &serde_json::Value,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "create") {
            return Err(Self::fault("create"));
        }
        self.inner
            .copy_object(&(), from_account_id, to_account_id, obj, create)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "update") {
            return Err(Self::fault("update"));
        }
        self.inner
            .update_object::<O>(&(), account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "destroy") {
            return Err(Self::fault("destroy"));
        }
        self.inner.destroy_object::<O>(&(), account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self, _caller: &(), account_id: &Id) -> bool {
        self.inner.supports_type::<O>(&(), account_id)
    }

    async fn task_list_has_tasks(
        &self,
        _caller: &(),
        account_id: &Id,
        task_list_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.inner
            .task_list_has_tasks(&(), account_id, task_list_id)
            .await
    }

    async fn query_tasks(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, jmap_tasks_types::Task>,
        args: &jmap_tasks_server::TaskQueryArgs,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner.query_tasks(&(), account_id, params, args).await
    }

    async fn update_task_per_user(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: jmap_types::PatchObject,
    ) -> Result<Option<jmap_tasks_types::Task>, BackendSetError<Self::Error>> {
        self.inner
            .update_task_per_user(&(), account_id, id, patch)
            .await
    }
}

// ---------------------------------------------------------------------------
// UserBackend — two users and a fixed time-zone table
// ---------------------------------------------------------------------------

/// Who is calling a [`UserBackend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum User {
    /// The owner of the account: every right on every list.
    Owner,
    /// A sharee whose only write right is `mayUpdatePrivate`
    /// (draft-ietf-jmap-tasks-06 §3).
    PrivateOnlySharee,
}

/// A [`MemoryBackend`] seen by two users ([`User`]), with a test-only
/// time-zone table for `compute_utc_times`.
///
/// Rights follow draft-ietf-jmap-tasks-06 §3: the sharee may not create,
/// update or destroy (it has neither `mayWriteAll` nor `mayWriteOwn`), and
/// may make per-user patches (`update_task_per_user`, `mayUpdatePrivate`).
/// The memory backend itself is single-user, so per-user values are not
/// kept apart; the wrapper only decides who may write.
///
/// Time zones: `Etc/UTC` is offset 0 and `Test/Plus2` is a fixed +02:00
/// zone (a test fixture, not IANA data). A floating task (no `timeZone`)
/// uses `tz_hint`, or `Etc/UTC` when that is `None`; any other zone
/// converts to `None`.
pub struct UserBackend {
    pub inner: MemoryBackend,
}

impl UserBackend {
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
        }
    }

    /// `forbidden` for the sharee, which has no `mayWriteAll` or
    /// `mayWriteOwn`.
    fn check_write(user: User) -> Result<(), BackendSetError<MemoryError>> {
        match user {
            User::Owner => Ok(()),
            User::PrivateOnlySharee => Err(BackendSetError::SetError(Box::new(
                jmap_server::SetError::new(jmap_server::SetErrorType::Forbidden),
            ))),
        }
    }
}

/// The `maxExpandedQueryDuration` a [`UserBackend`] reports: one day.
pub const USER_BACKEND_MAX_EXPAND_SECONDS: u64 = 86_400;

/// Offset in hours east of UTC of a [`UserBackend`] test zone.
fn test_zone_offset_hours(zone: &str) -> Option<i64> {
    match zone {
        "Etc/UTC" => Some(0),
        "Test/Plus2" => Some(2),
        _ => None,
    }
}

/// `local` (`YYYY-MM-DDTHH:MM:SS`) in a zone `offset` hours east of UTC,
/// as a UTCDate. Only shifts that stay within the day are handled, which
/// is all the fixtures need.
fn to_utc(local: &str, offset: i64) -> Option<jmap_types::UTCDate> {
    let hour: i64 = local.get(11..13)?.parse().ok()?;
    let utc_hour = hour - offset;
    if !(0..24).contains(&utc_hour) {
        return None;
    }
    Some(jmap_types::UTCDate::from(format!(
        "{}{utc_hour:02}{}Z",
        local.get(..11)?,
        local.get(13..)?
    )))
}

impl JmapAccounts for UserBackend {
    type Error = MemoryError;
    type CallerCtx = User;

    async fn account_exists(&self, _caller: &User, account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(&(), account_id).await
    }
}

impl JmapReads for UserBackend {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &User,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(&(), account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &User,
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(&(), account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &User,
        account_id: &Id,
        params: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner.get_changes::<O>(&(), account_id, params).await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &User,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner.query_objects::<O>(&(), account_id, params).await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &User,
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner.query_changes::<O>(&(), account_id, params).await
    }
}

impl jmap_tasks_server::TasksBackend for UserBackend {
    async fn create_object<O, F>(
        &self,
        caller: &User,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        Self::check_write(*caller)?;
        self.inner
            .create_object(&(), account_id, create_id, build)
            .await
    }

    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &User,
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
        create: &serde_json::Value,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        Self::check_write(*caller)?;
        self.inner
            .copy_object(&(), from_account_id, to_account_id, obj, create)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &User,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        Self::check_write(*caller)?;
        self.inner
            .update_object::<O>(&(), account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &User,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        Self::check_write(*caller)?;
        self.inner.destroy_object::<O>(&(), account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self, _caller: &User, account_id: &Id) -> bool {
        self.inner.supports_type::<O>(&(), account_id)
    }

    async fn task_list_has_tasks(
        &self,
        _caller: &User,
        account_id: &Id,
        task_list_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.inner
            .task_list_has_tasks(&(), account_id, task_list_id)
            .await
    }

    async fn query_tasks(
        &self,
        _caller: &User,
        account_id: &Id,
        params: QueryParams<'_, jmap_tasks_types::Task>,
        args: &jmap_tasks_server::TaskQueryArgs,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner.query_tasks(&(), account_id, params, args).await
    }

    /// A one-day `maxExpandedQueryDuration`, [`USER_BACKEND_MAX_EXPAND_SECONDS`].
    fn limits(&self, _caller: &User, _account_id: &Id) -> jmap_tasks_server::TasksLimits {
        jmap_tasks_server::TasksLimits::new(USER_BACKEND_MAX_EXPAND_SECONDS)
    }

    /// Both users may: the owner holds every right, and the sharee holds
    /// `mayUpdatePrivate` (draft-ietf-jmap-tasks-06 §3).
    async fn update_task_per_user(
        &self,
        _caller: &User,
        account_id: &Id,
        id: &Id,
        patch: jmap_types::PatchObject,
    ) -> Result<Option<jmap_tasks_types::Task>, BackendSetError<Self::Error>> {
        self.inner
            .update_task_per_user(&(), account_id, id, patch)
            .await
    }

    async fn compute_utc_times(
        &self,
        _caller: &User,
        _account_id: &Id,
        task: &jmap_tasks_types::Task,
        tz_hint: Option<&str>,
    ) -> (Option<jmap_types::UTCDate>, Option<jmap_types::UTCDate>) {
        let zone = task.time_zone.as_deref().or(tz_hint).unwrap_or("Etc/UTC");
        let Some(offset) = test_zone_offset_hours(zone) else {
            return (None, None);
        };
        let convert = |local: &Option<String>| local.as_deref().and_then(|l| to_utc(l, offset));
        (convert(&task.start), convert(&task.due))
    }
}

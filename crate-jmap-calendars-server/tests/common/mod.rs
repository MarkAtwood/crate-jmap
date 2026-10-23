//! Shared test infrastructure.
//!
//! The in-memory backend used by these tests lives in the crate itself
//! as the public reference implementation
//! [`jmap_calendars_server::memory::MemoryBackend`]. This module re-exports
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

pub use jmap_calendars_server::memory::{MemoryBackend, MemoryError};

use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesParams, ChangesResult,
    GetObject, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetObject,
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

impl JmapBackend for FaultyBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(&(), account_id).await
    }

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

impl jmap_calendars_server::CalendarsBackend for FaultyBackend {
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

    async fn update_per_user_properties(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: jmap_types::PatchObject,
    ) -> Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>> {
        self.inner
            .update_per_user_properties(&(), account_id, id, patch)
            .await
    }

    async fn calendar_has_events(
        &self,
        _caller: &(),
        account_id: &Id,
        calendar_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.inner
            .calendar_has_events(&(), account_id, calendar_id)
            .await
    }

    async fn set_default_calendar(
        &self,
        _caller: &(),
        account_id: &Id,
        calendar_id: &Id,
    ) -> Result<jmap_calendars_server::SetDefaultResult, Self::Error> {
        self.inner
            .set_default_calendar(&(), account_id, calendar_id)
            .await
    }

    async fn set_default_participant_identity(
        &self,
        _caller: &(),
        account_id: &Id,
        identity_id: &Id,
    ) -> Result<jmap_calendars_server::SetDefaultResult, Self::Error> {
        self.inner
            .set_default_participant_identity(&(), account_id, identity_id)
            .await
    }

    async fn parse_calendar_event_blobs(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_ids: &[Id],
        properties: Option<&[String]>,
    ) -> Result<jmap_calendars_server::CalendarEventParseResult, Self::Error> {
        self.inner
            .parse_calendar_event_blobs(&(), account_id, blob_ids, properties)
            .await
    }

    async fn get_availability(
        &self,
        _caller: &(),
        account_id: &Id,
        principal_id: &Id,
        utc_start: &jmap_types::UTCDate,
        utc_end: &jmap_types::UTCDate,
        show_details: bool,
        event_properties: Option<&[String]>,
    ) -> Result<Vec<jmap_calendars_types::BusyPeriod>, BackendQueryError<Self::Error>> {
        self.inner
            .get_availability(
                &(),
                account_id,
                principal_id,
                utc_start,
                utc_end,
                show_details,
                event_properties,
            )
            .await
    }
}

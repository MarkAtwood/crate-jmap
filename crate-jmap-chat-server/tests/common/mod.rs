//! Shared test infrastructure.
//!
//! Most of the in-memory backend used by these tests now lives in the
//! crate itself as the public reference implementation
//! [`jmap_chat_server::memory::MemoryBackend`]. This module:
//!
//! - re-exports that public reference impl (and `MemoryError`) under the
//!   historical `common::*` paths, so existing tests can use
//!   `use common::MemoryBackend;` unchanged.
//! - keeps the test-only [`FaultyBackend`] negative-path wrapper that
//!   always returns errors. This is testing scaffolding (not a reference
//!   impl) and so stays here.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(async_fn_in_trait)]

// Re-exports — keep `use common::MemoryBackend;` working for tests.
pub use jmap_chat_server::memory::{MemoryBackend, MemoryError};

use jmap_chat_server::{
    BackendChangesError, BackendSetError, ChangesResult, ChatBackend, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetObject,
};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// FaultyBackend — always returns errors, for negative-path testing
// ---------------------------------------------------------------------------

/// A backend that always returns errors. Used to test the handler's error paths.
#[derive(Clone, Default)]
pub struct FaultyBackend;

impl JmapBackend for FaultyBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), _account_id: &Id) -> Result<bool, Self::Error> {
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
    ) -> Result<State, Self::Error> {
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _since_state: &State,
        _max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        Err(BackendChangesError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
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
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        Err(BackendChangesError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }
}

impl ChatBackend for FaultyBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _create_id: &str,
        _obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _id: &Id,
        _patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        false
    }

    fn generate_invite_code(&self) -> String {
        // test-only: not a CSPRNG
        format!(
            "{:012x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                & 0xffff_ffff_ffff,
        )
    }
}

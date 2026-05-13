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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// Re-exports — keep `use common::MemoryBackend;` working for tests.
pub use jmap_chat_server::memory::{MemoryBackend, MemoryError};

use jmap_chat_server::{
    BackendChangesError, BackendSetError, ChangesResult, ChatBackend, ChatLimits, EmojiSetOp,
    GetObject, JmapBackend, JmapObject, OpResult, QueryChangesResult, QueryObject, QueryResult,
    SetObject, SlowModeError, SpacePatchOp,
};
use jmap_types::{Id, State, UTCDate};

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

    async fn apply_space_patch(
        &self,
        _caller: &(),
        _account_id: &Id,
        _space_id: &Id,
        _ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }
}

// ---------------------------------------------------------------------------
// TrackingBackend — MemoryBackend with policy hooks overridable per-test
// ---------------------------------------------------------------------------

/// A `MemoryBackend` wrapper that lets a test inject custom outcomes for
/// individual [`ChatBackend`] policy hooks (`slow_mode_check`, etc.) while
/// otherwise delegating to the reference impl for storage, change-log, and
/// all read/write methods.
///
/// The current implementation only knobs `slow_mode_check`. Future Layer B
/// beads (`is_contact_blocked`, `may_set_custom_emoji`, etc.) extend the
/// configuration surface with the same wrapper pattern rather than
/// inventing a fresh test backend per method.
#[derive(Clone, Default)]
pub struct TrackingBackend {
    inner: MemoryBackend,
    /// When `Some`, [`ChatBackend::slow_mode_check`] returns
    /// `Err(SlowModeError { retry_after: <this> })`. When `None`,
    /// forwards to `inner` (which is a no-op).
    slow_mode_block: Option<UTCDate>,
    /// When `true`, [`ChatBackend::may_set_custom_emoji`] returns
    /// `Ok(false)` for every op (Create/Update/Destroy). When `false`,
    /// forwards to `inner` (which returns `Ok(true)`).
    emoji_set_deny: bool,
    /// Counter incremented every time
    /// [`ChatBackend::is_contact_blocked`] is invoked. Lets a test
    /// verify the handler reached the consultation point without
    /// relying on a side effect of the consultation itself (the
    /// kit's wire response is unchanged regardless of the predicate
    /// result, see `handle_chat_typing` doc-comment).
    is_contact_blocked_calls: Arc<AtomicU64>,
}

impl TrackingBackend {
    /// Fresh `TrackingBackend` with all policy hooks at their default
    /// no-op behaviour (slow-mode allows everything, emoji-set
    /// authorization allows everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure the wrapper so [`ChatBackend::slow_mode_check`] always
    /// rejects with the given `retry_after` UTCDate. The wrapped
    /// `MemoryBackend` is otherwise functional.
    pub fn with_slow_mode_blocking(retry_after: UTCDate) -> Self {
        Self {
            inner: MemoryBackend::new(),
            slow_mode_block: Some(retry_after),
            emoji_set_deny: false,
            is_contact_blocked_calls: Arc::default(),
        }
    }

    /// Configure the wrapper so [`ChatBackend::may_set_custom_emoji`]
    /// returns `Ok(false)` for every op. The wrapped `MemoryBackend`
    /// is otherwise functional.
    pub fn with_emoji_set_denied() -> Self {
        Self {
            inner: MemoryBackend::new(),
            slow_mode_block: None,
            emoji_set_deny: true,
            is_contact_blocked_calls: Arc::default(),
        }
    }

    /// Borrow the underlying `MemoryBackend` for test seeding (e.g.
    /// `register_account`).
    pub fn inner(&self) -> &MemoryBackend {
        &self.inner
    }

    /// Return the number of times the wrapped
    /// [`ChatBackend::is_contact_blocked`] has been invoked. Used by
    /// tests that verify the handler reaches the predicate-call site.
    pub fn is_contact_blocked_call_count(&self) -> u64 {
        self.is_contact_blocked_calls.load(Ordering::SeqCst)
    }
}

impl JmapBackend for TrackingBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        self.inner
            .query_objects::<O>(caller, account_id, filter, sort, limit, position)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .query_changes::<O>(
                caller,
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

impl ChatBackend for TrackingBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        self.inner
            .create_object::<O>(caller, account_id, create_id, obj)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        self.inner
            .update_object::<O>(caller, account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.inner.destroy_object::<O>(caller, account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        self.inner.supports_type::<O>()
    }

    fn generate_invite_code(&self) -> String {
        self.inner.generate_invite_code()
    }

    fn limits(&self, caller: &(), account_id: &Id) -> ChatLimits {
        self.inner.limits(caller, account_id)
    }

    async fn apply_space_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_patch(caller, account_id, space_id, ops)
            .await
    }

    async fn slow_mode_check(
        &self,
        caller: &(),
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<(), SlowModeError> {
        match &self.slow_mode_block {
            Some(d) => Err(SlowModeError {
                retry_after: d.clone(),
            }),
            None => {
                self.inner
                    .slow_mode_check(caller, account_id, chat_id)
                    .await
            }
        }
    }

    async fn may_set_custom_emoji(
        &self,
        caller: &(),
        account_id: &Id,
        target_space_id: Option<&Id>,
        op: EmojiSetOp,
    ) -> Result<bool, Self::Error> {
        if self.emoji_set_deny {
            Ok(false)
        } else {
            self.inner
                .may_set_custom_emoji(caller, account_id, target_space_id, op)
                .await
        }
    }

    async fn is_contact_blocked(
        &self,
        caller: &(),
        account_id: &Id,
        contact_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.is_contact_blocked_calls.fetch_add(1, Ordering::SeqCst);
        self.inner
            .is_contact_blocked(caller, account_id, contact_id)
            .await
    }

    fn retains_edit_history(&self) -> bool {
        self.inner.retains_edit_history()
    }

    async fn expire_message(
        &self,
        caller: &(),
        account_id: &Id,
        message_id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.inner
            .expire_message(caller, account_id, message_id)
            .await
    }
}

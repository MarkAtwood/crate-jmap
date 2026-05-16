//! In-memory reference implementation of [`ChatBackend`](crate::ChatBackend).
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`ChatBackend`](crate::ChatBackend) trait to
//!    study when writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Chat dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of JMAP Chat draft edge cases are simplified (see source comments).
//!
//! # Feature flag and API stability
//!
//! This module is gated behind `feature = "memory"` and is **not** enabled
//! by default. Its public API stability is opt-in: it may break across
//! minor versions while the crate is pre-1.0.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use jmap_chat_server::{memory::MemoryBackend, register_chat_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_chat_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
//! ```
//!
//! # Concurrency
//!
//! `std::sync::Mutex` is used for simplicity. The `await_holding_lock`
//! clippy lint is enabled module-wide and enforces that no lock guard
//! is held across an `.await`. If a future change requires holding a
//! guard across `.await`, switch to `tokio::sync::Mutex` rather than
//! disabling the lint.
//!
//! # Tracking
//!
//! Promoted from `tests/common/mod.rs` per Beads issue JMAP-hwdv (epic)
//! and JMAP-hwdv.3 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, ChatBackend, ChatLimits,
    EmojiSetOp, GetObject, JmapBackend, JmapObject, OpResult, QueryChangesResult, QueryObject,
    QueryResult, SetError, SetErrorType, SetObject, SlowModeError, SpacePatchOp,
};
use jmap_server::{json_merge_patch, now_utc_string, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A change log entry for one state transition.
#[derive(Clone, Debug)]
struct ChangeEntry {
    /// The state counter AFTER this change.
    new_state: u64,
    created: Vec<Id>,
    updated: Vec<Id>,
    destroyed: Vec<Id>,
}

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// explicitly registered account ids (accounts may exist with no objects yet)
    known_accounts: HashSet<String>,
    /// Test-only override for [`ChatBackend::limits`]. `None` means use
    /// [`ChatLimits::default`]. Set via
    /// [`MemoryBackend::set_limits_for_test`].
    limits_override: Option<ChatLimits>,
    /// Test-only override for [`ChatBackend::retains_edit_history`].
    /// Set via [`MemoryBackend::set_retains_edit_history_for_test`].
    retains_edit_history: bool,
    /// Test-only override for [`ChatBackend::protect_last_admin`].
    ///
    /// Default `false` — opposite the trait default of `true` — so
    /// existing tests that do not seed admin memberships are not
    /// broken. Tests that exercise the protection path opt in via
    /// [`MemoryBackend::set_protect_last_admin_for_test`].
    /// Production deployments override the trait method instead.
    protect_last_admin: bool,
}

impl Inner {
    fn current_state(&self, type_name: &'static str, account_id: &str) -> u64 {
        self.states
            .get(&(type_name, account_id.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    fn bump_state(&mut self, type_name: &'static str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry((type_name, account_id.to_owned()))
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        self.known_accounts.insert(account_id.to_owned());
        self.objects
            .entry((type_name, account_id.to_owned()))
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &'static str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects.get(&(type_name, account_id.to_owned()))
    }

    fn change_log_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut Vec<ChangeEntry> {
        self.change_log
            .entry((type_name, account_id.to_owned()))
            .or_default()
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`ChatBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Used as both the integration-test
/// harness and the canonical example for backend implementors.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Create a new, empty `MemoryBackend`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an account as known even if it has no objects yet.
    /// Use this in tests that need an empty-but-valid account.
    pub fn register_account(&self, account_id: &Id) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
    }

    /// Demo-grade id minter for the in-memory reference backend.
    ///
    /// NOT A PRODUCTION PATTERN. Both modes below are explicitly
    /// demonstration-quality; production backends must mint real ULIDs
    /// (or equivalent globally-unique, monotonic, persistent-across-restarts
    /// ids) and never use this helper as a copy-paste source.
    ///
    /// Behavior is controlled by the `realistic-demo-ids` cargo feature:
    ///
    /// - **Default (deterministic):** returns `"<type><n:016x>"` where `n`
    ///   is the per-(type, account) object count + 1. Lex-orderable within
    ///   a (type, account) namespace, repeatable across test runs, easy to
    ///   read in test debug output. Load-bearing for
    ///   draft-atwood-jmap-chat-00 `Chat.unreadCount` semantics (count of
    ///   Messages whose id is lex-greater than `lastReadMessageId`).
    /// - **`realistic-demo-ids` enabled:** returns `"{n:016x}"` matching
    ///   the canonical `jmap-mail-server` pattern at `email.rs:1748` —
    ///   process-start nanos as base, atomic counter, no type prefix,
    ///   no per-account scoping. Lex-orderable globally within a process,
    ///   not repeatable across runs.
    fn demo_next_id(inner: &mut Inner, type_name: &'static str, account_id: &str) -> Id {
        #[cfg(feature = "realistic-demo-ids")]
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::sync::OnceLock;
            use std::time::{SystemTime, UNIX_EPOCH};

            let _ = (inner, type_name, account_id);
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            static BASE: OnceLock<u64> = OnceLock::new();
            let base = *BASE.get_or_init(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1_000_000_000)
            });
            let n = base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed));
            Id::from(format!("{n:016x}"))
        }

        #[cfg(not(feature = "realistic-demo-ids"))]
        {
            // CARGO-CULT WARNING: do NOT copy this into a production backend.
            // This deterministic mode uses an in-memory `len()` counter that:
            //   - collides after deletes (count goes down, next id re-uses)
            //   - resets to 0 on every process restart
            //   - is not unique across (type, account) namespaces
            // Production-grade id minting needs a real ULID or equivalent.
            let n = inner
                .objects_ref(type_name, account_id)
                .map_or(0, |m| m.len());
            let new_id = Id::from(format!("{}{:016x}", type_name.to_ascii_lowercase(), n + 1));
            debug_assert!(
                !inner
                    .objects_ref(type_name, account_id)
                    .is_some_and(|m| m.contains_key(&new_id)),
                "MemoryBackend demo_next_id collision: deterministic mode uses a len()-based \
                 counter that cannot survive deletes. This is the demo impl — production \
                 backends must use ULIDs."
            );
            new_id
        }
    }

    /// Test-only: flip the [`ChatBackend::retains_edit_history`] flag.
    ///
    /// Pass `true` to make `Message/get` return `editHistory` on
    /// fetched messages, or `false` to omit it. The default is
    /// `false`, matching the trait default (the reference backend
    /// does not retain edit history out of the box).
    #[doc(hidden)]
    pub fn set_retains_edit_history_for_test(&self, retain: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.retains_edit_history = retain;
    }

    /// Test-only: flip the [`ChatBackend::protect_last_admin`] flag.
    ///
    /// Pass `true` to enable last-admin protection on `RemoveMember`
    /// ops, or `false` to disable. The default is `false`, opposite
    /// the trait default of `true` — this is intentional: production
    /// deployments inherit the trait default; the reference backend
    /// opts out so existing demo tests don't have to seed admin
    /// memberships. Production callers must not use this method; the
    /// API stability disclaimer on the `memory` feature applies
    /// doubly here.
    ///
    /// See `bd:JMAP-g7wu.2.4.3` for the design rationale (last-admin
    /// protection replacing the dropped `Space.ownerId` design).
    #[doc(hidden)]
    pub fn set_protect_last_admin_for_test(&self, value: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.protect_last_admin = value;
    }

    /// Test-only: apply a [`SpacePatchOp`] sequence as if the caller's
    /// resolved principal id were `caller_id`, bypassing the normal
    /// `Self::principal_id(caller)` lookup.
    ///
    /// Used by integration-test backends whose `CallerCtx` is a
    /// richer type than `()` and that override
    /// [`jmap_server::JmapBackend::principal_id`] to expose a real
    /// caller identity. The wrapped `MemoryBackend`'s
    /// `CallerCtx = ()` prevents the test backend from driving
    /// identity through `MemoryBackend`'s own
    /// [`ChatBackend::apply_space_patch`] surface; this method offers
    /// a direct entry point that supplies the resolved caller id to
    /// the per-op enforcement code.
    ///
    /// `caller_id == None` produces single-user-mode behavior
    /// identical to the trait method's default (criterion 7 in
    /// `bd:JMAP-g7wu.2.4.3`): identity-dependent gates are skipped.
    /// Production callers must not use this method; the API
    /// stability disclaimer on the `memory` feature applies doubly
    /// here.
    ///
    /// See `bd:JMAP-g7wu.2.4.3`.
    #[doc(hidden)]
    #[allow(clippy::result_large_err)]
    pub fn apply_space_patch_with_caller_id(
        &self,
        caller_id: Option<&Id>,
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<MemoryError>> {
        let mut inner = self.inner.lock().unwrap();
        apply_space_patch_impl(&mut inner, caller_id, account_id, space_id, ops)
    }

    /// Apply a top-level metadata patch with an explicitly supplied
    /// caller id (test-only entry point — analog of
    /// [`Self::apply_space_patch_with_caller_id`]).
    ///
    /// Mirrors the rationale on the structural sibling: `MemoryBackend`
    /// uses `CallerCtx = ()` and so the trait method
    /// [`ChatBackend::apply_space_metadata_patch`] resolves caller
    /// identity to `None` (single-user mode), which skips the
    /// `manage_space` gate. Identity-bearing integration-test
    /// backends (`IdentityBackend`) route through this method instead
    /// to drive the gate.
    ///
    /// `caller_id == None` reproduces single-user-mode behavior.
    /// Production callers must not use this method; the API
    /// stability disclaimer on the `memory` feature applies doubly
    /// here.
    ///
    /// See `bd:JMAP-g7wu.2.4.13`.
    #[doc(hidden)]
    #[allow(clippy::result_large_err)]
    pub fn apply_space_metadata_patch_with_caller_id(
        &self,
        caller_id: Option<&Id>,
        account_id: &Id,
        space_id: &Id,
        patch_map: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Option<jmap_chat_types::Space>, BackendSetError<MemoryError>> {
        let mut inner = self.inner.lock().unwrap();
        apply_space_metadata_patch_impl(&mut inner, caller_id, account_id, space_id, patch_map)
    }

    /// Test-only: override the [`ChatLimits`] returned by
    /// [`ChatBackend::limits`].
    ///
    /// Pass `Some(limits)` to install a custom cap structure, or `None`
    /// to revert to [`ChatLimits::default`]. Used by count-limit
    /// enforcement integration tests (bd:JMAP-g7wu.2.4.8) that need
    /// tight caps to exercise the `overQuota` path without seeding
    /// hundreds of objects. Production callers must not use this
    /// method; the API stability disclaimer on the `memory` feature
    /// applies doubly here.
    #[doc(hidden)]
    pub fn set_limits_for_test(&self, limits: Option<ChatLimits>) {
        let mut inner = self.inner.lock().unwrap();
        inner.limits_override = limits;
    }

    /// Test-only: directly inject a JSON-shaped object into the backend
    /// store, bypassing the normal `create_object` validation/serde
    /// round-trip and skipping change-log emission.
    ///
    /// This exists for integration tests that need to seed objects for
    /// types whose `Chat/set` create path is not yet implemented (e.g.
    /// channels — bd:JMAP-g7wu.2.4.4). Production callers must not use
    /// this method; the API stability disclaimer on the `memory`
    /// feature applies doubly here.
    #[doc(hidden)]
    pub fn insert_object_for_test(
        &self,
        type_name: &'static str,
        account_id: &str,
        id: &str,
        value: serde_json::Value,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .objects_mut(type_name, account_id)
            .insert(Id::from(id), value);
    }

    /// Test-only: return the first Category id stored on the given Space.
    /// Used by Space/set Category tests that need to refer back to a
    /// server-assigned category id.
    ///
    /// Panics if the Space has no categories or does not exist.
    #[doc(hidden)]
    pub fn first_category_id(&self, space_id: &Id) -> Id {
        let inner = self.inner.lock().unwrap();
        // Walk every account looking for this space id. Integration
        // tests use a single account "a1" so the iteration is trivial.
        for ((type_name, _account), map) in &inner.objects {
            if *type_name != "Space" {
                continue;
            }
            if let Some(space) = map.get(space_id) {
                let cats = space
                    .get("categories")
                    .and_then(|v| v.as_array())
                    .expect("Space.categories must be an array");
                let id = cats
                    .first()
                    .and_then(|c| c.get("id"))
                    .and_then(|id| id.as_str())
                    .expect("Space has at least one category");
                return Id::from(id);
            }
        }
        panic!("Space {space_id} not found");
    }

    /// Test-only: return a snapshot of the named Chat's JSON value.
    /// Used to assert cross-reference fields like `categoryId` after
    /// Space/set Category mutations have cascaded.
    ///
    /// Panics if the Chat does not exist.
    #[doc(hidden)]
    pub fn peek_chat(&self, chat_id: &Id) -> serde_json::Value {
        let inner = self.inner.lock().unwrap();
        for ((type_name, _account), map) in &inner.objects {
            if *type_name != "Chat" {
                continue;
            }
            if let Some(chat) = map.get(chat_id) {
                return chat.clone();
            }
        }
        panic!("Chat {chat_id} not found");
    }
}

/// A simple string error for `MemoryBackend` failures.
#[derive(Debug)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MemoryError {}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.known_accounts.contains(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        let inner = self.inner.lock().unwrap();
        let map = inner.objects_ref(O::TYPE_NAME, account_id.as_ref());

        match ids {
            None => {
                // Return all objects.
                let mut list = Vec::new();
                if let Some(m) = map {
                    for val in m.values() {
                        match O::deserialize(val) {
                            Ok(obj) => list.push(obj),
                            Err(e) => {
                                return Err(MemoryError(format!(
                                    "deserialize {}: {e}",
                                    O::TYPE_NAME
                                )))
                            }
                        }
                    }
                }
                Ok((list, vec![]))
            }
            Some(id_slice) => {
                let mut found = Vec::new();
                let mut not_found = Vec::new();
                for id in id_slice {
                    match map.and_then(|m| m.get(id)) {
                        Some(val) => match O::deserialize(val) {
                            Ok(obj) => found.push(obj),
                            Err(e) => {
                                return Err(MemoryError(format!(
                                    "deserialize {}: {e}",
                                    O::TYPE_NAME
                                )))
                            }
                        },
                        None => not_found.push(id.clone()),
                    }
                }
                Ok((found, not_found))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
        Ok(State::from(n.to_string()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let inner = self.inner.lock().unwrap();

        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let log = inner
            .change_log
            .get(&(O::TYPE_NAME, account_id.as_ref().to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        // Collect entries with new_state > since_n.
        let relevant: Vec<&ChangeEntry> = log.iter().filter(|e| e.new_state > since_n).collect();

        let current_state = inner.current_state(O::TYPE_NAME, account_id.as_ref());

        if let Some(max) = max_changes {
            if relevant.len() as u64 > max {
                return Err(BackendChangesError::TooManyChanges { limit: max });
            }
        }

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();

        for entry in &relevant {
            for id in &entry.created {
                if !destroyed.contains(id) && !created.contains(id) {
                    created.push(id.clone());
                }
            }
            for id in &entry.updated {
                if !destroyed.contains(id) && !created.contains(id) && !updated.contains(id) {
                    updated.push(id.clone());
                }
            }
            for id in &entry.destroyed {
                created.retain(|c| c != id);
                updated.retain(|u| u != id);
                if !destroyed.contains(id) {
                    destroyed.push(id.clone());
                }
            }
        }

        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            false,
            State::from(current_state.to_string()),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        let inner = self.inner.lock().unwrap();

        let mut ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<Id> = m.keys().cloned().collect();
                keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                keys
            })
            .unwrap_or_default();

        let total = ids.len() as u64;
        let query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string(),
        );

        let start = if position >= 0 {
            (position as usize).min(ids.len())
        } else {
            let neg = position.saturating_neg() as usize;
            ids.len().saturating_sub(neg)
        };

        // `n.min(usize::MAX as u64) as usize` saturates rather than
        // truncates on 32-bit targets. Mirrors the canonical
        // mail-server pattern at crate-jmap-mail-server/src/memory.rs
        // (per workspace AGENTS.md "Canonical Templates").
        ids = ids[start..]
            .iter()
            .take(limit.map_or(usize::MAX, |n| n.min(usize::MAX as u64) as usize))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(total),
            query_state,
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        // Reuse get_changes to determine which ids were added/removed/updated.
        let changes = self
            .get_changes::<O>(caller, account_id, since_query_state, max_changes)
            .await?;

        let inner = self.inner.lock().unwrap();
        let new_query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string(),
        );

        // Build the current ordered id list for position assignment.
        let current_ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<Id> = m.keys().cloned().collect();
                keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                keys
            })
            .unwrap_or_default();

        let removed: Vec<Id> = changes.destroyed;
        let added: Vec<AddedItem> = changes
            .created
            .iter()
            .filter_map(|id| {
                current_ids
                    .iter()
                    .position(|cur| cur == id)
                    .map(|idx| AddedItem::new(id.clone(), idx as u64))
            })
            .collect();

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            new_query_state,
            Some(current_ids.len() as u64),
            removed,
            added,
        ))
    }
}

impl ChatBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Serialize, set "id" to the server-assigned id, then deserialize back.
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize: {e}"))))?;
        val["id"] = serde_json::Value::String(server_id.as_ref().to_owned());
        let stored_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("deserialize after create: {e}")))
        })?;

        inner.known_accounts.insert(account_id.as_ref().to_owned());
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(server_id.clone(), val);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![server_id.clone()],
                updated: vec![],
                destroyed: vec![],
            });

        Ok((server_id, stored_obj))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        let existing = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned();

        let mut current = match existing {
            Some(v) => v,
            None => {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )))
            }
        };

        // Apply JSON Merge Patch (RFC 7396): merge patch fields into current value.
        // A `MergePatchError::DepthExceeded` return (bd:JMAP-wlip.1) surfaces
        // as `SetErrorType::InvalidPatch` — the depth cap is a DoS guard,
        // never fires on legitimate JMAP `/set update` shapes. `current` is a
        // clone of the stored value, so a partially-applied patch on error is
        // discarded with the local without touching storage.
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize patch: {e}"))))?;
        if let Err(MergePatchError::DepthExceeded) = json_merge_patch(&mut current, patch_val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("patch nesting exceeds server limit"),
            ));
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), current);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![id.clone()],
                destroyed: vec![],
            });

        Ok(None)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        let removed = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .remove(id);

        match removed {
            Some(_) => {
                let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
                inner
                    .change_log_mut(O::TYPE_NAME, account_id.as_ref())
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![id.clone()],
                    });
                Ok(())
            }
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        true
    }

    fn generate_invite_code(&self) -> String {
        // CSPRNG-backed invite code, per the contract documented on
        // `ChatBackend::generate_invite_code` (bd:JMAP-sc1b.78, bd:JMAP-sc1b.93).
        //
        // 16 random bytes = 128 bits of entropy, hex-encoded to 32 chars.
        // Far above the 48-bit nanosecond-truncated value the original
        // implementation produced, and unguessable to a network attacker.
        //
        // `getrandom::fill` reads from the OS CSPRNG (e.g. `getrandom(2)` on
        // Linux, `RtlGenRandom` on Windows, `arc4random_buf` on BSD/Apple).
        // On the supported tier-1 targets it cannot fail except via a kernel
        // bug or missing entropy at very early boot; we surface that as a
        // panic on the reference backend because the alternative (a
        // predictable fallback) is exactly the failure mode this fix
        // closes. Production backends ship their own impl and own their
        // own failure semantics.
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)
            .expect("OS CSPRNG must be available for invite-code generation");
        // Hex-encode without pulling in the `hex` crate.
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            // Standard hex encoding; `{:02x}` on a u8 cannot fail.
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// Reference implementation of [`ChatBackend::limits`].
    ///
    /// Returns the test-only override set via
    /// [`MemoryBackend::set_limits_for_test`] when present, else falls
    /// back to [`ChatLimits::default`]. Production backends ignore the
    /// override knob and read from their own per-account
    /// configuration.
    fn limits(&self, _caller: &(), _account_id: &Id) -> ChatLimits {
        let inner = self.inner.lock().unwrap();
        inner.limits_override.unwrap_or_default()
    }

    /// Reference implementation of [`ChatBackend::protect_last_admin`].
    ///
    /// Returns the test-only override set via
    /// [`MemoryBackend::set_protect_last_admin_for_test`] when set;
    /// otherwise returns `false`, **opposite** the trait default of
    /// `true`. See that method's documentation and `bd:JMAP-g7wu.2.4.3`
    /// for the rationale: production deployments inherit the trait
    /// default; the reference backend opts out so existing demo tests
    /// don't have to seed admin memberships.
    fn protect_last_admin(&self, _caller: &(), _account_id: &Id) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.protect_last_admin
    }

    /// Reference implementation of [`ChatBackend::apply_space_patch`].
    ///
    /// Dispatches each [`SpacePatchOp`] to a per-variant helper. All
    /// twelve variants are implemented: Category (bd:JMAP-g7wu.2.4.5),
    /// Channel (bd:JMAP-g7wu.2.4.4), and Role/Member
    /// (bd:JMAP-g7wu.2.4.3).
    ///
    /// The entire patch runs under one mutex acquisition, providing
    /// best-effort transactional semantics for the reference impl. A
    /// failure mid-way through the op vector does NOT roll back ops
    /// that already succeeded — they remain applied. **Exception:**
    /// when caller identity is resolvable (`principal_id` returns
    /// `Some(id)`), the helper runs a pre-validation pass over
    /// Role/Member ops and rejects the whole patch up-front with a
    /// single `forbidden` SetError if any op would fail permission or
    /// role-hierarchy enforcement. This implements criterion 6
    /// "whole-patch reject on permission failure" from
    /// `bd:JMAP-g7wu.2.4.3`. Per-op failures with other error types
    /// (NotFound, InvalidProperties) keep the legacy per-op outcome
    /// shape. Production backends should wrap the sequence in a real
    /// transaction.
    ///
    /// # Caller identity
    ///
    /// MemoryBackend uses `CallerCtx = ()` and inherits the default
    /// `JmapBackend::principal_id` impl returning `None`, putting it
    /// in single-user mode (criterion 7): identity-dependent gates
    /// (permission, hierarchy) are skipped, so every caller may
    /// apply every Role/Member op. The identity-bearing integration-
    /// test backend in `tests/common/mod.rs` calls
    /// [`MemoryBackend::apply_space_patch_with_caller_id`] directly to
    /// drive identity through the helper.
    ///
    /// # Change-log emission
    ///
    /// One consolidated change-log entry per affected type per call:
    ///
    /// - `Space`: one `updated` entry if any op mutated the host Space.
    /// - `Chat`: one entry combining all channels created, updated, and
    ///   destroyed by this patch. Same batching rationale as the Space
    ///   entry — keeps `Chat/changes` from amplifying state-token
    ///   rotations.
    /// - `Message`: one `destroyed` entry listing every Message
    ///   cascade-destroyed by `RemoveChannel`.
    ///
    /// Category-cascade Chat mutations (`apply_category_op` →
    /// `set_channel_category`) and the Channel variants both populate
    /// the per-type tracking sets, so a `Chat/changes` subscriber sees
    /// the cascade regardless of which Space/set patch key triggered
    /// it (bd:JMAP-g7wu.2.4.9 closed the original gap).
    async fn apply_space_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        // Resolve caller identity through the foundation seam. With
        // `CallerCtx = ()` the default returns None and the helper
        // skips identity-dependent enforcement (criterion 7 of
        // bd:JMAP-g7wu.2.4.3 — "no-identity mode; not suitable for
        // multi-user deployments").
        let caller_id_owned = <Self as JmapBackend>::principal_id(caller).cloned();
        let mut inner = self.inner.lock().unwrap();
        apply_space_patch_impl(
            &mut inner,
            caller_id_owned.as_ref(),
            account_id,
            space_id,
            ops,
        )
    }

    /// Reference implementation of
    /// [`ChatBackend::apply_space_metadata_patch`].
    ///
    /// Applies the JSON Merge Patch to the target Space's top-level
    /// metadata fields under a single mutex acquisition, with the
    /// `manage_space` permission gate applied atomically. Caller
    /// identity is resolved through [`JmapBackend::principal_id`];
    /// with `CallerCtx = ()` the default returns `None` and the
    /// gate is skipped (single-user mode), mirroring the
    /// [`Self::apply_space_patch`] contract.
    ///
    /// See `bd:JMAP-g7wu.2.4.13`.
    async fn apply_space_metadata_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        patch_map: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>> {
        let caller_id_owned = <Self as JmapBackend>::principal_id(caller).cloned();
        let mut inner = self.inner.lock().unwrap();
        apply_space_metadata_patch_impl(
            &mut inner,
            caller_id_owned.as_ref(),
            account_id,
            space_id,
            patch_map,
        )
    }

    /// Reference implementation of [`ChatBackend::retains_edit_history`].
    ///
    /// Returns the test-only override set via
    /// [`MemoryBackend::set_retains_edit_history_for_test`] when set;
    /// otherwise returns `false`, matching the trait default. The
    /// reference backend does not retain edit history out of the box.
    ///
    /// Spec: draft-atwood-jmap-chat-00 commit `0783fc4`.
    fn retains_edit_history(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.retains_edit_history
    }

    /// Reference implementation of [`ChatBackend::is_contact_blocked`].
    ///
    /// Reads `ChatContact.blocked` from the in-memory store keyed by
    /// the supplied `contact_id`. If no such [`ChatContact`] record
    /// exists, returns `Ok(false)` — an unknown contact is not
    /// considered blocked.
    ///
    /// Spec: draft-atwood-jmap-chat-00 commit `d68b4e3` (typing /
    /// presence blocked-sender suppression). The kit's handler
    /// consults this predicate but does not enforce fan-out
    /// suppression itself — the consumer's transport layer (SSE / WS
    /// push) is the canonical enforcement point.
    ///
    /// [`ChatContact`]: jmap_chat_types::ChatContact
    async fn is_contact_blocked(
        &self,
        _caller: &(),
        account_id: &Id,
        contact_id: &Id,
    ) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let blocked = inner
            .objects_ref("ChatContact", account_id.as_ref())
            .and_then(|map| map.get(contact_id))
            .and_then(|val| val.get("blocked"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(blocked)
    }

    /// Reference implementation of [`ChatBackend::may_set_custom_emoji`].
    ///
    /// Demonstration-only: returns `Ok(true)` unconditionally. The
    /// reference backend does not honor identity-scoped permissions
    /// because `JmapBackend::principal_id(&())` returns `None` for the
    /// workspace's `CallerCtx = ()` default. A meaningful permission
    /// model — e.g. "only members of the target Space may modify a
    /// Space-scoped emoji" — requires the production backend to
    /// override both `principal_id` and this method.
    ///
    /// Spec: draft-atwood-jmap-chat-00 commit `9344aec` (authorization
    /// for `CustomEmoji/set` is implementation-defined).
    async fn may_set_custom_emoji(
        &self,
        _caller: &(),
        _account_id: &Id,
        _target_space_id: Option<&Id>,
        _op: EmojiSetOp,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Reference implementation of [`ChatBackend::slow_mode_check`].
    ///
    /// Demonstration-only: this backend has no rate-tracker and always
    /// returns `Ok(())`. The kit defines the hook; production
    /// backends plug in their own per-(account, chat, caller) tracker
    /// and the SHOULD-exempt permission logic per
    /// draft-atwood-jmap-chat-00 §Chat `slowModeSeconds` + commit
    /// `de60acb`.
    ///
    /// The override exists rather than relying on the trait's default
    /// `Ok(())` so the posture is explicit in the reference impl — a
    /// future contributor reading `MemoryBackend` cannot mistake the
    /// absence of a rate-tracker for an oversight.
    async fn slow_mode_check(
        &self,
        _caller: &(),
        _account_id: &Id,
        _chat_id: &Id,
    ) -> Result<(), SlowModeError> {
        Ok(())
    }

    /// Reference implementation of [`ChatBackend::expire_message`].
    ///
    /// Hard-deletes the message from the in-memory store and appends a
    /// `Message` change-log entry recording the id as `destroyed`, so
    /// subsequent `Message/changes` calls see the expiry per
    /// draft-atwood-jmap-chat-00.
    ///
    /// If the message is already gone (e.g., a previous `expire_message`
    /// call or a cascade destroy already removed it), returns `Ok(())`
    /// without bumping state — the expiry is a no-op, consistent with
    /// the trait's idempotency contract.
    async fn expire_message(
        &self,
        _caller: &(),
        account_id: &Id,
        message_id: &Id,
    ) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().unwrap();

        let removed = inner
            .objects_mut("Message", account_id.as_ref())
            .remove(message_id);

        if removed.is_some() {
            let new_state = inner.bump_state("Message", account_id.as_ref());
            inner
                .change_log_mut("Message", account_id.as_ref())
                .push(ChangeEntry {
                    new_state,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![message_id.clone()],
                });
        }
        Ok(())
    }
}

/// Apply a [`SpacePatchOp`] sequence to the in-memory Space store.
///
/// This is the implementation core shared by
/// [`ChatBackend::apply_space_patch`] (whose trait method resolves
/// `caller_id` from `Self::principal_id` and locks the `Inner` mutex
/// before invoking it) and
/// [`MemoryBackend::apply_space_patch_with_caller_id`] (the test-only
/// public entry point that lets an integration-test backend with a
/// richer `CallerCtx` drive a non-`None` `caller_id`).
///
/// `caller_id == None` is single-user mode: identity-dependent
/// enforcement (permission gating, role-position hierarchy) is
/// skipped. Identity-independent enforcement (last-admin protection
/// when `inner.protect_last_admin == true`) still fires.
///
/// See `bd:JMAP-g7wu.2.4.3` for the acceptance criteria this
/// implements (criteria 1, 2, 4, 5, 6, 7).
#[allow(clippy::result_large_err)]
fn apply_space_patch_impl(
    inner: &mut Inner,
    caller_id: Option<&Id>,
    account_id: &Id,
    space_id: &Id,
    ops: Vec<SpacePatchOp>,
) -> Result<Vec<OpResult>, BackendSetError<MemoryError>> {
    // Confirm the target Space exists before doing any work.
    if !inner
        .objects_ref("Space", account_id.as_ref())
        .is_some_and(|m| m.contains_key(space_id))
    {
        return Err(BackendSetError::SetError(SetError::new(
            SetErrorType::NotFound,
        )));
    }

    // ---------------------------------------------------------------
    // Pre-validation: identity-dependent and identity-independent
    // policy checks that, on failure, reject the WHOLE patch with a
    // single `forbidden` SetError. Per criterion 6 of
    // bd:JMAP-g7wu.2.4.3: "if any op in the patch is forbidden, the
    // WHOLE update for that Space id is rejected."
    //
    // Other failure shapes (NotFound on a non-existent role id,
    // InvalidProperties on a malformed payload) keep the legacy
    // per-op outcome — those are structural, not policy.
    // ---------------------------------------------------------------
    let protect_last_admin = inner.protect_last_admin;
    // Snapshot the Space for pre-validation reads.
    let space_snapshot = inner
        .objects_ref("Space", account_id.as_ref())
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;

    if let Err(e) = validate_space_patch_ops(&space_snapshot, caller_id, &ops, protect_last_admin) {
        return Err(BackendSetError::SetError(e));
    }

    let mut results = Vec::with_capacity(ops.len());
    let mut space_mutated = false;
    let mut chats_created: Vec<Id> = Vec::new();
    let mut chats_updated: HashSet<Id> = HashSet::new();
    let mut chats_destroyed: Vec<Id> = Vec::new();
    let mut messages_destroyed: Vec<Id> = Vec::new();

    for (op_index, op) in ops.into_iter().enumerate() {
        let outcome = match &op {
            SpacePatchOp::AddRole(_) => {
                apply_add_role(inner, account_id.as_ref(), space_id, op, &mut space_mutated)
            }
            SpacePatchOp::RemoveRole(_) => {
                apply_remove_role(inner, account_id.as_ref(), space_id, op, &mut space_mutated)
            }
            SpacePatchOp::UpdateRole { .. } => {
                apply_update_role(inner, account_id.as_ref(), space_id, op, &mut space_mutated)
            }
            SpacePatchOp::AddMember(_) => {
                apply_add_member(inner, account_id.as_ref(), space_id, op, &mut space_mutated)
            }
            SpacePatchOp::RemoveMember(_) => {
                apply_remove_member(inner, account_id.as_ref(), space_id, op, &mut space_mutated)
            }
            SpacePatchOp::UpdateMember { .. } => {
                apply_update_member(inner, account_id.as_ref(), space_id, op, &mut space_mutated)
            }
            SpacePatchOp::AddCategory(_)
            | SpacePatchOp::RemoveCategory(_)
            | SpacePatchOp::UpdateCategory { .. } => apply_category_op(
                inner,
                account_id.as_ref(),
                space_id,
                op,
                &mut chats_updated,
                &mut space_mutated,
            ),
            SpacePatchOp::AddChannel(_) => apply_add_channel(
                inner,
                account_id.as_ref(),
                space_id,
                op,
                &mut chats_created,
                &mut space_mutated,
            ),
            SpacePatchOp::RemoveChannel(_) => apply_remove_channel(
                inner,
                account_id.as_ref(),
                space_id,
                op,
                &mut chats_destroyed,
                &mut messages_destroyed,
                &mut space_mutated,
            ),
            SpacePatchOp::UpdateChannel { .. } => apply_update_channel(
                inner,
                account_id.as_ref(),
                space_id,
                op,
                &mut chats_updated,
                &mut space_mutated,
            ),
            // `SpacePatchOp` is `#[non_exhaustive]` upstream. A
            // future-added variant cannot be matched at compile time;
            // fail closed.
            _ => {
                Err(SetError::new(SetErrorType::Forbidden).with_description(stub_description(&op)))
            }
        };
        results.push(OpResult { op_index, outcome });
    }

    // Bump state + log a change entry on the Space if any op mutated it.
    // We deliberately log one change per call rather than one per op:
    // the wire response surfaces the whole patch as a single update,
    // and one change-log entry per call keeps `Space/changes` from
    // amplifying state-token rotations unnecessarily.
    if space_mutated {
        let new_state = inner.bump_state("Space", account_id.as_ref());
        inner
            .change_log_mut("Space", account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![space_id.clone()],
                destroyed: vec![],
            });
    }

    // Same batching rationale: one Chat change-log entry per call,
    // combining every channel created, updated, and destroyed by
    // this patch.
    if !chats_created.is_empty() || !chats_updated.is_empty() || !chats_destroyed.is_empty() {
        let new_state = inner.bump_state("Chat", account_id.as_ref());
        // De-dup `updated` against `created` and `destroyed` so a
        // channel touched by multiple ops in the same patch surfaces
        // in only the most-impactful list (destroyed > created >
        // updated). HashSet iteration is non-deterministic; sort the
        // updated ids so the change-log entry is reproducible.
        let mut updated: Vec<Id> = chats_updated
            .into_iter()
            .filter(|id| !chats_created.contains(id) && !chats_destroyed.contains(id))
            .collect();
        updated.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        inner
            .change_log_mut("Chat", account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: chats_created,
                updated,
                destroyed: chats_destroyed,
            });
    }

    if !messages_destroyed.is_empty() {
        let new_state = inner.bump_state("Message", account_id.as_ref());
        inner
            .change_log_mut("Message", account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![],
                destroyed: messages_destroyed,
            });
    }

    Ok(results)
}

/// Backend-canonical impl of
/// [`ChatBackend::apply_space_metadata_patch`] for the reference
/// in-memory store (bd:JMAP-g7wu.2.4.13).
///
/// Snapshots the target Space, enforces the `manage_space` gate
/// when a caller identity is resolvable, and applies the JSON
/// Merge Patch (RFC 7396) atomically under the inner mutex.
///
/// # Permission gate
///
/// Per draft-atwood-jmap-chat-00 §Space/set, every top-level
/// metadata field (`name`, `description`, `iconBlobId`, `isPublic`,
/// `isPubliclyPreviewable`) requires `manage_space`. The gate logic
/// mirrors [`validate_space_patch_ops`]: with `caller_id == None`
/// (single-user mode) the gate is skipped; with `caller_id ==
/// Some(id)` the caller's effective permission set must contain
/// `manage_space`, else the whole patch is rejected with
/// [`SetErrorType::Forbidden`].
///
/// Empty patches (`patch_map.is_empty()`) are still rejected at the
/// gate when the caller lacks `manage_space`. Letting them through
/// would be a small information leak (the caller learns the Space
/// exists) without practical value — RFC 8620 §5.3 tolerates no-op
/// updates but does not REQUIRE them to bypass authorization.
///
/// # Atomicity
///
/// The lock is held across the read-check-write sequence so the
/// gate decision and the write cannot race against a concurrent
/// role-change that would invalidate the decision. This matches the
/// canonical backend-side-enforcement pattern from
/// `apply_space_patch_impl`.
#[allow(clippy::result_large_err)]
fn apply_space_metadata_patch_impl(
    inner: &mut Inner,
    caller_id: Option<&Id>,
    account_id: &Id,
    space_id: &Id,
    patch_map: serde_json::Map<String, serde_json::Value>,
) -> Result<Option<jmap_chat_types::Space>, BackendSetError<MemoryError>> {
    use crate::permissions::MANAGE_SPACE;

    // Confirm the target Space exists. Surface NotFound at the
    // SetError level (per-target, NOT per-account) so the handler's
    // `notUpdated` bucket can render it.
    let space_val = inner
        .objects_ref("Space", account_id.as_ref())
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;

    // Permission gate: identity-dependent. When the caller is
    // resolvable, the caller's effective permissions in the Space
    // must include `manage_space`. When the caller is anonymous
    // (single-user mode), skip the gate.
    if let Some(caller_id) = caller_id {
        let caller_perms = caller_effective_permissions(&space_val, caller_id).unwrap_or_default();
        if !caller_perms.contains(MANAGE_SPACE) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::Forbidden).with_description(format!(
                    "caller lacks required permission `{MANAGE_SPACE}` to mutate Space top-level metadata"
                )),
            ));
        }
    }

    // Empty patch is a no-op. Treat as a successful update with no
    // server-set field echo; the handler still emits an entry into
    // the `updated` map (per RFC 8620 §5.3 tolerance for no-op
    // updates). Skip the state bump and change-log entry so a no-op
    // does not rotate the Space type's state token.
    if patch_map.is_empty() {
        return Ok(None);
    }

    // Apply the merge patch in place. The wire-shape JSON object is
    // canonical for in-memory storage (extras-preservation policy);
    // the patch keys have already been filtered to METADATA_FIELDS
    // by the handler.
    //
    // A `MergePatchError::DepthExceeded` return (bd:JMAP-wlip.1) surfaces
    // as `SetErrorType::InvalidPatch` — the depth cap is a DoS guard,
    // never fires on legitimate JMAP `/set update` shapes. `current` is a
    // local owned clone of `space_val`, so a partially-applied patch on
    // error is discarded without touching the storage map.
    let mut current = space_val;
    if let Err(MergePatchError::DepthExceeded) =
        json_merge_patch(&mut current, serde_json::Value::Object(patch_map))
    {
        return Err(BackendSetError::SetError(
            SetError::new(SetErrorType::InvalidPatch)
                .with_description("patch nesting exceeds server limit"),
        ));
    }

    let new_state = inner.bump_state("Space", account_id.as_ref());
    inner
        .objects_mut("Space", account_id.as_ref())
        .insert(space_id.clone(), current);
    inner
        .change_log_mut("Space", account_id.as_ref())
        .push(ChangeEntry {
            new_state,
            created: vec![],
            updated: vec![space_id.clone()],
            destroyed: vec![],
        });

    // Return `None` to match the `update_object` contract: no
    // server-set field echo beyond what the client requested.
    Ok(None)
}

/// Apply one Category-family [`SpacePatchOp`] to the in-memory Space.
///
/// Per draft-atwood-jmap-chat-00 §Space/set:
/// - `addCategories`: assigns a fresh CategoryId and pushes the category
///   into `space.categories`. If the entry's `channelIds` list references
///   channels of the Space, those channels' `categoryId` is updated.
/// - `removeCategories`: removes the named category. Channels currently
///   pointing at that category have their `categoryId` cleared and are
///   appended to `space.uncategorizedChannelIds` (cascade per line 1126).
/// - `updateCategories`: applies the per-field patch. A `channelIds`
///   wholesale-replacement updates each channel's `categoryId` to match:
///   channels that left the list get `categoryId = None` and join
///   `uncategorizedChannelIds`; channels that joined the list get
///   `categoryId = Some(this category)`.
///
/// All cross-reference updates use the same in-memory Chat store so the
/// Space-side `categories[].channel_ids` and the Chat-side `categoryId`
/// stay consistent. Every channel whose `categoryId` changes here is
/// also pushed into the caller's `chats_updated` set so the post-loop
/// bookkeeping in `apply_space_patch` emits a `Chat/changes` entry for
/// the cascade (bd:JMAP-g7wu.2.4.9).
///
/// The `SetError` `Err` variant is large; we tolerate this here because
/// `SetError` is the workspace-canonical per-op result shape (matches the
/// `OpResult.outcome` field on the trait method) and boxing would
/// diverge from every sibling backend.
#[allow(clippy::result_large_err)]
fn apply_category_op(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    chats_updated: &mut HashSet<Id>,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    use jmap_chat_types::space::Category;

    // The Space exists (the caller verified). Pull it out as a typed
    // value, apply the mutation, write it back.
    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    let categories: &mut Vec<serde_json::Value> = space_val
        .get_mut("categories")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.categories not an array")
        })?;

    let assigned_id = match op {
        SpacePatchOp::AddCategory(mut cat) => {
            // Mint a fresh CategoryId. Categories live inside the Space
            // (no separate `objects` slot) so we synthesize a unique id
            // via the shared `demo_next_id` helper with a synthetic type name.
            let new_id = MemoryBackend::demo_next_id(inner, "Category", account_id);
            cat.id = new_id.clone();

            // Validate every channel id in the entry's channel_ids refers
            // to an existing Chat with kind=channel and space_id matching
            // this Space. The spec says "channelIds (String[])" without
            // strict validation language, but accepting nonexistent ids
            // would create dangling references on the Space side.
            for ch_id in &cat.channel_ids {
                if !channel_belongs_to_space(inner, account_id, ch_id, space_id) {
                    return Err(SetError::new(SetErrorType::InvalidProperties)
                        .with_properties(vec!["channelIds".to_owned()])
                        .with_description(format!(
                            "channelId {} is not a channel of this Space",
                            ch_id.as_ref()
                        )));
                }
            }

            // Re-fetch space_val + categories after the validation borrows
            // (channel_belongs_to_space took an immutable inner borrow).
            let mut space_val = inner
                .objects_ref("Space", account_id)
                .and_then(|m| m.get(space_id))
                .cloned()
                .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

            // Set each named channel's category_id and pull it out of
            // uncategorized_channel_ids if present. Every channel whose
            // categoryId changes is recorded so Chat/changes sees the
            // cascade (bd:JMAP-g7wu.2.4.9).
            let owned_channel_ids: Vec<Id> = cat.channel_ids.clone();
            for ch_id in &owned_channel_ids {
                set_channel_category(inner, account_id, ch_id, Some(&new_id));
                chats_updated.insert(ch_id.clone());
            }
            if let Some(unc) = space_val
                .get_mut("uncategorizedChannelIds")
                .and_then(|v| v.as_array_mut())
            {
                unc.retain(|v| {
                    v.as_str()
                        .is_none_or(|s| !owned_channel_ids.iter().any(|id| id.as_ref() == s))
                });
            }
            let cats = space_val
                .get_mut("categories")
                .and_then(|v| v.as_array_mut())
                .ok_or_else(|| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description("internal: Space.categories not an array")
                })?;
            cats.push(serde_json::to_value(&cat).map_err(|e| {
                SetError::new(SetErrorType::Forbidden)
                    .with_description(format!("internal: serialize Category: {e}"))
            })?);

            inner
                .objects_mut("Space", account_id)
                .insert(space_id.clone(), space_val);
            *space_mutated = true;
            Some(new_id)
        }

        SpacePatchOp::RemoveCategory(target_id) => {
            // Find the category. Cascade: every channel currently in this
            // category gets its category_id cleared and is appended to
            // uncategorized. Per draft §Space/set line 1126.
            let pos = categories
                .iter()
                .position(|v| v.get("id").and_then(|s| s.as_str()) == Some(target_id.as_ref()));
            let Some(pos) = pos else {
                return Err(SetError::new(SetErrorType::NotFound)
                    .with_description(format!("category {} not found", target_id.as_ref())));
            };
            // The stored channel_ids on the category mirror the Chats'
            // category_id; scan Chats to be safe (Space-side could drift).
            let scanned: Vec<Id> = scan_channels_in_category(inner, account_id, &target_id);

            // Drop the category and append channels to uncategorized.
            let mut space_val = inner
                .objects_ref("Space", account_id)
                .and_then(|m| m.get(space_id))
                .cloned()
                .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;
            let cats = space_val
                .get_mut("categories")
                .and_then(|v| v.as_array_mut())
                .ok_or_else(|| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description("internal: Space.categories not an array")
                })?;
            cats.remove(pos);

            let unc = space_val
                .get_mut("uncategorizedChannelIds")
                .and_then(|v| v.as_array_mut())
                .ok_or_else(|| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description("internal: Space.uncategorizedChannelIds not an array")
                })?;
            for ch_id in &scanned {
                unc.push(serde_json::Value::String(ch_id.as_ref().to_owned()));
            }
            inner
                .objects_mut("Space", account_id)
                .insert(space_id.clone(), space_val);

            // Clear each scanned channel's category_id pointer.
            // Every cleared channel is recorded so Chat/changes sees
            // the cascade (bd:JMAP-g7wu.2.4.9).
            for ch_id in &scanned {
                set_channel_category(inner, account_id, ch_id, None);
                chats_updated.insert(ch_id.clone());
            }
            *space_mutated = true;
            None
        }

        SpacePatchOp::UpdateCategory { id, patch } => {
            // Find the category.
            let pos = categories
                .iter()
                .position(|v| v.get("id").and_then(|s| s.as_str()) == Some(id.as_ref()));
            let Some(pos) = pos else {
                return Err(SetError::new(SetErrorType::NotFound)
                    .with_description(format!("category {} not found", id.as_ref())));
            };

            // Deserialize, apply the patch, re-serialize.
            let mut cat: Category =
                serde_json::from_value(categories[pos].clone()).map_err(|e| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description(format!("internal: deserialize Category: {e}"))
                })?;

            if let Some(name) = patch.name {
                cat.name = name;
            }
            if let Some(position) = patch.position {
                cat.position = position;
            }

            // Wholesale channel_ids replacement requires cross-reference
            // bookkeeping on the channels.
            let channel_ids_changed = patch.channel_ids.is_some();
            let new_channel_ids: Option<Vec<Id>> = patch.channel_ids;
            if let Some(new_ids) = &new_channel_ids {
                // Validate every new id is a channel of this Space.
                for ch_id in new_ids {
                    if !channel_belongs_to_space(inner, account_id, ch_id, space_id) {
                        return Err(SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(vec!["channelIds".to_owned()])
                            .with_description(format!(
                                "channelId {} is not a channel of this Space",
                                ch_id.as_ref()
                            )));
                    }
                }
            }

            if channel_ids_changed {
                let new_ids = new_channel_ids.expect("checked above");
                let old_ids = cat.channel_ids.clone();

                // Channels in old but not in new: clear category_id and
                // append to uncategorized.
                let removed: Vec<&Id> = old_ids.iter().filter(|o| !new_ids.contains(o)).collect();
                // Channels in new but not in old: set category_id to this
                // category and remove from uncategorized.
                let added: Vec<&Id> = new_ids.iter().filter(|n| !old_ids.contains(n)).collect();

                cat.channel_ids = new_ids.clone();

                // Mutate Space first (uncategorized list).
                let mut space_val = inner
                    .objects_ref("Space", account_id)
                    .and_then(|m| m.get(space_id))
                    .cloned()
                    .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;
                if let Some(unc) = space_val
                    .get_mut("uncategorizedChannelIds")
                    .and_then(|v| v.as_array_mut())
                {
                    for ch_id in &removed {
                        unc.push(serde_json::Value::String(ch_id.as_ref().to_owned()));
                    }
                    unc.retain(|v| {
                        v.as_str()
                            .is_none_or(|s| !added.iter().any(|id| id.as_ref() == s))
                    });
                }
                // Write back the updated category.
                let cats = space_val
                    .get_mut("categories")
                    .and_then(|v| v.as_array_mut())
                    .ok_or_else(|| {
                        SetError::new(SetErrorType::Forbidden)
                            .with_description("internal: Space.categories not an array")
                    })?;
                cats[pos] = serde_json::to_value(&cat).map_err(|e| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description(format!("internal: serialize Category: {e}"))
                })?;
                inner
                    .objects_mut("Space", account_id)
                    .insert(space_id.clone(), space_val);

                // Update each channel's category_id. Every channel
                // whose categoryId changed is recorded so Chat/changes
                // sees the cascade (bd:JMAP-g7wu.2.4.9).
                for ch_id in &removed {
                    set_channel_category(inner, account_id, ch_id, None);
                    chats_updated.insert((*ch_id).clone());
                }
                for ch_id in &added {
                    set_channel_category(inner, account_id, ch_id, Some(&id));
                    chats_updated.insert((*ch_id).clone());
                }
            } else {
                // Only metadata changed; write back the category.
                let mut space_val = inner
                    .objects_ref("Space", account_id)
                    .and_then(|m| m.get(space_id))
                    .cloned()
                    .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;
                let cats = space_val
                    .get_mut("categories")
                    .and_then(|v| v.as_array_mut())
                    .ok_or_else(|| {
                        SetError::new(SetErrorType::Forbidden)
                            .with_description("internal: Space.categories not an array")
                    })?;
                cats[pos] = serde_json::to_value(&cat).map_err(|e| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description(format!("internal: serialize Category: {e}"))
                })?;
                inner
                    .objects_mut("Space", account_id)
                    .insert(space_id.clone(), space_val);
            }
            *space_mutated = true;
            None
        }

        // Caller filters non-category ops out before reaching this helper.
        _ => unreachable!("apply_category_op called with non-category variant"),
    };

    Ok(assigned_id)
}

/// Apply one [`SpacePatchOp::AddChannel`] to the in-memory Space.
///
/// Per draft-atwood-jmap-chat-00 §Space/set (lines 1114-1117):
/// - The server creates a Chat record of `kind: "channel"` with
///   `spaceId` set to this Space's id and a fresh server-assigned id.
/// - If `categoryId` is provided, validate that the named category
///   exists on this Space; append the new channel's id to that
///   category's `channelIds` array.
/// - If `categoryId` is absent, append the new channel's id to the
///   Space's `uncategorizedChannelIds` array.
/// - `name` is required; `position` and `topic` are optional.
///   `slowModeSeconds` and `permissionOverrides` are server-managed
///   and start absent (the spec does not allow the client to set them
///   at create time — those flow through `updateChannels` later).
///
/// The `SetError` `Err` variant is large; we tolerate this here because
/// `SetError` is the workspace-canonical per-op result shape (matches
/// the `OpResult.outcome` field on the trait method).
#[allow(clippy::result_large_err)]
fn apply_add_channel(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    chats_created: &mut Vec<Id>,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    use jmap_chat_types::space_set::ChannelCreate;

    let create: ChannelCreate = match op {
        SpacePatchOp::AddChannel(c) => c,
        _ => unreachable!("apply_add_channel called with non-AddChannel variant"),
    };

    // If categoryId is provided, verify the category exists on this Space.
    // This must happen before any mutation so a bad categoryId is a clean
    // rejection rather than a half-applied side effect.
    if let Some(cat_id) = &create.category_id {
        let space_val = inner
            .objects_ref("Space", account_id)
            .and_then(|m| m.get(space_id))
            .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;
        let exists = space_val
            .get("categories")
            .and_then(|v| v.as_array())
            .is_some_and(|cats| {
                cats.iter()
                    .any(|c| c.get("id").and_then(|s| s.as_str()) == Some(cat_id.as_ref()))
            });
        if !exists {
            return Err(SetError::new(SetErrorType::InvalidProperties)
                .with_properties(vec!["categoryId".to_owned()])
                .with_description(format!(
                    "categoryId {} does not refer to a category of this Space",
                    cat_id.as_ref()
                )));
        }
    }

    // Mint a fresh channel Chat id.
    let new_id = MemoryBackend::demo_next_id(inner, "Chat", account_id);

    // Build the channel JSON. Required fields per `Chat`:
    //   id, kind, createdAt, unreadCount, pinnedMessageIds, muted,
    //   receiveTypingIndicators
    // Channel-specific: spaceId, name, categoryId?, position?, topic?
    // Defaults for muted (false) and receiveTypingIndicators (true)
    // match the values used by Chat/set create at chat.rs:266-273.
    let now = now_utc_string();
    let mut chat_obj = serde_json::Map::new();
    chat_obj.insert(
        "id".to_owned(),
        serde_json::Value::String(new_id.as_ref().to_owned()),
    );
    chat_obj.insert(
        "kind".to_owned(),
        serde_json::Value::String("channel".to_owned()),
    );
    chat_obj.insert(
        "createdAt".to_owned(),
        serde_json::Value::String(now.into_inner()),
    );
    chat_obj.insert("unreadCount".to_owned(), serde_json::Value::from(0u64));
    chat_obj.insert(
        "pinnedMessageIds".to_owned(),
        serde_json::Value::Array(vec![]),
    );
    chat_obj.insert("muted".to_owned(), serde_json::Value::Bool(false));
    chat_obj.insert(
        "receiveTypingIndicators".to_owned(),
        serde_json::Value::Bool(true),
    );
    chat_obj.insert(
        "spaceId".to_owned(),
        serde_json::Value::String(space_id.as_ref().to_owned()),
    );
    chat_obj.insert("name".to_owned(), serde_json::Value::String(create.name));
    if let Some(cat_id) = &create.category_id {
        chat_obj.insert(
            "categoryId".to_owned(),
            serde_json::Value::String(cat_id.as_ref().to_owned()),
        );
    }
    if let Some(pos) = create.position {
        chat_obj.insert("position".to_owned(), serde_json::Value::from(pos));
    }
    if let Some(topic) = create.topic {
        chat_obj.insert("topic".to_owned(), serde_json::Value::String(topic));
    }

    inner
        .objects_mut("Chat", account_id)
        .insert(new_id.clone(), serde_json::Value::Object(chat_obj));

    // Update Space-side cross-references. Re-fetch the Space because
    // the validation borrow above was released, and the `objects_mut`
    // on "Chat" above invalidated any earlier borrow regardless.
    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    if let Some(cat_id) = &create.category_id {
        // Append the new channel id to that category's channelIds.
        let cats = space_val
            .get_mut("categories")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| {
                SetError::new(SetErrorType::Forbidden)
                    .with_description("internal: Space.categories not an array")
            })?;
        for cat in cats.iter_mut() {
            if cat.get("id").and_then(|s| s.as_str()) == Some(cat_id.as_ref()) {
                let ch_ids = cat
                    .get_mut("channelIds")
                    .and_then(|v| v.as_array_mut())
                    .ok_or_else(|| {
                        SetError::new(SetErrorType::Forbidden)
                            .with_description("internal: Category.channelIds not an array")
                    })?;
                ch_ids.push(serde_json::Value::String(new_id.as_ref().to_owned()));
                break;
            }
        }
    } else {
        // Append to uncategorizedChannelIds.
        let unc = space_val
            .get_mut("uncategorizedChannelIds")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| {
                SetError::new(SetErrorType::Forbidden)
                    .with_description("internal: Space.uncategorizedChannelIds not an array")
            })?;
        unc.push(serde_json::Value::String(new_id.as_ref().to_owned()));
    }
    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);

    chats_created.push(new_id.clone());
    *space_mutated = true;
    Ok(Some(new_id))
}

/// Apply one [`SpacePatchOp::RemoveChannel`] to the in-memory Space.
///
/// Per draft-atwood-jmap-chat-00 §Space/set line 1117: removeChannels
/// "Cascades to all Messages in those channels." This implementation:
///
/// 1. Verifies the named id refers to a channel-kind Chat with
///    `spaceId` matching this Space. Any other id is `notFound`.
/// 2. Destroys the Chat record itself.
/// 3. Scans the Message store for every Message whose `chatId` matches
///    the removed channel; destroys each one.
/// 4. Removes the channel id from the Space's `uncategorizedChannelIds`
///    and from any category's `channelIds` it appears in.
///
/// All four steps run within the single mutex hold inherited from
/// `apply_space_patch`, so the Chat removal, the Message cascade, and
/// the Space-side bookkeeping are atomic with respect to other
/// `MemoryBackend` callers.
#[allow(clippy::result_large_err)]
fn apply_remove_channel(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    chats_destroyed: &mut Vec<Id>,
    messages_destroyed: &mut Vec<Id>,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    let target_id: Id = match op {
        SpacePatchOp::RemoveChannel(id) => id,
        _ => unreachable!("apply_remove_channel called with non-RemoveChannel variant"),
    };

    // Confirm the id names a channel-kind Chat of this Space.
    if !channel_belongs_to_space(inner, account_id, &target_id, space_id) {
        return Err(
            SetError::new(SetErrorType::NotFound).with_description(format!(
                "channel {} is not a channel of this Space",
                target_id.as_ref()
            )),
        );
    }

    // Cascade Messages first so a half-completed run leaves Messages
    // referring to a still-extant Chat rather than orphaned Messages
    // pointing at a vanished Chat. (We are inside a single mutex hold
    // so partial visibility is internal, but the ordering preserves
    // referential integrity for tests that grep mid-run state.)
    let cascade_ids = scan_messages_in_chat(inner, account_id, &target_id);
    if !cascade_ids.is_empty() {
        let msg_map = inner.objects_mut("Message", account_id);
        for msg_id in &cascade_ids {
            msg_map.remove(msg_id);
        }
        messages_destroyed.extend(cascade_ids);
    }

    // Destroy the Chat record.
    inner.objects_mut("Chat", account_id).remove(&target_id);

    // Remove the channel id from Space-side cross-references.
    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    if let Some(unc) = space_val
        .get_mut("uncategorizedChannelIds")
        .and_then(|v| v.as_array_mut())
    {
        unc.retain(|v| v.as_str() != Some(target_id.as_ref()));
    }
    if let Some(cats) = space_val
        .get_mut("categories")
        .and_then(|v| v.as_array_mut())
    {
        for cat in cats.iter_mut() {
            if let Some(ch_ids) = cat.get_mut("channelIds").and_then(|v| v.as_array_mut()) {
                ch_ids.retain(|v| v.as_str() != Some(target_id.as_ref()));
            }
        }
    }
    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);

    chats_destroyed.push(target_id);
    *space_mutated = true;
    Ok(None)
}

/// Apply one [`SpacePatchOp::UpdateChannel`] to the in-memory Space.
///
/// Per draft-atwood-jmap-chat-00 §Space/set line 1120: `updateChannels`
/// patches one of `name`, `topic`, `categoryId`, `position`,
/// `slowModeSeconds`, `permissionOverrides`. The nullable fields use
/// [`jmap_chat_types::clearable::Clearable`] semantics: `null` clears
/// and absence leaves unchanged.
///
/// `categoryId` changes additionally maintain Space-side cross-references:
/// the channel id is removed from its previous category's `channelIds`
/// (or from `uncategorizedChannelIds`) and added to the new category's
/// `channelIds` (or to `uncategorizedChannelIds` when cleared). The new
/// category, if any, must already exist on this Space — otherwise the
/// op fails with `invalidProperties` on `categoryId`.
#[allow(clippy::result_large_err)]
fn apply_update_channel(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    chats_updated: &mut HashSet<Id>,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    use jmap_chat_types::clearable::Clearable;
    use jmap_chat_types::space_set::ChannelPatch;

    let (id, patch): (Id, ChannelPatch) = match op {
        SpacePatchOp::UpdateChannel { id, patch } => (id, patch),
        _ => unreachable!("apply_update_channel called with non-UpdateChannel variant"),
    };

    // Confirm the id names a channel-kind Chat of this Space.
    if !channel_belongs_to_space(inner, account_id, &id, space_id) {
        return Err(
            SetError::new(SetErrorType::NotFound).with_description(format!(
                "channel {} is not a channel of this Space",
                id.as_ref()
            )),
        );
    }

    // If categoryId is changing to a non-null value, validate that the
    // target category exists on this Space before mutating anything.
    // A null categoryId (Clearable::Clear) does not need this check.
    if let Some(Clearable::Set(new_cat)) = &patch.category_id {
        let space_val = inner
            .objects_ref("Space", account_id)
            .and_then(|m| m.get(space_id))
            .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;
        let exists = space_val
            .get("categories")
            .and_then(|v| v.as_array())
            .is_some_and(|cats| {
                cats.iter()
                    .any(|c| c.get("id").and_then(|s| s.as_str()) == Some(new_cat.as_ref()))
            });
        if !exists {
            return Err(SetError::new(SetErrorType::InvalidProperties)
                .with_properties(vec!["categoryId".to_owned()])
                .with_description(format!(
                    "categoryId {} does not refer to a category of this Space",
                    new_cat.as_ref()
                )));
        }
    }

    // Apply the patch to the Chat record. We collect categoryId before
    // mutation so the Space-side cross-reference update below knows
    // both the old and new pointers.
    let old_category_id: Option<Id> = inner
        .objects_ref("Chat", account_id)
        .and_then(|m| m.get(&id))
        .and_then(|v| v.get("categoryId"))
        .and_then(|c| c.as_str())
        .map(Id::from);

    {
        let chat_map = inner.objects_mut("Chat", account_id);
        let Some(chat_val) = chat_map.get_mut(&id) else {
            // We just confirmed existence via channel_belongs_to_space;
            // this is unreachable in single-thread execution.
            return Err(SetError::new(SetErrorType::NotFound));
        };
        let serde_json::Value::Object(obj) = chat_val else {
            return Err(SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Chat record not a JSON object"));
        };

        if let Some(name) = patch.name {
            obj.insert("name".to_owned(), serde_json::Value::String(name));
        }
        if let Some(pos) = patch.position {
            obj.insert("position".to_owned(), serde_json::Value::from(pos));
        }
        match patch.topic {
            Some(Clearable::Set(t)) => {
                obj.insert("topic".to_owned(), serde_json::Value::String(t));
            }
            Some(Clearable::Clear) => {
                obj.remove("topic");
            }
            None => {}
        }
        match &patch.category_id {
            Some(Clearable::Set(c)) => {
                obj.insert(
                    "categoryId".to_owned(),
                    serde_json::Value::String(c.as_ref().to_owned()),
                );
            }
            Some(Clearable::Clear) => {
                obj.remove("categoryId");
            }
            None => {}
        }
        match patch.slow_mode_seconds {
            Some(Clearable::Set(s)) => {
                obj.insert("slowModeSeconds".to_owned(), serde_json::Value::from(s));
            }
            Some(Clearable::Clear) => {
                obj.remove("slowModeSeconds");
            }
            None => {}
        }
        match patch.permission_overrides {
            Some(Clearable::Set(po)) => {
                let val = serde_json::to_value(&po).map_err(|e| {
                    SetError::new(SetErrorType::Forbidden)
                        .with_description(format!("internal: serialize permissionOverrides: {e}"))
                })?;
                obj.insert("permissionOverrides".to_owned(), val);
            }
            Some(Clearable::Clear) => {
                obj.remove("permissionOverrides");
            }
            None => {}
        }
    }

    // Maintain Space-side cross-references for categoryId changes.
    let category_changed = patch.category_id.is_some();
    if category_changed {
        let new_category_id: Option<Id> = match &patch.category_id {
            Some(Clearable::Set(c)) => Some(c.clone()),
            Some(Clearable::Clear) => None,
            None => unreachable!(),
        };

        // Only do bookkeeping if the assignment actually changed —
        // a no-op same-category update should not shuffle arrays.
        if old_category_id != new_category_id {
            let mut space_val = inner
                .objects_ref("Space", account_id)
                .and_then(|m| m.get(space_id))
                .cloned()
                .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

            // Remove from previous home.
            match &old_category_id {
                Some(prev_cat) => {
                    if let Some(cats) = space_val
                        .get_mut("categories")
                        .and_then(|v| v.as_array_mut())
                    {
                        for cat in cats.iter_mut() {
                            if cat.get("id").and_then(|s| s.as_str()) == Some(prev_cat.as_ref()) {
                                if let Some(ch_ids) =
                                    cat.get_mut("channelIds").and_then(|v| v.as_array_mut())
                                {
                                    ch_ids.retain(|v| v.as_str() != Some(id.as_ref()));
                                }
                                break;
                            }
                        }
                    }
                }
                None => {
                    if let Some(unc) = space_val
                        .get_mut("uncategorizedChannelIds")
                        .and_then(|v| v.as_array_mut())
                    {
                        unc.retain(|v| v.as_str() != Some(id.as_ref()));
                    }
                }
            }

            // Add to new home.
            match &new_category_id {
                Some(new_cat) => {
                    let cats = space_val
                        .get_mut("categories")
                        .and_then(|v| v.as_array_mut())
                        .ok_or_else(|| {
                            SetError::new(SetErrorType::Forbidden)
                                .with_description("internal: Space.categories not an array")
                        })?;
                    for cat in cats.iter_mut() {
                        if cat.get("id").and_then(|s| s.as_str()) == Some(new_cat.as_ref()) {
                            let ch_ids = cat
                                .get_mut("channelIds")
                                .and_then(|v| v.as_array_mut())
                                .ok_or_else(|| {
                                SetError::new(SetErrorType::Forbidden)
                                    .with_description("internal: Category.channelIds not an array")
                            })?;
                            ch_ids.push(serde_json::Value::String(id.as_ref().to_owned()));
                            break;
                        }
                    }
                }
                None => {
                    let unc = space_val
                        .get_mut("uncategorizedChannelIds")
                        .and_then(|v| v.as_array_mut())
                        .ok_or_else(|| {
                            SetError::new(SetErrorType::Forbidden).with_description(
                                "internal: Space.uncategorizedChannelIds not an array",
                            )
                        })?;
                    unc.push(serde_json::Value::String(id.as_ref().to_owned()));
                }
            }

            inner
                .objects_mut("Space", account_id)
                .insert(space_id.clone(), space_val);
            *space_mutated = true;
        }
    }

    chats_updated.insert(id);
    Ok(None)
}

// ===========================================================================
// Role + Member variant helpers (bd:JMAP-g7wu.2.4.3)
// ===========================================================================

/// Apply `SpacePatchOp::AddRole`: assign a fresh `RoleId`, push the role
/// into `space.roles`. Permission and role-position hierarchy
/// enforcement happens in [`validate_space_patch_ops`] before this
/// helper runs.
#[allow(clippy::result_large_err)]
fn apply_add_role(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    let role = match op {
        SpacePatchOp::AddRole(r) => r,
        _ => unreachable!("apply_add_role called with non-AddRole variant"),
    };

    // Server-assign a fresh RoleId. The wire payload's `id` field is
    // a client-supplied placeholder per `SpacePatchOp::AddRole` doc
    // comment; we overwrite it before storage.
    let new_id = MemoryBackend::demo_next_id(inner, "Role", account_id);
    let mut role = role;
    role.id = new_id.clone();

    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    let roles = space_val
        .get_mut("roles")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.roles not an array")
        })?;

    roles.push(serde_json::to_value(&role).map_err(|e| {
        SetError::new(SetErrorType::Forbidden)
            .with_description(format!("internal: serialize SpaceRole: {e}"))
    })?);

    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);
    *space_mutated = true;
    Ok(Some(new_id))
}

/// Apply `SpacePatchOp::RemoveRole`: remove the named role and cascade-
/// demote any members whose only remaining role would be the removed
/// one (draft-atwood-jmap-chat-00 §Space/set line 1099). Permission
/// and hierarchy checks happen in [`validate_space_patch_ops`].
#[allow(clippy::result_large_err)]
fn apply_remove_role(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    let target_id = match op {
        SpacePatchOp::RemoveRole(id) => id,
        _ => unreachable!("apply_remove_role called with non-RemoveRole variant"),
    };

    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    let roles = space_val
        .get_mut("roles")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.roles not an array")
        })?;
    let pos = roles
        .iter()
        .position(|v| v.get("id").and_then(|s| s.as_str()) == Some(target_id.as_ref()));
    let Some(pos) = pos else {
        return Err(SetError::new(SetErrorType::NotFound)
            .with_description(format!("role {} not found", target_id.as_ref())));
    };
    roles.remove(pos);

    // Cascade: members holding the removed role have it stripped from
    // their `roleIds`. Members whose only role was the removed one
    // are left with an empty `roleIds`, which is the `@everyone`-only
    // state (draft-atwood-jmap-chat-00 §Space/set line 1099 + the
    // conventional empty-role-ids representation).
    if let Some(members) = space_val.get_mut("members").and_then(|v| v.as_array_mut()) {
        for member in members.iter_mut() {
            if let Some(role_ids) = member.get_mut("roleIds").and_then(|v| v.as_array_mut()) {
                role_ids.retain(|v| v.as_str() != Some(target_id.as_ref()));
            }
        }
    }

    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);
    *space_mutated = true;
    Ok(None)
}

/// Apply `SpacePatchOp::UpdateRole`: apply the [`RolePatch`] to the
/// named role. Permission and hierarchy checks happen in
/// [`validate_space_patch_ops`].
#[allow(clippy::result_large_err)]
fn apply_update_role(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    use jmap_chat_types::clearable::Clearable;
    use jmap_chat_types::space::SpaceRole;

    let (target_id, patch) = match op {
        SpacePatchOp::UpdateRole { id, patch } => (id, patch),
        _ => unreachable!("apply_update_role called with non-UpdateRole variant"),
    };

    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    let roles = space_val
        .get_mut("roles")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.roles not an array")
        })?;
    let pos = roles
        .iter()
        .position(|v| v.get("id").and_then(|s| s.as_str()) == Some(target_id.as_ref()));
    let Some(pos) = pos else {
        return Err(SetError::new(SetErrorType::NotFound)
            .with_description(format!("role {} not found", target_id.as_ref())));
    };

    // Deserialize, apply patch, re-serialize. The `#[non_exhaustive]`
    // attribute on `SpaceRole` makes the struct-expression update
    // syntax illegal, so the in-place mutation pattern is required.
    let mut role: SpaceRole = serde_json::from_value(roles[pos].clone()).map_err(|e| {
        SetError::new(SetErrorType::Forbidden)
            .with_description(format!("internal: deserialize SpaceRole: {e}"))
    })?;
    if let Some(name) = patch.name {
        role.name = name;
    }
    match patch.color {
        Some(Clearable::Set(c)) => role.color = Some(c),
        Some(Clearable::Clear) => role.color = None,
        None => {}
    }
    if let Some(permissions) = patch.permissions {
        role.permissions = permissions;
    }
    if let Some(position) = patch.position {
        role.position = position;
    }
    roles[pos] = serde_json::to_value(&role).map_err(|e| {
        SetError::new(SetErrorType::Forbidden)
            .with_description(format!("internal: serialize SpaceRole: {e}"))
    })?;

    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);
    *space_mutated = true;
    Ok(None)
}

/// Apply `SpacePatchOp::AddMember`: push a new `SpaceMember` into the
/// Space's `members` array and bump `memberCount`. Permission and
/// hierarchy checks (and existence checks on each role id) happen in
/// [`validate_space_patch_ops`].
#[allow(clippy::result_large_err)]
fn apply_add_member(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    let (user_id, role_ids) = match op {
        SpacePatchOp::AddMember(create) => (create.user_id, create.role_ids),
        _ => unreachable!("apply_add_member called with non-AddMember variant"),
    };

    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    // Reject if already a member: duplicate-add is an
    // invalidProperties error against the wire-level userId field.
    if space_val
        .get("members")
        .and_then(|v| v.as_array())
        .is_some_and(|members| {
            members
                .iter()
                .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(user_id.as_ref()))
        })
    {
        return Err(SetError::new(SetErrorType::InvalidProperties)
            .with_properties(vec!["userId".to_owned()])
            .with_description(format!(
                "member {} is already a member of this Space",
                user_id.as_ref()
            )));
    }

    // Validate every role_id refers to an existing SpaceRole on this
    // Space. A nonexistent role_id would create a dangling
    // reference; reject as InvalidProperties on `roleIds`.
    if !role_ids.is_empty() {
        let existing: HashSet<String> = space_val
            .get("roles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        for rid in &role_ids {
            if !existing.contains(rid.as_ref()) {
                return Err(SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(vec!["roleIds".to_owned()])
                    .with_description(format!(
                        "role {} does not exist in this Space",
                        rid.as_ref()
                    )));
            }
        }
    }

    // Build the wire-shape member object directly. `SpaceMember` is
    // `#[non_exhaustive]`, so the struct-expression construction is
    // illegal; serializing a freshly-built JSON Map sidesteps that.
    let mut member_obj = serde_json::Map::new();
    member_obj.insert(
        "id".to_owned(),
        serde_json::Value::String(user_id.as_ref().to_owned()),
    );
    member_obj.insert(
        "roleIds".to_owned(),
        serde_json::Value::Array(
            role_ids
                .iter()
                .map(|id| serde_json::Value::String(id.as_ref().to_owned()))
                .collect(),
        ),
    );
    member_obj.insert(
        "joinedAt".to_owned(),
        serde_json::Value::String(now_utc_string().into_inner()),
    );

    let members = space_val
        .get_mut("members")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.members not an array")
        })?;
    members.push(serde_json::Value::Object(member_obj));

    // Keep `memberCount` consistent with `members.len()`. The field
    // is required on Space (draft §4.11) and clients rely on it for
    // UI counts.
    let new_count = members.len() as u64;
    if let Some(count_field) = space_val.get_mut("memberCount") {
        *count_field = serde_json::Value::from(new_count);
    }

    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);
    *space_mutated = true;
    Ok(None)
}

/// Apply `SpacePatchOp::RemoveMember`: remove the named member and
/// decrement `memberCount`. Permission and last-admin-protection
/// checks happen in [`validate_space_patch_ops`].
#[allow(clippy::result_large_err)]
fn apply_remove_member(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    let target_id = match op {
        SpacePatchOp::RemoveMember(id) => id,
        _ => unreachable!("apply_remove_member called with non-RemoveMember variant"),
    };

    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    let members = space_val
        .get_mut("members")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.members not an array")
        })?;

    let pos = members
        .iter()
        .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(target_id.as_ref()));
    let Some(pos) = pos else {
        return Err(SetError::new(SetErrorType::NotFound)
            .with_description(format!("member {} not found", target_id.as_ref())));
    };
    members.remove(pos);

    // Keep `memberCount` consistent with `members.len()`.
    let new_count = members.len() as u64;
    if let Some(count_field) = space_val.get_mut("memberCount") {
        *count_field = serde_json::Value::from(new_count);
    }

    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);
    *space_mutated = true;
    Ok(None)
}

/// Apply `SpacePatchOp::UpdateMember`: apply the [`MemberPatch`] to
/// the named member. Permission and hierarchy checks happen in
/// [`validate_space_patch_ops`].
#[allow(clippy::result_large_err)]
fn apply_update_member(
    inner: &mut Inner,
    account_id: &str,
    space_id: &Id,
    op: SpacePatchOp,
    space_mutated: &mut bool,
) -> Result<Option<Id>, SetError> {
    use jmap_chat_types::clearable::Clearable;

    let (user_id, patch) = match op {
        SpacePatchOp::UpdateMember { user_id, patch } => (user_id, patch),
        _ => unreachable!("apply_update_member called with non-UpdateMember variant"),
    };

    let mut space_val = inner
        .objects_ref("Space", account_id)
        .and_then(|m| m.get(space_id))
        .cloned()
        .ok_or_else(|| SetError::new(SetErrorType::NotFound))?;

    // Validate any new role ids referenced in the patch before mutating.
    if let Some(role_ids) = &patch.role_ids {
        let existing: HashSet<String> = space_val
            .get("roles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        for rid in role_ids {
            if !existing.contains(rid.as_ref()) {
                return Err(SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(vec!["roleIds".to_owned()])
                    .with_description(format!(
                        "role {} does not exist in this Space",
                        rid.as_ref()
                    )));
            }
        }
    }

    let members = space_val
        .get_mut("members")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| {
            SetError::new(SetErrorType::Forbidden)
                .with_description("internal: Space.members not an array")
        })?;

    let pos = members
        .iter()
        .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(user_id.as_ref()));
    let Some(pos) = pos else {
        return Err(SetError::new(SetErrorType::NotFound)
            .with_description(format!("member {} not found", user_id.as_ref())));
    };

    // Apply the patch in place on the wire-shape JSON object. The
    // `SpaceMember` type is `#[non_exhaustive]`, so direct
    // struct-expression update is illegal; in-place edits on the
    // JSON map preserve unknown fields (extras-preservation policy).
    let member_obj = members[pos].as_object_mut().ok_or_else(|| {
        SetError::new(SetErrorType::Forbidden)
            .with_description("internal: SpaceMember not a JSON object")
    })?;
    if let Some(role_ids) = patch.role_ids {
        member_obj.insert(
            "roleIds".to_owned(),
            serde_json::Value::Array(
                role_ids
                    .iter()
                    .map(|id| serde_json::Value::String(id.as_ref().to_owned()))
                    .collect(),
            ),
        );
    }
    match patch.nick {
        Some(Clearable::Set(s)) => {
            member_obj.insert("nick".to_owned(), serde_json::Value::String(s));
        }
        Some(Clearable::Clear) => {
            member_obj.remove("nick");
        }
        None => {}
    }

    inner
        .objects_mut("Space", account_id)
        .insert(space_id.clone(), space_val);
    *space_mutated = true;
    Ok(None)
}

// ---------------------------------------------------------------------------
// Permission / hierarchy / last-admin pre-validation helpers
// (bd:JMAP-g7wu.2.4.3 criteria 1, 2, 3-replacement, 6, 7)
// ---------------------------------------------------------------------------

/// Pre-validate all ops in the patch.
///
/// Walks every op (Role, Member, Channel, Category) and applies:
///
/// - **Permission gating** (criterion 1): the caller's effective
///   permissions must be a superset of [`required_permissions_for_op`]'s
///   return value for each op. Gated permissions per
///   draft-atwood-jmap-chat-00 §Space/set:
///   * Role ops (`AddRole`, `RemoveRole`, `UpdateRole`): `manage_roles`
///   * Member ops (`AddMember`, `RemoveMember`, `UpdateMember`):
///     `manage_members` (plus `manage_roles` for `UpdateMember`
///     entries that modify `roleIds`)
///   * Channel ops (`AddChannel`, `RemoveChannel`, `UpdateChannel`)
///     and Category ops (`AddCategory`, `RemoveCategory`,
///     `UpdateCategory`): `manage_channels`
/// - **Role-position hierarchy** (criterion 2): the caller may only
///   add or modify roles whose `position` is strictly less than their
///   own highest-position role (draft §Space/set lines 1096, 1102).
///   Cross-cuts AddRole, UpdateRole, AddMember (when `role_ids` is
///   non-empty), and UpdateMember (when `patch.role_ids` is set).
///   Channel and Category ops have no hierarchy check.
/// - **Last-admin protection** (criterion 3 replacement): when
///   `protect_last_admin` is true, the patch's `RemoveMember` ops in
///   aggregate must not leave the Space with zero members holding
///   either `manage_members` or `manage_space`.
///
/// Returns `Ok(())` if the patch passes all checks. Returns
/// `Err(SetError)` on the FIRST failure encountered — this is the
/// whole-patch reject of criterion 6.
///
/// Single-user mode (`caller_id == None`, criterion 7) skips the
/// identity-dependent checks (permission gating, hierarchy). The
/// last-admin-protection check is identity-independent and still
/// fires when its config flag is on.
///
/// History: prior to bd:JMAP-g7wu.2.4.14 this helper was named
/// `validate_role_member_ops` and only gated Role/Member ops, leaving
/// the Channel and Category ops at the backend without a permission
/// pre-check. The rename + filter drop close that gap.
#[allow(clippy::result_large_err)]
fn validate_space_patch_ops(
    space_val: &serde_json::Value,
    caller_id: Option<&Id>,
    ops: &[SpacePatchOp],
    protect_last_admin: bool,
) -> Result<(), SetError> {
    use crate::permissions::{
        required_permissions_for_op, RequiredPermissions, MANAGE_MEMBERS, MANAGE_SPACE,
    };

    // Identity-dependent checks: permission gating + hierarchy.
    if let Some(caller_id) = caller_id {
        let caller_perms = caller_effective_permissions(space_val, caller_id).unwrap_or_default();
        let caller_highest = caller_highest_position(space_val, caller_id).unwrap_or(0);

        for op in ops {
            // Permission gate (criterion 1). Applies to ALL op
            // families. `required_permissions_for_op` returns the
            // per-variant permission set from draft-atwood-jmap-chat-00
            // §Space/set; a caller missing any required permission
            // triggers a whole-patch reject (criterion 6). An unknown
            // op variant fails closed via `RequiredPermissions::UnknownOp`
            // — the kit cannot enumerate permissions for a variant it
            // does not recognize, so the safe answer is "no, you cannot
            // apply it".
            // `RequiredPermissions` is `#[non_exhaustive]` at the public
            // crate boundary, but the compiler sees all variants from
            // within this crate. Match exhaustively on the in-crate
            // variant set so a future Conditional / Layered / etc. arm
            // forces a deliberate fail-closed decision at compile time
            // rather than silently routing through a wildcard.
            let required = match required_permissions_for_op(op) {
                RequiredPermissions::Known(slice) => slice,
                RequiredPermissions::UnknownOp => {
                    return Err(
                        SetError::new(SetErrorType::Forbidden).with_description(format!(
                            "unknown SpacePatchOp variant `{}`; this version of \
                             the kit cannot determine its permission requirement, \
                             rejecting fail-closed",
                            variant_name(op)
                        )),
                    );
                }
            };
            for req in required {
                if !caller_perms.contains(*req) {
                    return Err(
                        SetError::new(SetErrorType::Forbidden).with_description(format!(
                            "caller lacks required permission `{}` for {}",
                            req,
                            variant_name(op)
                        )),
                    );
                }
            }

            // Hierarchy gate (criterion 2). Only applies to ops that
            // grant or modify role placements:
            //   - AddRole: the new role's position must be < caller's
            //     highest.
            //   - UpdateRole: both the existing role's position AND
            //     the new position (if patch.position is set) must
            //     be < caller's highest.
            //   - AddMember (when role_ids is non-empty): every
            //     role_id's position must be < caller's highest.
            //   - UpdateMember (when patch.role_ids is set): every
            //     role_id's position must be < caller's highest.
            //   - RemoveMember: no hierarchy check (removing a
            //     member doesn't reshape roles).
            //   - RemoveRole: the existing role's position must be <
            //     caller's highest.
            match op {
                SpacePatchOp::AddRole(role) if role.position >= caller_highest => {
                    return Err(hierarchy_error(role.position, caller_highest, "AddRole"));
                }
                SpacePatchOp::RemoveRole(target_id) => {
                    if let Some(pos) = role_position(space_val, target_id.as_ref()) {
                        if pos >= caller_highest {
                            return Err(hierarchy_error(pos, caller_highest, "RemoveRole"));
                        }
                    }
                    // Nonexistent role id is a per-op NotFound, not
                    // a permission failure — leave for the apply
                    // pass.
                }
                SpacePatchOp::UpdateRole { id, patch } => {
                    if let Some(pos) = role_position(space_val, id.as_ref()) {
                        if pos >= caller_highest {
                            return Err(hierarchy_error(pos, caller_highest, "UpdateRole"));
                        }
                    }
                    if let Some(new_pos) = patch.position {
                        if new_pos >= caller_highest {
                            return Err(hierarchy_error(
                                new_pos,
                                caller_highest,
                                "UpdateRole (new position)",
                            ));
                        }
                    }
                }
                SpacePatchOp::AddMember(create) => {
                    for rid in &create.role_ids {
                        if let Some(pos) = role_position(space_val, rid.as_ref()) {
                            if pos >= caller_highest {
                                return Err(hierarchy_error(pos, caller_highest, "AddMember"));
                            }
                        }
                        // Nonexistent role id surfaces in apply pass
                        // as InvalidProperties.
                    }
                }
                SpacePatchOp::UpdateMember { patch, .. } => {
                    if let Some(role_ids) = &patch.role_ids {
                        for rid in role_ids {
                            if let Some(pos) = role_position(space_val, rid.as_ref()) {
                                if pos >= caller_highest {
                                    return Err(hierarchy_error(
                                        pos,
                                        caller_highest,
                                        "UpdateMember",
                                    ));
                                }
                            }
                        }
                    }
                }
                SpacePatchOp::RemoveMember(_) => {
                    // No hierarchy check on member removal.
                }
                _ => {}
            }
        }
    }

    // Identity-independent check: last-admin protection (criterion 3
    // replacement). Fires regardless of caller_id when the config
    // flag is on.
    if protect_last_admin {
        // Set of user_ids the patch will remove.
        let mut to_remove: HashSet<String> = HashSet::new();
        for op in ops {
            if let SpacePatchOp::RemoveMember(id) = op {
                to_remove.insert(id.as_ref().to_owned());
            }
        }
        if !to_remove.is_empty() {
            // Project the post-patch admin count. An admin is a
            // member whose effective permissions include
            // `manage_members` or `manage_space`.
            //
            // Limitation: this projection does NOT model UpdateMember
            // ops that strip admin role_ids, nor RemoveRole ops that
            // remove a role granting admin perms. Those are valid
            // ways to demote admins. Modeling them would require a
            // more elaborate simulation. For bd:JMAP-g7wu.2.4.3 the
            // RemoveMember-only projection is the scoped behavior;
            // production backends with stricter requirements can
            // override [`ChatBackend::apply_space_patch`].
            let members = space_val
                .get("members")
                .and_then(|v| v.as_array())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let mut admin_remaining: usize = 0;
            for m in members {
                let Some(mid) = m.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                if to_remove.contains(mid) {
                    continue;
                }
                // Resolve this member's effective permissions.
                let perms = member_effective_permissions(space_val, mid).unwrap_or_default();
                if perms.contains(MANAGE_MEMBERS) || perms.contains(MANAGE_SPACE) {
                    admin_remaining += 1;
                    break; // one is enough
                }
            }
            if admin_remaining == 0 {
                return Err(SetError::new(SetErrorType::Forbidden).with_description(
                    "removing these members would leave the Space with no \
                     `manage_members`/`manage_space` holder (last-admin protection)",
                ));
            }
        }
    }

    Ok(())
}

/// Construct the standard "role position not strictly less than
/// caller's highest" SetError. Used by [`validate_space_patch_ops`].
#[allow(clippy::result_large_err)]
fn hierarchy_error(target_pos: u64, caller_highest: u64, op_label: &str) -> SetError {
    SetError::new(SetErrorType::Forbidden).with_description(format!(
        "{op_label}: target role position {target_pos} is not strictly less than \
         caller's highest role position {caller_highest} (role-position hierarchy, \
         draft §Space/set lines 1096, 1102)"
    ))
}

/// Resolve the caller's effective permission set within `space_val`.
///
/// Returns the union of `permissions` across every `SpaceRole`
/// referenced by the caller's `members[i].role_ids`. The reference
/// impl treats `@everyone` as having an empty (implementation-
/// defined) permission floor; production deployments override this
/// behavior via their own backend.
///
/// Returns `None` if the caller is not a member of the Space.
fn caller_effective_permissions(
    space_val: &serde_json::Value,
    caller_id: &Id,
) -> Option<HashSet<String>> {
    member_effective_permissions(space_val, caller_id.as_ref())
}

/// Resolve `member_id`'s effective permission set within `space_val`.
/// Same semantics as [`caller_effective_permissions`] but keyed on a
/// raw string id (used by the last-admin-protection scan).
fn member_effective_permissions(
    space_val: &serde_json::Value,
    member_id: &str,
) -> Option<HashSet<String>> {
    let members = space_val.get("members").and_then(|v| v.as_array())?;
    let member = members
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(member_id))?;
    let role_ids: HashSet<String> = member
        .get("roleIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut perms: HashSet<String> = HashSet::new();
    if let Some(roles) = space_val.get("roles").and_then(|v| v.as_array()) {
        for role in roles {
            let Some(rid) = role.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if !role_ids.contains(rid) {
                continue;
            }
            if let Some(role_perms) = role.get("permissions").and_then(|v| v.as_array()) {
                for p in role_perms {
                    if let Some(s) = p.as_str() {
                        perms.insert(s.to_owned());
                    }
                }
            }
        }
    }
    Some(perms)
}

/// Resolve the caller's highest role `position` within `space_val`.
///
/// Returns `Some(max position)` if the caller is a member; the value
/// is at least 0 (a member with no explicit roles implicitly holds
/// `@everyone` at position 0). Returns `None` if the caller is not
/// a member of the Space.
fn caller_highest_position(space_val: &serde_json::Value, caller_id: &Id) -> Option<u64> {
    let members = space_val.get("members").and_then(|v| v.as_array())?;
    let member = members
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(caller_id.as_ref()))?;
    let role_ids: HashSet<String> = member
        .get("roleIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut max: u64 = 0;
    if let Some(roles) = space_val.get("roles").and_then(|v| v.as_array()) {
        for role in roles {
            let Some(rid) = role.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if !role_ids.contains(rid) {
                continue;
            }
            let Some(pos) = role.get("position").and_then(|v| v.as_u64()) else {
                continue;
            };
            if pos > max {
                max = pos;
            }
        }
    }
    Some(max)
}

/// Look up an existing role's `position` by id.
fn role_position(space_val: &serde_json::Value, role_id: &str) -> Option<u64> {
    let roles = space_val.get("roles").and_then(|v| v.as_array())?;
    roles
        .iter()
        .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(role_id))
        .and_then(|r| r.get("position").and_then(|v| v.as_u64()))
}

/// Return every Message id whose `chatId` field equals `chat_id`.
fn scan_messages_in_chat(inner: &Inner, account_id: &str, chat_id: &Id) -> Vec<Id> {
    let Some(msgs) = inner.objects_ref("Message", account_id) else {
        return Vec::new();
    };
    msgs.iter()
        .filter(|(_, v)| v.get("chatId").and_then(|c| c.as_str()) == Some(chat_id.as_ref()))
        .map(|(id, _)| id.clone())
        .collect()
}

/// True if `chat_id` names an existing Chat with kind=channel and
/// space_id matching `space_id`.
fn channel_belongs_to_space(inner: &Inner, account_id: &str, chat_id: &Id, space_id: &Id) -> bool {
    inner
        .objects_ref("Chat", account_id)
        .and_then(|m| m.get(chat_id))
        .map(|v| {
            v.get("kind").and_then(|k| k.as_str()) == Some("channel")
                && v.get("spaceId").and_then(|s| s.as_str()) == Some(space_id.as_ref())
        })
        .unwrap_or(false)
}

/// Return every channel-kind Chat whose `categoryId` equals `target_id`.
fn scan_channels_in_category(inner: &Inner, account_id: &str, target_id: &Id) -> Vec<Id> {
    let Some(chats) = inner.objects_ref("Chat", account_id) else {
        return Vec::new();
    };
    chats
        .iter()
        .filter(|(_, v)| {
            v.get("kind").and_then(|k| k.as_str()) == Some("channel")
                && v.get("categoryId").and_then(|c| c.as_str()) == Some(target_id.as_ref())
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Set or clear a Chat's `categoryId` field in place. Silently no-ops if
/// the Chat does not exist (the caller has already validated existence
/// via `channel_belongs_to_space` or `scan_channels_in_category`).
fn set_channel_category(inner: &mut Inner, account_id: &str, chat_id: &Id, new_cat: Option<&Id>) {
    let map = inner.objects_mut("Chat", account_id);
    let Some(chat_val) = map.get_mut(chat_id) else {
        return;
    };
    let obj = match chat_val {
        serde_json::Value::Object(o) => o,
        _ => return,
    };
    match new_cat {
        Some(cat_id) => {
            obj.insert(
                "categoryId".to_owned(),
                serde_json::Value::String(cat_id.as_ref().to_owned()),
            );
        }
        None => {
            obj.remove("categoryId");
        }
    }
}

/// Per-variant rejection text for the `#[non_exhaustive]`
/// future-variant catch-all in `apply_space_patch_impl`.
///
/// As of `bd:JMAP-g7wu.2.4.3` all twelve known `SpacePatchOp`
/// variants (Role/Member/Channel/Category) are routed to dedicated
/// `apply_*` helpers and never reach this stub. Only an
/// `#[non_exhaustive]`-added future variant — visible after a
/// `jmap-chat-types` upgrade but before the corresponding handler
/// lands here — would fall through.
fn stub_description(op: &SpacePatchOp) -> String {
    let _ = op;
    "unknown SpacePatchOp variant (tracked under bd:JMAP-g7wu.2.4)".to_owned()
}

fn variant_name(op: &SpacePatchOp) -> &'static str {
    match op {
        SpacePatchOp::AddRole(_) => "AddRole",
        SpacePatchOp::RemoveRole(_) => "RemoveRole",
        SpacePatchOp::UpdateRole { .. } => "UpdateRole",
        SpacePatchOp::AddMember(_) => "AddMember",
        SpacePatchOp::RemoveMember(_) => "RemoveMember",
        SpacePatchOp::UpdateMember { .. } => "UpdateMember",
        SpacePatchOp::AddChannel(_) => "AddChannel",
        SpacePatchOp::RemoveChannel(_) => "RemoveChannel",
        SpacePatchOp::UpdateChannel { .. } => "UpdateChannel",
        SpacePatchOp::AddCategory(_) => "AddCategory",
        SpacePatchOp::RemoveCategory(_) => "RemoveCategory",
        SpacePatchOp::UpdateCategory { .. } => "UpdateCategory",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChatBackend;
    use std::collections::HashSet;

    /// Oracle: invite codes are CSPRNG-derived, so a batch of generated codes
    /// must show high entropy — no duplicates across a large sample, no
    /// monotone-in-time leakage, no shared prefix.
    ///
    /// This is a behavioural canary on bd:JMAP-sc1b.93. The pre-fix
    /// nanosecond-derived impl produced strictly monotone-in-time output
    /// with a 16-byte-wide identical prefix between consecutive calls; the
    /// CSPRNG impl does not.
    ///
    /// The test deliberately makes no claim about the *value* of any single
    /// code (that would require the code under test to be its own oracle).
    /// It only asserts structural properties that the failing implementation
    /// could not satisfy.
    #[test]
    fn generate_invite_code_is_csprng() {
        let backend = MemoryBackend::new();
        const N: usize = 256;
        let mut codes: HashSet<String> = HashSet::with_capacity(N);
        for _ in 0..N {
            codes.insert(backend.generate_invite_code());
        }
        // 256 CSPRNG samples of 128 bits each have negligible birthday-collision
        // probability (~256^2 / 2^129 ≈ 10^-34). Any duplicate indicates the
        // impl regressed to a counter, a timestamp, or a constant.
        assert_eq!(
            codes.len(),
            N,
            "expected {N} distinct invite codes; found {} duplicates",
            N - codes.len()
        );
        // Every code is 32 hex chars (16 bytes * 2).
        for c in &codes {
            assert_eq!(
                c.len(),
                32,
                "invite code must be 32 hex chars (16 random bytes); got len {} ({})",
                c.len(),
                c
            );
            assert!(
                c.chars().all(|ch| ch.is_ascii_hexdigit()),
                "invite code must be lowercase hex; got {c}"
            );
        }
        // Sanity tripwire for the pre-fix bug: the original nanos impl
        // produced sequential codes with a 10-12 char shared prefix
        // (high-order nanoseconds change slowly between two adjacent calls).
        // A CSPRNG produces near-uniform random output, so the longest
        // shared prefix across a 256-sample batch should be tiny.
        let sorted: Vec<&String> = {
            let mut v: Vec<&String> = codes.iter().collect();
            v.sort();
            v
        };
        let max_prefix: usize = sorted
            .windows(2)
            .map(|w| {
                w[0].as_bytes()
                    .iter()
                    .zip(w[1].as_bytes())
                    .take_while(|(a, b)| a == b)
                    .count()
            })
            .max()
            .unwrap_or(0);
        // Even with 256 samples a CSPRNG can incidentally produce one pair
        // sharing 4-5 hex chars; pre-fix the shared prefix was always >=10.
        assert!(
            max_prefix < 10,
            "max shared prefix across sorted codes is {max_prefix}; >=10 suggests timestamp-derived output (bd:JMAP-sc1b.93 regression)"
        );
    }

    // json_merge_patch tests live in jmap-server (the function's home
    // crate as of bd:JMAP-sc1b.103). Adding chat-specific behavioural
    // canaries here is fine; merge-patch RFC 7396 conformance is not
    // re-tested per sibling.
}

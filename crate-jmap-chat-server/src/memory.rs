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
    GetObject, JmapBackend, JmapObject, OpResult, QueryChangesResult, QueryObject, QueryResult,
    SetError, SetErrorType, SetObject, SlowModeError, SpacePatchOp,
};
use jmap_server::{json_merge_patch, now_utc_string};
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
}

impl Inner {
    fn current_state(&self, type_name: &'static str, account_id: &str) -> u64 {
        *self
            .states
            .get(&(type_name, account_id.to_owned()))
            .unwrap_or(&0)
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

    /// Allocate a server-assigned id for a new object.
    fn next_id(inner: &mut Inner, type_name: &'static str, account_id: &str) -> Id {
        let n = inner
            .objects_ref(type_name, account_id)
            .map_or(0, |m| m.len());
        Id::from(format!("{}{}", type_name.to_ascii_lowercase(), n + 1))
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
            .map_err(|_| BackendChangesError::TooManyChanges { limit: 0 })?;

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

        ids = ids[start..]
            .iter()
            .take(limit.map_or(usize::MAX, |n| n as usize))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as i64,
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
        let server_id = Self::next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

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
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize patch: {e}"))))?;
        json_merge_patch(&mut current, patch_val);

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

    /// Reference implementation of [`ChatBackend::apply_space_patch`].
    ///
    /// Dispatches each [`SpacePatchOp`] to a per-variant helper. Currently
    /// implemented: Category variants (Add/Remove/Update) per
    /// `bd:JMAP-g7wu.2.4.5` and Channel variants (Add/Remove/Update) per
    /// `bd:JMAP-g7wu.2.4.4`. Other variants return a `Forbidden` `OpResult`
    /// whose description names the tracking bead (`.4.3` for Role/Member).
    ///
    /// The entire patch runs under one mutex acquisition, providing
    /// best-effort transactional semantics for the reference impl: a
    /// failure mid-way through the op vector does NOT roll back ops that
    /// already succeeded — they remain applied. Production backends
    /// should wrap the sequence in a real transaction. This caveat is
    /// documented on the trait.
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
        _caller: &(),
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        // Confirm the target Space exists before doing any work.
        if !inner
            .objects_ref("Space", account_id.as_ref())
            .is_some_and(|m| m.contains_key(space_id))
        {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }

        let mut results = Vec::with_capacity(ops.len());
        let mut space_mutated = false;
        let mut chats_created: Vec<Id> = Vec::new();
        let mut chats_updated: HashSet<Id> = HashSet::new();
        let mut chats_destroyed: Vec<Id> = Vec::new();
        let mut messages_destroyed: Vec<Id> = Vec::new();

        for (op_index, op) in ops.into_iter().enumerate() {
            let outcome =
                match &op {
                    SpacePatchOp::AddCategory(_)
                    | SpacePatchOp::RemoveCategory(_)
                    | SpacePatchOp::UpdateCategory { .. } => apply_category_op(
                        &mut inner,
                        account_id.as_ref(),
                        space_id,
                        op,
                        &mut chats_updated,
                        &mut space_mutated,
                    ),
                    SpacePatchOp::AddChannel(_) => apply_add_channel(
                        &mut inner,
                        account_id.as_ref(),
                        space_id,
                        op,
                        &mut chats_created,
                        &mut space_mutated,
                    ),
                    SpacePatchOp::RemoveChannel(_) => apply_remove_channel(
                        &mut inner,
                        account_id.as_ref(),
                        space_id,
                        op,
                        &mut chats_destroyed,
                        &mut messages_destroyed,
                        &mut space_mutated,
                    ),
                    SpacePatchOp::UpdateChannel { .. } => apply_update_channel(
                        &mut inner,
                        account_id.as_ref(),
                        space_id,
                        op,
                        &mut chats_updated,
                        &mut space_mutated,
                    ),
                    _ => Err(SetError::new(SetErrorType::Forbidden)
                        .with_description(stub_description(&op))),
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
    ) -> Result<(), BackendSetError<Self::Error>> {
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
            // via the shared `next_id` helper with a synthetic type name.
            let new_id = MemoryBackend::next_id(inner, "Category", account_id);
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
    let new_id = MemoryBackend::next_id(inner, "Chat", account_id);

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
    chat_obj.insert("createdAt".to_owned(), serde_json::Value::String(now));
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

/// Per-variant rejection text for the stubbed-out variants
/// (Role/Member → `bd:JMAP-g7wu.2.4.3`). Channel variants are
/// implemented (`bd:JMAP-g7wu.2.4.4`) and never reach this helper;
/// Category variants are likewise implemented (`bd:JMAP-g7wu.2.4.5`).
fn stub_description(op: &SpacePatchOp) -> String {
    let (variant, bead) = match op {
        SpacePatchOp::AddRole(_)
        | SpacePatchOp::RemoveRole(_)
        | SpacePatchOp::UpdateRole { .. }
        | SpacePatchOp::AddMember { .. }
        | SpacePatchOp::RemoveMember(_)
        | SpacePatchOp::UpdateMember { .. } => (variant_name(op), "JMAP-g7wu.2.4.3"),
        // `SpacePatchOp` is `#[non_exhaustive]` upstream; fall back
        // gracefully. Channel and Category variants are routed to
        // dedicated apply_* helpers in apply_space_patch and never reach
        // this stub.
        _ => ("unknown", "JMAP-g7wu.2.4"),
    };
    format!("{variant} not yet implemented (tracked under bd:{bead})")
}

fn variant_name(op: &SpacePatchOp) -> &'static str {
    match op {
        SpacePatchOp::AddRole(_) => "AddRole",
        SpacePatchOp::RemoveRole(_) => "RemoveRole",
        SpacePatchOp::UpdateRole { .. } => "UpdateRole",
        SpacePatchOp::AddMember { .. } => "AddMember",
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

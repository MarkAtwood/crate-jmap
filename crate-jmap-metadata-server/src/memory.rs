//! In-memory reference implementation of [`crate::MetadataBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`crate::MetadataBackend`] trait to study when
//!    writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Metadata
//!    dispatcher with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and
//! several draft-ietf-jmap-metadata-01 edge cases are simplified (see
//! source comments below).
//!
//! # Feature flag and API stability
//!
//! This module is gated behind `feature = "memory"` and is **not** enabled
//! by default. Its public API stability is opt-in: it may break across
//! minor versions while the crate is pre-1.0.
//!
//! # Metadata-specific behaviour
//!
//! - **Uniqueness (§3.1)**: the backend enforces the uniqueness constraint
//!   on `(relatedType, relatedId, @type, isPrivate)`. A duplicate create
//!   returns
//!   [`BackendSetError::SetError`](crate::BackendSetError::SetError)
//!   wrapping a [`SetErrorType::AlreadyExists`](crate::SetErrorType::AlreadyExists)
//!   with `existing_id` set to the conflicting object's Id.
//! - **`maySetPrivate` gating (§1.2.1)**: not enforced by the reference
//!   impl — any `isPrivate` value is accepted. Real backends that need
//!   per-account gating should override `create_object` / `update_object`
//!   to consult their capability table.
//! - **Quota (§6)**: not enforced.
//! - **Related-object validation (§3.1)**: not enforced — `relatedType` and
//!   `relatedId` are accepted as-is. Real backends should verify that the
//!   referenced object exists.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use jmap_metadata_server::{memory::MemoryBackend, register_metadata_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_metadata_handlers(
//!     &mut dispatcher,
//!     Arc::new(MemoryBackend::new_with_accounts(&["acc1"])),
//! );
//! ```
//!
//! # Concurrency
//!
//! `std::sync::Mutex` is used for simplicity. The `await_holding_lock`
//! clippy lint is enabled module-wide and enforces that no lock guard
//! is held across an `.await`. If a future change requires holding a
//! guard across `.await`, switch to `tokio::sync::Mutex` rather than
//! disabling the lint.

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, MetadataBackend, QueryChangesResult, QueryObject, QueryResult, SetError,
    SetErrorType, SetObject,
};
// json_merge_patch lives in jmap-server (the shared foundation crate).
// Every reference backend imports it; the canonical RFC 7396 tests live
// with the function there.
use jmap_metadata_types::Metadata;
use jmap_server::{json_merge_patch, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A per-Id record in the change log.
///
/// `related_type` and `type_name` are populated at log-push time when the
/// stored object is Metadata. For change log entries belonging to other
/// JmapObject types (none in this crate today; MemoryBackend only supports
/// Metadata) the strings are empty — those entries are never consumed by
/// [`MemoryBackend::get_metadata_changes`], which filters on the
/// type-keyed log directly.
#[derive(Clone, Debug)]
struct ChangeRecord {
    id: Id,
    related_type: String,
    type_name: String,
}

/// A change log entry for one state transition.
#[derive(Clone, Debug)]
struct ChangeEntry {
    /// The state counter AFTER this change.
    new_state: u64,
    created: Vec<ChangeRecord>,
    updated: Vec<ChangeRecord>,
    destroyed: Vec<ChangeRecord>,
}

/// Shared inner state, behind `Arc<Mutex>`.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// Explicitly registered account ids (accounts may exist with no
    /// objects yet).
    known_accounts: HashSet<String>,
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
// Uniqueness key (§3.1)
// ---------------------------------------------------------------------------

/// The (relatedType, relatedId, `@type`, isPrivate) tuple that
/// draft-ietf-jmap-metadata-01 §3.1 requires to be unique within a user's
/// visible set.
///
/// `isPrivate` defaults to `false` per §2.2.1.5 when absent from the wire.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct UniquenessKey {
    related_type: String,
    related_id: String,
    type_name: String,
    is_private: bool,
}

impl UniquenessKey {
    /// Compute the uniqueness key from a stored Metadata value (the JSON
    /// representation kept in `Inner::objects`). Returns `None` if the
    /// value does not look like a Metadata object (defensive — should not
    /// happen for objects stored under `Metadata::TYPE_NAME`).
    fn from_stored(val: &serde_json::Value) -> Option<Self> {
        Some(Self {
            related_type: val.get("relatedType")?.as_str()?.to_owned(),
            related_id: val.get("relatedId")?.as_str()?.to_owned(),
            type_name: val.get("@type")?.as_str()?.to_owned(),
            is_private: val
                .get("isPrivate")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    /// Compute the uniqueness key from a typed `Metadata`.
    fn from_metadata(m: &Metadata) -> Self {
        Self {
            related_type: m.related_type().to_owned(),
            related_id: m.related_id().as_ref().to_owned(),
            type_name: m.type_name().to_owned(),
            is_private: m.is_private(),
        }
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`crate::MetadataBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Enforces the
/// draft-ietf-jmap-metadata-01 §3.1 uniqueness constraint on
/// `(relatedType, relatedId, @type, isPrivate)`.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Construct an empty `MemoryBackend` with no accounts or stored
    /// objects. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one or more accounts as known even if they have no objects
    /// yet.
    pub fn new_with_accounts(account_ids: &[&str]) -> Self {
        let b = Self::new();
        {
            let mut inner = b.inner.lock().unwrap();
            for id in account_ids {
                inner.known_accounts.insert((*id).to_owned());
            }
        }
        b
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
    ///   read in test debug output.
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

    /// Scan an account's Metadata store for an existing object whose
    /// uniqueness key matches `key`. Returns `Some(id)` if a conflict
    /// exists. Lock must already be held by the caller.
    fn find_uniqueness_conflict(
        inner: &Inner,
        account_id: &str,
        key: &UniquenessKey,
        exclude_id: Option<&Id>,
    ) -> Option<Id> {
        let map = inner.objects_ref(Metadata::TYPE_NAME, account_id)?;
        for (id, val) in map {
            if Some(id) == exclude_id {
                continue;
            }
            if UniquenessKey::from_stored(val).as_ref() == Some(key) {
                return Some(id.clone());
            }
        }
        None
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
            for rec in &entry.created {
                if !destroyed.contains(&rec.id) && !created.contains(&rec.id) {
                    created.push(rec.id.clone());
                }
            }
            for rec in &entry.updated {
                if !destroyed.contains(&rec.id)
                    && !created.contains(&rec.id)
                    && !updated.contains(&rec.id)
                {
                    updated.push(rec.id.clone());
                }
            }
            for rec in &entry.destroyed {
                created.retain(|c| c != &rec.id);
                updated.retain(|u| u != &rec.id);
                if !destroyed.contains(&rec.id) {
                    destroyed.push(rec.id.clone());
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
                .to_string()
                .as_str(),
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
            start as u64,
            Some(total),
            query_state,
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let changes = self
            .get_changes::<O>(&(), account_id, since_query_state, max_changes)
            .await?;

        let inner = self.inner.lock().unwrap();
        let new_query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string()
                .as_str(),
        );

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

impl MetadataBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        // Serialize so we can both inspect for the uniqueness key (when this
        // is a Metadata object) and stash the final JSON in the store.
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize: {e}"))))?;

        // §3.1 uniqueness enforcement — only when the type being created is
        // actually Metadata. The trait is generic over O for forward
        // compatibility; the constraint is type-specific.
        if O::TYPE_NAME == Metadata::TYPE_NAME {
            let typed: Metadata = serde_json::from_value(val.clone()).map_err(|e| {
                BackendSetError::SetError(
                    SetError::new(SetErrorType::InvalidProperties)
                        .with_description(format!("Metadata deserialize: {e}")),
                )
            })?;
            let key = UniquenessKey::from_metadata(&typed);
            if let Some(existing_id) =
                Self::find_uniqueness_conflict(&inner, account_id.as_ref(), &key, None)
            {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::AlreadyExists)
                        .with_description(format!(
                            "Metadata for (relatedType={:?}, relatedId={:?}, @type={:?}, isPrivate={}) already exists",
                            key.related_type, key.related_id, key.type_name, key.is_private,
                        ))
                        .with_existing_id(existing_id),
                ));
            }
        }

        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Inject the server-assigned id, then deserialize back to echo to
        // the caller per the MetadataBackend invariant.
        val["id"] = serde_json::Value::String(server_id.as_ref().to_owned());
        let stored_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("deserialize after create: {e}")))
        })?;

        inner.known_accounts.insert(account_id.as_ref().to_owned());
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        // Capture related_type / type_name from the stored value BEFORE the
        // val moves into objects_mut. For non-Metadata types these strings
        // are empty — get_metadata_changes only consumes them when the
        // change log key matches Metadata::TYPE_NAME.
        let record = ChangeRecord {
            id: server_id.clone(),
            related_type: val
                .get("relatedType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
            type_name: val
                .get("@type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
        };
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(server_id.clone(), val);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![record],
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

        // Apply JSON Merge Patch (RFC 7396). A `MergePatchError::DepthExceeded`
        // return (bd:JMAP-wlip.1) surfaces as `SetErrorType::InvalidPatch` —
        // the depth cap is a DoS guard, never fires on legitimate JMAP `/set
        // update` shapes. `current` is a clone of the stored value, so a
        // partially-applied patch on error is discarded with the local
        // without touching storage.
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize patch: {e}"))))?;
        if let Err(MergePatchError::DepthExceeded) = json_merge_patch(&mut current, patch_val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("patch nesting exceeds server limit"),
            ));
        }

        // §3.1 uniqueness re-check — only when the type is Metadata and
        // the patch could have moved a key into a colliding position.
        if O::TYPE_NAME == Metadata::TYPE_NAME {
            let typed: Metadata = serde_json::from_value(current.clone()).map_err(|e| {
                BackendSetError::SetError(
                    SetError::new(SetErrorType::InvalidProperties)
                        .with_description(format!("post-patch Metadata deserialize: {e}")),
                )
            })?;
            let key = UniquenessKey::from_metadata(&typed);
            if let Some(existing_id) =
                Self::find_uniqueness_conflict(&inner, account_id.as_ref(), &key, Some(id))
            {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::AlreadyExists)
                        .with_description(
                            "Update would violate Metadata uniqueness constraint".to_owned(),
                        )
                        .with_existing_id(existing_id),
                ));
            }
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        // Capture related_type / type_name from the post-patch value
        // BEFORE current moves into objects_mut. For non-Metadata types
        // these strings are empty (see ChangeRecord rustdoc).
        let record = ChangeRecord {
            id: id.clone(),
            related_type: current
                .get("relatedType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
            type_name: current
                .get("@type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
        };
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), current);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![record],
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
            // Capture related_type / type_name from the doomed value
            // BEFORE we drop it. Critical for §3.3 strict conformance:
            // without this snapshot the destroyed array cannot be
            // filtered after the fact (the object no longer exists in
            // `objects` for the override to inspect).
            Some(val) => {
                let record = ChangeRecord {
                    id: id.clone(),
                    related_type: val
                        .get("relatedType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    type_name: val
                        .get("@type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                };
                let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
                inner
                    .change_log_mut(O::TYPE_NAME, account_id.as_ref())
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![record],
                    });
                Ok(())
            }
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        O::TYPE_NAME == Metadata::TYPE_NAME
    }

    /// Override of [`crate::MetadataBackend::get_metadata_changes`] for
    /// strict draft-ietf-jmap-metadata-01 §3.3 conformance.
    ///
    /// Walks the Metadata-keyed change log entries newer than `since_state`
    /// and filters each change record by `(related_type, type_name)`
    /// against the supplied filter args in a single pass. Unlike the
    /// default impl on the trait (which post-filters via re-fetch and
    /// can therefore not filter the destroyed array), this override
    /// honors all three arrays — including destroyed Ids whose objects
    /// no longer exist in `objects`. The per-Id `(related_type,
    /// type_name)` snapshot is captured at mutation time (in
    /// `create_object`, `update_object`, and `destroy_object`) and
    /// stored in the change log entry, so destroyed records carry the
    /// tuple they had at destroy time.
    ///
    /// The state token returned is the current Metadata state for the
    /// account, independent of the filter args (§3.3): a backend MUST
    /// NOT advance state based on filtered-out changes only.
    async fn get_metadata_changes(
        &self,
        _caller: &Self::CallerCtx,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
        filter_related_type: Option<&str>,
        filter_metadata_type: Option<&[String]>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let inner = self.inner.lock().unwrap();

        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::TooManyChanges { limit: 0 })?;

        let log = inner
            .change_log
            .get(&(Metadata::TYPE_NAME, account_id.as_ref().to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let relevant: Vec<&ChangeEntry> = log.iter().filter(|e| e.new_state > since_n).collect();

        let current_state = inner.current_state(Metadata::TYPE_NAME, account_id.as_ref());

        if let Some(max) = max_changes {
            if relevant.len() as u64 > max {
                return Err(BackendChangesError::TooManyChanges { limit: max });
            }
        }

        let record_matches = |rec: &ChangeRecord| -> bool {
            if let Some(rt) = filter_related_type {
                if rec.related_type != rt {
                    return false;
                }
            }
            if let Some(types) = filter_metadata_type {
                if !types.iter().any(|t| t == &rec.type_name) {
                    return false;
                }
            }
            true
        };

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();

        for entry in &relevant {
            for rec in &entry.created {
                if !record_matches(rec) {
                    continue;
                }
                if !destroyed.contains(&rec.id) && !created.contains(&rec.id) {
                    created.push(rec.id.clone());
                }
            }
            for rec in &entry.updated {
                if !record_matches(rec) {
                    continue;
                }
                if !destroyed.contains(&rec.id)
                    && !created.contains(&rec.id)
                    && !updated.contains(&rec.id)
                {
                    updated.push(rec.id.clone());
                }
            }
            for rec in &entry.destroyed {
                // Suppress from created/updated even when the destroy
                // entry itself does not pass the filter — a destroy
                // strictly supersedes earlier create/update for the
                // same id. Then conditionally record in destroyed.
                created.retain(|c| c != &rec.id);
                updated.retain(|u| u != &rec.id);
                if !record_matches(rec) {
                    continue;
                }
                if !destroyed.contains(&rec.id) {
                    destroyed.push(rec.id.clone());
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: create a Metadata Annotation, then get it back by id.
    /// Verifies the server-assigned id is stored and retrievable.
    #[tokio::test]
    async fn create_get_roundtrip() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let meta: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": true,
            "acme.example.com:color": "blue"
        }))
        .expect("fixture must deserialize");

        let (new_id, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", meta)
            .await
            .expect("create must succeed");

        let (found, not_found) = backend
            .get_objects::<Metadata>(
                &(),
                &Id::from("acc1"),
                Some(std::slice::from_ref(&new_id)),
                None,
            )
            .await
            .expect("get must succeed");

        assert!(not_found.is_empty(), "must find newly created object");
        assert_eq!(found.len(), 1);
        match &found[0] {
            Metadata::Annotation(a) => {
                assert_eq!(a.id.as_ref(), Some(&new_id));
                assert_eq!(a.related_type, "Email");
                assert_eq!(a.is_private, Some(true));
                assert_eq!(
                    a.extra.get("acme.example.com:color"),
                    Some(&serde_json::Value::String("blue".to_owned()))
                );
            }
            other => panic!("expected Annotation variant, got {other:?}"),
        }
    }

    /// Oracle: draft §3.1 — creating a second Metadata with the same
    /// (relatedType, relatedId, @type, isPrivate) tuple returns
    /// `alreadyExists` with `existingId` pointing at the first object.
    #[tokio::test]
    async fn uniqueness_constraint_enforced() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);

        let first: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false
        }))
        .unwrap();
        let (first_id, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", first)
            .await
            .expect("first create must succeed");

        let dup: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false,
            "acme.example.com:color": "red"
        }))
        .unwrap();
        let err = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", dup)
            .await
            .expect_err("duplicate create must fail");

        match err {
            BackendSetError::SetError(set_err) => {
                assert_eq!(set_err.error_type, SetErrorType::AlreadyExists);
                assert_eq!(
                    set_err.existing_id.as_ref(),
                    Some(&first_id),
                    "existingId must point at first object"
                );
            }
            other => panic!("expected SetError::AlreadyExists, got {other:?}"),
        }
    }

    /// Oracle: §3.1 — `isPrivate` defaults to `false` per §2.2.1.5. A
    /// shared (isPrivate=false) and a private (isPrivate=true) entry for
    /// the same (relatedType, relatedId, @type) are NOT in conflict.
    #[tokio::test]
    async fn shared_and_private_coexist() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);

        let shared: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", shared)
            .await
            .expect("shared create");

        let private: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": true
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", private)
            .await
            .expect("private create must succeed when shared exists with same triple");
    }

    /// Oracle: §3.1 — different `@type` values for the same
    /// (relatedType, relatedId) are not in conflict.
    #[tokio::test]
    async fn different_type_names_coexist() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);

        let ann: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Mailbox",
            "relatedId": "MB1"
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", ann)
            .await
            .expect("Annotation create");

        let imap: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "ImapMetadata",
            "relatedType": "Mailbox",
            "relatedId": "MB1",
            "metadata": {"comment": "team mailbox"}
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", imap)
            .await
            .expect("ImapMetadata create must succeed when Annotation exists for same triple");
    }

    /// Oracle: destroy advances state and removes the object from get.
    #[tokio::test]
    async fn destroy_advances_state_and_removes_object() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let meta: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }))
        .unwrap();
        let (new_id, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", meta)
            .await
            .unwrap();

        let state_before = backend
            .get_state::<Metadata>(&(), &Id::from("acc1"))
            .await
            .unwrap();

        backend
            .destroy_object::<Metadata>(&(), &Id::from("acc1"), &new_id)
            .await
            .expect("destroy must succeed");

        let state_after = backend
            .get_state::<Metadata>(&(), &Id::from("acc1"))
            .await
            .unwrap();

        assert_ne!(
            state_before.as_ref(),
            state_after.as_ref(),
            "destroy must advance state"
        );

        let (_, not_found) = backend
            .get_objects::<Metadata>(
                &(),
                &Id::from("acc1"),
                Some(std::slice::from_ref(&new_id)),
                None,
            )
            .await
            .unwrap();
        assert_eq!(not_found, vec![new_id]);
    }
}

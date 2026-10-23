//! In-memory reference implementation of [`MailBackend`] (and, behind their
//! own feature flags, `MdnBackend` and `SieveBackend`).
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`MailBackend`] trait to study when writing a
//!    real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Mail dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of RFC 8621 edge cases are simplified (see source comments).
//!
//! # Feature flag and API stability
//!
//! This module is gated behind `feature = "memory"` and is **not** enabled
//! by default. Its public API stability is opt-in: it may break across
//! minor versions while the crate is pre-1.0.
//!
//! # MDN and Sieve extensions
//!
//! When the parent crate is built with `feature = "mdn"`, `MemoryBackend`
//! also implements `MdnBackend`. Likewise for `"sieve"` and `SieveBackend`.
//! The implementations live in this module behind matching
//! `#[cfg(feature = ...)]` gates.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use jmap_mail_server::{memory::MemoryBackend, register_mail_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_mail_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
//! ```
//!
//! # Seed data
//!
//! [`seed::setup_seed_data`](crate::memory::seed::setup_seed_data) populates
//! a `MemoryBackend` with a fixed set of mailboxes and emails derived from
//! the JMAP test suite seed-data spec. Timestamps are deterministic
//! (relative to `2026-01-01T00:00:00Z`), so seeded fixtures are suitable
//! for sort/filter assertions.
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
//! and JMAP-hwdv.1 (this crate, canonical). Sibling extension-server
//! crates mirror this layout.

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

pub mod seed;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

// MIME body parsing (jmap-mime + mime-tree) — used in import_email and parse_email.
use jmap_mime::message_to_jmap_body;

use crate::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, MailBackend, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetError, SetErrorType, SetObject,
};
use jmap_mail_types::{
    ComparatorProperty, Email, EmailAddress, EmailComparator, EmailFilter, EmailFilterCondition,
    EmailHeader, EmailSubmission, EmailSubmissionComparator, EmailSubmissionComparatorProperty,
    EmailSubmissionFilter, EmailSubmissionFilterCondition, Filter, Keyword, Mailbox,
    MailboxComparator, MailboxComparatorProperty, MailboxFilter, MailboxFilterCondition,
    MailboxRole, Operator, SearchSnippet,
};
use jmap_server::{apply_patch_object, limit_or_max, resolve_query_start};
use jmap_types::{Id, State, UTCDate};
use serde::Deserialize;

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
    /// blob_id → raw bytes (used by import_email and parse_email)
    blobs: HashMap<Id, Vec<u8>>,
    /// Optional maxSizeScript limit in bytes for sieve size enforcement tests.
    #[cfg(feature = "sieve")]
    max_sieve_script_limit: Option<u64>,
    /// Optional per-account script-count cap for sieve overQuota tests.
    /// `None` falls through to the trait default of 100; tests that need a
    /// lower cap set this to make overQuota observable without creating 100
    /// scripts.
    #[cfg(feature = "sieve")]
    max_sieve_scripts_limit: Option<usize>,
    /// account_id → (message_id_string → email_id) for duplicate detection in import_email
    message_id_index: HashMap<String, HashMap<String, Id>>,
    /// explicitly registered account ids (accounts may exist with no objects yet)
    known_accounts: HashSet<String>,
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

    /// Re-sort a Thread's `emailIds` by the `receivedAt` of each member email.
    ///
    /// RFC 8621 §3 requires `emailIds` to be sorted oldest-first by `receivedAt`.
    /// Called after every insertion of a new email into an existing thread.
    fn sort_thread_email_ids(&mut self, account_id: &str, thread_id: &Id) {
        // Collect current emailIds from the stored Thread JSON.
        let email_ids: Vec<String> = match self.objects_ref("Thread", account_id) {
            Some(store) => match store.get(thread_id) {
                Some(v) => v["emailIds"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
                            .collect()
                    })
                    .unwrap_or_default(),
                None => return,
            },
            None => return,
        };

        // Nothing to sort for zero or one email.
        if email_ids.len() <= 1 {
            return;
        }

        // Look up receivedAt for each email id from the Email store.
        // bd:JMAP-ic0j.72 — `Email.receivedAt` is wire-typed as `UTCDate`
        // (RFC 8621 §4.1, strict UTC), so parse via the foundation
        // `UTCDate::to_epoch_seconds` helper from `jmap-types` rather
        // than the local offset-tolerant parser. Malformed seeds still
        // sort to epoch 0.
        let mut id_and_date: Vec<(String, i64)> = email_ids
            .into_iter()
            .map(|eid| {
                let epoch = self
                    .objects_ref("Email", account_id)
                    .and_then(|s| s.get(eid.as_str()))
                    .and_then(|v| v["receivedAt"].as_str())
                    .map(utcdate_str_to_epoch_secs)
                    .unwrap_or(0);
                (eid, epoch)
            })
            .collect();

        // Sort ascending by UTC epoch so non-UTC offsets compare correctly.
        id_and_date.sort_by_key(|pair| pair.1);

        // Write the sorted list back to the Thread object.
        if let Some(store) = self.objects.get_mut(&("Thread", account_id.to_owned()))
            && let Some(thread_val) = store.get_mut(thread_id)
            && let Some(arr) = thread_val["emailIds"].as_array_mut()
        {
            *arr = id_and_date
                .into_iter()
                .map(|(eid, _)| serde_json::Value::String(eid))
                .collect();
        }
    }
}

// ---------------------------------------------------------------------------
// IdFate: per-ID fate tracker for RFC 8620 §5.6 deduplication
// ---------------------------------------------------------------------------

/// Per-ID fate tracker for RFC 8620 §5.6 ID deduplication across change log entries.
///
/// Rules across multiple entries in a single /changes window:
/// - created+updated → Created (update does not change that the object is new to the client)
/// - created+destroyed → removed from map (client never knew the object)
/// - updated+destroyed → Destroyed (client must remove it)
/// - updated+updated → Updated (deduplicated)
#[derive(Debug, Clone)]
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// In-memory [`MailBackend`] for integration tests and examples.
///
/// **Known limitation**: the internal change log grows without bound. This is
/// intentional for unit tests (which are short-lived).
///
/// Note: `query_changes` applies filter, sort, max_changes and up_to_id but
/// ignores collapse_threads. Do not write tests that rely on collapse_threads
/// with MemoryBackend.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Construct an empty [`MemoryBackend`] with no accounts, blobs, or
    /// stored objects. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a blob so that [`import_email`](MemoryBackend::import_email) can find it.
    pub fn store_blob(&self, blob_id: &Id, bytes: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.blobs.insert(blob_id.clone(), bytes);
    }

    /// Test-only entry point that registers an account as known even
    /// if it has no objects yet. Production callers must not use this
    /// method; the API stability disclaimer on the `memory` feature
    /// applies doubly here.
    ///
    /// The name is retained (not renamed to `register_account_for_test`)
    /// for backward compatibility with the workspace's existing
    /// integration test suite, which calls this method through the
    /// public API surface. `#[doc(hidden)]` removes it from `cargo doc`
    /// output so downstream consumers do not see it as a documented
    /// part of the surface.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
    }

    /// **Test-only**: set the maximum Sieve script size in bytes for
    /// size-enforcement tests. Production backends should implement
    /// [`SieveBackend::max_sieve_script_bytes`](crate::SieveBackend::max_sieve_script_bytes)
    /// directly on their own backend type — this setter exists so the
    /// reference [`MemoryBackend`] can simulate per-deployment caps
    /// inside integration tests without growing a separate fixture
    /// backend. The `pub` exposure plus the `memory` feature gate's
    /// existing "not production" disclaimer is sufficient; the
    /// `#[doc(hidden)]` keeps this knob out of `cargo doc` so downstream
    /// consumers do not mistake it for a recommended configuration
    /// surface.
    ///
    /// When set, `SieveBackend::max_sieve_script_bytes` returns this
    /// limit, causing `handle_sieve_set` to reject scripts exceeding it
    /// with `tooLarge`.
    #[cfg(feature = "sieve")]
    #[doc(hidden)]
    pub fn set_max_sieve_script_bytes(&self, limit: u64) {
        self.inner.lock().unwrap().max_sieve_script_limit = Some(limit);
    }

    /// **Test-only**: set the maximum per-account Sieve script count for
    /// `overQuota` enforcement tests. Production backends should
    /// implement
    /// [`SieveBackend::max_sieve_scripts_per_account`](crate::SieveBackend::max_sieve_scripts_per_account)
    /// directly on their own backend type — this setter exists so the
    /// reference [`MemoryBackend`] can simulate per-account quota caps
    /// inside integration tests. The `pub` exposure plus the `memory`
    /// feature gate's existing "not production" disclaimer is
    /// sufficient; the `#[doc(hidden)]` keeps this knob out of
    /// `cargo doc` so downstream consumers do not mistake it for a
    /// recommended configuration surface.
    ///
    /// When set, `SieveBackend::max_sieve_scripts_per_account` returns
    /// this limit, causing `handle_sieve_set` to reject the (N+1)th
    /// create with `overQuota` once the account already has N scripts.
    /// When unset, the trait default of 100 applies.
    #[cfg(feature = "sieve")]
    #[doc(hidden)]
    pub fn set_max_sieve_scripts_per_account(&self, limit: usize) {
        self.inner.lock().unwrap().max_sieve_scripts_limit = Some(limit);
    }

    /// Reference impl of `Mailbox/query` filter+sort+paginate (RFC 8621 §2.3).
    ///
    /// Decoupled from the generic `query_objects` pipeline because Mailbox sort
    /// keys (`name`, `sortOrder`) and the three-way `parentId` filter shape are
    /// Mailbox-specific. Called from `query_objects` when `O::TYPE_NAME ==
    /// "Mailbox"`.
    ///
    /// The handler in `mailbox.rs` validates wire-level argument shape
    /// (rejecting unknown filter keys, unknown sort properties, `sortAsTree`,
    /// `filterAsTree`) before reaching this method, so the inputs here are
    /// trusted to be well-formed RFC 8621 §2.3 filter and comparator values.
    #[allow(clippy::too_many_arguments)]
    async fn query_mailboxes(
        &self,
        account_id: &Id,
        filter: Option<&MailboxFilter>,
        sort: &[MailboxComparator],
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let (mut matching, state_n): (Vec<Mailbox>, u64) = {
            let inner = self.inner.lock().unwrap();
            let mailboxes: Vec<Mailbox> = inner
                .objects_ref("Mailbox", account_id.as_ref())
                .map(|map| {
                    map.values()
                        .filter_map(|val| Mailbox::deserialize(val).ok())
                        .filter(|m| {
                            filter.is_none_or(|f| f.matches(&|c| mailbox_matches_condition(m, c)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let state_n = inner.current_state("Mailbox", account_id.as_ref());
            (mailboxes, state_n)
        };

        // Sort by client comparators with id tiebreak for stable pagination.
        // `Other` properties are pre-rejected by the handler; treat any leftover
        // as Equal so we never panic on a comparator slipping through.
        matching.sort_by(|a, b| {
            let mut ord = std::cmp::Ordering::Equal;
            for c in sort {
                if ord != std::cmp::Ordering::Equal {
                    break;
                }
                let cmp = match c.property {
                    MailboxComparatorProperty::Name => a.name.cmp(&b.name),
                    MailboxComparatorProperty::SortOrder => a.sort_order.cmp(&b.sort_order),
                    _ => std::cmp::Ordering::Equal,
                };
                ord = if c.is_ascending { cmp } else { cmp.reverse() };
            }
            if ord == std::cmp::Ordering::Equal {
                a.id.as_ref().cmp(b.id.as_ref())
            } else {
                ord
            }
        });

        let all_ids: Vec<Id> = matching.into_iter().map(|m| m.id).collect();
        let total = all_ids.len();
        // RFC 8620 §5.5 position / anchor / anchorOffset.
        let start = resolve_query_start(&all_ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let ids: Vec<Id> = all_ids[start..]
            .iter()
            .take(limit_or_max(limit))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(total as u64),
            State::from(state_n.to_string()),
            true,
        ))
    }
}

/// Apply one `MailboxFilterCondition` (RFC 8621 §2.3) to a single Mailbox.
///
/// Returns `true` if the mailbox passes every property of the condition.
/// Roles compare by wire string so that `MailboxRole::Other("inbox")` still
/// matches `MailboxRole::Inbox`.
fn mailbox_matches_condition(m: &Mailbox, f: &MailboxFilterCondition) -> bool {
    // parentId and role are three-way: absent (None) = no filter;
    // explicit null (Some(None)) = the mailbox's value must be null;
    // a value (Some(Some(v))) = the mailbox's value must equal v.
    if let Some(want) = &f.parent_id
        && m.parent_id.as_ref() != want.as_ref()
    {
        return false;
    }

    if let Some(ref name_substr) = f.name
        && !m.name.contains(name_substr.as_str())
    {
        return false;
    }

    if let Some(want) = &f.role
        && m.role.as_ref().map(MailboxRole::to_wire_str)
            != want.as_ref().map(MailboxRole::to_wire_str)
    {
        return false;
    }

    if let Some(want_any_role) = f.has_any_role
        && m.role.is_some() != want_any_role
    {
        return false;
    }

    if let Some(want_subscribed) = f.is_subscribed
        && m.is_subscribed != want_subscribed
    {
        return false;
    }

    true
}

// ---------------------------------------------------------------------------
// MemoryError
// ---------------------------------------------------------------------------

/// Opaque storage-layer error returned by [`MemoryBackend`] operations.
///
/// The inner description is a human-readable string intended for
/// diagnostic logging; it is not a stable wire-format identifier.
///
/// # Forward compatibility
///
/// This type is `#[non_exhaustive]` and uses a named-field shape so
/// future revisions can add structured context (error kind, source
/// reference, account id, etc.) without a breaking change. Outside-
/// crate construction goes through [`MemoryError::new`]; outside-crate
/// reads go through [`MemoryError::description`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    /// Construct a [`MemoryError`] from a human-readable description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Human-readable description of the underlying failure.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryBackend error: {}", self.description)
    }
}

impl std::error::Error for MemoryError {}

/// RFC 8620 §5.3 `invalidProperties` for a patch whose result no longer
/// decodes as `type_name`. The property is named only when the patch
/// touches a single top-level property; otherwise the culprit is not known.
fn invalid_patched_properties(
    type_name: &str,
    patch_map: &serde_json::Map<String, serde_json::Value>,
) -> BackendSetError<MemoryError> {
    let mut props: Vec<&str> = patch_map
        .keys()
        .map(|k| k.split_once('/').map_or(k.as_str(), |(top, _)| top))
        .collect();
    props.sort_unstable();
    props.dedup();
    let err = SetError::new(SetErrorType::InvalidProperties).with_description(format!(
        "patched {type_name} violates its type specification"
    ));
    BackendSetError::SetError(match props.as_slice() {
        [prop] => Box::new(err.with_properties([*prop])),
        _ => Box::new(err),
    })
}

// ---------------------------------------------------------------------------
// JmapBackend impl (read-side)
// ---------------------------------------------------------------------------

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.known_accounts.contains(account_id.as_ref()))
    }

    // -----------------------------------------------------------------------
    // get_objects
    // -----------------------------------------------------------------------

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        let inner = self.inner.lock().unwrap();
        let store = match inner.objects_ref(O::TYPE_NAME, account_id.as_ref()) {
            Some(s) => s,
            None => return Ok((vec![], ids.map(|s| s.to_vec()).unwrap_or_default())),
        };

        let mut found = Vec::new();
        let mut not_found = Vec::new();

        if let Some(ids) = ids {
            for id in ids {
                match store.get(id) {
                    Some(val) => {
                        let obj: O = O::deserialize(val).map_err(|e| {
                            MemoryError::new(format!("deserialize {}: {e}", O::TYPE_NAME))
                        })?;
                        found.push(obj);
                    }
                    None => not_found.push(id.clone()),
                }
            }
        } else {
            for val in store.values() {
                let obj: O = O::deserialize(val)
                    .map_err(|e| MemoryError::new(format!("deserialize {}: {e}", O::TYPE_NAME)))?;
                found.push(obj);
            }
        }

        Ok((found, not_found))
    }

    // -----------------------------------------------------------------------
    // get_state
    // -----------------------------------------------------------------------

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
        Ok(State::from(n.to_string()))
    }

    // -----------------------------------------------------------------------
    // get_changes
    // -----------------------------------------------------------------------

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let since: u64 = since_state.as_ref().parse().map_err(|_| {
            BackendChangesError::Other(MemoryError::new(format!(
                "invalid state token: {since_state}"
            )))
        })?;

        // Snapshot relevant change log entries under a brief lock, then release.
        let (relevant, has_more, new_state) = {
            let inner = self.inner.lock().unwrap();
            let log = inner
                .change_log
                .get(&(O::TYPE_NAME, account_id.to_string()))
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            let limit = limit_or_max(max_changes);
            // Binary search for the first entry with new_state > since.
            let start = log.partition_point(|e| e.new_state <= since);
            // Take limit+1 as sentinel to detect has_more.
            let mut entries: Vec<ChangeEntry> = log[start..]
                .iter()
                .take(limit.saturating_add(1))
                .cloned()
                .collect();
            let has_more = entries.len() > limit;
            if has_more {
                entries.pop();
            }
            // If nothing changed, new_state == since_state (client is already up to date).
            let new_state = entries
                .last()
                .map(|e| State::from(e.new_state.to_string()))
                .unwrap_or_else(|| since_state.clone());
            (entries, has_more, new_state)
        };

        // RFC 8620 §5.6 ID deduplication across the window.
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        for entry in &relevant {
            for id in &entry.created {
                fates.insert(id.clone(), IdFate::Created);
            }
            for id in &entry.updated {
                let fate = match fates.get(id) {
                    Some(IdFate::Created) => IdFate::Created,
                    Some(IdFate::Destroyed) => IdFate::Destroyed,
                    _ => IdFate::Updated,
                };
                fates.insert(id.clone(), fate);
            }
            for id in &entry.destroyed {
                match fates.remove(id) {
                    // RFC 8620 §5.2: if an object is created and destroyed within a
                    // single /changes window, it must be omitted from both 'created'
                    // and 'destroyed' lists — the client never knew about it.
                    Some(IdFate::Created) => {} // created+destroyed in window → omit
                    Some(_) | None => {
                        fates.insert(id.clone(), IdFate::Destroyed);
                    }
                }
            }
        }

        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut destroyed = Vec::new();
        for (id, fate) in fates {
            match fate {
                IdFate::Created => created.push(id),
                IdFate::Updated => updated.push(id),
                IdFate::Destroyed => destroyed.push(id),
            }
        }

        Ok(ChangesResult::new(
            created, updated, destroyed, has_more, new_state,
        ))
    }

    // -----------------------------------------------------------------------
    // query_objects
    // -----------------------------------------------------------------------

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        let QueryParams {
            filter,
            sort,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
        // Collect and sort IDs outside the lock for deterministic ordering.
        // For Email and EmailSubmission objects, apply filter conditions in-process
        // using a JSON roundtrip (since O::Filter: Serialize, we can recover the
        // typed filter).
        //
        // O::TYPE_NAME is a const so the dispatch below is zero-cost at runtime.
        // Trait-based dispatch would require additional trait machinery not yet
        // worth the complexity. Each arm is an explicit case.

        // Mailbox dispatch (RFC 8621 §2.3).
        //
        // Decode the wire filter and sort into typed values via a JSON roundtrip
        // (`O::Filter: Serialize`, `O::Comparator: Serialize`) and dispatch to a
        // self-contained Mailbox handler. Mailbox filter/sort surfaces are small
        // enough that the dispatch is cleaner than retrofitting the Email
        // pipeline below.
        if O::TYPE_NAME == "Mailbox" {
            // Type-identity roundtrip: when O::TYPE_NAME == "Mailbox" the
            // generic O::Filter is necessarily MailboxFilter (see
            // the QueryObject impl in jmap-mail-types src/backend.rs), so both halves
            // of this serde roundtrip are infallible:
            //   - to_value(&f) cannot fail: derive(Serialize) on plain data.
            //   - from_value::<MailboxFilter>(v) cannot fail: v was
            //     just produced by Serialize on the same concrete type.
            // Pattern G policy (bc79c70) requires .expect() rather than
            // silent .ok() fallback, so a future custom-serde change here
            // surfaces as a panic instead of silently dropping the filter
            // (which would return ALL mailboxes when the client expected
            // a filtered subset).
            let mailbox_filter: Option<MailboxFilter> = filter.map(|f| {
                let v =
                    serde_json::to_value(f).expect("derive(Serialize) on plain data is infallible");
                serde_json::from_value(v).expect(
                    "type-identity roundtrip on MailboxFilter is infallible: \
                     the JSON came from Serialize on the same concrete type",
                )
            });
            // Type-identity roundtrip: when O::TYPE_NAME == "Mailbox" the
            // generic O::Comparator is necessarily MailboxComparator. Same
            // Pattern G rationale as the filter roundtrip above.
            let mailbox_sort: Vec<MailboxComparator> = sort
                .map(|s| {
                    let v = serde_json::to_value(s)
                        .expect("derive(Serialize) on plain data is infallible");
                    serde_json::from_value(v).expect(
                        "type-identity roundtrip on Vec<MailboxComparator> is infallible: \
                         the JSON came from Serialize on the same concrete type",
                    )
                })
                .unwrap_or_default();
            return self
                .query_mailboxes(
                    account_id,
                    mailbox_filter.as_ref(),
                    &mailbox_sort,
                    limit,
                    position,
                    anchor,
                    anchor_offset,
                )
                .await;
        }

        // Type-identity roundtrip: when O::TYPE_NAME == "Email" the generic
        // O::Filter is necessarily EmailFilter, so both halves of this serde
        // roundtrip are infallible. See the Mailbox arm above for the full
        // rationale and the Pattern G (bc79c70) policy reference.
        let email_filter: Option<EmailFilter> = if O::TYPE_NAME == "Email" {
            filter.map(|f| {
                let v =
                    serde_json::to_value(f).expect("derive(Serialize) on plain data is infallible");
                serde_json::from_value(v).expect(
                    "type-identity roundtrip on EmailFilter is infallible: \
                     the JSON came from Serialize on the same concrete type",
                )
            })
        } else {
            None
        };

        // Decode EmailComparator list for Email queries (JSON roundtrip via
        // O::Comparator). Type-identity roundtrip: when O::TYPE_NAME == "Email"
        // the generic O::Comparator is necessarily EmailComparator, so both
        // halves of the roundtrip are infallible. Same Pattern G rationale
        // as the filter roundtrips above.
        let email_sort: Option<Vec<EmailComparator>> = if O::TYPE_NAME == "Email" {
            sort.map(|s| {
                let v =
                    serde_json::to_value(s).expect("derive(Serialize) on plain data is infallible");
                serde_json::from_value(v).expect(
                    "type-identity roundtrip on Vec<EmailComparator> is infallible: \
                     the JSON came from Serialize on the same concrete type",
                )
            })
        } else {
            None
        };

        // Type-identity roundtrip: when O::TYPE_NAME == "EmailSubmission" the
        // generic O::Filter is necessarily EmailSubmissionFilter, so both
        // halves of this serde roundtrip are infallible. Same Pattern G
        // rationale as the Mailbox arm above.
        let submission_filter: Option<EmailSubmissionFilter> = if O::TYPE_NAME == "EmailSubmission"
        {
            filter.map(|f| {
                let v =
                    serde_json::to_value(f).expect("derive(Serialize) on plain data is infallible");
                serde_json::from_value(v).expect(
                    "type-identity roundtrip on EmailSubmissionFilter is infallible: \
                     the JSON came from Serialize on the same concrete type",
                )
            })
        } else {
            None
        };

        // Type-identity roundtrip: when O::TYPE_NAME == "EmailSubmission" the
        // generic O::Comparator is necessarily EmailSubmissionComparator. Same
        // Pattern G rationale as the filter roundtrips above.
        let submission_sort: Option<Vec<EmailSubmissionComparator>> =
            if O::TYPE_NAME == "EmailSubmission" {
                sort.map(|s| {
                    let v = serde_json::to_value(s)
                        .expect("derive(Serialize) on plain data is infallible");
                    serde_json::from_value(v).expect(
                        "type-identity roundtrip on Vec<EmailSubmissionComparator> is \
                         infallible: the JSON came from Serialize on the same concrete type",
                    )
                })
            } else {
                None
            };

        // Pre-build the inMailboxOtherThan exclusion set once for top-level Condition
        // filters to avoid O(N×k) HashSet allocations inside the per-email loop.
        let top_level_excluded_set: Option<std::collections::HashSet<&Id>> =
            email_filter.as_ref().and_then(|ef| {
                if let Filter::Condition(cond) = ef {
                    cond.in_mailbox_other_than
                        .as_ref()
                        .map(|v| v.iter().collect())
                } else {
                    None
                }
            });

        // Collect (id, receivedAt) pairs so we can sort by receivedAt when requested.
        let (mut id_date_pairs, state_n) = {
            let inner = self.inner.lock().unwrap();
            let pairs: Vec<(Id, String)> = if let Some(ref ef) = email_filter {
                // Apply email filter: deserialize each stored object and check.
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|map| {
                        map.iter()
                            .filter_map(|(id, val)| {
                                // Deserialize the stored JSON back into a typed Email.
                                // In a well-formed reference-impl backend this never
                                // fails: the value came from a typed Email written by
                                // create_object. A failure here would mean the stored
                                // object's JSON shape drifted from the current Email
                                // type (e.g. fixture corruption, manual JSON injection,
                                // or a type-evolution forward-compat gap).
                                //
                                // The reference impl logs and skips. A production
                                // backend that hits this branch should propagate the
                                // error via its own MemoryError-equivalent type
                                // (workspace AGENTS.md "library-kit posture": consumers
                                // bring the persistence + error reporting machinery).
                                // The debug_assert surfaces fixture/type drift in CI
                                // without changing release-build behaviour.
                                let email = match Email::deserialize(val) {
                                    Ok(e) => e,
                                    Err(_e) => {
                                        debug_assert!(
                                            false,
                                            "MemoryBackend: stored Email {id} failed to \
                                             deserialize (fixture drift, type evolution, \
                                             or manual JSON injection): {_e}"
                                        );
                                        return None;
                                    }
                                };
                                if email_matches_filter(&email, ef, top_level_excluded_set.as_ref())
                                {
                                    let received = val
                                        .get("receivedAt")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_owned();
                                    Some((id.clone(), received))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else if let Some(ref sf) = submission_filter {
                // Pre-build per-condition sets once before the per-submission loop so
                // that HashSet allocation is O(1) per filter, not O(N) per submission.
                let top_level_sub_sets: Option<SubmissionConditionSets<'_>> =
                    if let Filter::Condition(cond) = sf {
                        Some(SubmissionConditionSets::from_condition(cond))
                    } else {
                        None
                    };
                // Apply submission filter: deserialize each stored object and check.
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|map| {
                        map.iter()
                            .filter_map(|(id, val)| {
                                let sub: EmailSubmission =
                                    EmailSubmission::deserialize(val).ok()?;
                                let matches = match (sf, &top_level_sub_sets) {
                                    (Filter::Condition(cond), Some(sets)) => {
                                        submission_matches_condition(&sub, cond, sets)
                                    }
                                    _ => submission_matches_filter(&sub, sf),
                                };
                                if matches {
                                    Some((id.clone(), String::new()))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                inner
                    .objects_ref(O::TYPE_NAME, account_id.as_ref())
                    .map(|s| {
                        s.iter()
                            .map(|(id, val)| {
                                let received = val
                                    .get("receivedAt")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_owned();
                                (id.clone(), received)
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let mut pairs = pairs;
            if let (Some(ss), Some(map)) = (
                submission_sort.as_deref(),
                inner.objects_ref(O::TYPE_NAME, account_id.as_ref()),
            ) {
                sort_submission_pairs(&mut pairs, map, ss);
            }
            let state_n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
            (pairs, state_n)
        };

        // Apply sort. When a receivedAt comparator is present, sort by epoch seconds
        // so that sub-second timestamps (e.g. "T00:00:00.123Z") sort correctly relative
        // to whole-second timestamps (e.g. "T00:00:00Z") — lexicographic order is wrong
        // because '.' (0x2E) < 'Z' (0x5A).  Ties broken by id string for stable ordering.
        let received_at_sort = email_sort.as_deref().and_then(|s| {
            s.iter()
                .find(|c| c.property == ComparatorProperty::ReceivedAt)
        });
        if let Some(cmp) = received_at_sort {
            let ascending = cmp.is_ascending;
            // bd:JMAP-ic0j.72 — `date_a` / `date_b` were extracted from
            // the typed `Email.receivedAt` field (RFC 8621 §4.1 strict
            // UTC), so parse via the foundation `UTCDate::to_epoch_seconds`
            // helper rather than the local offset-tolerant parser.
            id_date_pairs.sort_by(|(id_a, date_a), (id_b, date_b)| {
                let epoch_a = utcdate_str_to_epoch_secs(date_a);
                let epoch_b = utcdate_str_to_epoch_secs(date_b);
                let ord = epoch_a.cmp(&epoch_b);
                let ord = if ascending { ord } else { ord.reverse() };
                ord.then_with(|| id_a.as_ref().cmp(id_b.as_ref()))
            });
        } else if submission_sort.is_none() {
            id_date_pairs.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));
        }
        let all_ids: Vec<Id> = id_date_pairs.into_iter().map(|(id, _)| id).collect();

        let total = all_ids.len();
        // RFC 8620 §5.5 position / anchor / anchorOffset.
        let start = resolve_query_start(&all_ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;

        let ids: Vec<Id> = all_ids[start..]
            .iter()
            .take(limit_or_max(limit))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(total as u64),
            State::from(state_n.to_string()),
            true,
        ))
    }

    // -----------------------------------------------------------------------
    // query_changes
    // -----------------------------------------------------------------------

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let QueryChangesParams {
            since_query_state,
            filter,
            sort,
            max_changes,
            up_to_id,
            ..
        } = params;
        // Step 1: Validate since_query_state by parsing it as a u64 counter.
        // An unparsable token means the client supplied a state we never issued;
        // return cannotCalculateChanges (limit=0) per RFC 8620 §5.6.
        let _since: u64 = since_query_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        // Step 2: Get the raw delta (created/updated/destroyed) since the given state.
        let changes = self
            .get_changes::<O>(_caller, account_id, since_query_state, None)
            .await?;
        let new_query_state = changes.new_state.clone();

        // Step 3: Get the current filtered+sorted result list (no pagination).
        let query_result = self
            .query_objects::<O>(
                _caller,
                account_id,
                QueryParams::new().filter(filter).sort(sort),
            )
            .await
            .map_err(|e| match e {
                BackendQueryError::Other(e) => BackendChangesError::Other(e),
                // Changes to a query the server cannot run cannot be
                // calculated; the client's follow-up /query receives the
                // method error itself.
                _ => BackendChangesError::CannotCalculate,
            })?;
        let current_result: Vec<Id> = query_result.ids;

        // Step 4: Build lookup sets.
        use std::collections::HashSet;
        let created_set: HashSet<&Id> = changes.created.iter().collect();
        let updated_set: HashSet<&Id> = changes.updated.iter().collect();

        // Step 5: Compute removed — every destroyed or updated ID. RFC 8620
        // §5.6: when the filter or sort includes a mutable property "the
        // server MUST include all Foos in the current results for which this
        // property may have changed", and `added` then reinserts them. Every
        // update is treated as possibly touching a filter/sort property, so an
        // updated ID still in the results appears in both arrays.
        let mut removed: Vec<Id> = changes
            .destroyed
            .iter()
            .chain(changes.updated.iter())
            .cloned()
            .collect();
        // Deduplicate removed (an id could appear in both destroyed and updated in theory).
        removed.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        removed.dedup();

        // Step 6: Compute added — IDs in created ∪ updated that are in the current result.
        // We iterate current_result (already sorted) to get correct positional indices.
        // Determine the up_to_id cutoff position in current_result (exclusive upper bound).
        let up_to_pos: Option<usize> =
            up_to_id.and_then(|target| current_result.iter().position(|id| id == target));

        let mut added: Vec<AddedItem> = Vec::new();
        for (pos, id) in current_result.iter().enumerate() {
            // If up_to_id is set, stop before reaching (and including) its position.
            if let Some(cutoff) = up_to_pos
                && pos >= cutoff
            {
                break;
            }
            if created_set.contains(id) || updated_set.contains(id) {
                added.push(AddedItem::new(id.clone(), pos as u64));
            }
        }

        // Step 7: Apply max_changes — if total changes exceed the limit, return
        // cannotCalculateChanges (limit=0) per RFC 8620 §5.6.
        if let Some(max) = max_changes {
            let total_changes = removed.len() as u64 + added.len() as u64;
            if total_changes > max {
                return Err(BackendChangesError::CannotCalculate);
            }
        }

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            new_query_state,
            None,
            removed,
            added,
        ))
    }
}

// ---------------------------------------------------------------------------
// MailBackend impl (write-side and mail-specific)
// ---------------------------------------------------------------------------

impl MailBackend for MemoryBackend {
    // -----------------------------------------------------------------------
    // create_object
    // -----------------------------------------------------------------------

    async fn create_object<O, F>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        // RFC 8621 §8: the VacationResponse "MUST have the id \"singleton\"".
        let id = if O::TYPE_NAME == "VacationResponse" {
            Id::from("singleton")
        } else {
            Id::from(uuid::Uuid::new_v4().to_string())
        };
        let mut val = serde_json::to_value(build(&id))
            .map_err(|e| BackendSetError::Other(MemoryError::new(format!("serialize: {e}"))))?;
        // Replace placeholder blobId with a server-assigned UUID. The Email/set
        // create handler sets blobId to [`crate::helpers::PLACEHOLDER_BLOB_ID`]
        // because it has no raw bytes to hash; the backend is responsible for
        // assigning the real value. MemoryBackend uses a UUID since it does
        // not store raw blobs on this path. Real backends should store the
        // blob and use a content hash here.
        if val.get("blobId").and_then(|v| v.as_str()) == Some(crate::helpers::PLACEHOLDER_BLOB_ID)
            && let serde_json::Value::Object(ref mut map) = val
        {
            let blob_uuid = Id::from(uuid::Uuid::new_v4().to_string());
            map.insert(
                "blobId".to_owned(),
                serde_json::Value::String(blob_uuid.to_string()),
            );
        }
        // Update size from the serialized JSON length. email.rs sets size=0 as a
        // placeholder (it has no raw bytes on the Email/set create path); the backend
        // is responsible for assigning the real value. MemoryBackend uses the
        // serialized-JSON byte length as a proxy — non-zero and stable within a test.
        if val.get("size").and_then(|v| v.as_u64()) == Some(0)
            && let serde_json::Value::Object(ref mut map) = val
        {
            let json_size = serde_json::to_vec(&serde_json::Value::Object(map.clone()))
                .map(|b| b.len() as u64)
                .unwrap_or(1);
            map.insert(
                "size".to_owned(),
                serde_json::Value::Number(json_size.into()),
            );
        }
        let created_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("deserialize after create: {e}")))
        })?;

        let mut inner = self.inner.lock().unwrap();
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), val);
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME, account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![id.clone()],
                updated: vec![],
                destroyed: vec![],
            });

        Ok((id, created_obj))
    }

    // -----------------------------------------------------------------------
    // update_object
    // -----------------------------------------------------------------------

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        let patch_val: serde_json::Value = serde_json::to_value(patch)
            .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?;

        let mut inner = self.inner.lock().unwrap();
        let store = inner.objects_mut(O::TYPE_NAME, account_id.as_ref());
        let existing = store.get_mut(id).ok_or_else(|| {
            BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
        })?;

        // JMAP patch (RFC 8620 §5.3): keys are JSON Pointer paths (e.g.
        // "mailboxIds/abc123"), applied all-or-nothing by the shared
        // `apply_patch_object`, so an invalidPatch leaves the stored object
        // unchanged.
        if let serde_json::Value::Object(base) = existing
            && let serde_json::Value::Object(patch_map) = patch_val
        {
            // RFC 8621 §4.1.1 defines Email `keywords` with "default: {}",
            // but the stored JSON omits it when empty, so materialize the
            // default for `keywords/<keyword>` pointers to find their parent.
            // Work on a copy so a rejected patch leaves storage untouched.
            let mut patched = base.clone();
            if O::TYPE_NAME == Email::TYPE_NAME {
                patched
                    .entry("keywords")
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            }
            apply_patch_object(&mut patched, &patch_map).map_err(|e| {
                BackendSetError::SetError(Box::new(
                    SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
                ))
            })?;
            // A null or wrong-typed value on a required property must
            // not reach storage, or every later /get fails to decode it.
            let patched = serde_json::Value::Object(patched);
            if O::deserialize(&patched).is_err() {
                return Err(invalid_patched_properties(O::TYPE_NAME, &patch_map));
            }
            *existing = patched;
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME, account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![id.clone()],
                destroyed: vec![],
            });

        Ok(None) // MemoryBackend does not echo server-modified fields
    }

    // -----------------------------------------------------------------------
    // destroy_object
    // -----------------------------------------------------------------------

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        let store = inner.objects_mut(O::TYPE_NAME, account_id.as_ref());
        store.remove(id).ok_or_else(|| {
            BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
        })?;
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME, account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![],
                destroyed: vec![id.clone()],
            });
        Ok(())
    }

    // -----------------------------------------------------------------------
    // import_email
    // -----------------------------------------------------------------------

    async fn import_email(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
        received_at: Option<&UTCDate>,
    ) -> Result<(Id, Email), BackendSetError<Self::Error>> {
        let bytes = {
            let inner = self.inner.lock().unwrap();
            inner.blobs.get(blob_id).cloned().ok_or_else(|| {
                BackendSetError::SetError(Box::new(SetError::new(SetErrorType::BlobNotFound)))
            })?
        };

        // Parse message headers from raw bytes (best-effort RFC 5322 parsing).
        let parsed = parse_rfc5322_headers(&bytes);

        // Build the Email object outside the lock (uses only local data).
        let email_id = Id::from(uuid::Uuid::new_v4().to_string());
        let mailbox_map: BTreeMap<Id, bool> =
            mailbox_ids.iter().map(|id| (id.clone(), true)).collect();
        let kw_map: BTreeMap<Keyword, bool> = keywords.iter().map(|k| (k.clone(), true)).collect();

        let received = received_at
            .cloned()
            .unwrap_or_else(|| UTCDate::from("1970-01-01T00:00:00Z"));

        // thread_id is a placeholder; set below after the lock is acquired.
        // We must build a placeholder email first so we can serialize it, then
        // patch in the real thread_id inside the lock.
        // Actually: build everything except thread_id, then acquire one lock for
        // duplicate check + thread assignment + insert (no TOCTOU window).
        let email_size = bytes.len() as u64;

        // Acquire a single lock that covers the duplicate check, thread assignment,
        // and the actual insert — eliminating the TOCTOU race window. A split lock
        // would allow two concurrent imports of the same Message-ID to both pass
        // the duplicate check before either inserts, resulting in duplicates.
        let (email, email_id) = {
            let mut inner = self.inner.lock().unwrap();

            // Check for duplicate Message-ID (RFC 8621 §4.8).
            if let Some(msg_ids) = &parsed.message_id
                && let Some(index) = inner.message_id_index.get(account_id.as_ref())
            {
                for msg_id in msg_ids {
                    if let Some(existing_id) = index.get(msg_id) {
                        return Err(BackendSetError::SetError(Box::new(
                            SetError::new(SetErrorType::AlreadyExists)
                                .with_existing_id(existing_id.clone()),
                        )));
                    }
                }
            }

            // Assign thread: look for existing email with matching message-id.
            let thread_id =
                assign_thread_inner(&inner, account_id, &parsed.in_reply_to, &parsed.references);

            // Build the full Email object now that we have the real thread_id.
            let mut email = Email::new(
                email_id.clone(),
                blob_id.clone(),
                thread_id.clone(),
                mailbox_map,
                email_size,
                received,
            );
            email.keywords = kw_map;
            email.subject = parsed.subject;
            email.message_id = parsed.message_id;
            email.in_reply_to = (!parsed.in_reply_to.is_empty()).then_some(parsed.in_reply_to);
            email.references = (!parsed.references.is_empty()).then_some(parsed.references);
            email.from = parsed.from;
            email.to = parsed.to;
            email.cc = parsed.cc;
            email.headers = parsed.raw_headers;
            if let Some(preview) = parsed.preview {
                email.preview = Some(preview);
            }

            // Populate body structure fields using the MIME parser.
            if let Ok(parsed_msg) = mime_tree::parse(&bytes) {
                let part_counter = std::cell::Cell::new(0usize);
                let blob_id_str = blob_id.to_string();
                let body_fields = message_to_jmap_body(&parsed_msg, |_part| {
                    let i = part_counter.get();
                    part_counter.set(i + 1);
                    jmap_types::Id::from(format!("{blob_id_str}-part-{i}"))
                });
                email.text_body = body_fields.text_body;
                email.html_body = body_fields.html_body;
                email.attachments = body_fields.attachments.clone();
                email.body_structure = Some(body_fields.body_structure);
                email.has_attachment = !body_fields.attachments.is_empty();
                if email.preview.is_none() {
                    email.preview = body_fields.preview;
                }
            }

            // Ensure the Thread object exists.
            let thread_val = serde_json::json!({
                "id": thread_id.to_string(),
                "emailIds": [email_id.to_string()]
            });

            // Serialize the email for storage.
            let email_val = serde_json::to_value(&email).map_err(|e| {
                BackendSetError::Other(MemoryError::new(format!("serialize email: {e}")))
            })?;

            // Insert or update Thread (append email_id if thread exists).
            let thread_store = inner.objects_mut("Thread", account_id.as_ref());
            let thread_existed = thread_store.contains_key(&thread_id);
            thread_store
                .entry(thread_id.clone())
                .and_modify(|v| {
                    if let Some(arr) = v.get_mut("emailIds").and_then(|a| a.as_array_mut()) {
                        arr.push(serde_json::Value::String(email_id.to_string()));
                    }
                })
                .or_insert(thread_val);

            // Insert Email.
            inner
                .objects_mut("Email", account_id.as_ref())
                .insert(email_id.clone(), email_val);

            // Re-sort the thread's emailIds by receivedAt ascending (RFC 8621 §3).
            // Only needed when joining an existing thread; new threads have one element.
            if thread_existed {
                inner.sort_thread_email_ids(account_id.as_ref(), &thread_id);
            }

            // Update Mailbox aggregate counts (RFC 8621 §2).
            let is_unread = !keywords.iter().any(|k| k.as_ref() == "$seen");
            for mbox_id in mailbox_ids {
                if let Some(v) = inner
                    .objects_mut("Mailbox", account_id.as_ref())
                    .get_mut(mbox_id)
                    && let Some(obj) = v.as_object_mut()
                {
                    let te = obj.get("totalEmails").and_then(|x| x.as_u64()).unwrap_or(0);
                    obj.insert("totalEmails".to_owned(), (te + 1).into());
                    if is_unread {
                        let ue = obj
                            .get("unreadEmails")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0);
                        obj.insert("unreadEmails".to_owned(), (ue + 1).into());
                    }
                    if !thread_existed {
                        let tt = obj
                            .get("totalThreads")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0);
                        obj.insert("totalThreads".to_owned(), (tt + 1).into());
                        if is_unread {
                            let ut = obj
                                .get("unreadThreads")
                                .and_then(|x| x.as_u64())
                                .unwrap_or(0);
                            obj.insert("unreadThreads".to_owned(), (ut + 1).into());
                        }
                    }
                }
            }

            // Update Message-ID index for future duplicate detection.
            if let Some(msg_ids) = &email.message_id {
                let account_index = inner
                    .message_id_index
                    .entry(account_id.to_string())
                    .or_default();
                for msg_id in msg_ids {
                    account_index.insert(msg_id.clone(), email_id.clone());
                }
            }

            // Bump state for both Email and Thread.
            let new_email_state = inner.bump_state("Email", account_id.as_ref());
            inner
                .change_log
                .entry(("Email", account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_email_state,
                    created: vec![email_id.clone()],
                    updated: vec![],
                    destroyed: vec![],
                });
            let new_thread_state = inner.bump_state("Thread", account_id.as_ref());
            inner
                .change_log
                .entry(("Thread", account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_thread_state,
                    created: if thread_existed {
                        vec![]
                    } else {
                        vec![thread_id.clone()]
                    },
                    updated: if thread_existed {
                        vec![thread_id]
                    } else {
                        vec![]
                    },
                    destroyed: vec![],
                });

            (email, email_id)
        };

        Ok((email_id, email))
    }

    // -----------------------------------------------------------------------
    // find_thread_by_message_ids
    // -----------------------------------------------------------------------

    async fn find_thread_by_message_ids(
        &self,
        _caller: &(),
        account_id: &Id,
        message_ids: &[&str],
    ) -> Result<Option<Id>, Self::Error> {
        if message_ids.is_empty() {
            return Ok(None);
        }
        let inner = self.inner.lock().unwrap();
        let store = match inner.objects_ref("Email", account_id.as_ref()) {
            Some(s) => s,
            None => return Ok(None),
        };
        for val in store.values() {
            if let Some(ids) = val.get("messageId").and_then(|v| v.as_array()) {
                for id in ids {
                    if let Some(s) = id.as_str()
                        && message_ids.contains(&s)
                        && let Some(tid) = val.get("threadId").and_then(|v| v.as_str())
                    {
                        return Ok(Some(Id::from(tid)));
                    }
                }
            }
        }
        Ok(None)
    }

    // -----------------------------------------------------------------------
    // blob_exists / parse_email
    // -----------------------------------------------------------------------

    async fn blob_exists(
        &self,
        _caller: &(),
        _account_id: &Id,
        blob_id: &Id,
    ) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.blobs.contains_key(blob_id))
    }

    async fn parse_email(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Email, Self::Error> {
        let bytes = {
            let inner = self.inner.lock().unwrap();
            inner
                .blobs
                .get(blob_id)
                .cloned()
                .ok_or_else(|| MemoryError::new(format!("blob not found: {blob_id}")))?
        };

        let parsed = parse_rfc5322_headers(&bytes);

        // parse_email does not store — use a synthetic id.
        let email_id = Id::from(format!("parse-{blob_id}"));
        // Assign a thread id based on account state but do not store the thread.
        let thread_id = {
            let inner = self.inner.lock().unwrap();
            assign_thread_inner(&inner, account_id, &parsed.in_reply_to, &parsed.references)
        };

        let mailbox_map = BTreeMap::new();
        let received = UTCDate::from("1970-01-01T00:00:00Z");
        let mut email = Email::new(
            email_id,
            blob_id.clone(),
            thread_id,
            mailbox_map,
            bytes.len() as u64,
            received,
        );
        email.subject = parsed.subject;
        email.message_id = parsed.message_id;
        email.in_reply_to = (!parsed.in_reply_to.is_empty()).then_some(parsed.in_reply_to);
        email.references = (!parsed.references.is_empty()).then_some(parsed.references);
        email.from = parsed.from;
        email.to = parsed.to;
        email.cc = parsed.cc;
        email.headers = parsed.raw_headers;
        if let Some(preview) = parsed.preview {
            email.preview = Some(preview);
        }

        // Populate body structure fields using the MIME parser.
        if let Ok(parsed_msg) = mime_tree::parse(&bytes) {
            let part_counter = std::cell::Cell::new(0usize);
            let blob_id_str = blob_id.to_string();
            let body_fields = message_to_jmap_body(&parsed_msg, |_part| {
                let i = part_counter.get();
                part_counter.set(i + 1);
                jmap_types::Id::from(format!("{blob_id_str}-part-{i}"))
            });
            email.text_body = body_fields.text_body;
            email.html_body = body_fields.html_body;
            email.attachments = body_fields.attachments.clone();
            email.body_structure = Some(body_fields.body_structure);
            email.has_attachment = !body_fields.attachments.is_empty();
            if email.preview.is_none() {
                email.preview = body_fields.preview;
            }
        }

        Ok(email)
    }

    // -----------------------------------------------------------------------
    // copy_object
    // -----------------------------------------------------------------------

    /// Blob ids are shared across accounts in this backend, so a copied
    /// Email keeps its `blobId`. A copied Email also joins its `threadId`
    /// in the destination, creating the Thread when it is new.
    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let new_id = Id::from(uuid::Uuid::new_v4().to_string());
        let mut val = serde_json::to_value(&obj).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("serialize copied object: {e}")))
        })?;
        if let serde_json::Value::Object(map) = &mut val {
            map.insert(
                "id".to_owned(),
                serde_json::Value::String(new_id.to_string()),
            );
        }
        let stored: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("deserialize copied object: {e}")))
        })?;
        let thread_id = (O::TYPE_NAME == Email::TYPE_NAME)
            .then(|| val.get("threadId").and_then(|v| v.as_str()).map(Id::from))
            .flatten();

        let mut inner = self.inner.lock().unwrap();
        inner
            .objects_mut(O::TYPE_NAME, to_account_id.as_ref())
            .insert(new_id.clone(), val);
        let new_state = inner.bump_state(O::TYPE_NAME, to_account_id.as_ref());
        inner
            .change_log
            .entry((O::TYPE_NAME, to_account_id.to_string()))
            .or_default()
            .push(ChangeEntry {
                new_state,
                created: vec![new_id.clone()],
                updated: vec![],
                destroyed: vec![],
            });

        if let Some(thread_id) = thread_id {
            let thread_existed = inner
                .objects_ref("Thread", to_account_id.as_ref())
                .is_some_and(|s| s.contains_key(&thread_id));
            inner
                .objects_mut("Thread", to_account_id.as_ref())
                .entry(thread_id.clone())
                .and_modify(|v| {
                    if let Some(arr) = v.get_mut("emailIds").and_then(|a| a.as_array_mut()) {
                        arr.push(serde_json::Value::String(new_id.to_string()));
                    }
                })
                .or_insert_with(|| {
                    serde_json::json!({ "id": thread_id.to_string(), "emailIds": [new_id.to_string()] })
                });
            // Re-sort the thread's emailIds by receivedAt ascending (RFC 8621 §3).
            if thread_existed {
                inner.sort_thread_email_ids(to_account_id.as_ref(), &thread_id);
            }
            let new_thread_state = inner.bump_state("Thread", to_account_id.as_ref());
            let (created, updated) = if thread_existed {
                (vec![], vec![thread_id])
            } else {
                (vec![thread_id], vec![])
            };
            inner
                .change_log
                .entry(("Thread", to_account_id.to_string()))
                .or_default()
                .push(ChangeEntry {
                    new_state: new_thread_state,
                    created,
                    updated,
                    destroyed: vec![],
                });
        }

        Ok((new_id, stored))
    }

    // -----------------------------------------------------------------------
    // search_snippets
    // -----------------------------------------------------------------------

    async fn search_snippets(
        &self,
        _caller: &(),
        account_id: &Id,
        email_ids: &[Id],
        filter: Option<&EmailFilterCondition>,
    ) -> Result<Vec<SearchSnippet>, Self::Error> {
        let text_needle = filter.and_then(|f| f.text.as_deref());
        let subject_needle = filter.and_then(|f| f.subject.as_deref());
        let body_needle = filter.and_then(|f| f.body.as_deref());

        let inner = self.inner.lock().unwrap();
        let store = inner.objects_ref("Email", account_id.as_ref());

        let mut snippets = Vec::new();
        for id in email_ids {
            let mut snippet = SearchSnippet::new(id.clone());

            if let Some(store) = store
                && let Some(val) = store.get(id)
            {
                let subject = val.get("subject").and_then(|s| s.as_str()).unwrap_or("");
                let preview = val.get("preview").and_then(|s| s.as_str()).unwrap_or("");

                // Build subject snippet.
                let subj_needle = subject_needle.or(text_needle);
                if let Some(needle) = subj_needle
                    && !subject.is_empty()
                {
                    snippet.subject = Some(highlight(subject, needle));
                }

                // Build preview snippet from preview or body needle.
                let prev_needle = body_needle.or(text_needle);
                if let Some(needle) = prev_needle
                    && !preview.is_empty()
                {
                    snippet.preview = Some(highlight(preview, needle));
                }
            }

            snippets.push(snippet);
        }

        Ok(snippets)
    }

    // -----------------------------------------------------------------------
    // batch_destroy_emails
    // -----------------------------------------------------------------------

    async fn batch_destroy_emails(
        &self,
        _caller: &(),
        account_id: &Id,
        email_ids: &[Id],
    ) -> Vec<(Id, Option<BackendSetError<Self::Error>>)> {
        let mut inner = self.inner.lock().unwrap();
        let account_str = account_id.to_string();
        let mut results = Vec::with_capacity(email_ids.len());
        for id in email_ids {
            let removed = inner
                .objects
                .get_mut(&("Email", account_str.clone()))
                .and_then(|store| store.remove(id))
                .is_some();
            let err = if removed {
                let new_state = inner.bump_state("Email", &account_str);
                inner
                    .change_log
                    .entry(("Email", account_str.clone()))
                    .or_default()
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![id.clone()],
                    });
                None
            } else {
                Some(BackendSetError::SetError(Box::new(SetError::new(
                    SetErrorType::NotFound,
                ))))
            };
            results.push((id.clone(), err));
        }
        results
    }

    // -----------------------------------------------------------------------
    // supports_type
    // -----------------------------------------------------------------------

    fn supports_type<O: JmapObject>(&self, _caller: &(), _account_id: &Id) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Minimal parsed fields from an RFC 5322 message header block.
struct ParsedHeaders {
    subject: Option<String>,
    message_id: Option<Vec<String>>,
    in_reply_to: Vec<String>,
    references: Vec<String>,
    from: Option<Vec<EmailAddress>>,
    to: Option<Vec<EmailAddress>>,
    cc: Option<Vec<EmailAddress>>,
    /// Short preview of the body (first 256 bytes of the text body, if any).
    preview: Option<String>,
    /// Raw header fields in order, for `Email.headers` (RFC 8621 §4.1.3).
    raw_headers: Vec<EmailHeader>,
}

/// Bare-minimum RFC 5322 header parser.
///
/// Reads raw bytes as UTF-8 (lossy), splits on the blank line that separates
/// headers from the body, and extracts the fields needed for threading and
/// snippet generation. Folded header lines (CRLF + whitespace) are unfolded.
///
/// This is intentionally simple — it handles the common cases in tests. A
/// production implementation would use a proper MIME library.
fn parse_rfc5322_headers(bytes: &[u8]) -> ParsedHeaders {
    let text = String::from_utf8_lossy(bytes);

    // Split headers from body at the first blank line.
    let (header_block, body_block) = if let Some(idx) = text.find("\r\n\r\n") {
        (&text[..idx], &text[idx + 4..])
    } else if let Some(idx) = text.find("\n\n") {
        (&text[..idx], &text[idx + 2..])
    } else {
        (text.as_ref(), "")
    };

    // Build raw_headers: fold continuation lines back into the preceding header.
    // A line beginning with whitespace is a continuation of the previous header value.
    let mut raw_headers: Vec<EmailHeader> = Vec::new();
    for line in header_block.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line — append to previous header value.
            if let Some(last) = raw_headers.last_mut() {
                last.value.push('\n');
                last.value.push_str(line);
            }
        } else if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].to_owned();
            let value = line[colon_pos + 1..].to_owned();
            raw_headers.push(EmailHeader::new(name, value));
        }
        // Lines with no colon and no leading whitespace (malformed) are skipped.
    }

    // Unfold header lines for the structured field extraction below.
    let unfolded = header_block
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ")
        .replace("\n ", " ")
        .replace("\n\t", " ");

    let mut subject = None;
    let mut message_id: Option<Vec<String>> = None;
    let mut in_reply_to: Vec<String> = Vec::new();
    let mut references: Vec<String> = Vec::new();
    let mut from_header: Option<String> = None;
    let mut to_header: Option<String> = None;
    let mut cc_header: Option<String> = None;

    for line in unfolded.lines() {
        // RFC 5322 §2.2: header field names are case-insensitive. Split on the
        // first ':' and compare the field name case-insensitively.
        if let Some(colon) = line.find(':') {
            let name = &line[..colon];
            let rest = &line[colon + 1..];
            if name.eq_ignore_ascii_case("Subject") {
                subject = Some(rest.trim().to_owned());
            } else if name.eq_ignore_ascii_case("Message-ID") {
                let ids = parse_msg_ids_via_mime_tree(rest);
                if !ids.is_empty() {
                    message_id = Some(ids);
                }
            } else if name.eq_ignore_ascii_case("In-Reply-To") {
                in_reply_to = parse_msg_ids_via_mime_tree(rest);
            } else if name.eq_ignore_ascii_case("References") {
                references = parse_msg_ids_via_mime_tree(rest);
            } else if name.eq_ignore_ascii_case("From") {
                from_header = Some(rest.trim().to_owned());
            } else if name.eq_ignore_ascii_case("To") {
                to_header = Some(rest.trim().to_owned());
            } else if name.eq_ignore_ascii_case("Cc") {
                cc_header = Some(rest.trim().to_owned());
            }
        }
    }

    let from = from_header.as_deref().map(parse_address_list_via_mime_tree);
    let to = to_header.as_deref().map(parse_address_list_via_mime_tree);
    let cc = cc_header.as_deref().map(parse_address_list_via_mime_tree);

    // Extract a short preview from the body.
    let preview = if body_block.trim().is_empty() {
        None
    } else {
        let trimmed = body_block.trim();
        let end = trimmed
            .char_indices()
            .take(256)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(trimmed.len());
        Some(trimmed[..end].to_owned())
    };

    ParsedHeaders {
        subject,
        message_id,
        in_reply_to,
        references,
        from,
        to,
        cc,
        preview,
        raw_headers,
    }
}

/// Parse `<id>` tokens from a Message-ID / In-Reply-To / References
/// header value via mime-tree.
///
/// This shares the workspace's single RFC 5322 parsing gateway with
/// `email.rs::apply_header_form` so the Email/get `messageId` /
/// `inReplyTo` / `references` properties stay consistent with
/// `header:<name>:asMessageIds` results on identical input
/// (`bd:JMAP-j7pa.10`).
fn parse_msg_ids_via_mime_tree(s: &str) -> Vec<String> {
    match mime_tree::parse_header_typed(mime_tree::HeaderForm::MessageIds, s.as_bytes()) {
        mime_tree::HeaderValueTyped::MessageIds(ids) => ids,
        _ => Vec::new(),
    }
}

/// Parse an RFC 5322 address-list header value (From / To / Cc) via
/// mime-tree.
///
/// Handles RFC 2047 encoded-words, RFC 5322 groups, and quoted-string
/// display names with embedded commas — all cases the previous
/// hand-rolled split-on-comma parser mis-handled. Shares the workspace's
/// single RFC 5322 parsing gateway with `email.rs::apply_header_form` so
/// the Email/get `from` / `to` / `cc` properties stay consistent with
/// `header:<name>:asAddresses` results on identical input
/// (`bd:JMAP-j7pa.10`).
fn parse_address_list_via_mime_tree(s: &str) -> Vec<EmailAddress> {
    match mime_tree::parse_header_typed(mime_tree::HeaderForm::Addresses, s.as_bytes()) {
        mime_tree::HeaderValueTyped::Addresses(list) => list
            .into_iter()
            .map(|addr| {
                let mut ea = EmailAddress::new(addr.address.unwrap_or_default());
                ea.name = addr.name;
                ea
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Assign a thread id for an email being imported or copied.
///
/// Searches existing emails in the account for a `message_id` that matches
/// any of the `in_reply_to` or `references` tokens. If found, reuses that
/// thread id. Otherwise returns a fresh id.
fn assign_thread_inner(
    inner: &Inner,
    account_id: &Id,
    in_reply_to: &[String],
    references: &[String],
) -> Id {
    let refs: Vec<&str> = in_reply_to
        .iter()
        .chain(references.iter())
        .map(|s| s.as_str())
        .collect();

    if !refs.is_empty()
        && let Some(store) = inner.objects_ref("Email", account_id.as_ref())
    {
        for val in store.values() {
            if let Some(msg_ids) = val.get("messageId").and_then(|v| v.as_array()) {
                for msg_id in msg_ids {
                    if let Some(s) = msg_id.as_str()
                        && refs.contains(&s)
                        && let Some(tid) = val.get("threadId").and_then(|v| v.as_str())
                    {
                        return Id::from(tid);
                    }
                }
            }
        }
    }

    Id::from(uuid::Uuid::new_v4().to_string())
}

/// Highlight occurrences of `needle` in `haystack` using `<mark>…</mark>` tags.
///
/// Case-insensitive match. HTML-escapes `&`, `<`, `>` in the surrounding text.
fn highlight(haystack: &str, needle: &str) -> String {
    if needle.is_empty() {
        return html_escape(haystack);
    }
    let lower_needle = needle.to_lowercase();
    let needle_char_count = lower_needle.chars().count();
    // Lowercase the whole haystack once. Because lowercasing can change a char's
    // byte length (e.g. Ω (2 bytes) → ω (2 bytes), but Σ (2 bytes) → σ (2 bytes),
    // and some chars expand), we match positions in lower_haystack and convert them
    // to char counts, then re-locate those char counts in the original haystack.
    let lower_haystack = haystack.to_lowercase();
    let mut result = String::with_capacity(haystack.len() + 32);
    // Byte offsets into lower_haystack and haystack respectively.
    let mut lower_pos = 0usize; // position in lower_haystack
    let mut orig_pos = 0usize; // corresponding byte position in haystack
    // Build a parallel char-index table: lower_char_starts[i] = byte offset of
    // the i-th char in lower_haystack; orig_char_starts[i] = byte offset of
    // the i-th char in haystack.
    //
    // Unicode lowercasing can change a character's byte length, so a byte
    // offset into the lowercased string is not a valid byte offset into the
    // original. The char-index tables map match positions in the lowercased
    // string back to the original correctly regardless of Unicode expansion.
    let lower_chars: Vec<usize> = lower_haystack.char_indices().map(|(i, _)| i).collect();
    let orig_chars: Vec<usize> = haystack.char_indices().map(|(i, _)| i).collect();
    // char_pos tracks which char index lower_pos corresponds to.
    let mut char_pos = 0usize;
    loop {
        match lower_haystack[lower_pos..].find(&lower_needle) {
            None => {
                result.push_str(&html_escape(&haystack[orig_pos..]));
                break;
            }
            Some(rel_lower_idx) => {
                // Byte offset in lower_haystack where the match starts.
                let abs_lower_idx = lower_pos + rel_lower_idx;
                // Count how many lower chars precede the match start from char_pos.
                let chars_before = lower_haystack[lower_pos..abs_lower_idx].chars().count();
                let match_char_start = char_pos + chars_before;
                let match_char_end = match_char_start + needle_char_count;
                // Byte offsets in original haystack.
                let orig_match_start = orig_chars
                    .get(match_char_start)
                    .copied()
                    .unwrap_or(haystack.len());
                let orig_match_end = orig_chars
                    .get(match_char_end)
                    .copied()
                    .unwrap_or(haystack.len());
                result.push_str(&html_escape(&haystack[orig_pos..orig_match_start]));
                result.push_str("<mark>");
                result.push_str(&html_escape(&haystack[orig_match_start..orig_match_end]));
                result.push_str("</mark>");
                // Advance past the match.
                let lower_match_end = lower_chars
                    .get(match_char_end)
                    .copied()
                    .unwrap_or(lower_haystack.len());
                orig_pos = orig_match_end;
                lower_pos = lower_match_end;
                char_pos = match_char_end;
            }
        }
    }
    result
}

/// Parse a strict RFC 8620 §1.4 `UTCDate` string
/// (`YYYY-MM-DDTHH:MM:SSZ`) to seconds since the Unix epoch. Returns `0`
/// on any parse / validation failure (the sort-stable fallback policy
/// also used by [`try_rfc3339_to_epoch_secs`]).
///
/// bd:JMAP-ic0j.72 — Thin wrapper around the foundation
/// [`jmap_types::UTCDate::to_epoch_seconds`] helper. Used by call sites
/// whose JSON-resident `&str` originated as a typed `UTCDate` field
/// (notably `Email.receivedAt` per RFC 8621 §4.1). Does NOT accept
/// `+HH:MM` / `-HH:MM` offsets or fractional seconds — use
/// [`try_rfc3339_to_epoch_secs`] for those (currently only filter
/// `before` / `after` per RFC 8620 §1.4 `Date`).
fn utcdate_str_to_epoch_secs(s: &str) -> i64 {
    UTCDate::new_validated(s)
        .ok()
        .and_then(|d| d.to_epoch_seconds().ok())
        .unwrap_or(0)
}

/// Parse an RFC 3339 / RFC 8620 §1.4 `Date` timestamp string to seconds
/// since the Unix epoch (UTC), returning `None` on any parse failure.
///
/// Handles both `Z` suffix and `+HH:MM` / `-HH:MM` offsets so that
/// timestamps with non-UTC offsets compare correctly by absolute UTC
/// time. Also accepts optional fractional seconds before the offset.
///
/// Limitations (acceptable for test code):
/// - Does not validate calendar date/time fields (e.g. month 13 is accepted).
/// - Does not handle leap seconds.
/// - Year must be in the range 1970–9999.
///
/// # When to use vs `UTCDate::to_epoch_seconds`
///
/// bd:JMAP-ic0j.72 — call sites whose input is typed `UTCDate` (RFC 8620
/// §1.4 strict UTC, the wire shape of `Email.receivedAt`) SHOULD use
/// [`jmap_types::UTCDate::to_epoch_seconds`] directly, or
/// [`utcdate_str_to_epoch_secs`] when the value is a `&str` from JSON.
/// Use this helper only when the input is a `Date` (RFC 8620 §1.4)
/// which permits non-UTC offsets — currently
/// `EmailFilterCondition.before` / `.after`, where the client may
/// legitimately send `+05:30` etc.
///
/// A future `jmap-types::Date::to_epoch_seconds` would let the `Date`
/// sites also migrate to the foundation helper; until then this
/// offset-tolerant parser stays.
fn try_rfc3339_to_epoch_secs(s: &str) -> Option<i64> {
    // Expected format: YYYY-MM-DDTHH:MM:SS[.fff](Z|+HH:MM|-HH:MM)
    // Length with Z offset: 20 chars; with millis+Z: 24 chars; with ±HH:MM: 25 chars.
    let s = s.trim();
    if s.len() < 20 {
        return None;
    }

    let year: i64 = s[0..4].parse().ok()?;
    if s.as_bytes()[4] != b'-' {
        return None;
    }
    let month: i64 = s[5..7].parse().ok()?;
    if s.as_bytes()[7] != b'-' {
        return None;
    }
    let day: i64 = s[8..10].parse().ok()?;
    if !matches!(s.as_bytes()[10], b'T' | b't') {
        return None;
    }
    let hour: i64 = s[11..13].parse().ok()?;
    if s.as_bytes()[13] != b':' {
        return None;
    }
    let minute: i64 = s[14..16].parse().ok()?;
    if s.as_bytes()[16] != b':' {
        return None;
    }
    let second: i64 = s[17..19].parse().ok()?;

    // Skip optional fractional seconds (.NNN or .NNNNNN etc.) before the offset.
    let frac_skip = if s.as_bytes().get(19) == Some(&b'.') {
        let frac_end = s[20..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| 20 + i)
            .unwrap_or(s.len());
        frac_end - 19
    } else {
        0
    };
    let offset_start = 19 + frac_skip;

    let offset_str = &s[offset_start..];
    let offset_secs: i64 = if offset_str.eq_ignore_ascii_case("z") {
        0
    } else if offset_str.len() == 6
        && (offset_str.starts_with('+') || offset_str.starts_with('-'))
        && offset_str.as_bytes()[3] == b':'
    {
        let sign: i64 = if offset_str.starts_with('-') { -1 } else { 1 };
        let oh: i64 = offset_str[1..3].parse().ok()?;
        let om: i64 = offset_str[4..6].parse().ok()?;
        sign * (oh * 3600 + om * 60)
    } else {
        return None;
    };

    // Days-since-epoch calculation using the proleptic Gregorian calendar.
    // Number of days from 1970-01-01 to year-01-01 (ignoring this year's months/days).
    let y = year - 1;
    let leap_days = y / 4 - y / 100 + y / 400;
    // 477 = number of leap days from year 1 to year 1969 inclusive (1969/4 - 1969/100 + 1969/400).
    let days_to_year_start = y * 365 + leap_days - (1969 * 365 + 477);

    // Days within the year up to the start of the month.
    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days_in_year: i64 = 0;
    for (m, days) in MONTH_DAYS.iter().enumerate().take((month - 1) as usize) {
        let extra = if m == 1 && is_leap { 1 } else { 0 };
        days_in_year += days + extra;
    }

    let total_days = days_to_year_start + days_in_year + (day - 1);
    let utc_secs = total_days * 86400 + hour * 3600 + minute * 60 + second - offset_secs;
    Some(utc_secs)
}

/// HTML-escape `&`, `<`, `>`.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// Email filter helpers (used by MemoryBackend::query_objects)
// ---------------------------------------------------------------------------

/// Apply a single `EmailFilterCondition` to an `Email`.
///
/// Only the fields most relevant for integration tests are implemented.
/// Unimplemented fields are silently treated as "no constraint" (always pass),
/// consistent with the note in RFC 8621 §4.4.1 that unspecified fields are
/// ignored.
///
/// `excluded_set` is a pre-built `HashSet` for the `inMailboxOtherThan` check,
/// built once before the per-email loop to avoid O(N×k) allocations.
/// Pass `None` to have the set built on the spot (correct but not optimal).
fn email_matches_condition(
    email: &Email,
    cond: &EmailFilterCondition,
    excluded_set: Option<&std::collections::HashSet<&Id>>,
) -> bool {
    if let Some(ref mbox_id) = cond.in_mailbox
        && !email.mailbox_ids.contains_key(mbox_id)
    {
        return false;
    }
    if let Some(exclude_ids) = &cond.in_mailbox_other_than {
        // Email must be in at least one mailbox NOT in the exclusion list.
        // Use the pre-built set when available; build on demand otherwise.
        let on_demand: std::collections::HashSet<&Id>;
        let set: &std::collections::HashSet<&Id> = if let Some(s) = excluded_set {
            s
        } else {
            on_demand = exclude_ids.iter().collect();
            &on_demand
        };
        let in_other = email.mailbox_ids.keys().any(|id| !set.contains(id));
        if !in_other {
            return false;
        }
    }
    if let Some(ref kw) = cond.has_keyword
        && !email.keywords.contains_key(kw)
    {
        return false;
    }
    if let Some(ref kw) = cond.not_keyword
        && email.keywords.contains_key(kw)
    {
        return false;
    }
    if let Some(want_attach) = cond.has_attachment
        && email.has_attachment != want_attach
    {
        return false;
    }
    if let Some(ref before) = cond.before {
        // receivedAt must be strictly before `before` (epoch-seconds comparison avoids
        // the lexicographic trap where "T00:00:00.123Z" < "T00:00:00Z" despite .123 being later).
        // bd:JMAP-ic0j.72 — `email.received_at` is typed `UTCDate` (RFC 8621 §4.1
        // strict UTC), so use the foundation `UTCDate::to_epoch_seconds`
        // helper from `jmap-types` rather than the local offset-tolerant
        // parser. Filter `before` / `after` remain on the local parser
        // because RFC 8620 §1.4 `Date` (the filter input type) permits
        // non-UTC offsets.
        let recv_epoch = email.received_at.to_epoch_seconds().unwrap_or(0);
        let before_epoch = try_rfc3339_to_epoch_secs(before.as_ref()).unwrap_or(i64::MAX);
        if recv_epoch >= before_epoch {
            return false;
        }
    }
    if let Some(ref after) = cond.after {
        // receivedAt must be strictly after `after`. bd:JMAP-ic0j.72 — see
        // the `before` arm above for the typed-vs-string parser split.
        let recv_epoch = email.received_at.to_epoch_seconds().unwrap_or(0);
        let after_epoch = try_rfc3339_to_epoch_secs(after.as_ref()).unwrap_or(i64::MIN);
        if recv_epoch <= after_epoch {
            return false;
        }
    }
    if let Some(min) = cond.min_size
        && email.size < min
    {
        return false;
    }
    // All specified conditions pass.
    true
}

/// Evaluate a full `EmailFilter` (which may be a logical combination of conditions).
///
/// `excluded_set` is a pre-built `HashSet` for `inMailboxOtherThan` in the
/// top-level condition. Pass `None` for nested conditions.
fn email_matches_filter(
    email: &Email,
    filter: &EmailFilter,
    excluded_set: Option<&std::collections::HashSet<&Id>>,
) -> bool {
    match filter {
        Filter::Condition(cond) => email_matches_condition(email, cond, excluded_set),
        Filter::Operator(op) => match op.operator {
            Operator::And => op
                .conditions
                .iter()
                .all(|f| email_matches_filter(email, f, None)),
            Operator::Or => op
                .conditions
                .iter()
                .any(|f| email_matches_filter(email, f, None)),
            Operator::Not => !op
                .conditions
                .iter()
                .any(|f| email_matches_filter(email, f, None)),
            _ => true, // unknown operator: no constraint
        },
        _ => true, // non_exhaustive: unknown variant, no constraint
    }
}

// ---------------------------------------------------------------------------
// EmailSubmission filter helpers (used by MemoryBackend::query_objects)
// ---------------------------------------------------------------------------

/// Sort EmailSubmission `(id, _)` pairs by the RFC 8621 §7.3 comparators
/// in `sort`, breaking ties by id so the order is deterministic.
///
/// `sentAt` compares the stored `sendAt` instant (RFC 8621 §7). An id with
/// no stored object, or an object that fails to deserialize, compares equal
/// on every comparator and falls through to the id tie-break.
fn sort_submission_pairs(
    pairs: &mut [(Id, String)],
    objects: &HashMap<Id, serde_json::Value>,
    sort: &[EmailSubmissionComparator],
) {
    let subs: HashMap<Id, EmailSubmission> = pairs
        .iter()
        .filter_map(|(id, _)| {
            let sub = EmailSubmission::deserialize(objects.get(id)?).ok()?;
            Some((id.clone(), sub))
        })
        .collect();
    pairs.sort_by(|(id_a, _), (id_b, _)| {
        let by_comparators = match (subs.get(id_a), subs.get(id_b)) {
            (Some(a), Some(b)) => sort
                .iter()
                .map(|c| {
                    let ord = match &c.property {
                        EmailSubmissionComparatorProperty::EmailId => {
                            a.email_id.as_ref().cmp(b.email_id.as_ref())
                        }
                        EmailSubmissionComparatorProperty::ThreadId => {
                            a.thread_id.as_ref().cmp(b.thread_id.as_ref())
                        }
                        EmailSubmissionComparatorProperty::SentAt => {
                            utcdate_str_to_epoch_secs(a.send_at.as_ref())
                                .cmp(&utcdate_str_to_epoch_secs(b.send_at.as_ref()))
                        }
                        _ => std::cmp::Ordering::Equal,
                    };
                    if c.is_ascending { ord } else { ord.reverse() }
                })
                .find(|o| o.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal),
            // An object that fails to deserialize sorts after every one that
            // does, so the comparator stays a total order (slice::sort_by may
            // panic on one that is not).
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        by_comparators.then_with(|| id_a.as_ref().cmp(id_b.as_ref()))
    });
}

/// Pre-built lookup sets for a single `EmailSubmissionFilterCondition`.
///
/// Constructed once per filter (not once per submission) so that the
/// `identityIds`, `emailIds`, and `threadIds` HashSets are not re-allocated
/// on every iteration of the per-submission loop.
struct SubmissionConditionSets<'a> {
    identity_ids: Option<std::collections::HashSet<&'a Id>>,
    email_ids: Option<std::collections::HashSet<&'a Id>>,
    thread_ids: Option<std::collections::HashSet<&'a Id>>,
}

impl<'a> SubmissionConditionSets<'a> {
    fn from_condition(cond: &'a EmailSubmissionFilterCondition) -> Self {
        Self {
            identity_ids: cond.identity_ids.as_ref().map(|v| v.iter().collect()),
            email_ids: cond.email_ids.as_ref().map(|v| v.iter().collect()),
            thread_ids: cond.thread_ids.as_ref().map(|v| v.iter().collect()),
        }
    }
}

/// Apply a single `EmailSubmissionFilterCondition` to an `EmailSubmission`.
///
/// All fields are optional; unset fields are treated as "no constraint" per
/// RFC 8621 §7.3.
///
/// `sets` must be pre-built from the same `cond` via
/// `SubmissionConditionSets::from_condition` before the per-submission loop.
fn submission_matches_condition(
    sub: &EmailSubmission,
    cond: &EmailSubmissionFilterCondition,
    sets: &SubmissionConditionSets<'_>,
) -> bool {
    if let Some(ref id_set) = sets.identity_ids
        && !id_set.contains(&sub.identity_id)
    {
        return false;
    }
    if let Some(ref id_set) = sets.email_ids
        && !id_set.contains(&sub.email_id)
    {
        return false;
    }
    if let Some(ref id_set) = sets.thread_ids
        && !id_set.contains(&sub.thread_id)
    {
        return false;
    }
    if let Some(ref status) = cond.undo_status
        && &sub.undo_status != status
    {
        return false;
    }
    if let Some(ref before) = cond.before {
        // sendAt must be strictly before `before` (lexicographic ISO 8601 comparison).
        if sub.send_at.as_ref() >= before.as_ref() {
            return false;
        }
    }
    if let Some(ref after) = cond.after {
        // sendAt must be on or after `after`.
        if sub.send_at.as_ref() < after.as_ref() {
            return false;
        }
    }
    true
}

/// Evaluate a full `EmailSubmissionFilter` (which may be a logical combination).
///
/// For `Filter::Condition`, the caller is responsible for pre-building
/// `SubmissionConditionSets` before the per-submission loop and passing it here
/// via the inner helper; for operator nodes the sets are built on demand per
/// nested condition (the operator case is uncommon in tests).
fn submission_matches_filter(sub: &EmailSubmission, filter: &EmailSubmissionFilter) -> bool {
    match filter {
        Filter::Condition(cond) => {
            let sets = SubmissionConditionSets::from_condition(cond);
            submission_matches_condition(sub, cond, &sets)
        }
        Filter::Operator(op) => match op.operator {
            Operator::And => op
                .conditions
                .iter()
                .all(|f| submission_matches_filter(sub, f)),
            Operator::Or => op
                .conditions
                .iter()
                .any(|f| submission_matches_filter(sub, f)),
            Operator::Not => !op
                .conditions
                .iter()
                .any(|f| submission_matches_filter(sub, f)),
            _ => true, // unknown operator: no constraint
        },
        _ => true, // non_exhaustive: unknown variant, no constraint
    }
}

// ---------------------------------------------------------------------------
// MdnBackend impl for MemoryBackend (feature = "mdn")
// ---------------------------------------------------------------------------

#[cfg(feature = "mdn")]
use crate::mdn::MdnBackend;
#[cfg(feature = "mdn")]
use crate::mdn::{MdnParseResult, MdnSendResult};
#[cfg(feature = "mdn")]
use jmap_mail_types::Mdn;

#[cfg(feature = "mdn")]
impl MdnBackend for MemoryBackend {
    // -----------------------------------------------------------------------
    // get_blob_bytes
    // -----------------------------------------------------------------------

    fn get_blob_bytes(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send {
        // test only: mutex is never poisoned in tests
        let bytes = self.inner.lock().unwrap().blobs.get(blob_id).cloned();
        async move { Ok(bytes) }
    }

    // -----------------------------------------------------------------------
    // send_mdns
    // -----------------------------------------------------------------------

    async fn send_mdns(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
        _identity_id: &jmap_types::Id,
        send: BTreeMap<String, (jmap_mail_types::Mdn, jmap_mail_types::Email)>,
    ) -> Result<MdnSendResult, jmap_server::BackendSetError<Self::Error>> {
        let mut sent: BTreeMap<String, Mdn> = BTreeMap::new();
        let mut not_sent: BTreeMap<String, SetError> = BTreeMap::new();

        for (creation_id, (mdn, email)) in send {
            // The handler (handle_mdn_send) has already:
            //   - verified for_email_id is Some
            //   - checked $mdnsent is not set
            //   - fetched the Email and passed it here
            // No re-fetch from storage is needed.

            // Step 1: check for Disposition-Notification-To header.
            let dnt_address: Option<String> = email
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("Disposition-Notification-To"))
                .map(|h| h.value.trim().to_owned());

            let Some(dnt_address) = dnt_address else {
                not_sent.insert(
                    creation_id,
                    SetError::new(SetErrorType::NotFound)
                        .with_description("email has no Disposition-Notification-To header"),
                );
                continue;
            };

            // Step 2: extract server-set fields from the email's headers.
            let original_message_id: Option<String> = email
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("Message-ID"))
                .map(|h| {
                    h.value
                        .trim()
                        .trim_matches('<')
                        .trim_matches('>')
                        .to_owned()
                });

            let original_recipient: Option<String> = email
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("Original-Recipient"))
                .map(|h| h.value.trim().to_owned());

            // final_recipient: fixed test value (no real identity lookup needed).
            // "rfc822" is the address-type identifier per RFC 8098 §3.2.4 and RFC 3798.
            // This test value is used consistently in mdn_integration.rs assertions.
            let final_recipient = Some("rfc822; test@example.com".to_owned());

            // Step 5: build a minimal RFC 5322 MDN blob.
            let orig_msg_id_line = original_message_id.as_deref().unwrap_or("");
            let subject_line = mdn.subject.as_deref().unwrap_or("");
            let text_body_line = mdn.text_body.as_deref().unwrap_or("");

            // Format the disposition enum fields to their RFC 8098 wire strings.
            // Display impls match the JMAP kebab-case wire values from draft §2.
            let action_mode_str = mdn.disposition.action_mode.to_string();
            let sending_mode_str = mdn.disposition.sending_mode.to_string();
            let type_str = mdn.disposition.type_.to_string();

            let mdn_blob = format!(
                "From: test@example.com\r\n\
                 To: {dnt_address}\r\n\
                 Subject: Read receipt for: {subject_line}\r\n\
                 MIME-Version: 1.0\r\n\
                 Content-Type: multipart/report; report-type=disposition-notification; boundary=\"bound\"\r\n\
                 \r\n\
                 --bound\r\n\
                 Content-Type: text/plain\r\n\
                 \r\n\
                 {text_body_line}\r\n\
                 --bound\r\n\
                 Content-Type: message/disposition-notification\r\n\
                 \r\n\
                 Final-Recipient: rfc822; test@example.com\r\n\
                 Original-Message-ID: {orig_msg_id_line}\r\n\
                 Disposition: {action_mode_str}/{sending_mode_str}; {type_str}\r\n\
                 \r\n\
                 --bound--\r\n"
            );

            // Step 6: store the blob.
            let blob_id = jmap_types::Id::from(uuid::Uuid::new_v4().to_string());
            {
                // test only: mutex is never poisoned in tests
                let mut inner = self.inner.lock().unwrap();
                inner.blobs.insert(blob_id, mdn_blob.into_bytes());
            }

            // Step 7: build the response Mdn with server-set fields.
            // Mdn is #[non_exhaustive] and defined outside this crate, so we
            // round-trip through JSON to construct a modified copy.
            let mut sent_val = serde_json::to_value(&mdn).map_err(|e| {
                jmap_server::BackendSetError::Other(MemoryError::new(format!("serialize mdn: {e}")))
            })?;
            if let serde_json::Value::Object(ref mut m) = sent_val {
                m.insert("mdnGateway".to_owned(), serde_json::Value::Null);
                m.insert("error".to_owned(), serde_json::Value::Null);
                match &original_recipient {
                    Some(s) => {
                        m.insert(
                            "originalRecipient".to_owned(),
                            serde_json::Value::String(s.clone()),
                        );
                    }
                    None => {
                        m.remove("originalRecipient");
                    }
                }
                match &final_recipient {
                    Some(s) => {
                        m.insert(
                            "finalRecipient".to_owned(),
                            serde_json::Value::String(s.clone()),
                        );
                    }
                    None => {
                        m.remove("finalRecipient");
                    }
                }
                match &original_message_id {
                    Some(s) => {
                        m.insert(
                            "originalMessageId".to_owned(),
                            serde_json::Value::String(s.clone()),
                        );
                    }
                    None => {
                        m.remove("originalMessageId");
                    }
                }
            }
            let sent_mdn: Mdn = serde_json::from_value(sent_val).map_err(|e| {
                jmap_server::BackendSetError::Other(MemoryError::new(format!(
                    "deserialize sent mdn: {e}"
                )))
            })?;
            sent.insert(creation_id, sent_mdn);
        }

        Ok(MdnSendResult::new(sent, not_sent))
    }

    // -----------------------------------------------------------------------
    // parse_mdns
    // -----------------------------------------------------------------------

    async fn parse_mdns(
        &self,
        _caller: &(),
        account_id: &jmap_types::Id,
        blob_ids: &[jmap_types::Id],
    ) -> Result<MdnParseResult, Self::Error> {
        let mut parsed: BTreeMap<jmap_types::Id, Mdn> = BTreeMap::new();
        let mut not_parsable: Vec<jmap_types::Id> = Vec::new();
        let mut not_found: Vec<jmap_types::Id> = Vec::new();

        // Header-presence quick-reject limit: 4 KiB is enough to cover all
        // RFC 5322 headers in a well-formed MDN.  get_blob_header_bytes avoids
        // loading the full blob before the cheap parsability check.
        const HEADER_LIMIT: usize = 4096;

        for blob_id in blob_ids.iter().cloned() {
            // Quick-reject: fetch only the first HEADER_LIMIT bytes for the
            // parsability heuristic.  If that prefix doesn't contain
            // "disposition:", the blob can't be an MDN — skip it without
            // loading the rest.
            let header_bytes = self
                .get_blob_header_bytes(&(), account_id, &blob_id, HEADER_LIMIT)
                .await?;
            match header_bytes {
                None => {
                    not_found.push(blob_id);
                    continue;
                }
                Some(ref hdr) => {
                    let hdr_text = String::from_utf8_lossy(hdr);
                    if !hdr_text.to_ascii_lowercase().contains("disposition:") {
                        not_parsable.push(blob_id);
                        continue;
                    }
                }
            }

            // Full fetch for actual parsing.
            let bytes = self.get_blob_bytes(&(), account_id, &blob_id).await?;
            match bytes {
                None => {
                    not_found.push(blob_id);
                }
                Some(raw) => {
                    let text = String::from_utf8_lossy(&raw);

                    // Parse fields from the disposition-notification part.
                    // Each field is extracted from the first matching "Field-Name:" line.
                    let final_recipient = find_header_value(&text, "Final-Recipient");
                    let original_message_id = find_header_value(&text, "Original-Message-ID")
                        .map(|s| s.trim_matches('<').trim_matches('>').to_owned());
                    let reporting_ua = find_header_value(&text, "Reporting-UA");
                    let original_recipient = find_header_value(&text, "Original-Recipient");

                    // Parse the Disposition: field.
                    // Format per RFC 8098: action-mode/sending-mode; disposition-type
                    let Some(disposition_str) = find_header_value(&text, "Disposition") else {
                        not_parsable.push(blob_id);
                        continue;
                    };
                    let Some(disposition_val) = parse_disposition_field(&disposition_str) else {
                        not_parsable.push(blob_id);
                        continue;
                    };

                    // Correlate original_message_id → for_email_id via message_id_index.
                    let for_email_id = original_message_id.as_ref().and_then(|msg_id| {
                        // test only: mutex is never poisoned in tests
                        let inner = self.inner.lock().unwrap();
                        inner
                            .message_id_index
                            .get(account_id.as_ref())
                            .and_then(|idx| idx.get(msg_id))
                            .cloned()
                    });

                    // Build the Mdn via JSON round-trip (Mdn is #[non_exhaustive]
                    // and cannot be constructed with struct literal syntax outside
                    // its defining crate).
                    // All known Mdn fields are listed explicitly here. When new fields
                    // are added to Mdn, add them here too — the #[non_exhaustive]
                    // constraint prevents using struct-literal construction so this
                    // manual enumeration is required.
                    let mdn_val = serde_json::json!({
                        "forEmailId": for_email_id.as_ref().map(|id| id.as_ref()),
                        "subject": serde_json::Value::Null,
                        "textBody": serde_json::Value::Null,
                        "includeOriginalMessage": false,
                        "reportingUA": reporting_ua,
                        "disposition": disposition_val,
                        "mdnGateway": serde_json::Value::Null,
                        "originalRecipient": original_recipient,
                        "finalRecipient": final_recipient,
                        "originalMessageId": original_message_id,
                        "error": serde_json::Value::Null,
                        "extensionFields": serde_json::Value::Null,
                    });
                    let mdn: Mdn = serde_json::from_value(mdn_val)
                        .map_err(|e| MemoryError::new(format!("deserialize parsed mdn: {e}")))?;
                    parsed.insert(blob_id, mdn);
                }
            }
        }

        Ok(MdnParseResult::new(parsed, not_parsable, not_found))
    }
}

// ---------------------------------------------------------------------------
// MDN parsing helpers (feature = "mdn")
// ---------------------------------------------------------------------------

/// Extract the value of a named header field from a raw RFC 5322 message string.
///
/// Scans line-by-line for the first line beginning with `"Field-Name:"` (case-insensitive)
/// and returns the trimmed value. Returns `None` if no such line is found.
///
/// Note: this parser does not handle RFC 5322 §2.2.3 folded headers (continuation
/// lines starting with whitespace). MDN messages generated by MUAs typically
/// do not fold headers, so this is acceptable for the test harness. A production
/// MDN parser should use a proper RFC 5322 header parser.
#[cfg(feature = "mdn")]
fn find_header_value(text: &str, field_name: &str) -> Option<String> {
    let prefix = format!("{field_name}:");
    for line in text.lines() {
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return Some(line[prefix.len()..].trim().to_owned());
        }
    }
    None
}

/// Parse an RFC 8098 `Disposition:` field value into a JSON value suitable for
/// deserializing into a [`Disposition`].
///
/// Expected format: `action-mode/sending-mode; disposition-type`
/// e.g. `manual-action/MDN-sent-manually; displayed`
///
/// All parts are lowercased before matching (RFC 8098 is case-insensitive;
/// RFC 9007 requires lowercase in JMAP wire format).
///
/// Returns `None` if the field cannot be parsed into recognized enum variants.
#[cfg(feature = "mdn")]
fn parse_disposition_field(value: &str) -> Option<serde_json::Value> {
    // Split on ';' to separate disposition-mode from disposition-type.
    let mut parts = value.splitn(2, ';');
    let mode_part = parts.next()?.trim().to_lowercase();
    let type_part = parts.next()?.trim().to_lowercase();

    // disposition-mode = action-mode "/" sending-mode
    let mut mode_iter = mode_part.splitn(2, '/');
    let action_str = mode_iter.next()?.trim();
    let sending_str = mode_iter.next()?.trim();

    // Validate against known enum variants (wire values from RFC 9007 §2).
    // Each string is already lowercased above; we only need membership validation —
    // the lowercased input is itself the correct wire value, so pass it through.
    if !matches!(action_str, "manual-action" | "automatic-action") {
        return None;
    }
    if !matches!(sending_str, "mdn-sent-manually" | "mdn-sent-automatically") {
        return None;
    }

    // RFC 8098 §3.2.6 allows modifiers after a '/' in the disposition-type token,
    // e.g. "displayed/error". Extract only the part before the first '/' as the type.
    let type_base = type_part.split('/').next()?.trim();
    if !matches!(
        type_base,
        "deleted" | "dispatched" | "displayed" | "processed"
    ) {
        return None;
    }

    Some(serde_json::json!({
        "actionMode": action_str,
        "sendingMode": sending_str,
        "type": type_base,
    }))
}

// ---------------------------------------------------------------------------
// SieveBackend impl for MemoryBackend (feature = "sieve")
// ---------------------------------------------------------------------------

#[cfg(feature = "sieve")]
use crate::SieveBackend;

#[cfg(feature = "sieve")]
impl SieveBackend for MemoryBackend {
    fn max_sieve_script_bytes(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<u64>, Self::Error>> + Send {
        let limit = self.inner.lock().unwrap().max_sieve_script_limit;
        async move { Ok(limit) }
    }

    fn max_sieve_scripts_per_account(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<usize, Self::Error>> + Send {
        let limit = self.inner.lock().unwrap().max_sieve_scripts_limit;
        async move { Ok(limit.unwrap_or(100)) }
    }

    fn get_sieve_blob(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, Self::Error>> + Send {
        // Same pattern as MdnBackend::get_blob_bytes:
        // lock, clone, release lock, return async move
        let bytes = self.inner.lock().unwrap().blobs.get(blob_id).cloned();
        async move { Ok(bytes) }
    }

    fn validate_sieve_script(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<String>, Self::Error>> + Send {
        // Trivial validation: blob must exist, be non-empty, and be valid UTF-8.
        // Returns Ok(None) for valid, Ok(Some(reason)) for invalid.
        // A real backend would call a Sieve parser here.
        let bytes = self.inner.lock().unwrap().blobs.get(blob_id).cloned();
        async move {
            match bytes {
                None => Ok(Some("blob not found".to_owned())),
                Some(b) => {
                    if b.is_empty() {
                        return Ok(Some("script must not be empty".to_owned()));
                    }
                    match std::str::from_utf8(&b) {
                        Err(_) => Ok(Some("script is not valid UTF-8".to_owned())),
                        Ok(_) => Ok(None),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_types::PatchObject;

    /// A stored submission that fails to deserialize sorts after every one
    /// that does, which keeps the comparator a total order (`slice::sort_by`
    /// may panic otherwise). Fixture: "a" (sendAt March) and "c" (sendAt
    /// January) are valid; "b" is not an EmailSubmission. Comparing "b"
    /// equal to both would give a < b < c < a by id tie-break.
    #[test]
    fn sort_submission_pairs_puts_undecodable_last() {
        let sub = |id: &str, send_at: &str| {
            serde_json::to_value(EmailSubmission::new(
                Id::from(id),
                Id::from("identity"),
                Id::from(format!("email-{id}")),
                Id::from(format!("thread-{id}")),
                jmap_types::UTCDate::from(send_at),
                jmap_mail_types::UndoStatus::Final,
            ))
            .expect("serialize submission")
        };
        let objects: HashMap<Id, serde_json::Value> = [
            (Id::from("a"), sub("a", "2025-03-01T00:00:00Z")),
            (Id::from("b"), serde_json::json!({ "id": "b" })),
            (Id::from("c"), sub("c", "2025-01-01T00:00:00Z")),
        ]
        .into_iter()
        .collect();
        let sort = [EmailSubmissionComparator::new(
            EmailSubmissionComparatorProperty::SentAt,
        )];
        let mut pairs: Vec<(Id, String)> = ["a", "b", "c"]
            .into_iter()
            .map(|id| (Id::from(id), String::new()))
            .collect();
        sort_submission_pairs(&mut pairs, &objects, &sort);
        let order: Vec<&str> = pairs.iter().map(|(id, _)| id.as_ref()).collect();
        assert_eq!(order, ["c", "a", "b"]);
    }

    /// RFC 8620 §5.3: a patch that breaks the PatchObject rules "MUST be
    /// rejected with an "invalidPatch" error". The rejected update must not
    /// leave anything behind in storage, including the Email `keywords`
    /// default that is filled in only to resolve `keywords/...` pointers.
    #[tokio::test]
    async fn rejected_email_patch_leaves_stored_json_unchanged() {
        let backend = MemoryBackend::new();
        let account = Id::from("account1");
        let id = Id::from("e1");
        let stored = serde_json::json!({ "id": "e1", "mailboxIds": { "m1": true } });
        backend
            .inner
            .lock()
            .unwrap()
            .objects_mut(Email::TYPE_NAME, account.as_ref())
            .insert(id.clone(), stored.clone());

        let mut patch = serde_json::Map::new();
        patch.insert("keywords/$seen/x".to_owned(), serde_json::json!(true));
        let result = backend
            .update_object::<Email>(&(), &account, &id, PatchObject::from_map(patch))
            .await;

        assert!(
            matches!(
                result,
                Err(BackendSetError::SetError(ref se))
                    if se.error_type == SetErrorType::InvalidPatch
            ),
            "expected invalidPatch, got {result:?}"
        );
        let after = backend
            .inner
            .lock()
            .unwrap()
            .objects_mut(Email::TYPE_NAME, account.as_ref())
            .get(&id)
            .cloned();
        assert_eq!(after, Some(stored));
    }
}

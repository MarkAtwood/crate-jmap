//! In-memory reference implementation of [`ContactsBackend`](crate::ContactsBackend).
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`ContactsBackend`](crate::ContactsBackend)
//!    trait to study when writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Contacts dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of RFC 9610 edge cases are simplified (see source comments).
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
//! use jmap_contacts_server::{memory::MemoryBackend, register_contacts_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_contacts_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
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
//! Greenfield reference impl per Beads issue JMAP-hwdv (epic) and
//! JMAP-hwdv.6 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server, following the multi-type-store shape established
//! by jmap-chat-server's `MemoryBackend` and the derived-index pattern
//! pioneered in JMAP-hwdv.5 jmap-calendars-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, ContactsBackend, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};
// json_merge_patch lives in jmap-server (the shared foundation crate)
// since bd:JMAP-sc1b.103. Every reference backend imports it; the
// canonical RFC 7396 tests live with the function there (including the
// bd:JMAP-sc1b.97 depth-cap and bd:JMAP-sc1b.87 absent-field regression
// tests).
use jmap_contacts_types::ContactCardFilterCondition;
use jmap_server::{json_merge_patch, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A simple string error for `MemoryBackend` failures.
#[derive(Debug)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MemoryError {}

// ---------------------------------------------------------------------------
// Change log
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

// ---------------------------------------------------------------------------
// Inner state
// ---------------------------------------------------------------------------

/// Per-account auxiliary state that is not keyed by object type.
#[derive(Default, Clone)]
struct AccountAux {
    /// Set of AddressBook ids that have at least one ContactCard attached
    /// (drives `AddressBook/set` destroy rejection with
    /// `addressBookHasContents` per RFC 9610 §3 when
    /// `onDestroyRemoveContents` is `false`). Maintained as a derived
    /// index over the ContactCard store; kept in sync by `create_object` /
    /// `update_object` / `destroy_object` for the ContactCard type.
    address_books_with_contents: HashSet<Id>,
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
    /// `account_id` → auxiliary per-account state (derived indexes)
    aux: HashMap<String, AccountAux>,
    /// Explicitly registered account ids (accounts may exist with no objects yet).
    known_accounts: HashSet<String>,
    /// `(type_name, account_id)` → monotonic counter used by
    /// `demo_next_id` to mint deterministic ids without collisions.
    /// Increments on every mint; never decrements on delete (bd:JMAP-qz9v.14).
    ///
    /// Only present in the default (non-`realistic-demo-ids`) mode —
    /// the realistic-demo-ids mode uses a process-global atomic
    /// counter and never touches this field.
    #[cfg(not(feature = "realistic-demo-ids"))]
    next_ids: HashMap<(&'static str, String), u64>,
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

    fn aux_mut(&mut self, account_id: &str) -> &mut AccountAux {
        self.known_accounts.insert(account_id.to_owned());
        self.aux.entry(account_id.to_owned()).or_default()
    }

    fn aux_ref(&self, account_id: &str) -> Option<&AccountAux> {
        self.aux.get(account_id)
    }

    /// Recompute `address_books_with_contents` for the given account by
    /// scanning the ContactCard store. Each ContactCard references one or
    /// more AddressBooks via `addressBookIds: HashMap<Id, bool>` (RFC 9610
    /// §3 — JMAP addition over RFC 9553).
    fn recompute_address_books_with_contents(&mut self, account_id: &str) {
        let cards = self
            .objects
            .get(&("ContactCard", account_id.to_owned()))
            .cloned()
            .unwrap_or_default();

        let mut set: HashSet<Id> = HashSet::new();
        for value in cards.values() {
            if let Some(map) = value.get("addressBookIds").and_then(|v| v.as_object()) {
                for k in map.keys() {
                    set.insert(Id::from(k.as_str()));
                }
            }
        }
        self.aux_mut(account_id).address_books_with_contents = set;
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`ContactsBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Used as both the integration-test
/// harness and the canonical example for backend implementors.
///
/// Cloning is cheap: every clone shares the same underlying `Arc<Mutex<…>>`.
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
    /// Returns `self` for builder-style chaining.
    #[must_use]
    pub fn with_account(self, account_id: &str) -> Self {
        self.register_account(&Id::from(account_id));
        self
    }

    /// Register an account as known even if it has no objects yet.
    pub fn register_account(&self, account_id: &Id) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
        inner.aux.entry(account_id.as_ref().to_owned()).or_default();
    }

    /// Seed a pre-existing object into the store without bumping the state
    /// counter or recording a change-log entry. Intended for test fixture
    /// setup; the `type_name` must match `O::TYPE_NAME` of the type being
    /// seeded (e.g. `"AddressBook"`, `"ContactCard"`).
    pub fn seed_object(
        &self,
        account_id: &str,
        type_name: &'static str,
        id: &str,
        value: serde_json::Value,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.to_owned());
        inner
            .objects_mut(type_name, account_id)
            .insert(Id::from(id), value);
        if type_name == "ContactCard" {
            inner.recompute_address_books_with_contents(account_id);
        }
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
    ///   is a per-(type, account) monotonic counter that increments on
    ///   every mint and never decrements on delete (bd:JMAP-qz9v.14).
    ///   Lex-orderable within a (type, account) namespace, repeatable
    ///   across test runs, easy to read in test debug output. Resets to
    ///   0 on every process restart (not persistent — the whole store
    ///   is in-memory).
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
            // This deterministic mode uses a per-(type, account) in-memory
            // counter that:
            //   - resets to 0 on every process restart
            //   - is not unique across (type, account) namespaces
            //   - is not globally collision-resistant
            // Production-grade id minting needs a real ULID or equivalent.
            //
            // It IS, however, monotonic within a single (type, account)
            // namespace across deletes (bd:JMAP-qz9v.14) — a destroyed
            // object's id will not be re-minted for a later create.
            let key = (type_name, account_id.to_owned());
            let counter = inner.next_ids.entry(key).or_insert(0);
            *counter += 1;
            let n = *counter;
            let new_id = Id::from(format!("{}{:016x}", type_name.to_ascii_lowercase(), n));
            debug_assert!(
                !inner
                    .objects_ref(type_name, account_id)
                    .is_some_and(|m| m.contains_key(&new_id)),
                "MemoryBackend demo_next_id collision: the monotonic counter \
                 produced an id that already exists in the store. This indicates \
                 the counter was somehow rewound, which should not happen — the \
                 counter is increment-only."
            );
            new_id
        }
    }
}

// ---------------------------------------------------------------------------
// JmapBackend impl (read-side supertrait)
// ---------------------------------------------------------------------------

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
            .map_err(|_| BackendChangesError::CannotCalculate)?;

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
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        let inner = self.inner.lock().unwrap();

        // Filter + sort dispatch.
        //
        // For ContactCard (the only type with a registered `*/query` handler
        // per RFC 9610), decode the typed filter and comparator list via the
        // canonical Pattern G type-identity roundtrip (`expect()` per
        // bc79c70 — silent drop would return ALL cards, a query-correctness
        // bug), then apply `contact_card_matches_filter` and
        // `compare_contact_cards_by_property` from this module.
        //
        // For any other type the result is the (id-sorted) set of all
        // objects in the store; RFC 9610 does not define a queryable
        // AddressBook, so this branch is defensive only.
        let mut ids: Vec<Id> = if O::TYPE_NAME == "ContactCard" {
            // Pattern G: type-identity roundtrip. When O::TYPE_NAME ==
            // "ContactCard" the generic O::Filter is necessarily
            // ContactCardFilterCondition (see
            // jmap_contacts_types::backend::QueryObject impl), so both
            // halves of this serde roundtrip are infallible. `.expect()`
            // surfaces a future custom-serde regression instead of
            // silently dropping the filter (which would return ALL cards
            // when the client expected a filtered subset — bd:JMAP-qz9v.3).
            let card_filter: Option<ContactCardFilterCondition> = filter.map(|f| {
                let v =
                    serde_json::to_value(f).expect("derive(Serialize) on plain data is infallible");
                serde_json::from_value(v).expect(
                    "type-identity roundtrip on ContactCardFilterCondition is infallible: \
                     the JSON came from Serialize on the same concrete type",
                )
            });

            // Decode each comparator into (property, isAscending) via JSON
            // roundtrip. Unknown properties produce Ordering::Equal in the
            // comparator (= no constraint, fall through to next), matching
            // the canonical Mailbox pattern in `crate-jmap-mail-server`.
            let comparators: Vec<(String, bool)> = sort
                .map(|s| {
                    s.iter()
                        .filter_map(|c| {
                            let v = serde_json::to_value(c).ok()?;
                            let prop = v.get("property")?.as_str()?.to_owned();
                            let asc = v
                                .get("isAscending")
                                .and_then(|x| x.as_bool())
                                .unwrap_or(true);
                            Some((prop, asc))
                        })
                        .collect()
                })
                .unwrap_or_default();

            let mut matching: Vec<(Id, serde_json::Value)> = inner
                .objects_ref(O::TYPE_NAME, account_id.as_ref())
                .map(|m| {
                    m.iter()
                        .filter(|(_, v)| {
                            card_filter
                                .as_ref()
                                .is_none_or(|f| contact_card_matches_filter(v, f))
                        })
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();

            // Sort by comparators in order, with Id-string tiebreak for
            // deterministic pagination.
            matching.sort_by(|a, b| {
                let mut ord = std::cmp::Ordering::Equal;
                for (prop, asc) in &comparators {
                    if ord != std::cmp::Ordering::Equal {
                        break;
                    }
                    let cmp = compare_contact_cards_by_property(&a.1, &b.1, prop);
                    ord = if *asc { cmp } else { cmp.reverse() };
                }
                if ord == std::cmp::Ordering::Equal {
                    a.0.as_ref().cmp(b.0.as_ref())
                } else {
                    ord
                }
            });

            matching.into_iter().map(|(id, _)| id).collect()
        } else {
            // Defensive: no registered handler reaches this branch via the
            // dispatcher, but if a future caller does we want deterministic
            // id-sorted output rather than HashMap iteration order.
            inner
                .objects_ref(O::TYPE_NAME, account_id.as_ref())
                .map(|m| {
                    let mut keys: Vec<Id> = m.keys().cloned().collect();
                    keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                    keys
                })
                .unwrap_or_default()
        };

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

// ---------------------------------------------------------------------------
// ContactsBackend impl
// ---------------------------------------------------------------------------

impl ContactsBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());

        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize: {e}"))))?;
        if let Some(map) = val.as_object_mut() {
            map.insert(
                "id".to_owned(),
                serde_json::Value::String(server_id.as_ref().to_owned()),
            );
        }

        // RFC 9610 §3: a ContactCard MUST belong to at least one
        // AddressBook at all times (until destroyed). Reject the create
        // if addressBookIds is absent or empty (bd:JMAP-qz9v.16).
        if O::TYPE_NAME == "ContactCard" && !contact_card_has_address_book_ids(&val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["addressBookIds"])
                    .with_description(
                        "ContactCard must have at least one entry in addressBookIds \
                         (RFC 9610 §3)",
                    ),
            ));
        }

        // RFC 9610 §3: a ContactCard's uid MUST be unique within an
        // Account. Reject the create if another card in the same
        // account already has the same uid (bd:JMAP-qz9v.6).
        if O::TYPE_NAME == "ContactCard" {
            if let Some(new_uid) = val.get("uid").and_then(|v| v.as_str()) {
                if account_has_card_with_uid(&inner, account_id.as_ref(), new_uid, None) {
                    return Err(BackendSetError::SetError(
                        SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["uid"])
                            .with_description(
                                "ContactCard.uid must be unique within an Account \
                                 (RFC 9610 §3)",
                            ),
                    ));
                }
            }
        }

        let stored_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("deserialize after create: {e}")))
        })?;

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

        if O::TYPE_NAME == "ContactCard" {
            inner.recompute_address_books_with_contents(account_id.as_ref());
        }

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

        // RFC 9610 §3: a ContactCard MUST belong to at least one
        // AddressBook at all times. Reject the update if the post-patch
        // state has empty addressBookIds (bd:JMAP-qz9v.16). `current` is
        // a local clone of the stored value, so a rejected patch is
        // discarded without touching storage.
        if O::TYPE_NAME == "ContactCard" && !contact_card_has_address_book_ids(&current) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["addressBookIds"])
                    .with_description(
                        "ContactCard must have at least one entry in addressBookIds \
                         (RFC 9610 §3)",
                    ),
            ));
        }

        // RFC 9610 §3: uid uniqueness within an Account. If the patch
        // changes uid to a value another card already uses, reject
        // (bd:JMAP-qz9v.6). The card being updated is excluded so a
        // no-op patch (or a patch that keeps the same uid) is allowed.
        if O::TYPE_NAME == "ContactCard" {
            if let Some(new_uid) = current.get("uid").and_then(|v| v.as_str()) {
                if account_has_card_with_uid(&inner, account_id.as_ref(), new_uid, Some(id)) {
                    return Err(BackendSetError::SetError(
                        SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["uid"])
                            .with_description(
                                "ContactCard.uid must be unique within an Account \
                                 (RFC 9610 §3)",
                            ),
                    ));
                }
            }
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

        if O::TYPE_NAME == "ContactCard" {
            inner.recompute_address_books_with_contents(account_id.as_ref());
        }

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
                if O::TYPE_NAME == "ContactCard" {
                    inner.recompute_address_books_with_contents(account_id.as_ref());
                }
                Ok(())
            }
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        matches!(O::TYPE_NAME, "AddressBook" | "ContactCard")
    }

    async fn copy_contact_card(
        &self,
        _caller: &(),
        from_account_id: &Id,
        to_account_id: &Id,
        card: jmap_contacts_types::ContactCard,
    ) -> Result<(Id, jmap_contacts_types::ContactCard), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        if !inner.known_accounts.contains(from_account_id.as_ref()) {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }

        inner
            .known_accounts
            .insert(to_account_id.as_ref().to_owned());

        let new_id = Self::demo_next_id(&mut inner, "ContactCard", to_account_id.as_ref());

        let mut val = serde_json::to_value(&card).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("serialize copied card: {e}")))
        })?;
        if let Some(map) = val.as_object_mut() {
            map.insert(
                "id".to_owned(),
                serde_json::Value::String(new_id.as_ref().to_owned()),
            );
        }
        let stored: jmap_contacts_types::ContactCard = serde_json::from_value(val.clone())
            .map_err(|e| {
                BackendSetError::Other(MemoryError(format!("deserialize copied card: {e}")))
            })?;

        let new_state = inner.bump_state("ContactCard", to_account_id.as_ref());
        inner
            .objects_mut("ContactCard", to_account_id.as_ref())
            .insert(new_id.clone(), val);
        inner
            .change_log_mut("ContactCard", to_account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![new_id.clone()],
                updated: vec![],
                destroyed: vec![],
            });
        inner.recompute_address_books_with_contents(to_account_id.as_ref());

        Ok((new_id, stored))
    }

    async fn address_book_has_contents(
        &self,
        _caller: &(),
        account_id: &Id,
        address_book_id: &Id,
    ) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .aux_ref(account_id.as_ref())
            .map(|a| a.address_books_with_contents.contains(address_book_id))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// ContactCard filter and sort helpers (RFC 9610 §3.3)
//
// Used by `MemoryBackend::query_objects` to honor the full
// `ContactCardFilterCondition` (RFC 9610 §3.3.1) and
// `ContactCardComparator` (RFC 9610 §3.3.2) surface. Previous behaviour
// silently match-all'd every field except `inAddressBook` and ignored
// sort entirely (bd:JMAP-qz9v.3 — fixed).
//
// The matcher walks the stored `serde_json::Value` directly rather than
// deserializing into a typed `ContactCard` per call: contacts' wire
// shape is largely `Option<serde_json::Value>` (the workspace sloppy-
// value pattern for RFC 9553 sub-objects), so direct JSON traversal
// avoids a round-trip allocation per match.
// ---------------------------------------------------------------------------

/// Apply a `ContactCardFilterCondition` (RFC 9610 §3.3.1) to a stored
/// ContactCard JSON value.
///
/// Returns `true` if the card passes the filter. Unset condition fields
/// are treated as "no constraint" per §3.3.1.
///
/// The matcher honors every field of the current
/// [`ContactCardFilterCondition`] surface; the type carries
/// `#[non_exhaustive]` so a future field addition will not silently fall
/// through to "match all" — instead, the matcher will continue to honor
/// the existing fields and the new field will need to be added here
/// explicitly. Failure to do so is a regression of the
/// silently-ignored-filter bug class.
fn contact_card_matches_filter(card: &serde_json::Value, f: &ContactCardFilterCondition) -> bool {
    // inAddressBook: card.addressBookIds[<id>] must be present.
    if let Some(ref book_id) = f.in_address_book {
        let matches = card
            .get("addressBookIds")
            .and_then(|v| v.as_object())
            .is_some_and(|m| m.contains_key(book_id.as_ref()));
        if !matches {
            return false;
        }
    }

    // uid: exact match on the top-level uid string.
    if let Some(ref want_uid) = f.uid {
        if card.get("uid").and_then(|v| v.as_str()) != Some(want_uid.as_str()) {
            return false;
        }
    }

    // hasMember: card.members map must contain the requested uid as a key.
    if let Some(ref want_member) = f.has_member {
        let matches = card
            .get("members")
            .and_then(|v| v.as_object())
            .is_some_and(|m| m.contains_key(want_member));
        if !matches {
            return false;
        }
    }

    // kind: exact match on the top-level kind string.
    if let Some(ref want_kind) = f.kind {
        if card.get("kind").and_then(|v| v.as_str()) != Some(want_kind.as_str()) {
            return false;
        }
    }

    // createdBefore / createdAfter: lexicographic string comparison on
    // `card.created`. UTCDate's fixed `YYYY-MM-DDTHH:MM:SSZ` format makes
    // lexicographic ordering equivalent to chronological ordering, so a
    // plain `<` / `>=` on `&str` is correct per RFC 8620 §1.4.
    if let Some(ref before) = f.created_before {
        match card.get("created").and_then(|v| v.as_str()) {
            Some(s) if s < before.as_ref() => {}
            _ => return false,
        }
    }
    if let Some(ref after) = f.created_after {
        match card.get("created").and_then(|v| v.as_str()) {
            Some(s) if s >= after.as_ref() => {}
            _ => return false,
        }
    }

    // updatedBefore / updatedAfter: same pattern on `card.updated`.
    if let Some(ref before) = f.updated_before {
        match card.get("updated").and_then(|v| v.as_str()) {
            Some(s) if s < before.as_ref() => {}
            _ => return false,
        }
    }
    if let Some(ref after) = f.updated_after {
        match card.get("updated").and_then(|v| v.as_str()) {
            Some(s) if s >= after.as_ref() => {}
            _ => return false,
        }
    }

    // text: case-sensitive substring across every string leaf in the
    // card. The exact field set is implementation-defined per §3.3.1; a
    // recursive scan covers all current and future typed fields without
    // requiring updates as the JSContact surface grows.
    if let Some(ref needle) = f.text {
        if !json_contains_substring_recursive(card, needle) {
            return false;
        }
    }

    // name: substring match on card.name.full OR any NameComponent.value.
    if let Some(ref needle) = f.name {
        let name = card.get("name");
        let full_match = name
            .and_then(|n| n.get("full"))
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.contains(needle.as_str()));
        let comp_match = name
            .and_then(|n| n.get("components"))
            .and_then(|v| v.as_array())
            .is_some_and(|arr| {
                arr.iter().any(|c| {
                    c.get("value")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.contains(needle.as_str()))
                })
            });
        if !(full_match || comp_match) {
            return false;
        }
    }

    // name/given, name/surname, name/surname2: substring match on
    // NameComponent.value where component.kind == this kind.
    for (needle_opt, kind_str) in [
        (&f.name_given, "given"),
        (&f.name_surname, "surname"),
        (&f.name_surname2, "surname2"),
    ] {
        if let Some(needle) = needle_opt {
            let matches = card
                .get("name")
                .and_then(|n| n.get("components"))
                .and_then(|v| v.as_array())
                .is_some_and(|arr| {
                    arr.iter().any(|c| {
                        c.get("kind").and_then(|v| v.as_str()) == Some(kind_str)
                            && c.get("value")
                                .and_then(|v| v.as_str())
                                .is_some_and(|s| s.contains(needle.as_str()))
                    })
                });
            if !matches {
                return false;
            }
        }
    }

    // nickname: substring match on any nicknames.values()[].name.
    if let Some(ref needle) = f.nickname {
        let matches = card
            .get("nicknames")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|nick| {
                    nick.get("name")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.contains(needle.as_str()))
                })
            });
        if !matches {
            return false;
        }
    }

    // organization: substring match on any organizations.values()[].name.
    if let Some(ref needle) = f.organization {
        let matches = card
            .get("organizations")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|org| {
                    org.get("name")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.contains(needle.as_str()))
                })
            });
        if !matches {
            return false;
        }
    }

    // email: substring match on any emails.values()[].address OR .label.
    if let Some(ref needle) = f.email {
        let matches = card
            .get("emails")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|e| {
                    field_contains(e, "address", needle) || field_contains(e, "label", needle)
                })
            });
        if !matches {
            return false;
        }
    }

    // phone: substring match on any phones.values()[].number OR .label.
    if let Some(ref needle) = f.phone {
        let matches = card
            .get("phones")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|p| {
                    field_contains(p, "number", needle) || field_contains(p, "label", needle)
                })
            });
        if !matches {
            return false;
        }
    }

    // onlineService: substring match on any onlineServices.values()[]
    // .service, .uri, .user, OR .label.
    if let Some(ref needle) = f.online_service {
        let matches = card
            .get("onlineServices")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|svc| {
                    ["service", "uri", "user", "label"]
                        .iter()
                        .any(|k| field_contains(svc, k, needle))
                })
            });
        if !matches {
            return false;
        }
    }

    // address: substring match on any addresses.values()[].full OR any
    // AddressComponent.value.
    if let Some(ref needle) = f.address {
        let matches = card
            .get("addresses")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|addr| {
                    let full_match = field_contains(addr, "full", needle);
                    let comp_match = addr
                        .get("components")
                        .and_then(|v| v.as_array())
                        .is_some_and(|arr| {
                            arr.iter().any(|c| {
                                c.get("value")
                                    .and_then(|v| v.as_str())
                                    .is_some_and(|s| s.contains(needle.as_str()))
                            })
                        });
                    full_match || comp_match
                })
            });
        if !matches {
            return false;
        }
    }

    // note: substring match on any notes.values()[].note.
    if let Some(ref needle) = f.note {
        let matches = card
            .get("notes")
            .and_then(|v| v.as_object())
            .is_some_and(|m| {
                m.values().any(|n| {
                    n.get("note")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.contains(needle.as_str()))
                })
            });
        if !matches {
            return false;
        }
    }

    true
}

/// Substring-match a string field of a JSON object.
///
/// Returns `false` when the field is absent, non-string, or does not
/// contain `needle`.
fn field_contains(obj: &serde_json::Value, field: &str, needle: &str) -> bool {
    obj.get(field)
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.contains(needle))
}

/// Recursively scan a JSON value for any string leaf containing `needle`.
///
/// Used for the `text` filter (§3.3.1) — the exact field set is
/// implementation-defined and a recursive scan covers every JSContact
/// sub-object without requiring updates as new typed fields are added
/// to [`ContactCard`].
fn json_contains_substring_recursive(v: &serde_json::Value, needle: &str) -> bool {
    match v {
        serde_json::Value::String(s) => s.contains(needle),
        serde_json::Value::Array(arr) => arr
            .iter()
            .any(|x| json_contains_substring_recursive(x, needle)),
        serde_json::Value::Object(m) => m
            .values()
            .any(|x| json_contains_substring_recursive(x, needle)),
        _ => false,
    }
}

/// Check whether a serialized ContactCard's `addressBookIds` field is
/// present and non-empty (RFC 9610 §3 invariant: a card MUST belong to
/// at least one AddressBook at all times).
fn contact_card_has_address_book_ids(card: &serde_json::Value) -> bool {
    card.get("addressBookIds")
        .and_then(|v| v.as_object())
        .is_some_and(|m| !m.is_empty())
}

/// Look up whether any ContactCard in `account_id` (other than
/// `exclude_id` if supplied) has `uid` set to `target_uid`.
///
/// Used to enforce RFC 9610 §3: 'there MUST NOT be more than one
/// ContactCard with the same uid in an Account.' Reference impl
/// scans every card in the account; production backends should
/// maintain a uid → id index.
fn account_has_card_with_uid(
    inner: &Inner,
    account_id: &str,
    target_uid: &str,
    exclude_id: Option<&Id>,
) -> bool {
    inner
        .objects_ref("ContactCard", account_id)
        .is_some_and(|m| {
            m.iter().any(|(id, v)| {
                exclude_id.is_none_or(|x| x != id)
                    && v.get("uid").and_then(|u| u.as_str()) == Some(target_uid)
            })
        })
}

/// Compare two ContactCard JSON values for sort by RFC 9610 §3.3.2
/// property name.
///
/// Honored properties: `"created"`, `"updated"`, `"uid"`, `"kind"`.
/// Unknown properties produce `Ordering::Equal` (= no constraint), so
/// the caller's tiebreak (Id-string) takes effect — matching the
/// canonical Mailbox pattern in `crate-jmap-mail-server` and avoiding
/// panics on a malformed comparator slipping through validation.
fn compare_contact_cards_by_property(
    a: &serde_json::Value,
    b: &serde_json::Value,
    property: &str,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let av = a.get(property).and_then(|v| v.as_str());
    let bv = b.get(property).and_then(|v| v.as_str());
    match (av, bv) {
        (Some(a), Some(b)) => a.cmp(b),
        // Absent value sorts before present value in ascending order.
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

// ---------------------------------------------------------------------------
// Tests for `contact_card_matches_filter` and
// `compare_contact_cards_by_property` (bd:JMAP-qz9v.3).
//
// Each test seeds a single ContactCard JSON value, constructs a
// `ContactCardFilterCondition` exercising one field, and asserts the
// expected pass/fail result. The previous implementation silently
// match-all'd every field except `inAddressBook` and ignored sort
// entirely; these tests pin the now-honoring behavior so a future
// regression does not silently reintroduce the bug.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod query_filter_tests {
    use super::*;
    use jmap_types::UTCDate;
    use serde_json::json;

    fn empty_filter() -> ContactCardFilterCondition {
        ContactCardFilterCondition::default()
    }

    // ── inAddressBook ────────────────────────────────────────────────────

    #[test]
    fn in_address_book_matches() {
        let card = json!({"addressBookIds": {"ab1": true, "ab2": true}});
        let mut f = empty_filter();
        f.in_address_book = Some(Id::from("ab1"));
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn in_address_book_no_match() {
        let card = json!({"addressBookIds": {"ab1": true}});
        let mut f = empty_filter();
        f.in_address_book = Some(Id::from("ab2"));
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── uid ──────────────────────────────────────────────────────────────

    #[test]
    fn uid_exact_match() {
        let card = json!({"uid": "urn:uuid:abc"});
        let mut f = empty_filter();
        f.uid = Some("urn:uuid:abc".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn uid_no_match() {
        let card = json!({"uid": "urn:uuid:abc"});
        let mut f = empty_filter();
        f.uid = Some("urn:uuid:xyz".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── hasMember ────────────────────────────────────────────────────────

    #[test]
    fn has_member_matches_key() {
        let card = json!({"members": {"urn:uuid:m1": true}});
        let mut f = empty_filter();
        f.has_member = Some("urn:uuid:m1".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn has_member_no_match() {
        let card = json!({"members": {"urn:uuid:m1": true}});
        let mut f = empty_filter();
        f.has_member = Some("urn:uuid:m2".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── kind ─────────────────────────────────────────────────────────────

    #[test]
    fn kind_exact_match() {
        let card = json!({"kind": "individual"});
        let mut f = empty_filter();
        f.kind = Some("individual".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn kind_no_match() {
        let card = json!({"kind": "individual"});
        let mut f = empty_filter();
        f.kind = Some("group".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── createdBefore / createdAfter ─────────────────────────────────────

    #[test]
    fn created_before_matches() {
        let card = json!({"created": "2020-01-01T00:00:00Z"});
        let mut f = empty_filter();
        f.created_before = Some(UTCDate::from("2021-01-01T00:00:00Z"));
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn created_before_excludes_equal_and_later() {
        let card = json!({"created": "2021-01-01T00:00:00Z"});
        let mut f = empty_filter();
        f.created_before = Some(UTCDate::from("2021-01-01T00:00:00Z"));
        // RFC 9610 §3.3.1 createdBefore is strict less-than.
        assert!(!contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn created_after_matches_at_boundary() {
        let card = json!({"created": "2021-01-01T00:00:00Z"});
        let mut f = empty_filter();
        f.created_after = Some(UTCDate::from("2021-01-01T00:00:00Z"));
        // createdAfter is inclusive at the boundary (`>=`).
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn created_after_excludes_earlier() {
        let card = json!({"created": "2020-06-01T00:00:00Z"});
        let mut f = empty_filter();
        f.created_after = Some(UTCDate::from("2021-01-01T00:00:00Z"));
        assert!(!contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn created_before_with_absent_created_excludes() {
        let card = json!({"uid": "no-created"});
        let mut f = empty_filter();
        f.created_before = Some(UTCDate::from("2021-01-01T00:00:00Z"));
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── updatedBefore / updatedAfter ─────────────────────────────────────

    #[test]
    fn updated_before_matches() {
        let card = json!({"updated": "2019-06-01T00:00:00Z"});
        let mut f = empty_filter();
        f.updated_before = Some(UTCDate::from("2020-01-01T00:00:00Z"));
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn updated_after_matches() {
        let card = json!({"updated": "2022-06-01T00:00:00Z"});
        let mut f = empty_filter();
        f.updated_after = Some(UTCDate::from("2022-01-01T00:00:00Z"));
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── text (recursive substring) ───────────────────────────────────────

    #[test]
    fn text_substring_in_nested_field() {
        let card = json!({
            "notes": {"n1": {"note": "Met at OSCON 2022"}}
        });
        let mut f = empty_filter();
        f.text = Some("OSCON".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn text_no_match_anywhere() {
        let card = json!({
            "notes": {"n1": {"note": "Met at OSCON 2022"}}
        });
        let mut f = empty_filter();
        f.text = Some("NotInCard".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── name (full or any component) ─────────────────────────────────────

    #[test]
    fn name_substring_matches_full() {
        let card = json!({"name": {"full": "Jane Doe"}});
        let mut f = empty_filter();
        f.name = Some("Jane".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn name_substring_matches_any_component_value() {
        let card = json!({
            "name": {"components": [{"kind": "given", "value": "Jane"},
                                    {"kind": "surname", "value": "Doe"}]}
        });
        let mut f = empty_filter();
        f.name = Some("Doe".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn name_no_match() {
        let card = json!({"name": {"full": "Jane Doe"}});
        let mut f = empty_filter();
        f.name = Some("Smith".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── name/given, name/surname, name/surname2 ──────────────────────────

    #[test]
    fn name_given_matches_only_given_kind() {
        let card = json!({
            "name": {"components": [{"kind": "given", "value": "Jane"},
                                    {"kind": "surname", "value": "Doe"}]}
        });
        let mut f = empty_filter();
        f.name_given = Some("Jane".to_owned());
        assert!(contact_card_matches_filter(&card, &f));

        // Surname value with name/given filter must NOT match.
        let mut f = empty_filter();
        f.name_given = Some("Doe".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn name_surname_kind_specificity() {
        let card = json!({
            "name": {"components": [{"kind": "surname", "value": "Doe"}]}
        });
        let mut f = empty_filter();
        f.name_surname = Some("Doe".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn name_surname2_kind_specificity() {
        let card = json!({
            "name": {"components": [{"kind": "surname2", "value": "Garcia"}]}
        });
        let mut f = empty_filter();
        f.name_surname2 = Some("Garcia".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── nickname ─────────────────────────────────────────────────────────

    #[test]
    fn nickname_matches_any_nickname_name() {
        let card = json!({
            "nicknames": {
                "n1": {"name": "Janie"},
                "n2": {"name": "JD"}
            }
        });
        let mut f = empty_filter();
        f.nickname = Some("JD".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── organization ─────────────────────────────────────────────────────

    #[test]
    fn organization_matches_any_name() {
        let card = json!({
            "organizations": {"o1": {"name": "Acme Corp"}}
        });
        let mut f = empty_filter();
        f.organization = Some("Acme".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── email ────────────────────────────────────────────────────────────

    #[test]
    fn email_matches_address() {
        let card = json!({
            "emails": {"e1": {"address": "jane@example.com"}}
        });
        let mut f = empty_filter();
        f.email = Some("example.com".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn email_matches_label() {
        let card = json!({
            "emails": {"e1": {"address": "jane@example.com", "label": "Personal"}}
        });
        let mut f = empty_filter();
        f.email = Some("Personal".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── phone ────────────────────────────────────────────────────────────

    #[test]
    fn phone_matches_number() {
        let card = json!({"phones": {"p1": {"number": "tel:+1-555-0100"}}});
        let mut f = empty_filter();
        f.phone = Some("555".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── onlineService ────────────────────────────────────────────────────

    #[test]
    fn online_service_matches_uri() {
        let card = json!({
            "onlineServices": {
                "s1": {"service": "GitHub", "uri": "https://github.com/jdoe", "user": "jdoe"}
            }
        });
        let mut f = empty_filter();
        f.online_service = Some("github.com".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn online_service_matches_user() {
        let card = json!({
            "onlineServices": {"s1": {"service": "GitHub", "user": "jdoe"}}
        });
        let mut f = empty_filter();
        f.online_service = Some("jdoe".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── address ──────────────────────────────────────────────────────────

    #[test]
    fn address_matches_full() {
        let card = json!({
            "addresses": {"a1": {"full": "123 Main St, Springfield"}}
        });
        let mut f = empty_filter();
        f.address = Some("Springfield".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn address_matches_component_value() {
        let card = json!({
            "addresses": {"a1": {
                "components": [
                    {"kind": "locality", "value": "Springfield"},
                    {"kind": "country", "value": "USA"}
                ]
            }}
        });
        let mut f = empty_filter();
        f.address = Some("Springfield".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── note ─────────────────────────────────────────────────────────────

    #[test]
    fn note_matches_any_note_text() {
        let card = json!({
            "notes": {"n1": {"note": "Met at conference"}}
        });
        let mut f = empty_filter();
        f.note = Some("conference".to_owned());
        assert!(contact_card_matches_filter(&card, &f));
    }

    // ── empty filter, conjunctive composition ────────────────────────────

    #[test]
    fn empty_condition_matches_all() {
        let card = json!({"uid": "anything"});
        let f = empty_filter();
        assert!(contact_card_matches_filter(&card, &f));
    }

    #[test]
    fn multiple_fields_are_conjunctive() {
        // RFC 9610 §3.3.1: all set fields must match (AND).
        let card = json!({"kind": "individual", "uid": "x"});
        let mut f = empty_filter();
        f.kind = Some("individual".to_owned());
        f.uid = Some("x".to_owned());
        assert!(contact_card_matches_filter(&card, &f));

        // Mismatch on one field → whole filter fails.
        f.kind = Some("group".to_owned());
        assert!(!contact_card_matches_filter(&card, &f));
    }

    // ── compare_contact_cards_by_property ────────────────────────────────

    #[test]
    fn compare_by_created_ascending() {
        use std::cmp::Ordering;
        let a = json!({"created": "2020-01-01T00:00:00Z"});
        let b = json!({"created": "2021-01-01T00:00:00Z"});
        assert_eq!(
            compare_contact_cards_by_property(&a, &b, "created"),
            Ordering::Less
        );
    }

    #[test]
    fn compare_by_uid_lexicographic() {
        use std::cmp::Ordering;
        let a = json!({"uid": "alpha"});
        let b = json!({"uid": "beta"});
        assert_eq!(
            compare_contact_cards_by_property(&a, &b, "uid"),
            Ordering::Less
        );
    }

    #[test]
    fn compare_by_unknown_property_returns_equal() {
        use std::cmp::Ordering;
        let a = json!({"uid": "a"});
        let b = json!({"uid": "b"});
        // "vendorProperty" is not honored — fall through to next comparator
        // (Equal means "no constraint" in the chained sort_by).
        assert_eq!(
            compare_contact_cards_by_property(&a, &b, "vendorProperty"),
            Ordering::Equal
        );
    }

    #[test]
    fn compare_absent_value_orders_before_present_in_ascending() {
        use std::cmp::Ordering;
        let absent = json!({});
        let present = json!({"created": "2020-01-01T00:00:00Z"});
        assert_eq!(
            compare_contact_cards_by_property(&absent, &present, "created"),
            Ordering::Less
        );
        assert_eq!(
            compare_contact_cards_by_property(&present, &absent, "created"),
            Ordering::Greater
        );
    }
}

// ---------------------------------------------------------------------------
// Tests for demo_next_id (bd:JMAP-qz9v.14)
// ---------------------------------------------------------------------------

#[cfg(all(test, not(feature = "realistic-demo-ids")))]
mod demo_id_tests {
    use super::*;

    /// Oracle (bd:JMAP-qz9v.14): minting an id, destroying the object,
    /// then minting again MUST produce a different id. The previous
    /// `len()`-based counter re-minted the destroyed id, causing the
    /// same Id to appear as both `created[K+2]` and `destroyed[K]` in
    /// the change log — silently corrupting client-side caches.
    #[test]
    fn id_not_recycled_after_destroy() {
        let mut inner = Inner::default();
        let acc = "acc1";

        // Mint id1 and stash a placeholder in the objects map so a
        // future debug_assert would catch a re-mint.
        let id1 = MemoryBackend::demo_next_id(&mut inner, "AddressBook", acc);
        inner
            .objects_mut("AddressBook", acc)
            .insert(id1.clone(), serde_json::Value::Null);

        // Destroy id1 by removing it from the store.
        inner.objects_mut("AddressBook", acc).remove(&id1);

        // The next mint MUST yield a different id even though the store
        // is now empty for this (type, account).
        let id2 = MemoryBackend::demo_next_id(&mut inner, "AddressBook", acc);
        assert_ne!(
            id1.as_ref(),
            id2.as_ref(),
            "destroyed id must not be re-minted: id1={id1}, id2={id2}"
        );
    }

    /// Oracle: the counter is per-(type, account), so two distinct
    /// (type, account) pairs do not share id space. A card in account
    /// "a" and a book in account "a" can both start at the n=1
    /// numbering without colliding.
    #[test]
    fn id_namespace_is_per_type_and_account() {
        let mut inner = Inner::default();

        let book_a = MemoryBackend::demo_next_id(&mut inner, "AddressBook", "acc1");
        let card_a = MemoryBackend::demo_next_id(&mut inner, "ContactCard", "acc1");
        let book_b = MemoryBackend::demo_next_id(&mut inner, "AddressBook", "acc2");

        // Different type → different prefix even at same counter value.
        assert_ne!(book_a.as_ref(), card_a.as_ref());
        // Different account → independent counter, same prefix but n=1.
        assert_eq!(
            book_a.as_ref(),
            "addressbook0000000000000001",
            "counter starts at 1 for the first mint in a namespace"
        );
        assert_eq!(
            book_b.as_ref(),
            "addressbook0000000000000001",
            "different account namespace gets its own counter starting at 1"
        );
    }

    /// Oracle: the counter is monotonic — successive mints in the same
    /// (type, account) namespace yield strictly increasing ids.
    #[test]
    fn ids_are_monotonic_within_namespace() {
        let mut inner = Inner::default();
        let acc = "acc1";

        let id1 = MemoryBackend::demo_next_id(&mut inner, "AddressBook", acc);
        let id2 = MemoryBackend::demo_next_id(&mut inner, "AddressBook", acc);
        let id3 = MemoryBackend::demo_next_id(&mut inner, "AddressBook", acc);

        assert!(id1.as_ref() < id2.as_ref());
        assert!(id2.as_ref() < id3.as_ref());
    }
}

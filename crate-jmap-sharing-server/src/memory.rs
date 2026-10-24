//! In-memory reference implementation of [`SharingBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`SharingBackend`] trait to study when writing
//!    a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Sharing dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of RFC 9670 edge cases are simplified (see source comments).
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
//! use jmap_sharing_server::{memory::MemoryBackend, register_sharing_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_sharing_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
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
//! Every `lock()` in this module ends in a bare `.unwrap()` on purpose.
//! `Mutex::lock` fails only when another thread panicked while holding the
//! guard, so the state behind it may be half-updated; that panic is
//! already a bug in this module, and there is no consistent state to
//! recover. Panicking again surfaces it instead of serving corrupt data.
//! The crate's tests never poison the lock. A production backend that must
//! survive a poisoned lock should implement the trait with its own locking
//! rather than copy this pattern.
//!
//! # Tracking
//!
//! Promoted from `tests/common/mod.rs` per Beads issue JMAP-hwdv (epic)
//! and JMAP-hwdv.2 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::SharingBackend;
use jmap_metadata_server::{
    ChangedProperties, MetadataCondition, MetadataFilterError, MetadataOnlyChanges, SetKind,
    changed_properties, check_metadata_depth, check_metadata_write, is_metadata_property,
    metadata_path_exists, metadata_text_contains, metadata_text_equals,
};
use jmap_metadata_types::{DataTypeMetadataInfo, MetadataTextMatch};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject,
};
use jmap_server::{
    PatchPathError, apply_patch_object, check_patch_paths, limit_or_max, resolve_query_start,
};
use jmap_sharing_types::{Principal, PrincipalFilter};
use jmap_types::{Filter, Id, JmapError, Operator, State};

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
    /// For a single-object update, the top-level properties it altered, or
    /// `None` when not known (always `None` for a create or destroy).
    /// Feeds draft-ietf-jmap-metadata-04 §3.3 `ignoreMetadataOnlyChanges`
    /// and `updatedProperties`.
    updated_props: Option<BTreeSet<String>>,
}

impl ChangeEntry {
    /// `true` for an update that changed only `metadata` and/or
    /// `privateMetadata` (draft-ietf-jmap-metadata-04 §3.3).
    fn is_metadata_only_update(&self) -> bool {
        self.created.is_empty()
            && self.destroyed.is_empty()
            && !self.updated.is_empty()
            && self.updated_props.as_ref().is_some_and(|props| {
                !props.is_empty() && props.iter().all(|p| is_metadata_property(p))
            })
    }
}

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `type_name` → `account_id` → `id → serialized object`
    objects: HashMap<&'static str, HashMap<String, HashMap<Id, serde_json::Value>>>,
    /// `type_name` → `account_id` → current state counter
    states: HashMap<&'static str, HashMap<String, u64>>,
    /// `type_name` → `account_id` → ordered change entries
    change_log: HashMap<&'static str, HashMap<String, Vec<ChangeEntry>>>,
    /// explicitly registered account ids (accounts may exist with no objects yet)
    known_accounts: HashSet<String>,
    /// The draft-ietf-jmap-metadata-04 §1.2.1 DataTypeMetadataInfo of
    /// Principal, set by [`MemoryBackend::with_metadata_info`]. `None`:
    /// Principal does not have the metadata properties.
    metadata_info: Option<DataTypeMetadataInfo>,
}

impl Inner {
    fn current_state(&self, type_name: &'static str, account_id: &str) -> u64 {
        self.states
            .get(type_name)
            .and_then(|m| m.get(account_id))
            .copied()
            .unwrap_or(0)
    }

    fn bump_state(&mut self, type_name: &'static str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry(type_name)
            .or_default()
            .entry(account_id.to_owned())
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        // Note: deliberately no `known_accounts.insert(...)` side-effect.
        // Account registration is the responsibility of the caller (the
        // matching `SharingBackend` write op) — `create_object` registers
        // explicitly after the create commits, while `update_object` and
        // `destroy_object` MUST gate on `known_accounts.contains(...)`
        // before invoking this helper. Coupling registration to lookup
        // would let an unknown accountId pass the handler-layer
        // `account_exists` check on its second visit (RFC 8620 §3.6.2
        // requires consistent `accountNotFound`).
        self.objects
            .entry(type_name)
            .or_default()
            .entry(account_id.to_owned())
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &'static str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects.get(type_name).and_then(|m| m.get(account_id))
    }

    fn change_log_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut Vec<ChangeEntry> {
        self.change_log
            .entry(type_name)
            .or_default()
            .entry(account_id.to_owned())
            .or_default()
    }
}

/// Per-ID fate tracker for RFC 8620 §5.2 ID deduplication across change log
/// entries in one `/changes` window.
///
/// - created+updated → `Created`
/// - created+destroyed → omitted (the client never knew the object)
/// - updated+destroyed → `Destroyed`
/// - updated+updated → `Updated`
///
/// Mirrors the canonical jmap-mail-server `IdFate` enum.
#[derive(Debug, Clone, Copy)]
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`SharingBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry.
///
/// # Clone semantics — shared state, not independent copies
///
/// The manual [`Clone`] impl is a cheap [`Arc`] clone:
/// `b1.clone()` produces a second handle to the **same** shared state
/// behind a [`Mutex`]. Mutating through one clone is visible through every
/// other clone. This is the workspace pattern for `Arc<Mutex<_>>`-backed
/// reference impls and matches the canonical `jmap-mail-server`.
///
/// To build an independent backend (e.g. for parameterized tests that need
/// distinct state per case), construct a new instance with
/// [`Self::new`] / [`Self::default`] rather than calling `.clone()`.
#[derive(Default)]
#[non_exhaustive]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

/// Manual `Clone` impl, not `#[derive(Clone)]`: cloning is **shared-state**.
///
/// A clone copies no data. The original and the clone hold `Arc::clone`s
/// of the same `Mutex`-guarded store and observe each other's mutations, so
/// one handle can seed or inspect state while another serves requests. This
/// impl must never become a deep copy; for independent state construct a
/// fresh [`MemoryBackend::new`] instead of calling `.clone()`.
impl Clone for MemoryBackend {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl MemoryBackend {
    /// Construct an empty [`MemoryBackend`] with no accounts or stored
    /// objects. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
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
    ///
    /// Matches the canonical [`jmap_mail_server::memory::MemoryBackend::register_account`](https://docs.rs/jmap-mail-server)
    /// shape (`&self` + `&Id`). Prefer this over the legacy
    /// [`Self::new_with_accounts`] constructor in new code — it composes
    /// with `Arc<MemoryBackend>` since it takes `&self`, and accepts the
    /// strongly-typed [`Id`] rather than a raw `&str`.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id) {
        debug_assert!(
            !account_id.as_ref().is_empty(),
            "register_account: account_id must not be the empty string \
             (RFC 8620 §1.2, bd:JMAP-dx0s)"
        );
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
    }

    /// Register `account_id` as known and return `self`, for
    /// builder-style fixture setup. Same as [`Self::register_account`].
    ///
    /// # Panics
    ///
    /// In debug builds, if `account_id` is the empty string (RFC 8620
    /// §1.2, bd:JMAP-dx0s).
    #[must_use]
    pub fn with_account(self, account_id: &Id) -> Self {
        self.register_account(account_id);
        self
    }

    /// Give `data_type` the draft-ietf-jmap-metadata-04 `metadata`
    /// property, configured by `info` (the DataTypeMetadataInfo of -04
    /// §1.2.1).
    ///
    /// Without this call Principal has no metadata: `Principal/set` rejects
    /// a write to `metadata` or `privateMetadata` with `invalidProperties`
    /// and `Principal/query` rejects a metadata filter condition with
    /// `unsupportedFilter`. With it, `Principal/set` validates metadata
    /// writes (-04 §3.2), `Principal/query` evaluates the -04 §3.6
    /// conditions, and `Principal/changes` honours
    /// `ignoreMetadataOnlyChanges` and reports `updatedProperties` (-04
    /// §3.3). Advertising the capability in the session is the server's
    /// job: list `info` under `"Principal"` in the account's
    /// `urn:ietf:params:jmap:metadata` `dataTypes`.
    ///
    /// A Principal has no rights object and no container, so -04 §4.1
    /// requires only that the account "is not read-only"; this backend has
    /// no read-only accounts, so it never answers `forbidden`.
    ///
    /// `Principal/query` evaluates a filter that carries a `metadata*`
    /// condition exactly: the tree may combine metadata conditions with
    /// AND/OR/NOT, and any other Principal condition in it is
    /// `unsupportedFilter` (RFC 8620 §5.5), because this backend evaluates
    /// no RFC 9670 §2.4.1 condition. A filter without metadata conditions
    /// is still ignored, as [`JmapBackend::query_objects`] documents.
    ///
    /// # Panics
    ///
    /// Panics if `data_type` is not `"Principal"` or if
    /// `info.supports_private` is true: this backend has no per-user
    /// identity (`CallerCtx` is `()`) to own `privateMetadata` (-04 §2).
    /// ShareNotification is not supported: RFC 9670 §3.3 rejects every
    /// create and update of it with `forbidden`, so its metadata could
    /// never be written.
    #[must_use]
    pub fn with_metadata_info(self, data_type: &str, info: DataTypeMetadataInfo) -> Self {
        assert!(
            data_type == Principal::TYPE_NAME,
            "MemoryBackend supports metadata only on Principal, not {data_type:?}"
        );
        assert!(
            !info.supports_private,
            "MemoryBackend does not support privateMetadata (supportsPrivate must be false)"
        );
        self.inner.lock().unwrap().metadata_info = Some(info);
        self
    }

    /// Seed a pre-existing object into the store without bumping the
    /// state counter or recording a change-log entry. Test-fixture setup
    /// only; registers `account_id` as known. Mirrors
    /// `jmap_mail_server::memory::MemoryBackend::seed_object`.
    ///
    /// `type_name` is the `TYPE_NAME` of a type this backend stores:
    /// `"Principal"`, `"ShareNotification"`.
    ///
    /// # Panics
    ///
    /// If `type_name` is not one of the names above, or `value` is not a
    /// JSON object whose `id` string equals `id`.
    pub fn seed_object(
        &self,
        account_id: &Id,
        type_name: &'static str,
        id: &Id,
        value: serde_json::Value,
    ) {
        const KNOWN_TYPES: &[&str] = &[
            jmap_sharing_types::Principal::TYPE_NAME,
            jmap_sharing_types::ShareNotification::TYPE_NAME,
        ];
        assert!(
            KNOWN_TYPES.contains(&type_name),
            "seed_object: type_name {type_name:?} is not one of the known \
             TYPE_NAME values {KNOWN_TYPES:?}"
        );
        let obj = value.as_object().unwrap_or_else(|| {
            panic!("seed_object: value must be a JSON object with an `id` field, got {value:?}")
        });
        let value_id = obj
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "seed_object: value must contain an `id` string field; got value = {value:?}"
                )
            });
        assert_eq!(
            value_id,
            id.as_ref(),
            "seed_object: value's `id` field {value_id:?} does not match the \
             `id` argument {id:?}"
        );
        let account = account_id.as_ref();
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account.to_owned());
        inner
            .objects_mut(type_name, account)
            .insert(id.clone(), value);
    }

    /// Register one or more accounts as known even if they have no objects yet.
    ///
    /// Convenience constructor that calls [`Self::register_account`] for
    /// each id. Equivalent to `Self::new()` followed by repeated
    /// `register_account` calls. The canonical sibling
    /// `jmap-mail-server` does NOT ship this shape — it only ships
    /// `register_account(&self, &Id)`. This crate keeps `new_with_accounts`
    /// for ergonomic test fixture construction; new code SHOULD prefer
    /// `register_account` post-construction for canonical-template
    /// alignment.
    pub fn new_with_accounts(account_ids: &[&str]) -> Self {
        let b = Self::new();
        for id in account_ids {
            b.register_account(&Id::from(*id));
        }
        b
    }
}

/// Opaque storage-layer error returned by [`MemoryBackend`] operations.
///
/// Carries a human-readable description of the underlying failure
/// (serialization round-trip miss, account-not-registered race, etc.).
/// The description is intended for test failure messages and
/// diagnostic logging; it is not a stable wire-format identifier.
///
/// # Forward compatibility
///
/// This type is `#[non_exhaustive]` and uses a named-field shape so
/// future revisions can add structured context (error kind, source
/// reference, account id, etc.) without a breaking change. Outside-
/// crate construction goes through [`MemoryError::new`]; outside-crate
/// reads go through [`MemoryError::description`].
///
/// Mirrors the canonical jmap-mail-server `MemoryError` shape.
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

    /// Construct a [`MemoryError`] for a `serde_json` failure during
    /// `context` (the operation that failed).
    ///
    /// Only the error's [`serde_json::error::Category`] is kept. A serde
    /// data error quotes the offending value (`invalid type: string
    /// "alice@example.com", expected u64`), and the `JmapBackend::Error`
    /// contract forbids PII and blob content in `Display` output
    /// (bd:JMAP-510h.37).
    pub(crate) fn serde(context: impl std::fmt::Display, e: &serde_json::Error) -> Self {
        Self::new(format!("{context}: serde_json {:?} error", e.classify()))
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
                                return Err(MemoryError::serde(
                                    format_args!("deserialize {}", O::TYPE_NAME),
                                    &e,
                                ));
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
                                return Err(MemoryError::serde(
                                    format_args!("deserialize {}", O::TYPE_NAME),
                                    &e,
                                ));
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
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let since_state = params.since_state;
        let max_changes = params.max_changes;
        // draft-ietf-jmap-metadata-04 §3: jmap_server::handle_changes has
        // already folded a `using` without the capability into this flag.
        let mode = if params.ignore_metadata_only_changes {
            MetadataOnlyChanges::Ignore
        } else {
            MetadataOnlyChanges::Report
        };
        let inner = self.inner.lock().unwrap();

        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let log = inner
            .change_log
            .get(O::TYPE_NAME)
            .and_then(|m| m.get(account_id.as_ref()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        // Under Ignore a metadata-only update entry is dropped here: each
        // update entry names one id, so an id stays in `updated` exactly
        // when one of its updates since sinceState touched more than
        // metadata (-04 §3.3: "exclude from the updated array any id whose
        // only changes since sinceState ... are confined to metadata and/or
        // privateMetadata"), and the dropped entries do not count toward
        // maxChanges ("Excluded ids do not count toward maxChanges").
        let relevant: Vec<&ChangeEntry> = log
            .iter()
            .filter(|e| e.new_state > since_n)
            .filter(|e| !(mode == MetadataOnlyChanges::Ignore && e.is_metadata_only_update()))
            .collect();

        let current_state = inner.current_state(O::TYPE_NAME, account_id.as_ref());

        if let Some(max) = max_changes
            && relevant.len() as u64 > max
        {
            return Err(BackendChangesError::TooManyChanges { limit: max });
        }

        // RFC 8620 §5.2 ID deduplication across the window, using the
        // canonical jmap-mail-server `IdFate` rules. `order` keeps first-seen
        // order so the response lists are deterministic. A create after a destroy
        // (recycled id, bd:JMAP-3t94.3) overwrites the earlier fate.
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        let mut order: Vec<Id> = Vec::new();
        let mut window_props = ChangedProperties::new();
        for entry in &relevant {
            for id in &entry.created {
                if fates.insert(id.clone(), IdFate::Created).is_none() {
                    order.push(id.clone());
                }
            }
            for id in &entry.updated {
                window_props.record(id.as_ref(), entry.updated_props.as_ref());
                let fate = match fates.get(id) {
                    Some(IdFate::Created) => IdFate::Created,
                    Some(IdFate::Destroyed) => IdFate::Destroyed,
                    _ => IdFate::Updated,
                };
                if fates.insert(id.clone(), fate).is_none() {
                    order.push(id.clone());
                }
            }
            for id in &entry.destroyed {
                // "If a record has been created AND destroyed since the old
                // state, the server SHOULD remove the id from the response
                // entirely." (RFC 8620 §5.2)
                if matches!(fates.get(id), Some(IdFate::Created)) {
                    fates.remove(id);
                } else if fates.insert(id.clone(), IdFate::Destroyed).is_none() {
                    order.push(id.clone());
                }
            }
        }

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();
        // `fates.remove` emits each id once and skips ids omitted above.
        for id in order {
            match fates.remove(&id) {
                Some(IdFate::Created) => created.push(id),
                Some(IdFate::Updated) => updated.push(id),
                Some(IdFate::Destroyed) => destroyed.push(id),
                None => {}
            }
        }

        // -04 §3.3 `updatedProperties`; Principal and ShareNotification
        // have none of the RFC 8621 §2.2 count properties that only
        // Mailbox/changes may list.
        let updated_properties = window_props.updated_properties(&updated, mode, &[]);
        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            false,
            State::from(current_state.to_string()),
        )
        .with_updated_properties(updated_properties))
    }

    /// **Reference-only** implementation of [`JmapBackend::query_objects`].
    ///
    /// This implementation **ignores `filter` and `sort`**: it returns every
    /// stored object for the type/account in lexicographic id order,
    /// independent of any filter expression or comparator list passed by the
    /// caller. Pagination (`position`, `limit`) is honored.
    ///
    /// Production backends MUST evaluate the filter expression and apply the
    /// comparators per RFC 8620 §5.5 / RFC 9670 §2.4 (Principal/query) and
    /// §3.4 (ShareNotification/query). Failing to do so causes clients to
    /// receive ALL objects when they asked for a filtered subset, with no
    /// loud error signal.
    ///
    /// The crate's integration tests wrap this backend in a
    /// `FilteringBackend` (see `tests/query_changes_tests.rs`) when filter
    /// evaluation is required. Downstream implementors building a real
    /// backend should treat that wrapper as the contract, not this method.
    ///
    /// Exception: a Principal filter that carries a draft-ietf-jmap-metadata-04
    /// §3.6 condition is evaluated (see [`MemoryBackend::with_metadata_info`]).
    ///
    /// Sibling-scope: six other `MemoryBackend` reference impls
    /// (chat, calendars, contacts, metadata, tasks, filenode) carry the
    /// same gap.
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        let QueryParams {
            filter,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
        let inner = self.inner.lock().unwrap();

        // draft-ietf-jmap-metadata-04 §3.6 on Principal: validate the
        // `metadata*` conditions, then evaluate a filter that has any.
        let metadata_filter = match filter {
            Some(f) if O::TYPE_NAME == Principal::TYPE_NAME => {
                let v = serde_json::to_value(f)
                    .map_err(|e| MemoryError::serde("serialize filter", &e))?;
                check_metadata_filter(&v, inner.metadata_info.as_ref())?;
                let f: PrincipalFilter = serde_json::from_value(v)
                    .map_err(|e| MemoryError::serde("parse filter", &e))?;
                if has_metadata_condition(&f) {
                    // Rejects an unevaluable leaf even when no object is
                    // stored: every leaf is visited, matched against `{}`.
                    principal_metadata_matches(&f, &serde_json::Map::new())?;
                    Some(f)
                } else {
                    None
                }
            }
            _ => None,
        };

        let empty = serde_json::Map::new();
        let mut ids: Vec<Id> = Vec::new();
        for (id, obj) in inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .into_iter()
            .flatten()
        {
            let keep = match &metadata_filter {
                Some(f) => {
                    let metadata = obj
                        .get("metadata")
                        .and_then(serde_json::Value::as_object)
                        .unwrap_or(&empty);
                    principal_metadata_matches(f, metadata)?
                }
                None => true,
            };
            if keep {
                ids.push(id.clone());
            }
        }
        ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));

        let total = ids.len() as u64;
        let query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string(),
        );

        // RFC 8620 §5.5 position / anchor / anchorOffset.
        let start = resolve_query_start(&ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;

        ids = ids[start..]
            .iter()
            .take(limit_or_max(limit))
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

    /// **Reference-only** implementation of [`JmapBackend::query_changes`].
    ///
    /// Delegates to [`Self::get_changes`] for the change interval and
    /// inherits the same filter/sort gap as [`Self::query_objects`]: the
    /// `filter`, `sort`, `up_to_id`, and `collapse_threads` parameters are
    /// ignored. Production backends MUST honor them per RFC 8620 §5.6 to
    /// produce a coherent `added`/`removed` diff against the filtered,
    /// sorted view.
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let QueryChangesParams {
            since_query_state,
            max_changes,
            ..
        } = params;
        let changes = self
            .get_changes::<O>(
                &(),
                account_id,
                jmap_server::ChangesParams::new(since_query_state).max_changes(max_changes),
            )
            .await?;

        let inner = self.inner.lock().unwrap();
        let new_query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string(),
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

impl SharingBackend for MemoryBackend {
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
        let server_id = Id::from(uuid::Uuid::new_v4().to_string());
        let stored_obj = build(&server_id);
        // Uses `map_err` because `O` is an associated type controlled by the
        // consumer: a downstream `SetObject` impl with a hand-rolled
        // `Serialize` could fail.
        let val = serde_json::to_value(&stored_obj)
            .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize", &e)))?;

        let mut inner = self.inner.lock().unwrap();

        if let Some(obj) = val.as_object() {
            check_metadata_create(metadata_info_for::<O>(&inner), obj)?;
        }

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
                updated_props: None,
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
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize patch", &e)))?;
        let serde_json::Value::Object(patch_map) = patch_val else {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("PatchObject must be a JSON object"),
            )));
        };
        let invalid_patch = |description: String| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch).with_description(description),
            ))
        };
        // draft-ietf-jmap-metadata-04 §3.2: "Patch errors that can be
        // determined from the request alone (for example, two paths where
        // one is a prefix of the other) MUST be reported with
        // "invalidPatch" before rule 1 is evaluated."
        if patch_map.keys().any(|k| crate::helpers::is_metadata_key(k)) {
            check_patch_paths(&patch_map)
                .map_err(|e: PatchPathError| invalid_patch(e.to_string()))?;
        }

        let mut inner = self.inner.lock().unwrap();
        let metadata_info = metadata_info_for::<O>(&inner).cloned();

        // -04 §3.2 rule 1: "If the user cannot read the related object,
        // reject with "notFound"". Here only a missing account or object is
        // unreadable.
        //
        // Backend-canonical re-check (AGENTS.md "Caller identity" /
        // "Permission enforcement: backend canonical"). The handler-layer
        // `account_exists` check is the optional pre-check; the backend
        // MUST re-verify atomically with the mutation. `NotFound` is
        // returned per RFC 8620 §5.3: an unknown account has no objects,
        // so any update target is `notFound`.
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        }

        // `.cloned()` is deliberate: it preserves atomic-on-failure
        // semantics. The patch is applied to the local clone first,
        // and only the successful result is written back to storage.
        // A failed patch discards the local without touching the
        // stored value.
        let mut current = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned()
            .ok_or_else(|| {
                BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
            })?;

        let serde_json::Value::Object(base) = &mut current else {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "stored {} is not a JSON object",
                O::TYPE_NAME
            ))));
        };
        // -04 §3.2 rules 2 and 3 run before the patch is applied.
        let info = check_metadata_set(
            metadata_info.as_ref(),
            SetKind::Update,
            &patch_map,
            Some(base),
        )?;
        // -04 §2 `metadata` has "default: {}", so a
        // `metadata/<namespace>` pointer finds its parent.
        if info.is_some() {
            base.entry("metadata")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        }
        let before = base.clone();
        // Apply the JMAP PatchObject (RFC 8620 §5.3): keys are JSON Pointer
        // paths and each value replaces the member at its path wholesale.
        // Any `PatchPathError` is an RFC 8620 §5.3 "invalidPatch", including
        // the shared helper's nesting cap on path plus value depth. The
        // value-dependent metadata patch errors (missing parent, path
        // through an array) land here, after rule 3 and before rule 4 (-04
        // §3.2).
        //
        // `map_err` (not `.expect`) on `to_value(&patch)` above because
        // `O::Patch` is a consumer-controlled associated type — see the
        // `create_object` comment above for the full rationale.
        apply_patch_object(base, &patch_map).map_err(|e| invalid_patch(e.to_string()))?;
        if let Some(info) = info {
            // -04 §3.2: "In an update, null is equivalent to {}"; the patch
            // removed the property instead.
            for property in ["metadata", "privateMetadata"] {
                if patch_map
                    .get(property)
                    .is_some_and(serde_json::Value::is_null)
                {
                    base.insert(
                        property.to_owned(),
                        serde_json::Value::Object(serde_json::Map::new()),
                    );
                }
            }
            // -04 §3.2 rule 4 runs on the value as it would be stored.
            check_metadata_depth(&patch_map, base, info)
                .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
        }
        let updated_props = Some(changed_properties(&before, base));
        // A null or wrong-typed value on a required property must not reach
        // storage, or every later /get fails to decode it.
        if O::deserialize(&current).is_err() {
            return Err(invalid_patched_properties(O::TYPE_NAME, &patch_map));
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
                updated_props,
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

        // Backend-canonical re-check (AGENTS.md "Caller identity" /
        // "Permission enforcement: backend canonical"). See `update_object`
        // for the rationale; `NotFound` is the correct SetErrorType for
        // an unknown account because no object can exist there.
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        }

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
                        updated_props: None,
                    });
                Ok(())
            }
            None => Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            )))),
        }
    }

    fn supports_type<O: JmapObject>(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// draft-ietf-jmap-metadata-04 helpers
// ---------------------------------------------------------------------------

/// The metadata configuration that applies to `O`: the one set by
/// [`MemoryBackend::with_metadata_info`] for Principal, `None` for any
/// other type.
fn metadata_info_for<O: JmapObject>(inner: &Inner) -> Option<&DataTypeMetadataInfo> {
    if O::TYPE_NAME == Principal::TYPE_NAME {
        inner.metadata_info.as_ref()
    } else {
        None
    }
}

fn invalid_metadata_property(property: &str, description: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([property])
            .with_description(description),
    ))
}

/// draft-ietf-jmap-metadata-04 §3.2 rules 2 and 3 for a metadata write,
/// before the write is applied. Returns the DataTypeMetadataInfo for the
/// rule 4 check after applying, or `None` when `entry` writes no metadata.
///
/// `entry` is the create object or update patch and `stored` the object
/// before an update (`None` on create). Rule 1's `notFound` is the
/// caller's; its `forbidden` never applies, because a Principal has no
/// rights object and this backend has no read-only accounts (-04 §4.1).
fn check_metadata_set<'a>(
    info: Option<&'a DataTypeMetadataInfo>,
    kind: SetKind,
    entry: &serde_json::Map<String, serde_json::Value>,
    stored: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<Option<&'a DataTypeMetadataInfo>, BackendSetError<MemoryError>> {
    let Some(first) = entry.keys().find(|k| crate::helpers::is_metadata_key(k)) else {
        return Ok(None);
    };
    // -04 §1.2.1: an unlisted type "does not gain the metadata or
    // privateMetadata properties".
    let Some(info) = info else {
        return Err(invalid_metadata_property(
            first,
            "metadata is not supported on this data type",
        ));
    };
    check_metadata_write(kind, entry, stored, info)
        .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
    Ok(Some(info))
}

/// draft-ietf-jmap-metadata-04 §3.2 checks for a created object `obj`:
/// rules 2 and 3 on its metadata properties, then rule 4 on the stored
/// value.
fn check_metadata_create(
    info: Option<&DataTypeMetadataInfo>,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), BackendSetError<MemoryError>> {
    let entry: serde_json::Map<String, serde_json::Value> = obj
        .iter()
        .filter(|(k, _)| is_metadata_property(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Some(info) = check_metadata_set(info, SetKind::Create, &entry, None)? {
        check_metadata_depth(&entry, obj, info)
            .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
    }
    Ok(())
}

/// The six draft-ietf-jmap-metadata-04 §3.6 FilterCondition fields.
const METADATA_CONDITION_KEYS: [&str; 6] = [
    "metadataExists",
    "privateMetadataExists",
    "metadataTextContains",
    "privateMetadataTextContains",
    "metadataTextEquals",
    "privateMetadataTextEquals",
];

fn unsupported_filter(description: &str) -> BackendQueryError<MemoryError> {
    let mut err = JmapError::unsupported_filter();
    err.description = Some(description.to_owned());
    BackendQueryError::Method(err)
}

/// Validate the draft-ietf-jmap-metadata-04 §3.6 conditions on every leaf
/// of `filter`, a FilterCondition or FilterOperator tree (RFC 8620 §5.5),
/// before any object is matched.
///
/// Without `info` Principal has no metadata (-04 §1.2.1: it "does not gain
/// the metadata or privateMetadata properties"), so a metadata leaf is
/// `unsupportedFilter` (RFC 8620 §5.5: "The filter is syntactically valid,
/// but the server cannot process it"). Otherwise
/// [`MetadataCondition::take_from`] applies -04 §3.6: a malformed path is
/// `invalidArguments`, and a `privateMetadata*` condition is
/// `unsupportedFilter` because `supportsPrivate` is false.
fn check_metadata_filter(
    filter: &serde_json::Value,
    info: Option<&DataTypeMetadataInfo>,
) -> Result<(), BackendQueryError<MemoryError>> {
    if filter.get("operator").is_some() {
        return filter
            .get("conditions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .try_for_each(|f| check_metadata_filter(f, info));
    }
    let Some(leaf) = filter.as_object() else {
        return Ok(());
    };
    if !METADATA_CONDITION_KEYS
        .iter()
        .any(|k| leaf.get(*k).is_some_and(|v| !v.is_null()))
    {
        return Ok(());
    }
    let Some(info) = info else {
        return Err(unsupported_filter(
            "metadata filter conditions are not supported",
        ));
    };
    MetadataCondition::take_from(&mut leaf.clone(), info)
        .map(drop)
        .map_err(|e| match e {
            MetadataFilterError::InvalidArguments(d) => {
                BackendQueryError::Method(JmapError::invalid_arguments(d))
            }
            other => unsupported_filter(&other.to_string()),
        })
}

/// Whether any leaf of `filter` carries a draft-ietf-jmap-metadata-04 §3.6
/// condition.
fn has_metadata_condition(filter: &PrincipalFilter) -> bool {
    match filter {
        Filter::Operator(op) => op.conditions.iter().any(has_metadata_condition),
        Filter::Condition(c) => c.has_metadata_condition(),
        _ => false,
    }
}

/// Evaluate `filter` against one Principal's `metadata` (absent means
/// `{}`, -04 §2), with RFC 8620 §5.5 AND/OR/NOT. Every leaf is evaluated,
/// so an unevaluable one is reported whatever the others match: a leaf
/// with an RFC 9670 §2.4.1 condition is `unsupportedFilter`, as this
/// backend evaluates none of them (RFC 8620 §5.5: a server "MUST reject the
/// call with an unsupportedFilter error if it cannot process the given
/// filter"). The `privateMetadata*` conditions never reach this point:
/// [`check_metadata_filter`] rejects them while `supportsPrivate` is false.
fn principal_metadata_matches(
    filter: &PrincipalFilter,
    metadata: &serde_json::Map<String, serde_json::Value>,
) -> Result<bool, BackendQueryError<MemoryError>> {
    match filter {
        Filter::Operator(op) => {
            let results = op
                .conditions
                .iter()
                .map(|c| principal_metadata_matches(c, metadata))
                .collect::<Result<Vec<bool>, _>>()?;
            match op.operator {
                Operator::And => Ok(results.iter().all(|m| *m)),
                Operator::Or => Ok(results.iter().any(|m| *m)),
                Operator::Not => Ok(!results.iter().any(|m| *m)),
                _ => Err(unsupported_filter("unknown filter operator")),
            }
        }
        Filter::Condition(c) => {
            if c.account_ids.is_some()
                || c.email.is_some()
                || c.name.is_some()
                || c.text.is_some()
                || c.type_.is_some()
                || c.time_zone.is_some()
            {
                return Err(unsupported_filter(
                    "this backend evaluates only metadata conditions in a filter that has one",
                ));
            }
            Ok(metadata_matches(
                metadata,
                c.metadata_exists.as_deref(),
                c.metadata_text_contains.as_ref(),
                c.metadata_text_equals.as_ref(),
            ))
        }
        _ => Err(unsupported_filter("unknown filter shape")),
    }
}

/// Match the draft-ietf-jmap-metadata-04 §3.6 shared-metadata conditions of
/// one filter leaf against an object's `metadata`.
fn metadata_matches(
    metadata: &serde_json::Map<String, serde_json::Value>,
    exists: Option<&str>,
    contains: Option<&MetadataTextMatch>,
    equals: Option<&MetadataTextMatch>,
) -> bool {
    exists.is_none_or(|p| metadata_path_exists(metadata, p))
        && contains.is_none_or(|m| metadata_text_contains(metadata, m))
        && equals.is_none_or(|m| metadata_text_equals(metadata, m))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_sharing_types::Principal;

    /// Oracle: create a Principal then get it back by id.
    ///
    /// Verifies that create_object stores the object and get_objects can
    /// retrieve it with the server-assigned id.
    #[tokio::test]
    async fn memory_backend_principal_create_get_roundtrip() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let principal = |id: &Id| -> Principal {
            serde_json::from_value(serde_json::json!({
                "type": "individual",
                "name": "Test User",
                "id": id,
                "description": null,
                "email": null,
                "timeZone": null,
                "capabilities": {},
                "accounts": null
            }))
            .expect("must deserialize")
        };

        let (new_id, _) = backend
            .create_object::<Principal, _>(&(), &Id::from("acc1"), "c1", principal)
            .await
            .expect("create must succeed");

        let (found, not_found) = backend
            .get_objects::<Principal>(
                &(),
                &Id::from("acc1"),
                Some(std::slice::from_ref(&new_id)),
                None,
            )
            .await
            .expect("get must succeed");

        assert!(
            not_found.is_empty(),
            "newly created principal must be found, not_found: {not_found:?}"
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.as_ref(), new_id.as_ref());
    }

    /// Oracle: a synthetic create → destroy → create sequence on the SAME id
    /// (the recycled-id case from bd:JMAP-3t94.3) MUST land in `created` in
    /// the /changes response, not silently disappear.
    ///
    /// Drives change_log_mut directly because the public surface mints fresh
    /// ids and would not naturally produce the recycle. RFC 8620 §5.2: net
    /// effect across the interval determines the bucket; here the id IS
    /// present in the final state so it MUST appear in `created`.
    #[tokio::test]
    async fn get_changes_recycled_id_lands_in_created() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let recycled = Id::from("recycled");

        {
            let mut inner = backend.inner.lock().unwrap();
            let log = inner.change_log_mut(Principal::TYPE_NAME, "acc1");
            log.push(ChangeEntry {
                new_state: 1,
                created: vec![recycled.clone()],
                updated: vec![],
                destroyed: vec![],
                updated_props: None,
            });
            log.push(ChangeEntry {
                new_state: 2,
                created: vec![],
                updated: vec![],
                destroyed: vec![recycled.clone()],
                updated_props: None,
            });
            log.push(ChangeEntry {
                new_state: 3,
                created: vec![recycled.clone()],
                updated: vec![],
                destroyed: vec![],
                updated_props: None,
            });
            inner
                .states
                .entry(Principal::TYPE_NAME)
                .or_default()
                .insert("acc1".to_owned(), 3);
        }

        let changes = backend
            .get_changes::<Principal>(
                &(),
                &Id::from("acc1"),
                jmap_server::ChangesParams::new(&State::from("0")),
            )
            .await
            .expect("get_changes must succeed");

        assert!(
            changes.created.contains(&recycled),
            "recycled id must appear in created; got created={:?} destroyed={:?}",
            changes.created,
            changes.destroyed
        );
        assert!(
            !changes.destroyed.contains(&recycled),
            "recycled id must NOT appear in destroyed (final state has it); \
             got destroyed={:?}",
            changes.destroyed
        );
    }

    /// Oracle: `SharingBackend::create_object` invariant — the returned `O`
    /// MUST carry the server-assigned `id` as its own `id` field.
    /// Regression guard for bd:JMAP-3t94.17.
    #[tokio::test]
    async fn create_object_returned_o_carries_server_assigned_id() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let principal = |id: &Id| -> Principal {
            serde_json::from_value(serde_json::json!({
                "type": "individual",
                "name": "Invariant Probe",
                "id": id,
                "description": null,
                "email": null,
                "timeZone": null,
                "capabilities": {},
                "accounts": null
            }))
            .expect("must deserialize")
        };

        let (new_id, stored_obj) = backend
            .create_object::<Principal, _>(&(), &Id::from("acc1"), "c1", principal)
            .await
            .expect("create must succeed");

        assert_ne!(
            new_id.as_ref(),
            "placeholder",
            "server-assigned id must differ from placeholder"
        );
        assert_eq!(
            stored_obj.id.as_ref(),
            new_id.as_ref(),
            "returned O.id MUST equal the server-assigned tuple Id \
             (SharingBackend::create_object invariant); got O.id={:?}, \
             tuple Id={:?}",
            stored_obj.id.as_ref(),
            new_id.as_ref()
        );
    }

    /// Oracle: RFC 8620 §5.2, "If a record has been created AND destroyed
    /// since the old state, the server SHOULD remove the id from the
    /// response entirely." The MemoryBackend takes the SHOULD path, matching
    /// the canonical jmap-mail-server `IdFate` rules.
    #[tokio::test]
    async fn get_changes_create_then_destroy_is_omitted() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let ephemeral = Id::from("ephemeral");

        {
            let mut inner = backend.inner.lock().unwrap();
            let log = inner.change_log_mut(Principal::TYPE_NAME, "acc1");
            log.push(ChangeEntry {
                new_state: 1,
                created: vec![ephemeral.clone()],
                updated: vec![],
                destroyed: vec![],
                updated_props: None,
            });
            log.push(ChangeEntry {
                new_state: 2,
                created: vec![],
                updated: vec![],
                destroyed: vec![ephemeral.clone()],
                updated_props: None,
            });
            inner
                .states
                .entry(Principal::TYPE_NAME)
                .or_default()
                .insert("acc1".to_owned(), 2);
        }

        let changes = backend
            .get_changes::<Principal>(
                &(),
                &Id::from("acc1"),
                jmap_server::ChangesParams::new(&State::from("0")),
            )
            .await
            .expect("get_changes must succeed");

        assert!(
            !changes.created.contains(&ephemeral),
            "ephemeral id MUST NOT appear in created (final state lacks it); \
             got created={:?}",
            changes.created
        );
        assert!(
            !changes.destroyed.contains(&ephemeral),
            "create→destroy in one window SHOULD be omitted from destroyed; \
             got destroyed={:?}",
            changes.destroyed
        );
    }
}

#[cfg(test)]
mod memory_error_tests {
    use super::MemoryError;

    /// The `JmapBackend::Error` contract: "Implementations MUST NOT
    /// include ... PII shaped like an email address" in `Display`
    /// output. serde_json data errors quote the offending value, so
    /// [`MemoryError::serde`] must drop the message (bd:JMAP-510h.37).
    #[test]
    fn serde_error_display_omits_offending_value() {
        let canary = "alice@example.com";
        let e = serde_json::from_value::<u64>(serde_json::Value::String(canary.into()))
            .expect_err("a string is not a u64");
        assert!(
            e.to_string().contains(canary),
            "precondition: serde_json quotes the value"
        );

        let shown = MemoryError::serde("decode", &e).to_string();
        assert!(!shown.contains(canary), "canary leaked: {shown}");
        assert!(
            shown.contains("decode") && shown.contains("Data"),
            "{shown}"
        );
    }
}

#[cfg(test)]
mod clone_tests {
    use super::MemoryBackend;
    use std::sync::Arc;

    /// Oracle (bd:JMAP-q7k6): `MemoryBackend::clone` is a shared-state
    /// `Arc` clone, never a deep copy, so a clone and its original hold
    /// the same store while a fresh instance does not.
    #[test]
    fn clone_shares_store() {
        let a = MemoryBackend::new();
        assert!(Arc::ptr_eq(&a.inner, &a.clone().inner));
        assert!(!Arc::ptr_eq(&a.inner, &MemoryBackend::new().inner));
    }
}

#[cfg(test)]
mod fixture_helper_tests {
    use super::*;
    use serde_json::json;

    /// `with_account` makes the account known (RFC 8620 §3.6.2: an
    /// unknown accountId is `accountNotFound`, so `account_exists` is the
    /// observable).
    #[tokio::test]
    async fn with_account_registers_account() {
        let acc = Id::from("acc1");
        let backend = MemoryBackend::new().with_account(&acc);
        assert!(backend.account_exists(&(), &acc).await.unwrap());
        assert!(
            !backend
                .account_exists(&(), &Id::from("acc2"))
                .await
                .unwrap()
        );
    }

    /// A seeded object is readable, registers its account, and leaves the
    /// state at its initial value. Fixture: hand-written JSON.
    #[tokio::test]
    async fn seed_object_is_readable_without_state_change() {
        let acc = Id::from("acc1");
        let backend = MemoryBackend::new();
        let before = backend
            .get_state::<jmap_sharing_types::Principal>(&(), &acc)
            .await
            .unwrap();
        backend.seed_object(
            &acc,
            jmap_sharing_types::Principal::TYPE_NAME,
            &Id::from("x1"),
            json!({"id": "x1", "type": "individual", "name": "Alice", "description": null, "email": null, "timeZone": null, "capabilities": {}, "accounts": null}),
        );
        assert!(backend.account_exists(&(), &acc).await.unwrap());
        let (found, not_found) = backend
            .get_objects::<jmap_sharing_types::Principal>(&(), &acc, Some(&[Id::from("x1")]), None)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert!(not_found.is_empty());
        assert_eq!(
            backend
                .get_state::<jmap_sharing_types::Principal>(&(), &acc)
                .await
                .unwrap(),
            before
        );
    }

    #[test]
    #[should_panic(expected = "not one of the known")]
    fn seed_object_rejects_unknown_type_name() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            "NoSuchType",
            &Id::from("x1"),
            json!({"id": "x1"}),
        );
    }

    #[test]
    #[should_panic(expected = "does not match the `id` argument")]
    fn seed_object_rejects_id_mismatch() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            jmap_sharing_types::Principal::TYPE_NAME,
            &Id::from("x1"),
            json!({"id": "other"}),
        );
    }
}

#[cfg(all(test, debug_assertions))]
mod empty_account_id_tests {
    use super::{Id, MemoryBackend};

    /// Oracle: RFC 8620 §1.2, an Id "is a String of at least 1 and a
    /// maximum of 255 octets in size" (bd:JMAP-dx0s).
    #[test]
    #[should_panic(expected = "account_id must not be the empty string")]
    fn register_account_rejects_empty_id() {
        MemoryBackend::new().register_account(&Id::from(""));
    }
}

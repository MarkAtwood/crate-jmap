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
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, ChatBackend, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};
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
    objects: HashMap<(String, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(String, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(String, String), Vec<ChangeEntry>>,
    /// explicitly registered account ids (accounts may exist with no objects yet)
    known_accounts: HashSet<String>,
}

impl Inner {
    fn current_state(&self, type_name: &str, account_id: &str) -> u64 {
        *self
            .states
            .get(&(type_name.to_owned(), account_id.to_owned()))
            .unwrap_or(&0)
    }

    fn bump_state(&mut self, type_name: &str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        self.known_accounts.insert(account_id.to_owned());
        self.objects
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects
            .get(&(type_name.to_owned(), account_id.to_owned()))
    }

    fn change_log_mut(&mut self, type_name: &str, account_id: &str) -> &mut Vec<ChangeEntry> {
        self.change_log
            .entry((type_name.to_owned(), account_id.to_owned()))
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
    fn next_id(inner: &mut Inner, type_name: &str, account_id: &str) -> Id {
        let n = inner
            .objects_ref(type_name, account_id)
            .map_or(0, |m| m.len());
        Id::from(format!("{}{}", type_name.to_ascii_lowercase(), n + 1).as_str())
    }
}

/// A simple string error for `MemoryBackend` failures.
#[derive(Debug, Clone)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MemoryError {}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;

    async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.known_accounts.contains(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
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
                            Err(e) => return Err(MemoryError(e.to_string())),
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
                            Err(e) => return Err(MemoryError(e.to_string())),
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
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
        Ok(State::from(n.to_string().as_str()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
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
            .get(&(O::TYPE_NAME.to_owned(), account_id.as_ref().to_owned()))
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
            State::from(current_state.to_string().as_str()),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
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
            start as i64,
            Some(total),
            query_state,
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
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
            .get_changes::<O>(account_id, since_query_state, max_changes)
            .await?;

        let inner = self.inner.lock().unwrap();
        let new_query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string()
                .as_str(),
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
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        let server_id = Self::next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Serialize, set "id" to the server-assigned id, then deserialize back.
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        val["id"] = serde_json::Value::String(server_id.as_ref().to_owned());
        let stored_obj: O =
            O::deserialize(&val).map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

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
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
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
}

// ---------------------------------------------------------------------------
// JSON Merge Patch (RFC 7396)
// ---------------------------------------------------------------------------

/// Apply a JSON Merge Patch (RFC 7396) to `target` in-place.
///
/// Patches deeper than [`MAX_MERGE_PATCH_DEPTH`] are silently truncated to
/// bound stack use on adversarial input (bd:JMAP-sc1b.97). Below the cap the
/// behaviour is exactly RFC 7396.
fn json_merge_patch(target: &mut serde_json::Value, patch: serde_json::Value) {
    json_merge_patch_inner(target, patch, 0);
}

/// Maximum recursion depth for JSON Merge Patch application.
///
/// Beyond this depth the patch is silently ignored at the affected sub-tree:
/// the target value at that level is left unchanged. Mitigates stack DoS
/// from adversarial `PatchObject` values (bd:JMAP-sc1b.97). 32 levels
/// comfortably exceeds any legitimate JMAP `/set update` shape.
const MAX_MERGE_PATCH_DEPTH: usize = 32;

fn json_merge_patch_inner(target: &mut serde_json::Value, patch: serde_json::Value, depth: usize) {
    if depth > MAX_MERGE_PATCH_DEPTH {
        return;
    }
    match patch {
        serde_json::Value::Object(patch_map) => {
            let target_map = target
                .as_object_mut()
                .expect("merge patch target must be an object");
            for (key, patch_val) in patch_map {
                if patch_val.is_null() {
                    target_map.remove(&key);
                } else {
                    let entry = target_map.entry(key).or_insert(serde_json::Value::Null);
                    json_merge_patch_inner(entry, patch_val, depth + 1);
                }
            }
        }
        other => *target = other,
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

    /// Oracle: bd:JMAP-sc1b.97 — a 1000-deep merge patch must NOT crash via
    /// stack overflow. The depth cap silently truncates beyond
    /// MAX_MERGE_PATCH_DEPTH, so the call returns; the topmost levels are
    /// applied, and the deeper levels are ignored.
    ///
    /// The test does not use the function as its own oracle: the input is
    /// hand-built (a 1000-deep `{ "a": { "a": ... { "a": {} } } }` chain
    /// where every level is Object, matching the structural shape of a
    /// real PatchObject — the documented latent panic from bd:JMAP-sc1b.87
    /// only fires on non-Object leaves, which a typed PatchObject cannot
    /// produce). The assertion only checks that the call completes without
    /// panicking and without overflowing the stack. A pre-fix
    /// recursion-unlimited implementation would overflow before returning.
    #[test]
    fn json_merge_patch_does_not_stack_overflow() {
        const DEPTH: usize = 1000;
        // Build a target with the same Object-only shape, so the recursion
        // walks legitimately at every level until the cap fires.
        let mut target = serde_json::json!({});
        for _ in 0..DEPTH {
            target = serde_json::json!({ "a": target });
        }
        let mut patch = serde_json::json!({});
        for _ in 0..DEPTH {
            patch = serde_json::json!({ "a": patch });
        }
        // If the function recursed without bound this call would crash the
        // test thread on stack overflow. With the depth cap it returns.
        json_merge_patch(&mut target, patch);
        // We do not assert the *contents* of `target` (that would require
        // the test to track exactly what the depth cap retains, which
        // amounts to using the function as its own oracle). We assert
        // only that target was mutated to an Object — the outermost
        // patch level always applies.
        assert!(
            target.is_object(),
            "after a deeply-nested merge patch, target must remain a JSON object; got {target:?}"
        );
    }

    /// Oracle: a shallow merge patch under the cap still applies normally.
    /// Positive control paired with the stack-overflow test above to prove
    /// the depth cap only fires at the boundary, not on every call.
    #[test]
    fn json_merge_patch_shallow_applies_normally() {
        let mut target = serde_json::json!({ "a": 1, "b": { "c": 2 } });
        let patch = serde_json::json!({ "b": { "c": 99, "d": 7 }, "e": null });
        json_merge_patch(&mut target, patch);
        // RFC 7396 §3 example shape: replaced fields, added fields, null
        // removes. The expected JSON is hand-derived from the patch and
        // RFC, not from the function output.
        assert_eq!(
            target,
            serde_json::json!({ "a": 1, "b": { "c": 99, "d": 7 } }),
            "RFC 7396 merge semantics broken at shallow depth"
        );
    }
}

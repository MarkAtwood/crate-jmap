//! Object history for the memory backend (draft-ietf-jmap-object-history-03
//! with the -04 editor's copy changes; docs/design/object-history.md §7),
//! for [`AddressBook`] and [`ContactCard`].
//!
//! Off until [`MemoryBackend::with_object_history`]. Versions live in a
//! `jmap_object_history_server::HistoryStore` inside the scoped state, so a
//! conditional rollback discards them with the rest.

use std::collections::HashMap;

use jmap_contacts_types::{AddressBook, ContactCard, ContactCardFilter};
use jmap_object_history_server::{HistoryRetention, HistoryStore};
use jmap_object_history_types::{HistoryEntry, HistoryObject, ObjectHistoryAccountCapability};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError,
    DestroyedQueryChangesParams, DestroyedQueryParams, HistoryGetParams, HistoryGetResult,
    JmapObject, MemoryCaller, ObjectHistoryBackend, QueryChangesResult, QueryObject, QueryResult,
    Restored, SetError, SetErrorType, SetObject, apply_restore_patch, limit_or_max,
    resolve_query_start,
};
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::{Map, Value};

use super::{
    ChangeEntry, Inner, MemoryBackend, MemoryError, compare_contact_cards_by_property,
    contact_card_matches_filter,
};

/// The types this backend keeps history for.
const SUPPORTED: &[&str] = &["AddressBook", "ContactCard"];

impl Inner {
    /// Record the update of `id` from `previous`, when the type keeps
    /// history.
    pub(super) fn record_replace(
        &mut self,
        type_name: &str,
        account: &Id,
        id: &Id,
        previous: Value,
    ) {
        if let Some(h) = self.history.as_mut() {
            h.record_replace(type_name, account, id, previous);
        }
    }

    /// Record the destruction of `id`, whose last state was `last`, when
    /// the type keeps history; every route counts (§3), so the
    /// `onDestroyRemoveContents` cascade too.
    pub(super) fn record_destroy(&mut self, type_name: &str, account: &Id, id: &Id, last: Value) {
        if let Some(h) = self.history.as_mut() {
            h.record_destroy(type_name, account, id, last);
        }
    }

    /// Whether minting `id` would reuse the id of a destroyed object with
    /// retained history (§5.1 forbids it).
    pub(super) fn history_retains(&self, type_name: &str, account: &Id, id: &Id) -> bool {
        self.history
            .as_ref()
            .is_some_and(|h| h.retains(type_name, account, id))
    }

    fn prune_history(&mut self) {
        if let Some(h) = self.history.as_mut() {
            h.prune();
        }
    }

    /// Destroyed [`ContactCard`]s in result order: the window, then the
    /// ContactCard/query filter and sort over final versions, most recently
    /// destroyed first when there is no sort (§4).
    fn destroyed_results<O: QueryObject>(
        &self,
        account: &Id,
        after: Option<&UTCDate>,
        before: Option<&UTCDate>,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
    ) -> Result<Vec<Id>, BackendQueryError<MemoryError>> {
        let Some(h) = self.history.as_ref() else {
            return Ok(Vec::new());
        };
        let rows = h
            .destroyed(O::TYPE_NAME, account, after, before)
            .map_err(|e| BackendQueryError::Method(JmapError::invalid_arguments(e.to_string())))?;
        if O::TYPE_NAME != ContactCard::TYPE_NAME {
            return Ok(rows.into_iter().map(|d| d.id).collect());
        }
        let filter: Option<ContactCardFilter> = filter
            .map(|f| {
                serde_json::to_value(f)
                    .and_then(serde_json::from_value)
                    .map_err(|e| BackendQueryError::Method(JmapError::server_fail(e.to_string())))
            })
            .transpose()?;
        let comparators: Vec<(String, bool)> = sort
            .unwrap_or_default()
            .iter()
            .filter_map(|c| {
                let v = serde_json::to_value(c).ok()?;
                let prop = v.get("property")?.as_str()?.to_owned();
                let asc = v
                    .get("isAscending")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                Some((prop, asc))
            })
            .collect();
        let mut kept: Vec<(Id, Value)> = rows
            .into_iter()
            .filter(|d| {
                filter
                    .as_ref()
                    .is_none_or(|f| f.matches(&|c| contact_card_matches_filter(&d.snapshot, c)))
            })
            .map(|d| (d.id, d.snapshot))
            .collect();
        // Stable over the destruction order the store returns.
        kept.sort_by(|a, b| {
            let mut ord = std::cmp::Ordering::Equal;
            for (prop, asc) in &comparators {
                if ord.is_ne() {
                    break;
                }
                let cmp = compare_contact_cards_by_property(&a.1, &b.1, prop);
                ord = if *asc { cmp } else { cmp.reverse() };
            }
            ord
        });
        Ok(kept.into_iter().map(|(id, _)| id).collect())
    }
}

impl<C: MemoryCaller> MemoryBackend<C> {
    /// Keep object history for the types `capability.supportedTypes` lists
    /// ([`AddressBook`], [`ContactCard`]), in every account, with `retention`;
    /// `capability` is also what
    /// [`ObjectHistoryBackend::object_history_capability`] advertises.
    ///
    /// # Panics
    ///
    /// When `supportedTypes` names a type without history, or another type.
    #[must_use]
    pub fn with_object_history(
        self,
        capability: ObjectHistoryAccountCapability,
        retention: HistoryRetention,
    ) -> Self {
        for t in &capability.supported_types {
            assert!(
                SUPPORTED.contains(&t.as_str()),
                "the memory backend keeps no history for {t}"
            );
        }
        self.inner.lock_unscoped().history = Some(HistoryStore::new(capability, retention));
        self
    }

    /// Fix the history clock at `ms` milliseconds since the epoch.
    pub fn set_history_clock_for_test(&self, ms: i64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.set_clock(ms);
        }
    }

    /// Discard one version of an object, in any order (§3.1).
    pub fn discard_version_for_test(&self, type_name: &str, account: &Id, id: &Id, version: u64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.discard_version(type_name, account, id, version);
        }
    }

    /// Discard every version of an object: the §8.3 administrative purge.
    pub fn purge_object_history(&self, type_name: &str, account: &Id, id: &Id) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.purge(type_name, account, id);
        }
    }

    /// Set the §8.2 cut for `type_name` in `account`: versions replaced
    /// before `ms`, and objects destroyed before it, are hidden.
    pub fn set_rights_changed_at_for_test(&self, type_name: &str, account: &Id, ms: i64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.set_rights_changed(type_name, account, ms);
        }
    }
}

fn not_found() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

fn invalid(property: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties).with_properties([property]),
    ))
}

impl<C: MemoryCaller> ObjectHistoryBackend for MemoryBackend<C> {
    fn object_history_capability(
        &self,
        _caller: &C,
        account_id: &Id,
    ) -> Option<ObjectHistoryAccountCapability> {
        let inner = self.inner.lock_unscoped();
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return None;
        }
        inner.history.as_ref().map(|h| h.capability().clone())
    }

    async fn get_object_history<O: HistoryObject>(
        &self,
        _caller: &C,
        account_id: &Id,
        params: HistoryGetParams<'_>,
    ) -> Result<HistoryGetResult<O>, BackendQueryError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.prune_history();
        let empty = HashMap::new();
        let live = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .unwrap_or(&empty);
        let Some(h) = inner.history.as_ref() else {
            return Ok(HistoryGetResult::new(
                Vec::new(),
                params.ids.map(<[Id]>::to_vec).unwrap_or_default(),
            ));
        };
        let (rows, not_found) = h
            .select(O::TYPE_NAME, account_id, params.ids, live, params.arguments)
            .map_err(|e| BackendQueryError::Method(JmapError::invalid_arguments(e.to_string())))?;
        let mut entries = Vec::with_capacity(rows.len());
        for (snapshot, info) in rows {
            let object = O::deserialize(&snapshot)
                .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))?;
            entries.push(HistoryEntry::new(object, info));
        }
        Ok(HistoryGetResult::new(entries, not_found))
    }

    async fn query_destroyed<O: HistoryObject + QueryObject>(
        &self,
        _caller: &C,
        account_id: &Id,
        params: DestroyedQueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.prune_history();
        let q = &params.query;
        let all = inner.destroyed_results::<O>(
            account_id,
            params.destroyed_after,
            params.destroyed_before,
            q.filter,
            q.sort,
        )?;
        let start = resolve_query_start(&all, q.position, q.anchor, q.anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let ids: Vec<Id> = all[start..]
            .iter()
            .take(limit_or_max(q.limit))
            .cloned()
            .collect();
        let state = inner.history.as_ref().map_or_else(String::new, |h| {
            h.destroyed_query_state(
                O::TYPE_NAME,
                account_id,
                params.destroyed_after,
                params.destroyed_before,
            )
        });
        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(all.len() as u64),
            State::from(state),
            true,
        ))
    }

    async fn query_destroyed_changes<O: HistoryObject + QueryObject>(
        &self,
        _caller: &C,
        account_id: &Id,
        params: DestroyedQueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.prune_history();
        let q = &params.query_changes;
        let all = inner
            .destroyed_results::<O>(
                account_id,
                params.destroyed_after,
                params.destroyed_before,
                q.filter,
                q.sort,
            )
            .map_err(|e| match e {
                BackendQueryError::Other(e) => BackendChangesError::Other(e),
                _ => BackendChangesError::CannotCalculate,
            })?;
        let Some(h) = inner.history.as_ref() else {
            return Err(BackendChangesError::CannotCalculate);
        };
        // A state of another kind or window cannot be calculated (decision
        // 25).
        let changed = h
            .destroyed_changes_since(
                O::TYPE_NAME,
                account_id,
                q.since_query_state.as_ref(),
                params.destroyed_after,
                params.destroyed_before,
            )
            .ok_or(BackendChangesError::CannotCalculate)?;
        if let Some(max) = q.max_changes
            && changed.len() as u64 > max
        {
            return Err(BackendChangesError::TooManyChanges { limit: max });
        }
        let new_state = h.destroyed_query_state(
            O::TYPE_NAME,
            account_id,
            params.destroyed_after,
            params.destroyed_before,
        );
        let added: Vec<AddedItem> = all
            .iter()
            .enumerate()
            .filter(|(_, id)| changed.contains(id))
            .map(|(i, id)| AddedItem::new(id.clone(), i as u64))
            .collect();
        Ok(QueryChangesResult::new(
            q.since_query_state.clone(),
            State::from(new_state),
            Some(all.len() as u64),
            changed,
            added,
        ))
    }

    async fn restore_object<O: HistoryObject + SetObject>(
        &self,
        _caller: &C,
        account_id: &Id,
        id: &Id,
        patch: Option<PatchObject>,
    ) -> Result<Restored<O>, BackendSetError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.require_account_for_target(account_id.as_ref())?;
        inner.prune_history();
        if inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .is_some_and(|m| m.contains_key(id))
        {
            return Err(not_found());
        }
        let Some(Value::Object(final_version)) = inner
            .history
            .as_ref()
            .and_then(|h| h.final_version(O::TYPE_NAME, account_id, id))
            .cloned()
        else {
            return Err(not_found());
        };
        let baseline = apply_restore_patch::<O>(&final_version, patch.as_ref())
            .map_err(BackendSetError::SetError)?;
        if O::TYPE_NAME == ContactCard::TYPE_NAME {
            // RFC 9610 §3: a card is in at least one AddressBook, and §5.2
            // of object history rejects a reference to a missing one as an
            // update would.
            let empty = HashMap::new();
            let books = inner
                .objects_ref(AddressBook::TYPE_NAME, account_id.as_ref())
                .unwrap_or(&empty);
            let ok = baseline
                .get("addressBookIds")
                .and_then(Value::as_object)
                .is_some_and(|m: &Map<String, Value>| {
                    !m.is_empty() && m.keys().all(|k| books.contains_key(&Id::from(k.as_str())))
                });
            if !ok {
                return Err(invalid("addressBookIds"));
            }
        }
        let mut stored = baseline.clone();
        if O::TYPE_NAME == AddressBook::TYPE_NAME
            && stored.get("isDefault") == Some(&Value::Bool(true))
            && inner
                .objects_ref(AddressBook::TYPE_NAME, account_id.as_ref())
                .is_some_and(|m| {
                    m.values()
                        .any(|b| b.get("isDefault") == Some(&Value::Bool(true)))
                })
        {
            // RFC 9610 §2: at most one default AddressBook; the current one
            // keeps the role and `restored` reports the change.
            stored.insert("isDefault".to_owned(), Value::Bool(false));
        }
        let value = Value::Object(stored);
        let Ok(object) = O::deserialize(&value) else {
            // -04 §5.1: values that no longer make a valid object.
            let props: Vec<String> = patch
                .as_ref()
                .map(|p| p.as_map().keys().cloned().collect())
                .filter(|v: &Vec<String>| !v.is_empty())
                .unwrap_or_else(|| vec!["id".to_owned()]);
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties).with_properties(props),
            )));
        };
        if let Some(h) = inner.history.as_mut() {
            h.restore(O::TYPE_NAME, account_id, id);
        }
        if O::TYPE_NAME == ContactCard::TYPE_NAME {
            inner.apply_contact_card_index_delta(account_id.as_ref(), None, Some(&value));
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), value);
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![],
                destroyed: vec![],
                restored: vec![id.clone()],
                updated_props: None,
            });
        Ok(Restored::new(object, baseline))
    }
}

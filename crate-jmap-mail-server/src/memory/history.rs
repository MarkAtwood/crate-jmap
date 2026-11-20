//! Object history for the memory backend (draft-ietf-jmap-object-history-03
//! with the -04 editor's copy changes; docs/design/object-history.md §7).
//!
//! Off until [`MemoryBackend::with_object_history`]. Replaced and destroyed
//! versions live in a `jmap_object_history_server::HistoryStore` inside the
//! scoped state, so a conditional rollback discards them with the rest.

use std::collections::HashMap;

use jmap_mail_types::{
    Email, EmailComparator, EmailFilter, Mailbox, MailboxComparator, MailboxComparatorProperty,
    MailboxFilter,
};
use jmap_object_history_server::{HistoryRetention, HistoryStore};
use jmap_object_history_types::{HistoryEntry, HistoryObject, ObjectHistoryAccountCapability};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError,
    DestroyedQueryChangesParams, DestroyedQueryParams, HistoryGetParams, HistoryGetResult,
    ObjectHistoryBackend, QueryChangesResult, QueryObject, QueryResult, Restored, SetError,
    SetErrorType, SetObject, apply_restore_patch, limit_or_max, resolve_query_start,
};
use jmap_types::{Id, JmapObject, PatchObject, State, UTCDate};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{
    ChangeEntry, Inner, MemoryBackend, MemoryError, ThreadIndex, mailbox_matches_condition, retype,
    sort_email_ids,
};

impl Inner {
    /// Record the update of `id` from `previous`, when the type keeps
    /// history.
    pub(super) fn record_replace(
        &mut self,
        type_name: &'static str,
        account: &Id,
        id: &Id,
        previous: Value,
    ) {
        if let Some(h) = self.history.as_mut() {
            h.record_replace(type_name, account, id, previous);
        }
    }

    /// Record the destruction of `id`, whose last state was `last`, when
    /// the type keeps history.
    pub(super) fn record_destroy(
        &mut self,
        type_name: &'static str,
        account: &Id,
        id: &Id,
        last: Value,
    ) {
        if let Some(h) = self.history.as_mut() {
            h.record_destroy(type_name, account, id, last);
        }
    }

    /// Whether minting `id` for `type_name` would reuse the id of a
    /// destroyed object with retained history (§5.1 forbids it).
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
}

impl MemoryBackend {
    /// Keep object history for the types `capability.supportedTypes` lists,
    /// in every account, with `retention`; `capability` is also what
    /// [`ObjectHistoryBackend::object_history_capability`] advertises.
    ///
    /// # Panics
    ///
    /// When `supportedTypes` names a type without history
    /// (`jmap_object_history_types::is_history_type`), or one this backend
    /// does not keep history for (Mailbox, Email, Identity, VacationResponse,
    /// and SieveScript with the `sieve` feature).
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

/// The types this backend keeps history for.
pub(super) const SUPPORTED: &[&str] = &[
    "Mailbox",
    "Email",
    "Identity",
    "VacationResponse",
    #[cfg(feature = "sieve")]
    "SieveScript",
];

fn not_found() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

fn set_err(e: SetError) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(e))
}

fn serde_fail(e: &serde_json::Error) -> MemoryError {
    MemoryError::serde("history snapshot", e)
}

fn bad_date(e: &jmap_types::ValidationError) -> jmap_types::JmapError {
    jmap_types::JmapError::invalid_arguments(e.to_string())
}

impl Inner {
    /// The destroyed objects of `O` in `account` that pass the window and
    /// `filter`, in result order, with their destruction times.
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
        let mut rows: Vec<(i64, Id, Value)> = h
            .destroyed(O::TYPE_NAME, account, after, before)
            .map_err(|e| BackendQueryError::Method(bad_date(&e)))?
            .into_iter()
            .map(|d| (d.destroyed_at_ms, d.id, d.snapshot))
            .collect();
        if O::TYPE_NAME == Mailbox::TYPE_NAME {
            let filter: Option<MailboxFilter> = filter.map(retype).transpose()?;
            let sort: Vec<MailboxComparator> = sort.map(retype).transpose()?.unwrap_or_default();
            if sort
                .iter()
                .any(|c| matches!(c.property, MailboxComparatorProperty::Other(_)))
            {
                return Err(BackendQueryError::Method(
                    jmap_types::JmapError::unsupported_sort(),
                ));
            }
            let mut boxes: Vec<(i64, Id, Mailbox)> = Vec::new();
            for (t, id, v) in rows {
                let m = Mailbox::deserialize(&v)
                    .map_err(|e| BackendQueryError::Other(serde_fail(&e)))?;
                if filter
                    .as_ref()
                    .is_none_or(|f| f.matches(&|c| mailbox_matches_condition(&m, &m.name, c)))
                {
                    boxes.push((t, id, m));
                }
            }
            boxes.sort_by(|(ta, ia, a), (tb, ib, b)| {
                let mut ord = std::cmp::Ordering::Equal;
                for c in &sort {
                    if ord.is_ne() {
                        break;
                    }
                    let cmp = match c.property {
                        MailboxComparatorProperty::Name => a.name.cmp(&b.name),
                        MailboxComparatorProperty::SortOrder => a.sort_order.cmp(&b.sort_order),
                        _ => std::cmp::Ordering::Equal,
                    };
                    ord = if c.is_ascending { cmp } else { cmp.reverse() };
                }
                // §4: with no sort, by destruction time, most recent first.
                ord.then(tb.cmp(ta))
                    .then_with(|| ia.as_ref().cmp(ib.as_ref()))
            });
            return Ok(boxes.into_iter().map(|(_, id, _)| id).collect());
        }
        if O::TYPE_NAME == Email::TYPE_NAME {
            let filter: Option<EmailFilter> = filter.map(retype).transpose()?;
            super::check_email_filter_dates(filter.as_ref())?;
            let sort: Vec<EmailComparator> = sort.map(retype).transpose()?.unwrap_or_default();
            let mut kept: Vec<(i64, Id)> = Vec::new();
            let mut snapshots: HashMap<Id, Value> = HashMap::new();
            let threads = filter
                .as_ref()
                .map(|f| ThreadIndex::new(f, self.objects_ref(Email::TYPE_NAME, account.as_ref())));
            for (t, id, v) in rows {
                let e =
                    Email::deserialize(&v).map_err(|e| BackendQueryError::Other(serde_fail(&e)))?;
                let keep = match (&filter, &threads) {
                    (Some(f), Some(t)) => t.matches(&e, f).map_err(BackendQueryError::Method)?,
                    _ => true,
                };
                if keep {
                    kept.push((t, id.clone()));
                    snapshots.insert(id, v);
                }
            }
            // §4: with no sort, by destruction time, most recent first; it
            // also breaks ties for an explicit sort (applied stably below).
            kept.sort_by(|(ta, ia), (tb, ib)| {
                tb.cmp(ta).then_with(|| ia.as_ref().cmp(ib.as_ref()))
            });
            let mut ids: Vec<Id> = kept.into_iter().map(|(_, id)| id).collect();
            if !sort.is_empty() {
                sort_email_ids(&mut ids, &snapshots, &sort).map_err(BackendQueryError::Method)?;
            }
            return Ok(ids);
        }
        // Identity and VacationResponse have no /query.
        if filter.is_some() {
            return Err(BackendQueryError::Method(
                jmap_types::JmapError::unsupported_filter(),
            ));
        }
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(BackendQueryError::Method(
                jmap_types::JmapError::unsupported_sort(),
            ));
        }
        rows.sort_by(|(ta, ia, _), (tb, ib, _)| {
            tb.cmp(ta).then_with(|| ia.as_ref().cmp(ib.as_ref()))
        });
        Ok(rows.into_iter().map(|(_, id, _)| id).collect())
    }

    /// Mailbox rules a restore must pass, as an update would (RFC 8621
    /// §2.5): the parent exists, no sibling has the name, no other Mailbox
    /// has the role. The counts start at zero: a destroyed Mailbox held no
    /// Email, and no Email can name it while it is destroyed.
    fn check_mailbox_restore(
        &self,
        account: &Id,
        id: &Id,
        restored: &mut Map<String, Value>,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let empty = HashMap::new();
        let boxes = self
            .objects_ref(Mailbox::TYPE_NAME, account.as_ref())
            .unwrap_or(&empty);
        let parent = restored
            .get("parentId")
            .and_then(Value::as_str)
            .map(Id::from);
        if let Some(p) = &parent
            && !boxes.contains_key(p)
        {
            return Err(set_err(
                SetError::new(SetErrorType::InvalidProperties).with_properties(["parentId"]),
            ));
        }
        let name = restored.get("name").cloned();
        let role = restored.get("role").filter(|r| !r.is_null()).cloned();
        for (other, v) in boxes {
            if other == id {
                continue;
            }
            let same_parent =
                v.get("parentId").and_then(Value::as_str) == parent.as_ref().map(Id::as_ref);
            if same_parent && v.get("name") == name.as_ref() {
                return Err(set_err(SetError::new(SetErrorType::AlreadyExists)));
            }
            if role.is_some() && v.get("role") == role.as_ref() {
                return Err(set_err(
                    SetError::new(SetErrorType::InvalidProperties).with_properties(["role"]),
                ));
            }
        }
        for count in [
            "totalEmails",
            "unreadEmails",
            "totalThreads",
            "unreadThreads",
        ] {
            restored.insert(count.to_owned(), Value::from(0));
        }
        Ok(())
    }
}

impl Inner {
    /// An Email restore names only current Mailboxes (§5.2: a missing one
    /// is rejected "as it would an update that refers to a nonexistent
    /// object").
    fn check_email_restore(
        &self,
        account: &Id,
        email: &Map<String, Value>,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let empty = HashMap::new();
        let boxes = self
            .objects_ref(Mailbox::TYPE_NAME, account.as_ref())
            .unwrap_or(&empty);
        let missing = email
            .get("mailboxIds")
            .and_then(Value::as_object)
            .is_none_or(|m| {
                m.is_empty() || m.keys().any(|k| !boxes.contains_key(&Id::from(k.as_str())))
            });
        if missing {
            return Err(set_err(
                SetError::new(SetErrorType::InvalidProperties).with_properties(["mailboxIds"]),
            ));
        }
        Ok(())
    }

    /// Put a restored Email back in its Thread under the same `threadId`
    /// (-04 §5.1: "if no other Email is in that Thread, the Thread exists
    /// again with the same id"), and re-index its `messageId`. The caller
    /// sorts the Thread and recounts the Mailboxes once the Email is
    /// stored again.
    fn rejoin_thread(&mut self, account: &Id, email: &Value) {
        let (Some(email_id), Some(thread_id)) = (
            email.get("id").and_then(Value::as_str),
            email.get("threadId").and_then(Value::as_str),
        ) else {
            return;
        };
        let thread_id = Id::from(thread_id);
        let threads = self.objects_mut("Thread", account.as_ref());
        let existed = threads.contains_key(&thread_id);
        let entry = threads
            .entry(thread_id.clone())
            .or_insert_with(|| serde_json::json!({"id": thread_id.as_ref(), "emailIds": []}));
        let mut changed = false;
        if let Some(ids) = entry.get_mut("emailIds").and_then(Value::as_array_mut)
            && !ids.iter().any(|v| v.as_str() == Some(email_id))
        {
            ids.push(Value::from(email_id));
            changed = true;
        }
        if changed {
            let state = self.bump_state("Thread", account.as_ref());
            let (created, updated) = if existed {
                (vec![], vec![thread_id.clone()])
            } else {
                (vec![thread_id.clone()], vec![])
            };
            self.change_log_mut("Thread", account.as_ref())
                .push(ChangeEntry {
                    new_state: state,
                    created,
                    updated,
                    destroyed: vec![],
                    restored: vec![],
                    updated_props: None,
                });
        }
        if let Some(msg_ids) = email.get("messageId").and_then(Value::as_array) {
            let index = self
                .message_id_index
                .entry(account.as_ref().to_owned())
                .or_default();
            for m in msg_ids.iter().filter_map(Value::as_str) {
                index.insert(m.to_owned(), Id::from(email_id));
            }
        }
    }
}

impl Inner {
    /// §5.2: a restored SieveScript "is not active" (RFC 9661 §2.4 lets
    /// only `onSuccessActivateScript` activate one), and a name another
    /// script now uses is `alreadyExists`.
    fn check_script_restore(
        &self,
        account: &Id,
        id: &Id,
        script: &mut Map<String, Value>,
    ) -> Result<(), BackendSetError<MemoryError>> {
        script.insert("isActive".to_owned(), Value::Bool(false));
        let name = script.get("name").filter(|n| !n.is_null());
        let taken = self
            .objects_ref("SieveScript", account.as_ref())
            .is_some_and(|m| {
                m.iter()
                    .any(|(other, v)| other != id && name.is_some() && v.get("name") == name)
            });
        if taken {
            return Err(set_err(SetError::new(SetErrorType::AlreadyExists)));
        }
        Ok(())
    }
}

impl ObjectHistoryBackend for MemoryBackend {
    fn object_history_capability(
        &self,
        _caller: &(),
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
        _caller: &(),
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
            .map_err(|e| BackendQueryError::Method(bad_date(&e)))?;
        let mut entries = Vec::with_capacity(rows.len());
        for (snapshot, info) in rows {
            let object = O::deserialize(&inner.view(O::TYPE_NAME, &snapshot))
                .map_err(|e| BackendQueryError::Other(serde_fail(&e)))?;
            entries.push(HistoryEntry::new(object, info));
        }
        Ok(HistoryGetResult::new(entries, not_found))
    }

    async fn query_destroyed<O: HistoryObject + QueryObject>(
        &self,
        _caller: &(),
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
        _caller: &(),
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
        let new_state = h.destroyed_query_state(
            O::TYPE_NAME,
            account_id,
            params.destroyed_after,
            params.destroyed_before,
        );
        // RFC 8620 §5.6: every changed id is removed, and those in the
        // results now are added at their index.
        let added: Vec<AddedItem> = all
            .iter()
            .enumerate()
            .filter(|(_, id)| changed.contains(id))
            .map(|(i, id)| AddedItem::new(id.clone(), i as u64))
            .collect();
        // "Each item in the removed or added array is considered to be one
        // change."
        if let Some(max) = q.max_changes
            && (changed.len() + added.len()) as u64 > max
        {
            return Err(BackendChangesError::TooManyChanges { limit: max });
        }
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
        _caller: &(),
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
        let mut stored = baseline.clone();
        if O::TYPE_NAME == Mailbox::TYPE_NAME {
            inner.check_mailbox_restore(account_id, id, &mut stored)?;
        }
        if O::TYPE_NAME == Email::TYPE_NAME {
            inner.check_email_restore(account_id, &stored)?;
        }
        if O::TYPE_NAME == "SieveScript" {
            inner.check_script_restore(account_id, id, &mut stored)?;
        }
        let value = Value::Object(stored);
        let Ok(object) = O::deserialize(&value) else {
            // -04 §5.1: values that no longer make a valid object.
            let props: Vec<String> = patch
                .as_ref()
                .map(|p| p.as_map().keys().cloned().collect())
                .filter(|v: &Vec<String>| !v.is_empty())
                .unwrap_or_else(|| vec!["id".to_owned()]);
            return Err(set_err(
                SetError::new(SetErrorType::InvalidProperties).with_properties(props),
            ));
        };
        if let Some(h) = inner.history.as_mut() {
            h.restore(O::TYPE_NAME, account_id, id);
        }
        if O::TYPE_NAME == Email::TYPE_NAME {
            inner.rejoin_thread(account_id, &value);
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), value);
        if O::TYPE_NAME == Email::TYPE_NAME
            && let Some(thread) = object_thread_id(&inner, account_id, id)
        {
            inner.sort_thread_email_ids(account_id.as_ref(), &thread);
        }
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
        if O::TYPE_NAME == Email::TYPE_NAME || O::TYPE_NAME == Mailbox::TYPE_NAME {
            inner.recount_mailboxes(account_id.as_ref());
        }
        Ok(Restored::new(object, baseline))
    }
}

/// The `threadId` of stored Email `id`.
fn object_thread_id(inner: &Inner, account: &Id, id: &Id) -> Option<Id> {
    inner
        .objects_ref(Email::TYPE_NAME, account.as_ref())?
        .get(id)?
        .get("threadId")?
        .as_str()
        .map(Id::from)
}

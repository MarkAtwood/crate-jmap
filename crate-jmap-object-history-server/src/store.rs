//! [`HistoryStore`]: everything a JSON-storing reference backend keeps for
//! object history, behind one value it can put in its state.

use std::collections::HashMap;

use jmap_object_history_types::{
    HistoryGetArguments, ObjectHistoryAccountCapability, ObjectHistoryInfo, is_history_type,
};
use jmap_types::{Id, UTCDate, ValidationError};
use serde_json::Value;

use crate::log::{DestroyedObject, HistoryRetention, VersionLog};
use crate::select::{HistorySelector, Selection};
use crate::time::millis_from_utc_date;

/// The object history of a reference backend that stores objects as JSON:
/// the advertised capability, a [`VersionLog`], an injectable clock, the
/// §8.2 rights cut per (type, account), and the changes to each type's set
/// of destroyed objects for destroyed `Foo/queryChanges`.
///
/// `Clone`, so a backend whose conditional rollback restores a snapshot of
/// its state rolls history back with it. The backend calls
/// [`record_replace`](Self::record_replace) and
/// [`record_destroy`](Self::record_destroy) from every update and destroy
/// path, and builds its `ObjectHistoryBackend` methods on the rest.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    capability: ObjectHistoryAccountCapability,
    log: VersionLog<Value>,
    clock_ms: Option<i64>,
    rights_changed_ms: HashMap<(String, String), i64>,
    destroyed_changes: HashMap<(String, String), Vec<(u64, Id)>>,
}

/// What [`HistoryStore::select`] returns: the (snapshot, objectHistory)
/// pairs selected, and the ids not found.
pub type SelectedHistory = (Vec<(Value, ObjectHistoryInfo)>, Vec<Id>);

fn key(type_name: &str, account: &Id) -> (String, String) {
    (type_name.to_owned(), account.as_ref().to_owned())
}

impl HistoryStore {
    /// History for the types `capability.supportedTypes` lists, with
    /// `retention`.
    ///
    /// # Panics
    ///
    /// When `supportedTypes` names a type without history
    /// (`jmap_object_history_types::is_history_type`).
    pub fn new(capability: ObjectHistoryAccountCapability, retention: HistoryRetention) -> Self {
        for t in &capability.supported_types {
            assert!(is_history_type(t), "{t} does not support history");
        }
        Self {
            capability,
            log: VersionLog::new(retention),
            clock_ms: None,
            rights_changed_ms: HashMap::new(),
            destroyed_changes: HashMap::new(),
        }
    }

    /// The capability to advertise.
    pub fn capability(&self) -> &ObjectHistoryAccountCapability {
        &self.capability
    }

    /// Whether `type_name` keeps history.
    pub fn records(&self, type_name: &str) -> bool {
        self.capability.supports_type(type_name)
    }

    /// The current time in milliseconds: the fixed clock, or the system
    /// clock.
    pub fn now_ms(&self) -> i64 {
        self.clock_ms.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0)
        })
    }

    /// Fix the clock at `ms` (tests).
    pub fn set_clock(&mut self, ms: i64) {
        self.clock_ms = Some(ms);
    }

    /// Set the §8.2 cut for `type_name` in `account`: versions replaced
    /// before `ms`, and objects destroyed before it, are hidden.
    pub fn set_rights_changed(&mut self, type_name: &str, account: &Id, ms: i64) {
        self.rights_changed_ms.insert(key(type_name, account), ms);
    }

    fn cut(&self, type_name: &str, account: &Id) -> Option<i64> {
        self.rights_changed_ms
            .get(&key(type_name, account))
            .copied()
    }

    fn destroyed_changed(&mut self, type_name: &str, account: &Id, id: &Id) {
        let entry = self
            .destroyed_changes
            .entry(key(type_name, account))
            .or_default();
        let seq = entry.last().map_or(1, |(s, _)| s + 1);
        entry.push((seq, id.clone()));
    }

    fn seq(&self, type_name: &str, account: &Id) -> u64 {
        self.destroyed_changes
            .get(&key(type_name, account))
            .and_then(|v| v.last())
            .map_or(0, |(s, _)| *s)
    }

    /// Record that `id` was updated from `previous`, if the type keeps
    /// history.
    pub fn record_replace(&mut self, type_name: &str, account: &Id, id: &Id, previous: Value) {
        if self.records(type_name) {
            let now = self.now_ms();
            self.log
                .record_replace(type_name, account, id, previous, now);
        }
    }

    /// Record that `id` was destroyed with `last` as its final state, if
    /// the type keeps history.
    pub fn record_destroy(&mut self, type_name: &str, account: &Id, id: &Id, last: Value) {
        if self.records(type_name) && !self.log.is_destroyed(type_name, account, id) {
            let now = self.now_ms();
            self.log.record_destroy(type_name, account, id, last, now);
            self.destroyed_changed(type_name, account, id);
        }
    }

    /// Whether any history of `id` is retained: minting it again is
    /// forbidden (§5.1).
    pub fn retains(&self, type_name: &str, account: &Id, id: &Id) -> bool {
        self.log.retains(type_name, account, id)
    }

    /// Apply the retention limits at the current time. A destroyed object
    /// that loses its last version leaves the destroyed set.
    pub fn prune(&mut self) {
        let now = self.now_ms();
        let before: Vec<(String, Id, Id)> = self.destroyed_keys();
        self.log.prune(now);
        for (t, a, id) in before {
            if !self.log.is_destroyed(&t, &a, &id) {
                self.destroyed_changed(&t, &a, &id);
            }
        }
    }

    fn destroyed_keys(&self) -> Vec<(String, Id, Id)> {
        self.destroyed_changes
            .keys()
            .flat_map(|(t, a)| {
                let account = Id::from(a.as_str());
                self.log
                    .destroyed_objects(t, &account)
                    .into_iter()
                    .map(move |d| (t.clone(), account.clone(), d.id))
            })
            .collect()
    }

    /// Discard one version in any order (§3.1). `false` when there was no
    /// such version.
    pub fn discard_version(
        &mut self,
        type_name: &str,
        account: &Id,
        id: &Id,
        version: u64,
    ) -> bool {
        let was = self.log.is_destroyed(type_name, account, id);
        let found = self.log.discard_version(type_name, account, id, version);
        if was && !self.log.is_destroyed(type_name, account, id) {
            self.destroyed_changed(type_name, account, id);
        }
        found
    }

    /// Discard all history of `id` (the §8.3 administrative purge).
    pub fn purge(&mut self, type_name: &str, account: &Id, id: &Id) {
        let was = self.log.is_destroyed(type_name, account, id);
        self.log.purge(type_name, account, id);
        if was {
            self.destroyed_changed(type_name, account, id);
        }
    }

    /// Discard all history of each `type_name` object whose newest
    /// retained version satisfies `pred` (a type rule that hard-deletes
    /// records with their history); destroyed ones leave the destroyed set.
    pub fn purge_where(&mut self, type_name: &str, account: &Id, pred: impl FnMut(&Value) -> bool) {
        let destroyed: Vec<Id> = self
            .log
            .destroyed_objects(type_name, account)
            .into_iter()
            .map(|d| d.id)
            .collect();
        for id in self.log.purge_where(type_name, account, pred) {
            if destroyed.contains(&id) {
                self.destroyed_changed(type_name, account, &id);
            }
        }
    }

    /// The §3 selection: for each of `ids` (or, for `None`, every live id
    /// and, with `includeDestroyed`, every destroyed one), the selected
    /// (snapshot, objectHistory) pairs, and the ids not found. `live` is
    /// the type's current objects in the account.
    ///
    /// # Errors
    ///
    /// [`ValidationError`] for a malformed `historyAfter` or
    /// `historyBefore`.
    pub fn select(
        &self,
        type_name: &str,
        account: &Id,
        ids: Option<&[Id]>,
        live: &HashMap<Id, Value>,
        arguments: &HistoryGetArguments,
    ) -> Result<SelectedHistory, ValidationError> {
        let ids: Vec<Id> = match ids {
            Some(ids) => ids.to_vec(),
            None => {
                let mut all: Vec<Id> = live.keys().cloned().collect();
                if arguments.include_destroyed {
                    all.extend(
                        self.log
                            .destroyed_objects(type_name, account)
                            .into_iter()
                            .map(|d| d.id),
                    );
                }
                all.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                all
            }
        };
        let selector = HistorySelector::from_arguments(arguments);
        let cut = self.cut(type_name, account);
        let mut entries = Vec::new();
        let mut not_found = Vec::new();
        for id in ids {
            match self.log.select(
                type_name,
                account,
                &id,
                live.get(&id).cloned(),
                &selector,
                cut,
            )? {
                Selection::Entries(list) => entries.extend(list),
                _ => not_found.push(id),
            }
        }
        Ok((entries, not_found))
    }

    /// The destroyed objects of `type_name` in `account` destroyed strictly
    /// after `after` and before `before`, past the rights cut, most
    /// recently destroyed first (§4 default order; ties by id).
    ///
    /// # Errors
    ///
    /// [`ValidationError`] for a malformed bound.
    pub fn destroyed(
        &self,
        type_name: &str,
        account: &Id,
        after: Option<&UTCDate>,
        before: Option<&UTCDate>,
    ) -> Result<Vec<DestroyedObject<Value>>, ValidationError> {
        let after = after.map(millis_from_utc_date).transpose()?;
        let before = before.map(millis_from_utc_date).transpose()?;
        let cut = self.cut(type_name, account);
        let mut rows: Vec<DestroyedObject<Value>> = self
            .log
            .destroyed_objects(type_name, account)
            .into_iter()
            .filter(|d| after.is_none_or(|a| d.destroyed_at_ms > a))
            .filter(|d| before.is_none_or(|b| d.destroyed_at_ms < b))
            .filter(|d| cut.is_none_or(|c| d.destroyed_at_ms >= c))
            .collect();
        rows.sort_by(|a, b| {
            b.destroyed_at_ms
                .cmp(&a.destroyed_at_ms)
                .then_with(|| a.id.as_ref().cmp(b.id.as_ref()))
        });
        Ok(rows)
    }

    /// The `queryState` of a destroyed query over `type_name` with this
    /// window: `dq|<window>|<sequence>` (decision 25).
    pub fn destroyed_query_state(
        &self,
        type_name: &str,
        account: &Id,
        after: Option<&UTCDate>,
        before: Option<&UTCDate>,
    ) -> String {
        format!(
            "dq|{}|{}",
            window(after, before),
            self.seq(type_name, account)
        )
    }

    /// The ids whose destroyed status changed since `since_state`, sorted,
    /// or `None` when `since_state` is not a destroyed query state of this
    /// window (`cannotCalculateChanges`).
    pub fn destroyed_changes_since(
        &self,
        type_name: &str,
        account: &Id,
        since_state: &str,
        after: Option<&UTCDate>,
        before: Option<&UTCDate>,
    ) -> Option<Vec<Id>> {
        let want = window(after, before);
        let since: u64 = since_state
            .strip_prefix("dq|")
            .and_then(|rest| rest.rsplit_once('|'))
            .filter(|(w, _)| *w == want)
            .and_then(|(_, n)| n.parse().ok())?;
        let mut ids: Vec<Id> = self
            .destroyed_changes
            .get(&key(type_name, account))
            .map(|v| {
                v.iter()
                    .filter(|(s, _)| *s > since)
                    .map(|(_, id)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        ids.dedup();
        Some(ids)
    }

    /// The final version of the destroyed object `id`: the newest
    /// retained one (-04 §3.1), or `None` when `id` is not a destroyed
    /// object with history.
    pub fn final_version(&self, type_name: &str, account: &Id, id: &Id) -> Option<&Value> {
        self.log.final_version(type_name, account, id)
    }

    /// Mark the destroyed object `id` restored: its history continues
    /// (§5.1) and it leaves the destroyed set. `false` when it was not
    /// destroyed.
    pub fn restore(&mut self, type_name: &str, account: &Id, id: &Id) -> bool {
        let done = self.log.restore(type_name, account, id).is_some();
        if done {
            self.destroyed_changed(type_name, account, id);
        }
        done
    }
}

fn window(after: Option<&UTCDate>, before: Option<&UTCDate>) -> String {
    format!(
        "{}~{}",
        after.map_or("", |d| d.as_ref()),
        before.map_or("", |d| d.as_ref())
    )
}

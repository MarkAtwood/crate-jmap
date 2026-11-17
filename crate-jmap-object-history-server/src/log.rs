//! [`VersionLog`]: the replaced and destroyed versions a reference backend
//! keeps, with retention.

use std::collections::BTreeMap;
use std::time::Duration;

use jmap_types::{Id, UTCDate, ValidationError};

use crate::select::{HistorySelector, Selection, Version, select_versions};
use crate::time::utc_date_from_millis;

/// How long a [`VersionLog`] keeps versions. Both limits are off by
/// default (§2.1 retention is best effort).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryRetention {
    /// Discard a version this long after it was replaced; also the
    /// `expires` of each version. Advertise it as `maxHistoryDuration`.
    pub max_duration: Option<Duration>,
    /// Keep at most this many replaced versions per object, discarding the
    /// oldest first.
    pub max_versions_per_object: Option<usize>,
}

impl HistoryRetention {
    /// No limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`max_duration`](Self::max_duration).
    #[must_use]
    pub fn max_duration(mut self, d: Option<Duration>) -> Self {
        self.max_duration = d;
        self
    }

    /// Set [`max_versions_per_object`](Self::max_versions_per_object).
    #[must_use]
    pub fn max_versions_per_object(mut self, n: Option<usize>) -> Self {
        self.max_versions_per_object = n;
        self
    }

    fn duration_ms(&self) -> Option<i64> {
        self.max_duration
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

#[derive(Debug, Clone)]
struct Stored<V> {
    version: u64,
    snapshot: V,
    replaced_ms: i64,
}

#[derive(Debug, Clone)]
struct ObjectLog<V> {
    /// Replaced versions, ascending by version and by `replaced_ms`.
    replaced: Vec<Stored<V>>,
    /// The number the live version has, or would have after a restore.
    live_version: u64,
    destroyed: bool,
}

impl<V> ObjectLog<V> {
    fn new() -> Self {
        Self {
            replaced: Vec::new(),
            live_version: 1,
            destroyed: false,
        }
    }

    /// Record the current version as replaced at `now_ms`. Two versions
    /// replaced in the same millisecond are collapsed into the newer one
    /// (§3.1: replaced values "MUST be distinct"; a server "can collapse
    /// them into a single version"), and a clock that went back is held
    /// at the last value, so `replaced` stays strictly increasing.
    fn push(&mut self, snapshot: V, now_ms: i64) {
        let mut replaced_ms = now_ms;
        if let Some(last) = self.replaced.last()
            && last.replaced_ms >= now_ms
        {
            replaced_ms = last.replaced_ms;
            self.replaced.pop();
        }
        self.replaced.push(Stored {
            version: self.live_version,
            snapshot,
            replaced_ms,
        });
        self.live_version += 1;
    }
}

/// A destroyed object, as [`VersionLog::destroyed_objects`] lists it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestroyedObject<V> {
    /// The object's id.
    pub id: Id,
    /// Its final version: the newest one retained (-04 §3.1).
    pub snapshot: V,
    /// The final version's `replaced`: the time of destruction, or the
    /// earlier time standing in for it when the version current at
    /// destruction was discarded (-04 §3.1).
    pub destroyed_at: UTCDate,
    /// [`destroyed_at`](Self::destroyed_at) in milliseconds since the
    /// epoch, for sorting and windows.
    pub destroyed_at_ms: i64,
}

/// The replaced and destroyed versions of objects, keyed by (type name,
/// account, id); the live versions stay in the backend's own store.
///
/// The clock is injected: every method that records or prunes takes the
/// current time in milliseconds since the epoch. `Clone`, so a backend
/// that snapshots its state for a rollback (conditional `atomic`) rolls
/// the log back with it.
#[derive(Debug, Clone)]
pub struct VersionLog<V = serde_json::Value> {
    retention: HistoryRetention,
    objects: BTreeMap<(String, Id, Id), ObjectLog<V>>,
}

impl<V> Default for VersionLog<V> {
    fn default() -> Self {
        Self {
            retention: HistoryRetention::new(),
            objects: BTreeMap::new(),
        }
    }
}

impl<V: Clone> VersionLog<V> {
    /// An empty log with `retention`.
    pub fn new(retention: HistoryRetention) -> Self {
        Self {
            retention,
            objects: BTreeMap::new(),
        }
    }

    /// The retention limits.
    pub fn retention(&self) -> HistoryRetention {
        self.retention
    }

    fn key(type_name: &str, account_id: &Id, id: &Id) -> (String, Id, Id) {
        (type_name.to_owned(), account_id.clone(), id.clone())
    }

    /// Record that the live object was updated at `now_ms`: `previous`,
    /// its state before the update, becomes a replaced version.
    pub fn record_replace(
        &mut self,
        type_name: &str,
        account_id: &Id,
        id: &Id,
        previous: V,
        now_ms: i64,
    ) {
        let log = self
            .objects
            .entry(Self::key(type_name, account_id, id))
            .or_insert_with(ObjectLog::new);
        if !log.destroyed {
            log.push(previous, now_ms);
        }
    }

    /// Record that the object was destroyed at `now_ms`, however it was
    /// removed (§3): `last`, its state when destroyed, becomes its final
    /// version. A second destroy of a destroyed object is ignored.
    pub fn record_destroy(
        &mut self,
        type_name: &str,
        account_id: &Id,
        id: &Id,
        last: V,
        now_ms: i64,
    ) {
        let log = self
            .objects
            .entry(Self::key(type_name, account_id, id))
            .or_insert_with(ObjectLog::new);
        if !log.destroyed {
            log.push(last, now_ms);
            log.destroyed = true;
        }
    }

    /// Whether the object is destroyed with a version retained: it can be
    /// restored, and a history `Foo/get` with `includeDestroyed` returns
    /// it.
    pub fn is_destroyed(&self, type_name: &str, account_id: &Id, id: &Id) -> bool {
        self.objects
            .get(&Self::key(type_name, account_id, id))
            .is_some_and(|l| l.destroyed)
    }

    /// Whether any history of the object is retained. A server "MUST NOT
    /// reuse the id of a destroyed object for which it retains any
    /// history, other than by restoring" (§5.1): check this before minting
    /// an id.
    pub fn retains(&self, type_name: &str, account_id: &Id, id: &Id) -> bool {
        self.objects
            .get(&Self::key(type_name, account_id, id))
            .is_some_and(|l| l.destroyed || !l.replaced.is_empty())
    }

    /// The final version of a destroyed object: the newest retained one
    /// (-04 §3.1), or `None` when the object is not destroyed.
    pub fn final_version(&self, type_name: &str, account_id: &Id, id: &Id) -> Option<&V> {
        self.objects
            .get(&Self::key(type_name, account_id, id))
            .filter(|l| l.destroyed)
            .and_then(|l| l.replaced.last())
            .map(|s| &s.snapshot)
    }

    /// Restore a destroyed object: return its final version, which the
    /// backend applies the patch to and stores as the live object under
    /// the old id. The history continues: the destroyed object's versions
    /// become previous versions of the restored one (§5.1). `None` when
    /// the object is not destroyed (a current or unknown id).
    pub fn restore(&mut self, type_name: &str, account_id: &Id, id: &Id) -> Option<V> {
        let log = self
            .objects
            .get_mut(&Self::key(type_name, account_id, id))?;
        if !log.destroyed {
            return None;
        }
        let last = log.replaced.last()?.snapshot.clone();
        log.destroyed = false;
        Some(last)
    }

    /// The §3 selection for one id. `live` is the current object, or
    /// `None` when it does not exist. `rights_changed_ms` is the §8.2
    /// conservative cut: versions replaced before it are hidden, and so is
    /// a destroyed object destroyed before it.
    ///
    /// # Errors
    ///
    /// [`ValidationError`] for an invalid `historyAfter` or
    /// `historyBefore`.
    pub fn select(
        &self,
        type_name: &str,
        account_id: &Id,
        id: &Id,
        live: Option<V>,
        selector: &HistorySelector,
        rights_changed_ms: Option<i64>,
    ) -> Result<Selection<V>, ValidationError> {
        let log = self.objects.get(&Self::key(type_name, account_id, id));
        let destroyed = log.is_some_and(|l| l.destroyed);
        let expires = self.retention.duration_ms();
        let mut versions: Vec<Version<V>> = log
            .map(|l| {
                l.replaced
                    .iter()
                    .filter(|s| rights_changed_ms.is_none_or(|cut| s.replaced_ms >= cut))
                    .map(|s| {
                        Version::replaced(
                            s.version,
                            s.snapshot.clone(),
                            utc_date_from_millis(s.replaced_ms),
                            expires.map(|d| utc_date_from_millis(s.replaced_ms.saturating_add(d))),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !destroyed {
            let Some(live) = live else {
                return Ok(Selection::NotFound);
            };
            versions.push(Version::live(log.map_or(1, |l| l.live_version), live));
        }
        select_versions(&versions, destroyed, selector)
    }

    /// Every destroyed object of `type_name` in `account_id` with a
    /// retained version, by id.
    pub fn destroyed_objects(&self, type_name: &str, account_id: &Id) -> Vec<DestroyedObject<V>> {
        self.objects
            .iter()
            .filter(|((t, a, _), l)| t == type_name && a == account_id && l.destroyed)
            .filter_map(|((_, _, id), l)| {
                let last = l.replaced.last()?;
                Some(DestroyedObject {
                    id: id.clone(),
                    snapshot: last.snapshot.clone(),
                    destroyed_at: utc_date_from_millis(last.replaced_ms),
                    destroyed_at_ms: last.replaced_ms,
                })
            })
            .collect()
    }

    /// Apply the retention limits at `now_ms`, discarding the oldest
    /// versions first, so the final version of a destroyed object goes
    /// last (docs/design/DECISIONS.md decision 24). A destroyed object
    /// goes when its last version does.
    pub fn prune(&mut self, now_ms: i64) {
        let duration = self.retention.duration_ms();
        let max = self.retention.max_versions_per_object;
        self.objects.retain(|_, log| {
            if let Some(d) = duration {
                log.replaced
                    .retain(|s| s.replaced_ms.saturating_add(d) > now_ms);
            }
            if let Some(max) = max {
                let excess = log.replaced.len().saturating_sub(max);
                log.replaced.drain(..excess);
            }
            !(log.destroyed && log.replaced.is_empty())
        });
    }

    /// Discard one version, in any order (§3.1: the server "MAY also prune
    /// old versions in any order"). When it was a destroyed object's final
    /// version, the next newest stands in (-04 §3.1); when it was the
    /// last, the object goes. `false` when there was no such version.
    pub fn discard_version(
        &mut self,
        type_name: &str,
        account_id: &Id,
        id: &Id,
        version: u64,
    ) -> bool {
        let key = Self::key(type_name, account_id, id);
        let Some(log) = self.objects.get_mut(&key) else {
            return false;
        };
        let before = log.replaced.len();
        log.replaced.retain(|s| s.version != version);
        let found = log.replaced.len() != before;
        if log.destroyed && log.replaced.is_empty() {
            self.objects.remove(&key);
        }
        found
    }

    /// Discard every version of each `type_name` object of `account_id`
    /// whose newest retained version satisfies `pred`, and return their
    /// ids. For a type rule such as chat-00's Space destroy, which
    /// "hard-deletes all records" of the Space.
    pub fn purge_where(
        &mut self,
        type_name: &str,
        account_id: &Id,
        mut pred: impl FnMut(&V) -> bool,
    ) -> Vec<Id> {
        let mut gone = Vec::new();
        self.objects.retain(|(t, a, id), log| {
            let hit = t == type_name
                && a == account_id
                && log.replaced.last().is_some_and(|v| pred(&v.snapshot));
            if hit {
                gone.push(id.clone());
            }
            !hit
        });
        gone
    }

    /// Discard every version of the object (the §8.3 administrative
    /// purge). A destroyed object is then gone for good.
    pub fn purge(&mut self, type_name: &str, account_id: &Id, id: &Id) {
        self.objects.remove(&Self::key(type_name, account_id, id));
    }
}

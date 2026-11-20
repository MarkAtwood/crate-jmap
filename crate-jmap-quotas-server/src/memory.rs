//! In-memory reference [`QuotasBackend`].
//!
//! Not production: a reference implementation, an integration-test harness
//! and a documentation example. Quotas are server-managed (RFC 9425 has no
//! `Quota/set`), so the backend's owner changes them directly with
//! [`put_quota`], [`set_used`] and [`remove_quota`], each one a new Quota
//! state.
//!
//! `Quota/changes` reports `updatedProperties: ["used"]` when every update
//! since the old state came from [`set_used`] (RFC 9425 §4.3), and `null`
//! otherwise.
//!
//! [`put_quota`]: crate::memory::MemoryBackend::put_quota
//! [`set_used`]: crate::memory::MemoryBackend::set_used
//! [`remove_quota`]: crate::memory::MemoryBackend::remove_quota
//!
//! Quotas may carry `locale` and `localizations` for `name` and
//! `description` (draft-atwood-jmap-updates-00 §5.2); every account
//! supports `urn:ietf:params:jmap:i18n`.
//!
//! `Quota/query` evaluates every RFC 9425 §4.4 condition. `name` matches
//! case-insensitively (Unicode lowercase). It sorts by `name` and `used`,
//! with no collation or `i;octet`, `i;ascii-casemap` or
//! `i;unicode-casemap`; anything else is `unsupportedSort`. Ties are broken
//! by id, and the default order is by id.

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use jmap_quotas_types::{
    Quota, QuotaComparator, QuotaComparatorProperty, QuotaFilter, QuotaFilterCondition,
};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, ChangesResult, GetObject, JmapAccounts,
    JmapObject, JmapReads, MemoryCaller, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, limit_or_max, resolve_query_start,
};
use jmap_types::i18n::localized_value;
use jmap_types::{Filter, Id, JmapError, Operator, State};
use serde_json::Value;

use crate::QuotasBackend;

/// One state change of an account's Quotas.
#[derive(Debug, Clone)]
struct Change {
    state: u64,
    created: Option<Id>,
    updated: Option<Id>,
    destroyed: Option<Id>,
    /// `true` for an update that changed only `used`.
    used_only: bool,
}

#[derive(Debug, Default)]
struct Account {
    quotas: BTreeMap<Id, Value>,
    state: u64,
    log: Vec<Change>,
}

/// In-memory Quota storage. Cloning shares the storage.
///
/// `C` is the [`JmapAccounts::CallerCtx`]: [`Id`], the caller's Principal
/// id, by default, or `()` for a deployment without identity. Quotas are
/// per account, not per user, so every caller sees the account's quotas;
/// [`JmapAccounts::principal_id`] returns [`MemoryCaller::principal`].
#[derive(Debug)]
#[non_exhaustive]
pub struct MemoryBackend<C = Id> {
    accounts: Arc<Mutex<HashMap<Id, Account>>>,
    caller: PhantomData<fn() -> C>,
}

/// An empty backend: [`MemoryBackend::new`] for any caller context, such
/// as `MemoryBackend::<()>::default()` for a deployment without identity.
impl<C> Default for MemoryBackend<C> {
    fn default() -> Self {
        Self {
            accounts: Arc::default(),
            caller: PhantomData,
        }
    }
}

/// Cloning shares the storage.
impl<C> Clone for MemoryBackend<C> {
    fn clone(&self) -> Self {
        Self {
            accounts: Arc::clone(&self.accounts),
            caller: PhantomData,
        }
    }
}

/// The error type of [`MemoryBackend`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// What went wrong.
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

impl MemoryBackend {
    /// An empty backend with no accounts, whose callers are Principal ids
    /// (`C = Id`). Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl<C: MemoryCaller> MemoryBackend<C> {
    /// Add an empty account, at state `0`.
    #[must_use]
    pub fn with_account(self, account_id: &Id) -> Self {
        self.lock().entry(account_id.clone()).or_default();
        self
    }

    /// Store `quota` (a Quota object as JSON) in `account_id` without a
    /// state change, adding the account if needed: the quotas a server
    /// starts with.
    ///
    /// # Panics
    ///
    /// If `quota` does not decode as a [`Quota`].
    pub fn seed_quota(&self, account_id: &Id, quota: Value) {
        let id = Self::check(&quota);
        self.lock()
            .entry(account_id.clone())
            .or_default()
            .quotas
            .insert(id, quota);
    }

    /// Create or replace `quota` in an existing account, as one state
    /// change that may touch any property.
    ///
    /// # Panics
    ///
    /// If `quota` does not decode as a [`Quota`] or the account does not
    /// exist.
    pub fn put_quota(&self, account_id: &Id, quota: Value) {
        let id = Self::check(&quota);
        let mut accounts = self.lock();
        let acc = Self::account(&mut accounts, account_id);
        let existed = acc.quotas.insert(id.clone(), quota).is_some();
        acc.state += 1;
        let (created, updated) = if existed {
            (None, Some(id))
        } else {
            (Some(id), None)
        };
        acc.log.push(Change {
            state: acc.state,
            created,
            updated,
            destroyed: None,
            used_only: false,
        });
    }

    /// Set the `used` of Quota `id` to `used`, as one state change that
    /// touches only `used` (RFC 9425 §4.3).
    ///
    /// # Panics
    ///
    /// If the account or the Quota does not exist.
    pub fn set_used(&self, account_id: &Id, id: &Id, used: u64) {
        let mut accounts = self.lock();
        let acc = Self::account(&mut accounts, account_id);
        let quota = acc
            .quotas
            .get_mut(id)
            .unwrap_or_else(|| panic!("set_used: no Quota {id:?}"));
        quota["used"] = used.into();
        acc.state += 1;
        acc.log.push(Change {
            state: acc.state,
            created: None,
            updated: Some(id.clone()),
            destroyed: None,
            used_only: true,
        });
    }

    /// Remove Quota `id`, as one state change.
    ///
    /// # Panics
    ///
    /// If the account or the Quota does not exist.
    pub fn remove_quota(&self, account_id: &Id, id: &Id) {
        let mut accounts = self.lock();
        let acc = Self::account(&mut accounts, account_id);
        assert!(
            acc.quotas.remove(id).is_some(),
            "remove_quota: no Quota {id:?}"
        );
        acc.state += 1;
        acc.log.push(Change {
            state: acc.state,
            created: None,
            updated: None,
            destroyed: Some(id.clone()),
            used_only: false,
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Id, Account>> {
        self.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn account<'a>(accounts: &'a mut HashMap<Id, Account>, account_id: &Id) -> &'a mut Account {
        accounts
            .get_mut(account_id)
            .unwrap_or_else(|| panic!("no account {account_id:?}"))
    }

    fn check(quota: &Value) -> Id {
        let q: Quota = serde_json::from_value(quota.clone())
            .unwrap_or_else(|e| panic!("not a Quota: {e}: {quota}"));
        q.id
    }
}

/// Only Quota is stored; any other type is a programming error.
fn only_quota<O: JmapObject>() -> Result<(), MemoryError> {
    if O::TYPE_NAME == Quota::TYPE_NAME {
        Ok(())
    } else {
        Err(MemoryError::new(format!(
            "MemoryBackend stores only Quota, not {}",
            O::TYPE_NAME
        )))
    }
}

fn quota_of(v: &Value) -> Result<Quota, MemoryError> {
    serde_json::from_value(v.clone()).map_err(|e| MemoryError::new(format!("stored Quota: {e}")))
}

fn condition_matches(c: &QuotaFilterCondition, q: &Quota) -> bool {
    c.name
        .as_ref()
        .is_none_or(|n| q.name.to_lowercase().contains(&n.to_lowercase()))
        && c.scope.as_ref().is_none_or(|s| *s == q.scope)
        && c.resource_type
            .as_ref()
            .is_none_or(|r| *r == q.resource_type)
        && c.type_name
            .as_ref()
            .is_none_or(|t| q.types.iter().any(|x| x == t))
}

fn filter_matches(f: &QuotaFilter, q: &Quota) -> bool {
    match f {
        Filter::Condition(c) => condition_matches(c, q),
        Filter::Operator(op) => {
            let mut results = op.conditions.iter().map(|c| filter_matches(c, q));
            match op.operator {
                Operator::And => results.all(|r| r),
                Operator::Or => results.any(|r| r),
                Operator::Not => !results.any(|r| r),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Text comparison for a supported collation (`None` is `i;octet`).
fn text_cmp(collation: Option<&str>, a: &str, b: &str) -> Option<Ordering> {
    match collation {
        None | Some("i;octet") => Some(a.cmp(b)),
        Some("i;ascii-casemap") => Some(a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())),
        Some("i;unicode-casemap") => Some(a.to_lowercase().cmp(&b.to_lowercase())),
        Some(_) => None,
    }
}

fn check_sort(sort: &[QuotaComparator]) -> Result<(), JmapError> {
    for c in sort {
        let ok = match c.property {
            QuotaComparatorProperty::Name => text_cmp(c.collation.as_deref(), "", "").is_some(),
            QuotaComparatorProperty::Used => c.collation.is_none(),
            _ => false,
        };
        if !ok {
            return Err(JmapError::unsupported_sort());
        }
    }
    Ok(())
}

fn compare(sort: &[QuotaComparator], a: &Quota, b: &Quota) -> Ordering {
    for c in sort {
        let ord = match c.property {
            QuotaComparatorProperty::Name => {
                text_cmp(c.collation.as_deref(), &a.name, &b.name).unwrap_or(Ordering::Equal)
            }
            QuotaComparatorProperty::Used => a.used.cmp(&b.used),
            _ => Ordering::Equal,
        };
        let ord = if c.is_ascending { ord } else { ord.reverse() };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.id.as_ref().cmp(b.id.as_ref())
}

impl<C: MemoryCaller> JmapAccounts for MemoryBackend<C> {
    type Error = MemoryError;
    type CallerCtx = C;

    /// [`MemoryCaller::principal`].
    fn principal_id(caller: &C) -> Option<&Id> {
        caller.principal()
    }

    /// Every account supports `urn:ietf:params:jmap:i18n`: Quota `name`
    /// and `description` are returned with the `locale` and
    /// `localizations` the backend's owner stored.
    fn i18n_info(&self, _caller: &C, _account_id: &Id) -> Option<jmap_server::I18nInfo> {
        Some(jmap_server::I18nInfo::new())
    }

    async fn account_exists(&self, _caller: &C, account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.lock().contains_key(account_id))
    }
}

impl<C: MemoryCaller> JmapReads for MemoryBackend<C> {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &C,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        only_quota::<O>()?;
        let accounts = self.lock();
        let quotas = accounts.get(account_id).map(|a| &a.quotas);
        let decode = |v: &Value| {
            O::deserialize(v).map_err(|e| MemoryError::new(format!("stored Quota: {e}")))
        };
        match ids {
            None => {
                let list = quotas
                    .into_iter()
                    .flat_map(BTreeMap::values)
                    .map(decode)
                    .collect::<Result<_, _>>()?;
                Ok((list, Vec::new()))
            }
            Some(ids) => {
                let mut found = Vec::new();
                let mut not_found = Vec::new();
                for id in ids {
                    match quotas.and_then(|q| q.get(id)) {
                        Some(v) => found.push(decode(v)?),
                        None => not_found.push(id.clone()),
                    }
                }
                Ok((found, not_found))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &C,
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        only_quota::<O>()?;
        let state = self.lock().get(account_id).map_or(0, |a| a.state);
        Ok(State::from(state.to_string()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &C,
        account_id: &Id,
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        only_quota::<O>().map_err(BackendChangesError::Other)?;
        let since: u64 = params
            .since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;
        let accounts = self.lock();
        let Some(acc) = accounts.get(account_id) else {
            return Err(BackendChangesError::CannotCalculate);
        };
        if since > acc.state {
            return Err(BackendChangesError::CannotCalculate);
        }
        let mut window: Vec<&Change> = acc.log.iter().filter(|c| c.state > since).collect();
        // Every change names one id, so maxChanges entries give at most
        // maxChanges ids: an intermediate state after them (RFC 8620 §5.2).
        let mut has_more = false;
        if let Some(max) = params.max_changes {
            let max = usize::try_from(max).unwrap_or(usize::MAX);
            if window.len() > max {
                if max == 0 {
                    return Err(BackendChangesError::TooManyChanges { limit: 0 });
                }
                window.truncate(max);
                has_more = true;
            }
        }
        let new_state = window.last().map_or(acc.state, |c| c.state);
        let new_state = if has_more { new_state } else { acc.state };

        // RFC 8620 §5.2 de-duplication: created then destroyed vanishes;
        // created then updated stays created; updated then destroyed is
        // destroyed.
        #[derive(Clone, Copy, PartialEq)]
        enum Fate {
            Created,
            Updated,
            Destroyed,
        }
        let mut fates: BTreeMap<Id, Fate> = BTreeMap::new();
        let mut not_used_only: HashSet<Id> = HashSet::new();
        for c in &window {
            if let Some(id) = &c.created {
                fates.insert(id.clone(), Fate::Created);
            }
            if let Some(id) = &c.updated {
                if !c.used_only {
                    not_used_only.insert(id.clone());
                }
                fates.entry(id.clone()).or_insert(Fate::Updated);
            }
            if let Some(id) = &c.destroyed {
                if fates.get(id) == Some(&Fate::Created) {
                    fates.remove(id);
                } else {
                    fates.insert(id.clone(), Fate::Destroyed);
                }
            }
        }
        let pick = |f: Fate| -> Vec<Id> {
            fates
                .iter()
                .filter(|(_, x)| **x == f)
                .map(|(id, _)| id.clone())
                .collect()
        };
        let updated = pick(Fate::Updated);
        let used_only = updated.iter().all(|id| !not_used_only.contains(id));
        Ok(ChangesResult::new(
            pick(Fate::Created),
            updated,
            pick(Fate::Destroyed),
            has_more,
            State::from(new_state.to_string()),
        )
        .with_updated_properties(used_only.then(|| vec!["used".to_owned()])))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &C,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        only_quota::<O>().map_err(BackendQueryError::Other)?;
        let to = |what: &str, v: Result<Value, serde_json::Error>| {
            v.map_err(|e| BackendQueryError::Other(MemoryError::new(format!("{what}: {e}"))))
        };
        let filter: Option<QuotaFilter> = match params.filter {
            Some(f) => Some(
                serde_json::from_value(to("filter", serde_json::to_value(f))?)
                    .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))?,
            ),
            None => None,
        };
        let sort: Vec<QuotaComparator> = match params.sort {
            Some(s) => serde_json::from_value(to("sort", serde_json::to_value(s))?)
                .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))?,
            None => Vec::new(),
        };
        check_sort(&sort).map_err(BackendQueryError::Method)?;

        let (mut matched, state) = {
            let accounts = self.lock();
            let Some(acc) = accounts.get(account_id) else {
                return Err(BackendQueryError::Method(JmapError::account_not_found()));
            };
            let mut matched = Vec::new();
            for v in acc.quotas.values() {
                let mut q = quota_of(v).map_err(BackendQueryError::Other)?;
                // draft-atwood-jmap-updates-00 §4.2.2: with `language`, the
                // `name` condition and sort see the localized name.
                if let Some(ranges) = params.language
                    && let Some(Value::String(name)) = v
                        .as_object()
                        .and_then(|o| localized_value(o, "name", ranges))
                {
                    q.name = name;
                }
                if filter.as_ref().is_none_or(|f| filter_matches(f, &q)) {
                    matched.push(q);
                }
            }
            (matched, acc.state)
        };
        matched.sort_by(|a, b| compare(&sort, a, b));
        let ids: Vec<Id> = matched.into_iter().map(|q| q.id).collect();
        let total = ids.len() as u64;
        let start = resolve_query_start(&ids, params.position, params.anchor, params.anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let page = ids[start..]
            .iter()
            .take(limit_or_max(params.limit))
            .cloned()
            .collect();
        Ok(QueryResult::new(
            page,
            start as u64,
            Some(total),
            State::from(state.to_string()),
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &C,
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        let QueryChangesParams {
            since_query_state,
            filter,
            sort,
            max_changes,
            ..
        } = params;
        let changes = self
            .get_changes::<O>(
                caller,
                account_id,
                jmap_server::ChangesParams::new(since_query_state).max_changes(max_changes),
            )
            .await?;
        if changes.has_more_changes {
            // RFC 8620 §5.6: a /queryChanges result must reach the current
            // state; past maxChanges it cannot.
            return Err(BackendChangesError::TooManyChanges {
                limit: max_changes.unwrap_or(0),
            });
        }
        let current = self
            .query_objects::<O>(
                caller,
                account_id,
                QueryParams::new().filter(filter).sort(sort),
            )
            .await
            .map_err(|e| match e {
                BackendQueryError::Other(e) => BackendChangesError::Other(e),
                _ => BackendChangesError::CannotCalculate,
            })?;
        let touched: HashSet<&Id> = changes.created.iter().chain(&changes.updated).collect();
        // RFC 8620 §5.6: every updated or destroyed id is removed, and an id
        // that matches now is added back at its current index.
        let mut removed: Vec<Id> = changes
            .updated
            .iter()
            .chain(&changes.destroyed)
            .cloned()
            .collect();
        removed.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        let added = current
            .ids
            .iter()
            .enumerate()
            .filter(|(_, id)| touched.contains(id))
            .map(|(i, id)| AddedItem::new(id.clone(), i as u64))
            .collect();
        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            changes.new_state,
            current.total,
            removed,
            added,
        ))
    }
}

impl<C: MemoryCaller> QuotasBackend for MemoryBackend<C> {}

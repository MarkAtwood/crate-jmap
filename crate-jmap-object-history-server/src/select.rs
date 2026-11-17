//! §3 version selection: which entries a history `Foo/get` returns for one
//! id.

use jmap_object_history_types::{HistoryGetArguments, ObjectHistoryInfo};
use jmap_types::{UTCDate, ValidationError};

/// The §3 history arguments of one `Foo/get`, as the selection reads them.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistorySelector {
    /// `includeReplaced`.
    pub include_replaced: bool,
    /// `includeDestroyed`.
    pub include_destroyed: bool,
    /// `historyAfter`: keep versions replaced strictly after it, and the
    /// live version.
    pub history_after: Option<UTCDate>,
    /// `historyBefore`: keep versions replaced strictly before it; drops
    /// the live version.
    pub history_before: Option<UTCDate>,
    /// `historySkip`: entries to skip per id, newest first.
    pub history_skip: u64,
    /// `historyLimit`: the most entries to return per id, the newest.
    pub history_limit: Option<u64>,
}

impl HistorySelector {
    /// Every argument at its default: no history.
    pub fn new() -> Self {
        Self::default()
    }

    /// The selector for parsed `Foo/get` arguments.
    pub fn from_arguments(args: &HistoryGetArguments) -> Self {
        Self {
            include_replaced: args.include_replaced,
            include_destroyed: args.include_destroyed,
            history_after: args.history_after.clone(),
            history_before: args.history_before.clone(),
            history_skip: args.history_skip,
            history_limit: args.history_limit,
        }
    }

    /// Set `includeReplaced`.
    #[must_use]
    pub fn include_replaced(mut self, on: bool) -> Self {
        self.include_replaced = on;
        self
    }

    /// Set `includeDestroyed`.
    #[must_use]
    pub fn include_destroyed(mut self, on: bool) -> Self {
        self.include_destroyed = on;
        self
    }

    /// Set `historyAfter`.
    #[must_use]
    pub fn history_after(mut self, after: Option<UTCDate>) -> Self {
        self.history_after = after;
        self
    }

    /// Set `historyBefore`.
    #[must_use]
    pub fn history_before(mut self, before: Option<UTCDate>) -> Self {
        self.history_before = before;
        self
    }

    /// Set `historySkip`.
    #[must_use]
    pub fn history_skip(mut self, skip: u64) -> Self {
        self.history_skip = skip;
        self
    }

    /// Set `historyLimit`.
    #[must_use]
    pub fn history_limit(mut self, limit: Option<u64>) -> Self {
        self.history_limit = limit;
        self
    }

    /// Whether either flag is set. With neither, §3: "the server ignores
    /// historyAfter, historyBefore, historySkip, and historyLimit".
    pub fn any_history(&self) -> bool {
        self.include_replaced || self.include_destroyed
    }
}

/// One retained version of an object, as input to [`select_versions`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version<V> {
    /// `objectHistory.version`.
    pub version: u64,
    /// The object as this version stored it.
    pub snapshot: V,
    /// `objectHistory.replaced`; `None` only for the live version.
    pub replaced: Option<UTCDate>,
    /// `objectHistory.expires`; always `None` for the live version.
    pub expires: Option<UTCDate>,
}

impl<V> Version<V> {
    /// The live version `version`.
    pub fn live(version: u64, snapshot: V) -> Self {
        Self {
            version,
            snapshot,
            replaced: None,
            expires: None,
        }
    }

    /// A version replaced at `replaced`, expected to be discarded at
    /// `expires`.
    pub fn replaced(
        version: u64,
        snapshot: V,
        replaced: UTCDate,
        expires: Option<UTCDate>,
    ) -> Self {
        Self {
            version,
            snapshot,
            replaced: Some(replaced),
            expires,
        }
    }
}

/// What [`select_versions`] decided for one id.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection<V> {
    /// These entries, ascending by version (possibly none, for
    /// `historyLimit: 0`: the id is then in neither `list` nor
    /// `notFound`).
    Entries(Vec<(V, ObjectHistoryInfo)>),
    /// The id goes in `notFound`.
    NotFound,
}

/// Select the entries a history `Foo/get` returns for one object (§3).
///
/// `retained` is every version the server retains, ascending by version
/// with distinct `replaced` values; for a live object its last entry is the
/// live version (`replaced: None`), and for a destroyed object (`destroyed`
/// true) every entry has a `replaced`. The final version of a destroyed
/// object is its newest retained one (-04 §3.1). An object with no
/// retained history passes just its live version (§3.1: "it MUST still
/// return the current version").
///
/// The steps follow §3: the flags choose the candidates (a destroyed
/// object needs `includeDestroyed`; without `includeReplaced` only the
/// live, or final, version is a candidate), then `historyAfter` and
/// `historyBefore` (strict; the live version is kept by after and dropped
/// by before), then `historySkip` from the newest, then `historyLimit`
/// keeps the newest. The id is `NotFound` when nothing is left after the
/// skip. With neither flag set the four other arguments are ignored and
/// only a live object's live version is returned. `hasOlder` is true when
/// any older version is retained, returned or not.
///
/// # Errors
///
/// [`ValidationError`] when a `historyAfter`, `historyBefore` or
/// `replaced` value is not a valid `UTCDate`.
pub fn select_versions<V: Clone>(
    retained: &[Version<V>],
    destroyed: bool,
    selector: &HistorySelector,
) -> Result<Selection<V>, ValidationError> {
    if destroyed && !selector.include_destroyed {
        return Ok(Selection::NotFound);
    }
    let Some(newest) = retained.len().checked_sub(1) else {
        return Ok(Selection::NotFound);
    };
    let any = selector.any_history();
    let mut picked: Vec<usize> = if selector.include_replaced {
        (0..retained.len()).collect()
    } else {
        vec![newest]
    };
    if any {
        let mut kept = Vec::with_capacity(picked.len());
        for i in picked {
            let replaced = retained[i].replaced.as_ref();
            if let Some(after) = &selector.history_after
                && let Some(r) = replaced
                && r.cmp_instant(after)?.is_le()
            {
                continue;
            }
            if let Some(before) = &selector.history_before {
                match replaced {
                    None => continue,
                    Some(r) if r.cmp_instant(before)?.is_ge() => continue,
                    Some(_) => {}
                }
            }
            kept.push(i);
        }
        picked = kept;
        let skip = usize::try_from(selector.history_skip).unwrap_or(usize::MAX);
        picked.truncate(picked.len().saturating_sub(skip));
    }
    if picked.is_empty() {
        return Ok(Selection::NotFound);
    }
    if any && let Some(limit) = selector.history_limit {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        picked.drain(..picked.len().saturating_sub(limit));
    }
    Ok(Selection::Entries(
        picked
            .into_iter()
            .map(|i| {
                let v = &retained[i];
                let info = match &v.replaced {
                    None => ObjectHistoryInfo::live(v.version, i > 0),
                    Some(r) => {
                        ObjectHistoryInfo::replaced(v.version, r.clone(), i > 0, v.expires.clone())
                    }
                };
                (v.snapshot.clone(), info)
            })
            .collect(),
    ))
}

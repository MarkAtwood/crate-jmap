//! Shared backend infrastructure for all JMAP server crates.
//!
//! Re-exports the marker traits from `jmap-types` and adds the result types,
//! `BackendChangesError`, and [`JmapBackend`] supertrait. Domain crates add
//! their write-side methods and domain-specific error variants on top.

pub use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Backend error envelopes
// ---------------------------------------------------------------------------

/// Error type returned by [`JmapBackend::get_changes`] and
/// [`JmapBackend::query_changes`].
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendChangesError<E> {
    /// The `sinceState` is too old or the server cannot calculate the full set
    /// of intermediate states. Maps to `tooManyChanges` in the response with
    /// the given suggested limit. Use `limit: 0` for `cannotCalculateChanges`.
    TooManyChanges { limit: u64 },
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendChangesError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyChanges { limit: 0 } => write!(f, "cannot calculate changes"),
            Self::TooManyChanges { limit } => write!(f, "too many changes (limit: {limit})"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendChangesError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<E> for BackendChangesError<E> {
    fn from(e: E) -> Self {
        Self::Other(e)
    }
}

impl<E: std::error::Error> From<BackendChangesError<E>> for jmap_types::JmapError {
    fn from(e: BackendChangesError<E>) -> Self {
        match e {
            BackendChangesError::TooManyChanges { limit: 0 } => {
                jmap_types::JmapError::cannot_calculate_changes()
            }
            BackendChangesError::TooManyChanges { limit } => {
                jmap_types::JmapError::too_many_changes_with_limit(limit)
            }
            BackendChangesError::Other(inner) => {
                jmap_types::JmapError::server_fail(inner.to_string())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of a `/changes` call (RFC 8620 §5.2).
#[derive(Debug)]
#[non_exhaustive]
pub struct ChangesResult {
    /// Ids of objects that were created since `sinceState`.
    pub created: Vec<jmap_types::Id>,
    /// Ids of objects that were updated since `sinceState`.
    pub updated: Vec<jmap_types::Id>,
    /// Ids of objects that were destroyed since `sinceState`.
    pub destroyed: Vec<jmap_types::Id>,
    /// `true` if there are more changes beyond this batch.
    pub has_more_changes: bool,
    /// The current state token after applying all reported changes.
    pub new_state: jmap_types::State,
}

impl ChangesResult {
    /// Construct a [`ChangesResult`].
    pub fn new(
        created: Vec<jmap_types::Id>,
        updated: Vec<jmap_types::Id>,
        destroyed: Vec<jmap_types::Id>,
        has_more_changes: bool,
        new_state: jmap_types::State,
    ) -> Self {
        Self {
            created,
            updated,
            destroyed,
            has_more_changes,
            new_state,
        }
    }
}

/// Result of a `/query` call (RFC 8620 §5.5).
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryResult {
    /// The ordered list of matching object ids.
    pub ids: Vec<jmap_types::Id>,
    /// The 0-based index of the first returned id in the complete result list.
    pub position: i64,
    /// Total number of results, if the backend can calculate it.
    pub total: Option<u64>,
    /// Opaque query state token for subsequent `/queryChanges` calls.
    pub query_state: jmap_types::State,
    /// Whether the backend supports `/queryChanges` for this query.
    pub can_calculate_changes: bool,
}

impl QueryResult {
    /// Construct a [`QueryResult`].
    pub fn new(
        ids: Vec<jmap_types::Id>,
        position: i64,
        total: Option<u64>,
        query_state: jmap_types::State,
        can_calculate_changes: bool,
    ) -> Self {
        Self {
            ids,
            position,
            total,
            query_state,
            can_calculate_changes,
        }
    }
}

/// One entry in the `added` list of a `/queryChanges` response (RFC 8620 §5.6).
#[derive(Debug)]
#[non_exhaustive]
pub struct AddedItem {
    /// The id of the newly-added object.
    pub id: jmap_types::Id,
    /// Its 0-based position in the result list after applying all changes.
    pub index: u64,
}

impl AddedItem {
    /// Construct an [`AddedItem`].
    pub fn new(id: jmap_types::Id, index: u64) -> Self {
        Self { id, index }
    }
}

/// Result of a `/queryChanges` call (RFC 8620 §5.6).
#[derive(Debug)]
#[non_exhaustive]
pub struct QueryChangesResult {
    /// The query state token supplied by the client in `sinceQueryState`.
    pub old_query_state: jmap_types::State,
    /// The current query state token.
    pub new_query_state: jmap_types::State,
    /// Total number of results in the new query, if the backend can calculate it.
    pub total: Option<u64>,
    /// Ids removed from the result set since `oldQueryState`.
    pub removed: Vec<jmap_types::Id>,
    /// Ids added to the result set since `oldQueryState`, with their positions.
    pub added: Vec<AddedItem>,
}

impl QueryChangesResult {
    /// Construct a [`QueryChangesResult`].
    pub fn new(
        old_query_state: jmap_types::State,
        new_query_state: jmap_types::State,
        total: Option<u64>,
        removed: Vec<jmap_types::Id>,
        added: Vec<AddedItem>,
    ) -> Self {
        Self {
            old_query_state,
            new_query_state,
            total,
            removed,
            added,
        }
    }
}

// ---------------------------------------------------------------------------
// JmapBackend — the read-side supertrait
// ---------------------------------------------------------------------------

/// Read-side backend supertrait shared by all JMAP server crates.
///
/// Domain-specific backend traits (`MailBackend`, `ChatBackend`, etc.) require
/// this trait as a supertrait and add write-side methods on top.
///
/// Only the read operations that have an identical signature across all JMAP
/// object types belong here. Write operations (`create_object`, `update_object`,
/// `destroy_object`) and domain-specific operations remain in the domain crate.
///
/// The `collapse_threads` parameter on `query_changes` is included for
/// `Email/queryChanges` (RFC 8621 §4.5). Non-mail backends should pass `false`
/// and may ignore the parameter.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl JmapBackend>` when sharing across tasks.
pub trait JmapBackend: Send + Sync + 'static {
    /// The error type returned by storage operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Fetch objects by id (or all objects when `ids` is `None`).
    ///
    /// Returns `(found, not_found)` — objects that exist and ids that do not.
    fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
        properties: Option<&[<O as JmapObject>::Property]>,
    ) -> impl std::future::Future<Output = Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error>> + Send;

    /// Return the current state token for an object type in the given account.
    fn get_state<O: JmapObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<jmap_types::State, Self::Error>> + Send;

    /// Return changes since `since_state`, up to `max_changes` entries.
    fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        since_state: &jmap_types::State,
        max_changes: Option<u64>,
    ) -> impl std::future::Future<Output = Result<ChangesResult, BackendChangesError<Self::Error>>> + Send;

    /// Execute a `/query` and return a page of matching ids.
    ///
    /// `position` may be negative — negative values are relative to the end of
    /// the result set per RFC 8620 §5.5 (e.g. -1 means the last result).
    fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> impl std::future::Future<Output = Result<QueryResult, Self::Error>> + Send;

    /// Execute a `/queryChanges` and return deltas since `since_query_state`.
    ///
    /// `collapse_threads` is only meaningful for `Email/queryChanges`
    /// (RFC 8621 §4.5). Pass `false` for all other object types.
    #[allow(clippy::too_many_arguments)]
    fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        since_query_state: &jmap_types::State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&jmap_types::Id>,
        collapse_threads: bool,
    ) -> impl std::future::Future<
        Output = Result<QueryChangesResult, BackendChangesError<Self::Error>>,
    > + Send;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: BackendChangesError::TooManyChanges { limit: 0 } must map to
    /// cannotCalculateChanges (RFC 8620 §5.6), not tooManyChanges with limit 0.
    ///
    /// limit=0 is the convention for "cannot calculate".
    #[test]
    fn backend_changes_error_limit_zero_maps_to_cannot_calculate() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::TooManyChanges { limit: 0 },
        );
        assert_eq!(
            err.error_type.as_str(),
            "cannotCalculateChanges",
            "limit=0 must produce cannotCalculateChanges; got: {:?}",
            err.error_type
        );
    }

    /// Oracle: BackendChangesError::TooManyChanges { limit: N } (N > 0) maps to
    /// tooManyChanges with the suggested limit.
    #[test]
    fn backend_changes_error_nonzero_limit_maps_to_too_many_changes() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::TooManyChanges { limit: 50 },
        );
        assert_eq!(
            err.error_type.as_str(),
            "tooManyChanges",
            "limit=50 must produce tooManyChanges; got: {:?}",
            err.error_type
        );
    }
}

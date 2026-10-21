//! Shared backend infrastructure for all JMAP server crates.
//!
//! Re-exports the marker traits and the `SetError` types from `jmap-types` and adds the result types,
//! `BackendChangesError`, `BackendQueryError`, and [`JmapBackend`] supertrait. Domain crates add
//! their write-side methods and domain-specific error variants on top.

pub use jmap_types::{
    GetObject, JmapObject, QueryObject, ReservedExtrasKey, SetError, SetErrorType, SetObject,
    RESERVED_SET_ERROR_WIRE_NAMES,
};

/// Error type returned by create/update/destroy backend methods.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendSetError<E> {
    /// A well-typed JMAP [`SetError`] to place verbatim in the
    /// `notCreated`/`notUpdated`/`notDestroyed` map.
    SetError(SetError),
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendSetError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetError(se) => write!(f, "set error: {se}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendSetError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e),
            _ => None,
        }
    }
}

impl<E> From<SetError> for BackendSetError<E> {
    fn from(e: SetError) -> Self {
        Self::SetError(e)
    }
}

impl<E: std::fmt::Display> BackendSetError<E> {
    /// Split into the per-object [`SetError`] (`Ok`) or a redacted
    /// method-level `serverFail` (`Err`).
    ///
    /// RFC 8620 §5.3 places a per-object `SetError` in the
    /// `notCreated`/`notUpdated`/`notDestroyed` map, so the `SetError`
    /// variant is returned unchanged for the caller to insert there.
    /// [`Other`](Self::Other) becomes
    /// `JmapError::server_fail(SERVER_FAIL_INTERNAL_DESC)`: the inner
    /// error's `Display` text never reaches the wire (bd:JMAP-jfia.36).
    ///
    /// There is deliberately no `From<BackendSetError<E>> for JmapError`:
    /// a `?` on that impl would turn per-object set errors into
    /// method-level errors.
    ///
    /// [`SERVER_FAIL_INTERNAL_DESC`]: crate::handlers::SERVER_FAIL_INTERNAL_DESC
    ///
    /// # Errors
    ///
    /// Returns the redacted `serverFail` [`JmapError`](jmap_types::JmapError) for
    /// [`Other`](Self::Other).
    pub fn into_set_error_or_method_error(self) -> Result<SetError, jmap_types::JmapError> {
        match self {
            Self::SetError(se) => Ok(se),
            Self::Other(e) => Err(crate::handlers::server_fail_from_backend(&e)),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend error envelopes
// ---------------------------------------------------------------------------

/// Error type returned by [`JmapBackend::get_changes`] and
/// [`JmapBackend::query_changes`].
///
/// The two non-`Other` variants map to the two distinct RFC 8620 §5.2 and
/// §5.6 wire errors, `cannotCalculateChanges` and `tooManyChanges`.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendChangesError<E> {
    /// The server has no usable change log for the given `sinceState`
    /// and cannot supply incremental changes — the client MUST
    /// discard ALL locally cached objects for the affected type,
    /// reset its local state token to the empty string, and perform a
    /// full resync (`/get` with `ids: null`). Partial recovery is not
    /// permitted. Maps to `cannotCalculateChanges` (RFC 8620 §5.6;
    /// authoritative behavior documented in jmapio/jmap-js
    /// `mail-model.js`).
    CannotCalculate,
    /// The change window exceeds what the server can supply in a
    /// single `/changes` response. Maps to `tooManyChanges` with the
    /// `limit` as the suggested maximum — the client may retry with
    /// a smaller window.
    TooManyChanges {
        /// Maximum window size the server can supply in a single
        /// `/changes` response, the suggested maximum the client may
        /// retry with.
        limit: u64,
    },
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendChangesError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CannotCalculate => write!(f, "cannot calculate changes"),
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
            BackendChangesError::CannotCalculate => {
                jmap_types::JmapError::cannot_calculate_changes()
            }
            BackendChangesError::TooManyChanges { limit } => {
                jmap_types::JmapError::too_many_changes_with_limit(limit)
            }
            // bd:JMAP-jfia.1 / bd:JMAP-wlip.2 — the `Other` arm wraps a
            // backend `Error` whose `Display` impl is contractually
            // forbidden from carrying credential/blob/PII text but which
            // we still treat as untrusted at the wire boundary. Use the
            // same static [`SERVER_FAIL_INTERNAL_DESC`] string that the
            // [`server_fail_from_backend`] handler-layer helper uses so
            // the defence-in-depth chain (backend Display contract →
            // handler helper → this From impl) cannot be bypassed by
            // handlers that take the ergonomic `.map_err(JmapError::from)?`
            // path on `BackendChangesError`.
            //
            // [`SERVER_FAIL_INTERNAL_DESC`]: crate::handlers::SERVER_FAIL_INTERNAL_DESC
            // [`server_fail_from_backend`]: crate::handlers::server_fail_from_backend
            BackendChangesError::Other(_inner) => {
                jmap_types::JmapError::server_fail(crate::handlers::SERVER_FAIL_INTERNAL_DESC)
            }
        }
    }
}

/// Error type returned by [`JmapBackend::query_objects`].
///
/// RFC 8620 §5.5 lets a `/query` fail with method errors that only the
/// backend can detect (`unsupportedFilter`, `unsupportedSort`,
/// `anchorNotFound`, ...). [`Self::Method`] carries such an error to the
/// client unchanged; [`Self::Other`] is an internal storage-layer failure
/// that handlers surface as the redacted `serverFail`.
#[derive(Debug)]
pub enum BackendQueryError<E> {
    /// An RFC 8620 §5.5 method error, returned to the client as-is
    /// (type and description). Backends MUST NOT put internal error
    /// text in it.
    Method(jmap_types::JmapError),
    /// An unexpected storage-layer error.
    Other(E),
}

impl<E: std::fmt::Display> std::fmt::Display for BackendQueryError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Method(e) => write!(f, "{e}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BackendQueryError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Method(_) => None,
            Self::Other(e) => Some(e),
        }
    }
}

impl<E> From<E> for BackendQueryError<E> {
    fn from(e: E) -> Self {
        Self::Other(e)
    }
}

impl<E> From<BackendQueryError<E>> for jmap_types::JmapError {
    fn from(e: BackendQueryError<E>) -> Self {
        match e {
            BackendQueryError::Method(err) => err,
            // Same redaction as `BackendChangesError::Other`: the backend
            // error's text never reaches the wire.
            BackendQueryError::Other(_inner) => {
                jmap_types::JmapError::server_fail(crate::handlers::SERVER_FAIL_INTERNAL_DESC)
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
    ///
    /// RFC 8620 §5.5 specifies this as `UnsignedInt` in the response —
    /// a non-negative integer (bd:JMAP-wlip.25). The request-side
    /// position parameter accepts negative values as end-relative
    /// offsets, but the response position cannot validly be negative.
    /// Backends that derive `position` from a request-side `i64`
    /// offset MUST clamp / normalize to `u64` before constructing this
    /// struct.
    pub position: u64,
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
        position: u64,
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

    /// Construct a [`QueryResult`] from a signed request-side position,
    /// clamping negatives to `0` and normalizing to `u64`
    /// (bd:JMAP-jfia.25).
    ///
    /// RFC 8620 §5.5 specifies the response `position` as `UnsignedInt`,
    /// but the request-side `position` parameter accepts negative
    /// values as end-relative offsets. Backends typically receive the
    /// signed value, walk the offset into the result list, and need
    /// to surface the resulting 0-based index. This constructor takes
    /// the resolved-offset value and clamps to `u64`, matching the
    /// spec contract.
    ///
    /// Negative inputs clamp to `0` (representing "the start of the
    /// result list"). Backends that have already done the
    /// offset-to-index resolution and have a `u64` already SHOULD use
    /// the plain [`Self::new`] constructor instead.
    pub fn new_clamped(
        ids: Vec<jmap_types::Id>,
        position_signed: i64,
        total: Option<u64>,
        query_state: jmap_types::State,
        can_calculate_changes: bool,
    ) -> Self {
        // i64::max(_, 0) then cast to u64 — both fit in u64 because
        // i64 >= 0 has range [0, i64::MAX], which is a strict subset
        // of u64's range.
        let position = u64::try_from(position_signed.max(0)).unwrap_or(0);
        Self::new(ids, position, total, query_state, can_calculate_changes)
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
///
/// # CallerCtx
///
/// Every backend method takes a `caller: &Self::CallerCtx` parameter as the
/// first argument after `&self`. This is the per-request authentication /
/// authorisation context produced by the caller's auth layer and forwarded
/// unchanged through [`crate::Dispatcher::dispatch`] → [`crate::JmapHandler`]
/// → the registered closure → the backend.
///
/// Implementations that do not need an auth identity can use the unit type:
///
/// ```rust,ignore
/// impl JmapBackend for MyBackend {
///     type Error = MyError;
///     type CallerCtx = ();
///     // ...
/// }
/// ```
///
/// Implementations that do need to differentiate behaviour per caller (e.g.
/// applying per-user visibility rules, or rejecting reads with
/// `forbidden` when the caller is not the owner of the account) read the
/// `caller` parameter to decide.
///
/// The trait bound `Clone + Send + 'static` is what [`crate::Dispatcher`]
/// requires; the bound is repeated here so the supertrait can stand on its
/// own without depending on the dispatcher.
pub trait JmapBackend: Send + Sync + 'static {
    /// The error type returned by storage operations.
    ///
    /// # Security
    ///
    /// The `Display` impl of this type is surfaced through
    /// [`BackendSetError::Other`]'s and [`BackendChangesError::Other`]'s
    /// own `Display` impls, which in turn flow into
    /// [`crate::request_error`]'s `RequestError::Display` output. When a
    /// downstream consumer wires tracing-style logging on top, the
    /// formatted error text lands in operator logs verbatim.
    ///
    /// Implementations MUST NOT include any of the following in this
    /// type's `Display` output:
    ///
    /// - **Credential material** — auth tokens, passwords, push
    ///   verification codes, invite codes, session cookies, or anything
    ///   derived byte-for-byte from an `Authorization`-header value.
    /// - **Blob content** — email bodies, sieve scripts, file contents,
    ///   or any user-supplied opaque payload. An error like
    ///   `"sieve parse error at line 42: <script excerpt>"` violates
    ///   this — emit the line number and a short type-only summary
    ///   ("sieve parse error at line 42: unexpected token") and let the
    ///   server log the full script body separately under a redacted
    ///   path.
    /// - **PII shaped like an email address** in any code path that an
    ///   unauthenticated caller can trigger. Wrapping a downstream
    ///   service error that interpolates the caller's email is the
    ///   common foot-gun.
    ///
    /// Errors that wrap a downstream-service failure should sanitize
    /// the downstream error text — or strip it entirely and replace it
    /// with a static summary — before constructing the `Display`
    /// string. The same rule applies to every extension `*Backend`
    /// trait that inherits this associated type by transitivity:
    /// `MailBackend::Error`, `ChatBackend::Error`,
    /// `CalendarsBackend::Error`, `TasksBackend::Error`,
    /// `ContactsBackend::Error`, `FileNodeBackend::Error`, and
    /// `SharingBackend::Error` are all the same `JmapBackend::Error`
    /// associated type — the contract here governs all of them.
    ///
    /// Precedent: bd:JMAP-sc1b.79 redacted `BearerAuth` and `BasicAuth`
    /// at the type-derive level; bd:JMAP-sc1b.100 documents the
    /// equivalent contract at the trait-associated-type level.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The per-request caller context type produced by the auth layer and
    /// forwarded by [`crate::Dispatcher::dispatch`] into every method call.
    ///
    /// Use `()` when no auth context is needed.
    ///
    /// The bound is `Clone + Send + Sync + 'static`:
    /// - `Clone` because [`crate::Dispatcher`] clones the value once per
    ///   method call in the batch.
    /// - `Send + 'static` because each method call is spawned on a
    ///   [`tokio::task`].
    /// - `Sync` because handler method bodies take `&Self::CallerCtx`
    ///   and hold that reference across `.await` boundaries inside a
    ///   `Send` future (a `&T` is `Send` iff `T: Sync`).
    type CallerCtx: Clone + Send + Sync + 'static;

    /// Return `true` if the given account exists in this backend.
    ///
    /// Handlers call this at the start of each method to return
    /// `accountNotFound` (RFC 8620 §3.6.2) rather than surfacing
    /// the wrong error when `accountId` is unknown.
    ///
    /// # Performance contract (bd:JMAP-jfia.27)
    ///
    /// Implementations SHOULD return in sub-millisecond time. The
    /// JMAP standard handlers (`handle_get`, `handle_changes`,
    /// `handle_query`, `handle_query_changes`) each call
    /// `account_exists` at the START of the method, BEFORE delegating
    /// to the per-domain backend. A typical JMAP request batch is
    /// 4–16 method calls; a naive remote round-trip per call would
    /// add 4–16× the network latency to every batch.
    ///
    /// Acceptable backing strategies: in-memory cache (the
    /// reference `MemoryBackend` impls all use this); an indexed
    /// primary-key lookup against a local database; a bloom filter
    /// for negative lookups paired with a cache for positives. A
    /// naive `SELECT 1 FROM accounts WHERE id = ?` against a remote
    /// database is INCORRECT for this method even though it would
    /// return the right value — the round-trip cost is the bug.
    ///
    /// The dispatcher does not cache this call across the method
    /// calls in a single batch (workspace-architectural decision —
    /// the dispatcher is intentionally stateless across the batch
    /// loop). Caching is the backend implementor's responsibility.
    fn account_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Fetch objects by id (or all objects when `ids` is `None`).
    ///
    /// `properties` is the list of property names requested by the client
    /// (RFC 8620 §5.1). `None` means the client did not send a `properties`
    /// field; the backend should return all properties. When `Some`, the backend
    /// MAY filter the response to only the named properties, but is not required
    /// to — implementations that always return all properties are correct.
    ///
    /// Returns `(found, not_found)` — objects that exist and ids that do not.
    fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
        properties: Option<&[String]>,
    ) -> impl std::future::Future<Output = Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error>> + Send;

    /// Return the current state token for an object type in the given account.
    fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<jmap_types::State, Self::Error>> + Send;

    /// Return changes since `since_state`, up to `max_changes` entries.
    fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        since_state: &jmap_types::State,
        max_changes: Option<u64>,
    ) -> impl std::future::Future<Output = Result<ChangesResult, BackendChangesError<Self::Error>>> + Send;

    /// Execute a `/query` and return a page of matching ids.
    ///
    /// `position` may be negative — negative values are relative to the end of
    /// the result set per RFC 8620 §5.5 (e.g. -1 means the last result).
    ///
    /// `anchor` and `anchor_offset` are the RFC 8620 §5.5 `anchor` and
    /// `anchorOffset` arguments. With an `anchor`, `position` MUST be
    /// ignored and the page starts at the anchor's index in the filtered,
    /// sorted results plus `anchor_offset`, clamped to 0; an anchor not in
    /// the results is [`JmapError::anchor_not_found`] via
    /// [`BackendQueryError::Method`]. Without one, `anchor_offset` MUST be
    /// ignored. [`QueryResult::position`] is the page's actual start index.
    /// [`crate::resolve_query_start`] implements this over an in-memory id
    /// list.
    ///
    /// [`JmapError::anchor_not_found`]: jmap_types::JmapError::anchor_not_found
    ///
    /// # Filter and sort handling
    ///
    /// Implementations MUST honour the supplied `filter` and `sort` arguments
    /// efficiently — typically by pushing both into the indexed storage layer
    /// (database WHERE / ORDER BY, search index, etc.). Returning every
    /// matching id and relying on the caller to paginate after the fact
    /// degenerates to O(n) per page for IMAP-migration accounts.
    ///
    /// Handler implementations in `jmap-*-server` crates SHOULD NOT
    /// post-filter or post-sort the backend's result; doing so re-introduces
    /// the O(n) cost this method exists to avoid. The Mailbox handler in
    /// `jmap-mail-server` is the canonical example of pushing filter/sort
    /// fully into the backend.
    ///
    /// # Errors
    ///
    /// Return [`BackendQueryError::Method`] for an RFC 8620 §5.5 method
    /// error (`unsupportedFilter`, `unsupportedSort`, `anchorNotFound`,
    /// ...); handlers pass it to the client unchanged. Return
    /// [`BackendQueryError::Other`] (or use `?` on `Self::Error`) for an
    /// internal failure; handlers surface it as a redacted `serverFail`.
    #[allow(clippy::too_many_arguments)]
    fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&jmap_types::Id>,
        anchor_offset: i64,
    ) -> impl std::future::Future<Output = Result<QueryResult, BackendQueryError<Self::Error>>> + Send;

    /// Execute a `/queryChanges` and return deltas since `since_query_state`.
    ///
    /// `collapse_threads` is only meaningful for `Email/queryChanges`
    /// (RFC 8621 §4.5). Pass `false` for all other object types.
    #[allow(clippy::too_many_arguments)]
    fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
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

    /// The caller's stable identity within this account namespace.
    ///
    /// Returns `None` for deployments that have not wired identity
    /// (test fixtures, single-user dev servers). A `None`-returning
    /// backend CANNOT honor JMAP semantics that depend on caller
    /// identity — chat role-hierarchy, calendar ACLs, sharing/myRights,
    /// per-user $seen on shared mailboxes, per-user metadata
    /// `privateMetadata` filtering, etc. Authentication is still the HTTP
    /// layer's job; this method exposes the result of that
    /// authentication to the JMAP layer for in-method semantics.
    ///
    /// Implementations MUST NOT mint identity — they MUST read it
    /// from the `CallerCtx` populated by the HTTP/auth middleware
    /// before `dispatch()` was called.
    ///
    /// Backends that honor identity-dependent semantics MUST override
    /// this method. Handlers and downstream backend traits MAY rely on
    /// it being correct when it returns `Some`.
    ///
    /// # Why an associated function and not a method (bd:JMAP-wlip.6 / bd:JMAP-jfia.13)
    ///
    /// The signature deliberately takes `caller: &Self::CallerCtx`
    /// without a `&self` receiver. Backends therefore have no access
    /// to their own storage state from inside `principal_id`. The
    /// auth-layer middleware MUST pre-resolve the principal (e.g. map
    /// a JWT `sub` claim to a local `Id` via an internal lookup) and
    /// stash the result inside `CallerCtx` *before* it calls
    /// `dispatch`. The JMAP layer reads the pre-resolved value here;
    /// no JIT lookup is possible.
    ///
    /// This is a structural enforcement of the "identity is not the
    /// JMAP layer's job to mint" rule. A consumer that wants
    /// JIT-resolved identity (e.g. database-backed JWT → principal
    /// mapping) wires that mapping into the HTTP layer's `CallerCtx`
    /// construction step instead of trying to fit it inside the
    /// backend's `principal_id` impl.
    ///
    /// **Decision record (bd:JMAP-jfia.13)**: a future reviewer or AI
    /// tool will reasonably suggest "this is an oversight, surely the
    /// backend wants `&self` access to its identity store" and propose
    /// `fn principal_id(&self, caller: ...) -> ...`. That suggestion
    /// is **WRONG** and must be rejected: the function-vs-method
    /// distinction is the structural enforcement of the no-JIT-lookup
    /// policy. Comments alone could be ignored; the type system makes
    /// the wrong thing impossible. Defending this shape protects the
    /// workspace identity-seam policy from drift.
    fn principal_id(caller: &Self::CallerCtx) -> Option<&jmap_types::Id> {
        let _ = caller;
        None
    }

    /// Maximum number of objects this account permits in a single
    /// `/set` call (RFC 8620 §5.3 `maxObjectsInSet`) (bd:JMAP-ayoz.41.1).
    ///
    /// Counts the sum of `create` + `update` + `destroy` entries in the
    /// wire arguments. Handlers MUST enforce this at the top of every
    /// `handle_*_set` via [`crate::helpers::enforce_max_objects_in_set`]
    /// so a single batched request cannot drive O(M·N) work against
    /// the storage layer.
    ///
    /// The default value `500` mirrors the workspace's testjig Session
    /// JSON advertised cap and matches common Fastmail / Cyrus IMAP
    /// server defaults. The cap is a *floor* on permissiveness, not a
    /// floor on capability — backends MAY override per account (Free vs
    /// Pro tier, multi-tenant SaaS, etc.). The `caller` and `account_id`
    /// arguments are passed even though the default impl ignores them
    /// so production backends can vary without an API break.
    ///
    /// Returning `0` makes every non-empty `/set` call fail with
    /// `requestTooLarge` (defensive read-only mode). Returning
    /// `u64::MAX` effectively disables the cap (NOT recommended — the
    /// cap is a DoS defence and disabling it forfeits that defence).
    ///
    /// # Why on `JmapBackend` and not a per-extension `XxxLimits` struct
    ///
    /// `maxObjectsInSet` is RFC 8620 §5.3 base-protocol scope, not an
    /// extension concept. Putting it on the foundation supertrait
    /// covers all 8 extension server crates with one default impl;
    /// per-extension `XxxLimits` structs are the right shape for
    /// extension-specific caps (per-Space content limits, per-Mailbox
    /// message size, etc. per workspace AGENTS.md "Backend caps and
    /// limits") but are out of scope here.
    fn max_objects_in_set(&self, caller: &Self::CallerCtx, account_id: &jmap_types::Id) -> u64 {
        let _ = (caller, account_id);
        500
    }

    /// Maximum number of ids this account permits in a single `/get`
    /// call (RFC 8620 §5.1 `maxObjectsInGet`) (bd:JMAP-yt1w.31).
    ///
    /// Handlers MUST enforce this at the top of every `/get` handler via
    /// [`crate::helpers::enforce_max_objects_in_get`]; the generic
    /// [`crate::handlers::handle_get`] already does. A null `ids` (fetch
    /// all) is not counted.
    ///
    /// The default value `500` is the RFC 8620 §2 suggested minimum and
    /// mirrors [`JmapBackend::max_objects_in_set`]. Backends MAY override
    /// per account; the `caller` and `account_id` arguments exist for
    /// that. Keep the value in step with the `maxObjectsInGet` advertised
    /// in the Session object.
    fn max_objects_in_get(&self, caller: &Self::CallerCtx, account_id: &jmap_types::Id) -> u64 {
        let _ = (caller, account_id);
        500
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle (RFC 8620 §5.6): `CannotCalculate` maps to the
    /// `cannotCalculateChanges` wire error.
    #[test]
    fn backend_changes_error_cannot_calculate_maps_to_cannot_calculate_changes() {
        let err = jmap_types::JmapError::from(
            BackendChangesError::<std::convert::Infallible>::CannotCalculate,
        );
        assert_eq!(
            err.error_type.to_string(),
            "cannotCalculateChanges",
            "CannotCalculate must produce cannotCalculateChanges; got: {:?}",
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
            err.error_type.to_string(),
            "tooManyChanges",
            "limit=50 must produce tooManyChanges; got: {:?}",
            err.error_type
        );
    }

    /// Oracle (bd:JMAP-jfia.1 / bd:JMAP-wlip.2): the
    /// `From<BackendChangesError<E>> for JmapError` impl MUST NOT echo
    /// the wrapped backend error's `Display` text into the resulting
    /// `JmapError`'s description. The defence-in-depth contract is that
    /// even if a backend implementor accidentally violates the
    /// [`JmapBackend::Error`] Display MUST-NOT (credential / blob /
    /// PII), the leaked text never reaches the wire — and the
    /// ergonomic `.map_err(JmapError::from)?` path that
    /// `handle_changes` / `handle_query_changes` take on
    /// `BackendChangesError` must redact identically to the explicit
    /// `server_fail_from_backend(&e)` helper used elsewhere.
    ///
    /// Test vector: an `Other` variant whose Display contains a canary
    /// string resembling a credential leak. The canary literal is
    /// hand-built and not derived from any production type's
    /// behaviour. Mirrors
    /// `server_fail_from_backend_drops_display_text` in
    /// `handlers.rs`.
    #[test]
    fn backend_changes_error_other_drops_display_text() {
        #[derive(Debug)]
        struct LeakyError(&'static str);
        impl std::fmt::Display for LeakyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
        impl std::error::Error for LeakyError {}

        const CANARY: &str = "TOKEN-DO-NOT-LEAK-c0ffee";
        let err: BackendChangesError<LeakyError> = BackendChangesError::Other(LeakyError(CANARY));

        let jmap_err = jmap_types::JmapError::from(err);

        // Serialize to wire shape and assert the canary is absent from
        // the resulting JSON. The error_invocation wraps a JmapError as
        // { "type": "serverFail", "description": "..." } — both fields
        // are wire-visible.
        let wire = serde_json::to_value(&jmap_err).expect("JmapError must serialize");
        let wire_str = wire.to_string();
        assert!(
            !wire_str.contains(CANARY),
            "From<BackendChangesError<E>> for JmapError must not echo \
             backend error Display onto the wire; got {wire_str}"
        );
        // The description MUST be exactly SERVER_FAIL_INTERNAL_DESC.
        assert_eq!(
            wire["description"],
            crate::handlers::SERVER_FAIL_INTERNAL_DESC,
            "description must be the static 'internal error' string"
        );
        assert_eq!(wire["type"], "serverFail");
    }

    /// Oracle (bd:JMAP-jfia.36): `into_set_error_or_method_error` on
    /// `Other` MUST NOT echo the wrapped error's `Display` text; the
    /// wire form is exactly `{"type":"serverFail","description":"internal
    /// error"}` (hand-written expected JSON). The canary literal is
    /// hand-built. Mirrors `backend_changes_error_other_drops_display_text`.
    #[test]
    fn backend_set_error_other_drops_display_text() {
        struct LeakyError(&'static str);
        impl std::fmt::Display for LeakyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }

        const CANARY: &str = "TOKEN-DO-NOT-LEAK-c0ffee";
        let err: BackendSetError<LeakyError> = BackendSetError::Other(LeakyError(CANARY));

        let Err(jmap_err) = err.into_set_error_or_method_error() else {
            panic!("Other must map to a method-level JmapError");
        };
        let wire = serde_json::to_value(&jmap_err).expect("JmapError must serialize");
        assert!(
            !wire.to_string().contains(CANARY),
            "into_set_error_or_method_error must not echo backend Display; got {wire}"
        );
        assert_eq!(
            wire,
            serde_json::json!({"type": "serverFail", "description": "internal error"})
        );
    }

    /// Oracle (RFC 8620 §5.3): a per-object `SetError` belongs in the
    /// `notCreated`/`notUpdated`/`notDestroyed` map, so the `SetError`
    /// arm must come back as `Ok` with its wire form intact
    /// (hand-written expected JSON).
    #[test]
    fn backend_set_error_set_error_passes_through() {
        let err: BackendSetError<std::fmt::Error> =
            BackendSetError::SetError(SetError::new(SetErrorType::Forbidden));
        let se = err
            .into_set_error_or_method_error()
            .expect("SetError arm must be Ok");
        assert_eq!(
            serde_json::to_value(&se).expect("SetError must serialize"),
            serde_json::json!({"type": "forbidden"})
        );
    }

    /// Oracle (bd:JMAP-ga0q.1): `JmapBackend::principal_id` has a default impl
    /// that returns `None`. A backend whose `CallerCtx = ()` and that does NOT
    /// override `principal_id` inherits that default and signals "identity not
    /// wired" to callers. JMAP semantics that depend on caller identity must
    /// treat `None` as a hard "cannot honor".
    #[test]
    fn principal_id_default_impl_returns_none_for_unit_caller_ctx() {
        // Minimal stub backend exercising only the default impl. All other
        // trait methods are stubbed with `unreachable!()` and never invoked.
        struct StubBackend;

        #[derive(Debug)]
        struct StubError;

        impl std::fmt::Display for StubError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("stub")
            }
        }
        impl std::error::Error for StubError {}

        impl JmapBackend for StubBackend {
            type Error = StubError;
            type CallerCtx = ();

            async fn account_exists(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
            ) -> Result<bool, Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn get_objects<O: GetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _ids: Option<&[jmap_types::Id]>,
                _properties: Option<&[String]>,
            ) -> Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn get_state<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
            ) -> Result<jmap_types::State, Self::Error> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn get_changes<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _since_state: &jmap_types::State,
                _max_changes: Option<u64>,
            ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn query_objects<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _limit: Option<u64>,
                _position: i64,
                _anchor: Option<&jmap_types::Id>,
                _anchor_offset: i64,
            ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
                unreachable!("only principal_id is exercised in this test")
            }

            async fn query_changes<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _since_query_state: &jmap_types::State,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _max_changes: Option<u64>,
                _up_to_id: Option<&jmap_types::Id>,
                _collapse_threads: bool,
            ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
                unreachable!("only principal_id is exercised in this test")
            }
        }

        let caller: <StubBackend as JmapBackend>::CallerCtx = ();
        let id = <StubBackend as JmapBackend>::principal_id(&caller);
        assert!(
            id.is_none(),
            "default principal_id impl must return None; got Some({:?})",
            id
        );
    }

    /// Oracle (bd:JMAP-ayoz.41.1): `JmapBackend::max_objects_in_set`
    /// has a default impl that returns `500`. The constant is the
    /// workspace's testjig Session JSON advertised cap; a backend that
    /// does not override the method inherits a sane DoS-defence
    /// default rather than silently disabling the cap.
    #[test]
    fn max_objects_in_set_default_impl_returns_500() {
        struct StubBackend;

        #[derive(Debug)]
        struct StubError;

        impl std::fmt::Display for StubError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("stub")
            }
        }
        impl std::error::Error for StubError {}

        impl JmapBackend for StubBackend {
            type Error = StubError;
            type CallerCtx = ();

            async fn account_exists(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
            ) -> Result<bool, Self::Error> {
                unreachable!("only max_objects_in_set is exercised in this test")
            }

            async fn get_objects<O: GetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _ids: Option<&[jmap_types::Id]>,
                _properties: Option<&[String]>,
            ) -> Result<(Vec<O>, Vec<jmap_types::Id>), Self::Error> {
                unreachable!("only max_objects_in_set is exercised in this test")
            }

            async fn get_state<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
            ) -> Result<jmap_types::State, Self::Error> {
                unreachable!("only max_objects_in_set is exercised in this test")
            }

            async fn get_changes<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _since_state: &jmap_types::State,
                _max_changes: Option<u64>,
            ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
                unreachable!("only max_objects_in_set is exercised in this test")
            }

            async fn query_objects<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _limit: Option<u64>,
                _position: i64,
                _anchor: Option<&jmap_types::Id>,
                _anchor_offset: i64,
            ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
                unreachable!("only max_objects_in_set is exercised in this test")
            }

            async fn query_changes<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _since_query_state: &jmap_types::State,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _max_changes: Option<u64>,
                _up_to_id: Option<&jmap_types::Id>,
                _collapse_threads: bool,
            ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
                unreachable!("only max_objects_in_set is exercised in this test")
            }
        }

        let backend = StubBackend;
        let caller: <StubBackend as JmapBackend>::CallerCtx = ();
        let id = jmap_types::Id::from("any-account");
        assert_eq!(
            backend.max_objects_in_set(&caller, &id),
            500,
            "default max_objects_in_set must return 500"
        );
    }
}

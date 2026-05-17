//! `FileNodeBackend` trait and supporting type re-exports.
//!
//! Consumers implement [`FileNodeBackend`] for their storage system.  The
//! method handlers in [`crate::filenode`] call into the backend through this
//! trait.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait.  Only write operations and the
//! FileNode-specific structural queries live here.

pub use jmap_filenode_types::backend::FileNodeProperty;
use jmap_filenode_types::FileNode;
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

// ---------------------------------------------------------------------------
// CaseFolding
// ---------------------------------------------------------------------------

/// Case-folding mode for [`FileNodeBackend::find_sibling_by_name`].
///
/// JMAP FileNode (`draft-ietf-jmap-filenode-13` §3.2.3
/// `compareCaseInsensitively`) is a Boolean on the wire, but the backend
/// trait uses a typed enum so call sites are self-documenting and the
/// algorithm space is open for future variants without a SemVer break.
///
/// The specific algorithm used by `Insensitive` is implementation-defined;
/// see [`FileNodeBackend::find_sibling_by_name`] for the contract gap and
/// the workspace policy on case-folding.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseFolding {
    /// Byte-for-byte (case-sensitive) comparison.
    Sensitive,
    /// Implementation-defined case-insensitive comparison. The specific
    /// algorithm is the backend's choice; see the trait method's doc for
    /// the operational contract.
    Insensitive,
}

impl CaseFolding {
    /// Map the JMAP wire-format `compareCaseInsensitively` boolean to a
    /// `CaseFolding` value. Convenience for handler call sites.
    #[must_use]
    pub fn from_wire_bool(case_insensitive: bool) -> Self {
        if case_insensitive {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }
}

// ---------------------------------------------------------------------------
// FileNodeBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP FileNode method handlers
/// (draft-ietf-jmap-filenode-13).
///
/// Implementors provide the actual data access; the handler module
/// translates between the JMAP wire protocol and these backend calls.
///
/// Read-side operations are defined on the [`JmapBackend`] supertrait.
/// Only write operations and FileNode-specific structural queries are here.
///
/// This trait is not object-safe by design (generic methods).  Use
/// `Arc<impl FileNodeBackend>` when sharing across tasks.
pub trait FileNodeBackend: JmapBackend {
    /// Create a new FileNode.
    ///
    /// Returns `(assigned_id, created_object)` on success.  `create_id` is
    /// the client-side creation id from the `/set` request.
    ///
    /// # Invariants the backend MUST re-verify atomically with the insert
    ///
    /// The handler in [`crate::handle_filenode_set`] does best-effort
    /// pre-checks before invoking `create_object`. These pre-checks are
    /// **not transactional** with the insert — a concurrent client may
    /// mutate the same parent between the pre-check and the insert. The
    /// backend is the canonical enforcement point for the following
    /// FileNode-specific invariants and MUST re-verify each one inside
    /// the same atomic boundary as the insert (e.g. inside a single SQL
    /// transaction with appropriate row locks, or under the same in-
    /// memory lock for non-SQL backends):
    ///
    /// - **Name uniqueness under `parentId`** (draft-ietf-jmap-filenode-13
    ///   §3.2.3): no two siblings may share the same `name` under the
    ///   same `parentId`. The backend MUST return
    ///   [`BackendSetError::SetError`] carrying `SetErrorType::AlreadyExists`
    ///   (or the FileNode-specific equivalent the handler maps to wire) if
    ///   the insert would violate this. The handler's `onExists` mode
    ///   (`reject` / `replace` / `rename`) decides the pre-check shape;
    ///   the backend's job is to refuse the actual collision.
    ///
    /// - **Blob existence when `blobId` is non-null** (§3.1, §3.2.3): if
    ///   the requested node is a file referencing a blob, the backend
    ///   MUST verify the blob exists at insert time. A pre-check via
    ///   [`Self::blob_exists`] is not transactional with the insert.
    ///
    /// - **Parent existence and node-type compatibility** (§3.1): the
    ///   parent referenced by `parentId` (when non-null) MUST exist at
    ///   insert time and MUST be of node-type `directory`.
    ///
    /// A backend that follows the docstring literally — "if create
    /// succeeds, return Ok" — and skips the atomic re-verification will
    /// silently corrupt the FileNode invariants under concurrent writes.
    /// MemoryBackend gets this right because the single in-memory lock
    /// covers both the read pre-check and the insert; production SQL
    /// backends MUST use explicit row locks or unique constraints.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing FileNode.
    ///
    /// Returns `Some(updated_object)` if the backend modified properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    ///
    /// # Invariants the backend MUST re-verify atomically with the update
    ///
    /// Like [`Self::create_object`], the handler's pre-checks are
    /// best-effort and the backend is the canonical enforcement point.
    /// When the patch mutates `parentId` and/or `name`, the backend MUST
    /// re-verify the following invariants inside the same atomic
    /// boundary as the write:
    ///
    /// - **Cycle prevention when `parentId` changes** (draft-ietf-jmap-
    ///   filenode-13 §3.2.3): the new `parentId` MUST NOT be the node
    ///   itself nor any of its descendants. A pre-check via
    ///   [`Self::get_descendant_ids`] is not transactional with the
    ///   update — a concurrent move could change the tree shape between
    ///   the pre-check and the patch.
    ///
    /// - **Name uniqueness under the new (or unchanged) `parentId`**
    ///   when `name` changes: same shape as the create-side invariant.
    ///
    /// - **Blob existence when `blobId` changes** to a non-null value:
    ///   same shape as the create-side invariant.
    ///
    /// - **Immutable-property prevention**: certain fields (`id`,
    ///   `nodeType`, `created`, server-managed timestamps) are
    ///   immutable per the spec. The handler rejects patches that
    ///   touch them with `invalidProperties` before reaching this
    ///   method; the backend MAY assume the patch does not contain
    ///   those keys.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a FileNode by id.
    ///
    /// # Invariants the backend MUST re-verify atomically with the destroy
    ///
    /// - **`nodeHasChildren` guard**: when the FileNode has descendants
    ///   AND the handler's `onDestroyRemoveChildren` flag is `false`,
    ///   the destroy MUST be refused. The handler does a pre-check via
    ///   [`Self::get_descendant_ids`] and returns the wire-level
    ///   `nodeHasChildren` error before invoking this method, but the
    ///   pre-check is not transactional. A backend that supports
    ///   concurrent writers MUST re-verify atomically — either via
    ///   referential-integrity constraints (FOREIGN KEY ... ON DELETE
    ///   RESTRICT) or via an explicit re-check under the same lock as
    ///   the row removal.
    ///
    /// - **Cascade destroy ordering**: when the handler invokes
    ///   `destroy_object` as part of a cascade (handler has called
    ///   [`Self::get_descendant_ids`] and is destroying descendants
    ///   first, then the parent), the backend MUST NOT reject the
    ///   parent destroy on `nodeHasChildren` grounds even if the
    ///   descendants are not yet fully removed — the handler is
    ///   coordinating the ordering. A backend that enforces
    ///   `nodeHasChildren` purely via FK constraint will refuse the
    ///   parent removal until the descendants are gone; this is
    ///   acceptable because the handler destroys descendants first.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this account supports the given JMAP object type.
    ///
    /// Backends that support all types unconditionally can always return `true`.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Returns the union of all ancestor nodes (immediate parent, grandparent,
    /// ..., up to the root) for every input id in `ids`.
    ///
    /// # Contract
    ///
    /// - **Union semantics.** The result is a single flat `Vec<FileNode>`
    ///   that is the union of every input id's ancestor chain. The input
    ///   ids themselves are NOT included.
    /// - **Deduplication.** When two input ids share an ancestor (e.g. a
    ///   common parent), that ancestor MUST appear at most once in the
    ///   result. Backends doing `SELECT ... WHERE id IN (parents)` must
    ///   add a DISTINCT/dedup step.
    /// - **Ordering is unspecified.** Callers MUST NOT depend on the order
    ///   of results. The reference [`MemoryBackend`] returns BFS order
    ///   (all immediate parents, then all grandparents, ...) but that is
    ///   not a contract; production backends may use any order. If a
    ///   caller needs the per-id chain (the path from a specific input id
    ///   up to root), it must reconstruct that walk itself using each
    ///   FileNode's `parentId` field.
    /// - **Empty input.** An empty `ids` slice returns `Ok(vec![])`.
    /// - **Unknown ids.** Ids in the input that do not exist in the account
    ///   are silently skipped (no error, no entry in the result). Detection
    ///   of unknown ids belongs to `get_objects` if needed.
    ///
    /// # Use cases
    ///
    /// - Cycle detection: if a proposed new `parentId` is in this set, the
    ///   move would create a cycle.
    /// - `fetchParents` expansion in `FileNode/get`: the result is dumped
    ///   into the flat `list` alongside the requested ids; ordering does
    ///   not matter to RFC 8620 §5.1.
    ///
    /// [`MemoryBackend`]: crate::memory::MemoryBackend
    fn get_ancestors(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ids: &[jmap_types::Id],
    ) -> impl std::future::Future<Output = Result<Vec<FileNode>, Self::Error>> + Send;

    /// Returns all IDs that are descendants of the given node (children,
    /// grandchildren, etc.).
    ///
    /// Used for: (1) cycle detection — if proposed new `parentId` is in the
    /// descendant set, the move would create a cycle; (2) `nodeHasChildren`
    /// guard — if result is non-empty, the node has children.
    fn get_descendant_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_types::Id>, Self::Error>> + Send;

    /// Returns whether a blob exists in the given account.
    ///
    /// Used by `FileNode/set` to validate `blobId` fields before creating or
    /// updating a file node with type `"file"`.
    ///
    /// # Three-way result
    ///
    /// The return type is `Result<bool, Self::Error>` to distinguish three
    /// states that callers actually need to tell apart:
    ///
    /// - `Ok(true)` — the blob is definitely present and reachable.
    /// - `Ok(false)` — the blob is definitely absent. The handler maps this
    ///   to `invalidProperties` ("blob not found") on a create.
    /// - `Err(_)` — connectivity/transient failure. The handler maps this
    ///   to `serverFail` so the client knows to retry. Returning `Ok(false)`
    ///   for a transient backend failure is a bug: the client receives a
    ///   deterministic-looking error and will not retry.
    ///
    /// Mirrors the canonical `MailBackend::blob_exists` contract.
    fn blob_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Returns the id of any sibling node that already has the given name, or
    /// `None` if the name is unique within that parent.
    ///
    /// `parent_id` is `None` for the root level. `folding` controls the
    /// comparison (many file systems treat names case-insensitively).
    ///
    /// Used by `FileNode/set` to enforce the `alreadyExists` constraint.
    ///
    /// # Case-folding policy
    ///
    /// **The case-folding algorithm used for `case_insensitive = true` is
    /// implementation-defined and the workspace does not standardise it.**
    /// Implementations vary widely:
    ///
    /// - ASCII fold only (a..z ↔ A..Z).
    /// - Unicode simple case fold (e.g. `str::to_lowercase` in Rust).
    /// - Unicode full case fold ("ß" → "ss"; Unicode Standard Annex §3.13).
    /// - Turkish/Azerbaijani locale folding ("I" ↔ "ı", "İ" ↔ "i").
    /// - HFS+ decomposition + fold (macOS native).
    /// - ICU locale-aware collation.
    /// - Filesystem-native (NTFS upcase table, APFS normalisation-insensitive).
    ///
    /// Each choice can disagree with another on the same input pair. A backend
    /// that uses Unicode simple lowercase will not collide on "I" / "ı"; an
    /// NTFS-backed CIFS share on a Turkish-locale Windows host will. JMAP
    /// FileNode (`draft-ietf-jmap-filenode-13` §3.2.3 `compareCaseInsensitively`)
    /// is silent on the algorithm and so is the workspace.
    ///
    /// This is by design: the workspace ships a kit, not a server. The choice
    /// of folding algorithm is properly the consumer's, scoped to the storage
    /// backend they wire up. The reference `memory::MemoryBackend` (gated
    /// behind the `memory` feature) uses `str::to_lowercase` (Unicode simple
    /// lowercase, locale-independent), which is suitable for tests and demos
    /// but is NOT a recommendation for production.
    ///
    /// Implementers SHOULD document the algorithm their backend uses in
    /// public-facing docs, and SHOULD NOT mix folding algorithms within a
    /// single account namespace.
    fn find_sibling_by_name(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        parent_id: Option<&jmap_types::Id>,
        name: &str,
        folding: CaseFolding,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;

    /// Return all FileNode IDs in the subtree rooted at any node in `root_ids`,
    /// up to `max_depth` levels deep.
    ///
    /// # `max_depth` semantics (draft-ietf-jmap-filenode-13 §3.2.5)
    ///
    /// `max_depth` is the **number of levels of subdirectories to recurse
    /// into**, matching the spec's `depth` argument:
    ///
    /// - `0`: do not recurse — returns an empty `Vec`. (Spec: "If absent,
    ///   null, or zero, do not recurse.")
    /// - `1`: include direct children of every `root_id` (one level of
    ///   descent).
    /// - `N`: include up to `N` levels of descendants.
    /// - `u64::MAX`: practical "full subtree". The default impl terminates
    ///   when `frontier` becomes empty (i.e. the deepest reachable level
    ///   has been visited), well before the counter could wrap. A
    ///   pathological tree deep enough to exhaust `u64::MAX` levels is
    ///   not reachable in practice.
    ///
    /// The `root_ids` themselves are NOT included in the result.
    ///
    /// # Default impl
    ///
    /// The default implementation calls `query_objects` with a `parentId` filter
    /// once per level — O(max_depth) backend calls.  Backends with a nested-sets
    /// model, closure table, or recursive CTE SHOULD override this to a single query.
    ///
    /// Returned IDs are deduplicated; ordering is unspecified.
    ///
    /// Errors from the per-level `query_objects` calls are propagated. The
    /// default impl does NOT silently truncate the subtree on a transient
    /// backend error — workspace policy treats silent-drop in a query
    /// result as a server-side correctness bug (see workspace AGENTS.md,
    /// Filter algebra exclusion §1). Callers that want best-effort
    /// behaviour must override this method and document the partiality
    /// in the consumer's response shape.
    fn query_subtree(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        root_ids: &[jmap_types::Id],
        max_depth: u64,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_types::Id>, Self::Error>> + Send {
        // Default: loop using query_objects with parentId filter per level.
        // This is correct but O(max_depth) in backend calls.
        let account_id = account_id.clone();
        let root_ids = root_ids.to_vec();
        async move {
            let mut all_ids: Vec<jmap_types::Id> = Vec::new();
            let mut seen: std::collections::HashSet<jmap_types::Id> =
                root_ids.iter().cloned().collect();
            let mut frontier: Vec<jmap_types::Id> = root_ids;
            let mut depth_remaining = max_depth;

            while !frontier.is_empty() && depth_remaining > 0 {
                depth_remaining = depth_remaining.saturating_sub(1);
                let mut next_frontier: Vec<jmap_types::Id> = Vec::new();
                for parent_id in frontier.drain(..) {
                    // FileNodeFilterCondition is #[non_exhaustive], so build
                    // via Default and field mutation (struct literal is not
                    // permitted outside the defining crate). This is
                    // infallible — no silent-drop window for filter
                    // construction.
                    let mut child_filter = jmap_filenode_types::FileNodeFilterCondition::default();
                    child_filter.parent_id = Some(parent_id.clone());
                    let result = self
                        .query_objects::<FileNode>(
                            caller,
                            &account_id,
                            Some(&child_filter),
                            None,
                            None,
                            0,
                        )
                        .await?;
                    for id in result.ids {
                        if seen.insert(id.clone()) {
                            all_ids.push(id.clone());
                            next_frontier.push(id);
                        }
                    }
                }
                frontier = next_frontier;
            }
            Ok(all_ids)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CaseFolding;

    #[test]
    fn case_folding_from_wire_bool_maps_true_to_insensitive() {
        // Oracle: bd:JMAP-510h.54 — the wire bool true must round-trip
        // into CaseFolding::Insensitive at the handler boundary.
        assert_eq!(CaseFolding::from_wire_bool(true), CaseFolding::Insensitive);
        assert_eq!(CaseFolding::from_wire_bool(false), CaseFolding::Sensitive);
    }
}

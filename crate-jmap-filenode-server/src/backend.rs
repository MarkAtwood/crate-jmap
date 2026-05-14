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
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a FileNode by id.
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

    /// Returns the ancestor chain of the given nodes from immediate parent to root.
    ///
    /// Used for cycle detection (if proposed new parent is in this list, a cycle
    /// would be created) and for `fetchParents` expansion in `FileNode/get`.
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
    fn blob_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send;

    /// Returns the id of any sibling node that already has the given name, or
    /// `None` if the name is unique within that parent.
    ///
    /// `parent_id` is `None` for the root level.  `case_insensitive` controls
    /// the comparison (many file systems treat names case-insensitively).
    ///
    /// Used by `FileNode/set` to enforce the `alreadyExists` constraint.
    fn find_sibling_by_name(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        parent_id: Option<&jmap_types::Id>,
        name: &str,
        case_insensitive: bool,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;

    /// Return all FileNode IDs in the subtree rooted at any node in `root_ids`,
    /// up to `max_depth` levels deep (0 = direct children only, `u64::MAX` = full subtree).
    ///
    /// The default implementation calls `query_objects` with a `parentId` filter
    /// once per level — O(max_depth) backend calls.  Backends with a nested-sets
    /// model, closure table, or recursive CTE SHOULD override this to a single query.
    ///
    /// Returned IDs are deduplicated; ordering is unspecified.
    /// The `root_ids` themselves are NOT included in the result.
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
    ) -> impl std::future::Future<Output = Result<Vec<jmap_types::Id>, Self::Error>> + Send
    where
        Self: Sized,
    {
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

            loop {
                if frontier.is_empty() || depth_remaining == 0 {
                    break;
                }
                depth_remaining = depth_remaining.saturating_sub(1);
                let mut next_frontier: Vec<jmap_types::Id> = Vec::new();
                for parent_id in frontier.drain(..) {
                    // FileNodeFilterCondition is #[non_exhaustive], so build
                    // via Default and field mutation (struct literal is not
                    // permitted outside the defining crate). This is
                    // infallible — no silent-drop window for filter
                    // construction.
                    let mut child_filter =
                        jmap_filenode_types::FileNodeFilterCondition::default();
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

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
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a FileNode by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
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
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_types::Id>, Self::Error>> + Send;

    /// Returns whether a blob exists in the given account.
    ///
    /// Used by `FileNode/set` to validate `blobId` fields before creating or
    /// updating a file node with type `"file"`.
    fn blob_exists(
        &self,
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
        account_id: &jmap_types::Id,
        parent_id: Option<&jmap_types::Id>,
        name: &str,
        case_insensitive: bool,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;
}

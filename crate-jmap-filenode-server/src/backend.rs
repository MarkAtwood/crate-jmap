//! `FileNodeBackend` trait and supporting type re-exports.
//!
//! Consumers implement [`FileNodeBackend`] for their storage system.  The
//! method handlers in [`crate::filenode`] call into the backend through this
//! trait.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait.  Only write operations and the two
//! FileNode-specific structural checks live here.

pub use jmap_filenode_types::backend::FileNodeProperty;
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
/// Only write operations and two FileNode-specific structural checks are
/// here.
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

    /// Returns `true` if setting `node_id`'s `parentId` to `new_parent_id`
    /// would create a cycle in the tree (i.e. `new_parent_id` is `node_id`
    /// itself, or is a descendant of `node_id`).
    ///
    /// Used by `FileNode/set` to enforce the "no cycles" constraint from
    /// draft-ietf-jmap-filenode-13 §3.2.3.
    fn would_create_cycle(
        &self,
        account_id: &jmap_types::Id,
        node_id: &jmap_types::Id,
        new_parent_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send;

    /// Returns `true` if the node has at least one child node.
    ///
    /// Used by `FileNode/set` destroy to enforce `onDestroyRemoveChildren`
    /// semantics per draft-ietf-jmap-filenode-13 §3.2.3.
    fn node_has_children(
        &self,
        account_id: &jmap_types::Id,
        node_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send;
}

//! GraphBackend trait for the JMAP Graph method handlers.
//!
//! Read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the write
//! operations and the capability object the handlers consult.
//!
//! The `pub use` block re-exports the [`jmap_server`] backend types so a
//! downstream impl can `use jmap_graph_server::backend::*`; each re-export
//! pins the upstream type's major-version contract into this crate.

pub use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};

use jmap_graph_types::GraphCapability;

/// Storage backend for the JMAP Graph method handlers
/// (draft-atwood-jmap-graph-00).
///
/// # Rights (draft §Graph, §Security Considerations)
///
/// "The Graph is the unit of access": a user's rights on a Graph cover
/// every GraphNode and GraphEdge in it. [`JmapBackend::get_objects`] for
/// `Graph` MUST return each Graph with `myRights` and `isSubscribed` as
/// they are for this caller ("The owner of the account that holds the
/// Graph implicitly has all rights"; `isSubscribed` is per user, RFC 9670
/// §4). The handlers read `myRights` from that `/get` to enforce the
/// draft's rules, and hide a Graph without `mayRead`, with its nodes and
/// edges, as if it did not exist.
///
/// Per the workspace AGENTS.md "Caller identity (foundation seam)" rule the
/// backend re-verifies every right atomically with the mutation; the
/// handler checks are defense in depth. `createdBy` and `updatedBy` come
/// from [`JmapBackend::principal_id`].
///
/// `query_objects` and `query_changes` MUST omit every Graph the caller
/// may not read and every GraphNode and GraphEdge in one: "A Graph the
/// user may not read SHOULD be indistinguishable from one that does not
/// exist: ... absence from `/query` results".
///
/// # Sharing
///
/// `shareWith` follows RFC 9670, whose Principal and ShareNotification
/// methods live in `jmap-sharing-server`. A backend that supports sharing
/// validates `shareWith` (each Principal's `mayShareWith`) and creates the
/// ShareNotifications of a change; "the `name` of each resulting
/// ShareNotification ... is the Graph's `name`", and its `objectType` is
/// `"Graph"`.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl GraphBackend>` when sharing across tasks.
pub trait GraphBackend: JmapBackend {
    /// The account-level `urn:ietf:params:jmap:graph` capability object
    /// (draft-atwood-jmap-graph-00 §Account-Level Capability Object) the
    /// consumer advertises for `account_id` to this caller.
    ///
    /// `/set` reads `maxAnnotationSize` from it: "A create or update that
    /// would exceed the limit is rejected with a `tooLarge` SetError".
    /// There is no default: return the value the Session object carries,
    /// or the handlers contradict the advertised capability.
    fn graph_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<GraphCapability, Self::Error>> + Send;

    /// Create an object.
    ///
    /// The handler has validated `obj` and filled the server-set
    /// properties; its `id` is a placeholder. The backend MUST, in one
    /// atomic step, assign the `id`, re-verify the rules below, and store
    /// the object. Returns the assigned id and the stored object, which for
    /// a `Graph` carries the caller's `myRights`. Error mapping,
    /// state-counter and change-log rules are those of the canonical
    /// `jmap-mail-server` `create_object`: the state advances only when the
    /// create commits.
    ///
    /// A `Graph`'s `myRights` is a placeholder; the backend decides who may
    /// create a Graph in `account_id` (`forbidden` otherwise).
    ///
    /// A `GraphNode` or `GraphEdge` needs `mayWrite` on its Graph
    /// ("Creating, updating, or destroying a node requires `mayWrite` on its
    /// Graph"): `forbidden` when the caller may read the Graph but lacks
    /// it, and `invalidProperties` naming `graphId` when the Graph does not
    /// exist or the caller may not read it. A `GraphEdge` whose `fromId` or
    /// `toId` does not name a GraphNode of the edge's Graph is
    /// `invalidProperties` naming the failing property ("The server SHOULD
    /// NOT distinguish a node that does not exist from a node in another
    /// Graph"). A backend MAY refuse with `overQuota` past a limit on nodes
    /// per Graph or edges per node (§Denial of Service).
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply an RFC 8620 §5.3 PatchObject to an existing object.
    ///
    /// `Ok(Some(o))` echoes server-changed properties; `Ok(None)` means the
    /// patch applied verbatim. Both MUST advance the state and record an
    /// `updated` change. Unknown `id` is `notFound`; a malformed patch is
    /// `invalidPatch`; a result that no longer decodes is
    /// `invalidProperties`. Apply the whole patch or none of it.
    ///
    /// The backend re-verifies the rights of §Graph/set atomically: on a
    /// `Graph`, `isSubscribed` needs `mayRead` and changes only the
    /// caller's own value, `shareWith` needs `mayAdmin`, every other
    /// property `mayWrite`; on a `GraphNode` or `GraphEdge`, `mayWrite` on
    /// its Graph. A Graph the caller may not read is `notFound`, a missing
    /// right `forbidden`. A `GraphEdge` patch that changes `fromId` or
    /// `toId` re-verifies that each names a GraphNode of the edge's Graph,
    /// as on create.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id. Unknown `id` is `notFound`; the state
    /// advances only when the destroy commits. The backend re-verifies the
    /// right atomically: `mayAdmin` for a `Graph` ("Destroying a Graph
    /// requires `mayAdmin`"), `mayWrite` on its Graph for a `GraphNode` or
    /// `GraphEdge`.
    ///
    /// Destroying a `Graph` MUST "also destroy every GraphNode and
    /// GraphEdge in it. These objects are reported as destroyed by
    /// subsequent `GraphNode/changes` and `GraphEdge/changes` calls."
    /// Destroying a `GraphNode` MUST "also destroy every GraphEdge whose
    /// `fromId` or `toId` is that node", each reported as a destroyed
    /// GraphEdge change. Both cascades happen in the same atomic step.
    /// "Destroying an edge does not affect its nodes."
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Per-backend capability probe: `true` if this backend supports the
    /// JMAP object type `O` for `account_id`. Not called by the handlers;
    /// consumers use it when assembling their Session capabilities.
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;
}

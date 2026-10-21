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
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject,
};

use jmap_graph_types::edit::GraphCursorEvent;
use jmap_graph_types::search::GraphSearchCapability;
use jmap_graph_types::{GraphCapability, GraphEditCapability};

/// The internal bounds of one node's edit log
/// (draft-atwood-jmap-graph-edit-00 §GraphNode Extensions): "The server
/// tracks the head of each node's edit log, and the lowest `seq` it still
/// holds, internally."
///
/// Compaction destroys ops "only from the start of the log", so the
/// destroyed ops are exactly `1..=compacted_through` and the held ones
/// `compacted_through + 1..=head`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditLog {
    /// "The highest sequence number assigned in a node's edit log", 0 when
    /// no op has been sequenced.
    pub head: u64,
    /// The highest `seq` compacted away, 0 when no op has been destroyed.
    pub compacted_through: u64,
}

impl EditLog {
    /// The bounds of a log whose head is `head` and whose ops up to
    /// `compacted_through` have been destroyed.
    pub fn new(head: u64, compacted_through: u64) -> Self {
        Self {
            head,
            compacted_through,
        }
    }
}

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
/// [`JmapBackend::get_changes`] for `Graph`, `GraphNode`, `GraphEdge` and
/// `GraphNodeOp` SHOULD be scoped to the caller the same way: the draft
/// evaluates rights "for every method call that reads or changes the Graph
/// or its contents", and of `GraphNodeOp/get` and `/changes`
/// draft-atwood-jmap-graph-edit-00 says "Both require `mayRead` on the op's
/// Graph". It reports no id of a
/// Graph the caller may not read, nor of a node, edge or op in one; a Graph
/// the caller gains `mayRead` on since `sinceState` is reported, with its
/// contents, as created, and one it loses `mayRead` on as destroyed. The
/// `/changes` handlers pass the backend's result through unfiltered, since
/// only the backend knows what the caller could read at `sinceState`.
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
/// # Live editing (draft-atwood-jmap-graph-edit-00)
///
/// A backend that returns `Some` from [`Self::graph_edit_capability`]
/// stores GraphNodeOp objects through the generic methods:
///
/// - [`Self::create_object`] of a `GraphNodeOp` sequences it. In one
///   atomic step the backend re-verifies `mayWrite` on the node's Graph
///   (`forbidden`), that `nodeId` names a node the caller may read whose
///   `editFormat` is not `null` (`invalidProperties` naming `nodeId`) and
///   that `baseSeq` is at most the head (`invalidProperties` naming
///   `baseSeq`), then assigns the `id` and "the next `seq` of the node",
///   replacing the handler's placeholder 0. Creating an op advances the
///   GraphNodeOp state only, never the GraphNode's. When the WebSocket
///   stream is served, the same step hands a `GraphNodeOpEvent` to the
///   transport, so "a node's op events" leave "in `seq` order"; the
///   transport filters it per connection with [`crate::should_deliver`].
/// - [`Self::update_object`] of a `GraphNode` of an edit-enabled account
///   re-verifies the §GraphNode Extensions and §Snapshots rules atomically
///   (`editFormat` never changes once set, `snapshotSeq` never decreases
///   and is at most the head), "so that two clients writing snapshots
///   concurrently cannot move the node back to an older one".
/// - [`Self::destroy_object`] of a node destroys its ops, and of a Graph
///   the ops of all its nodes: "When a node is destroyed, its ops are
///   destroyed with it; when a Graph is destroyed, so are the ops of all
///   its nodes." Each is a destroyed GraphNodeOp change.
/// - Compaction is the backend's policy (§Compaction and History): it
///   "MAY destroy ops whose `seq` is less than or equal to the node's
///   `snapshotSeq`, but only from the start of the log", never one younger
///   than `opRetention` seconds, each reported by `GraphNodeOp/changes`.
///   [`Self::edit_log`] reports how far it has gone.
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

    /// The account-level `urn:ietf:params:jmap:graph:edit` capability
    /// object (draft-atwood-jmap-graph-edit-00 §Account-Level Capability
    /// Object) the consumer advertises for `account_id`, or `None` when it
    /// does not advertise the capability.
    ///
    /// With `Some`, `GraphNode/set` applies the §GraphNode Extensions rules
    /// to `editFormat` and `snapshotSeq`, and the GraphNodeOp and
    /// `GraphNode/cursor` handlers read `maxOpSize` and `maxCursorSize`
    /// from it. With `None` those handlers answer
    /// `accountNotSupportedByMethod` and `GraphNode/set` treats both
    /// properties as it did before this extension.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(None)`: no live editing.
    fn graph_edit_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<GraphEditCapability>, Self::Error>> + Send
    {
        let _ = (caller, account_id);
        async { Ok(None) }
    }

    /// The account-level `urn:ietf:params:jmap:graph:search` capability
    /// object (draft-atwood-jmap-graph-search-00 §Account-Level Capability
    /// Object) the consumer advertises for `account_id`, or `None` when it
    /// does not advertise the capability.
    ///
    /// With `None`, a GraphNode or GraphEdge `/query` or `/queryChanges`
    /// filter that sets `where`, `language` or `expression` is
    /// `unsupportedFilter`. With `Some`, the handlers check those properties
    /// against it ([`crate::search::check_search_filter`]), answer
    /// `/queryChanges` with `cannotCalculateChanges` ("Servers MAY return
    /// `cannotCalculateChanges` for any `/queryChanges` call whose filter
    /// uses a property defined in this document") and report
    /// `canCalculateChanges: false` from `/query`. `query_objects` then
    /// evaluates them over "exactly those [nodes and edges] the user could
    /// fetch with `/get`" of the filter's Graph: `where` with a
    /// [`crate::search::GraphView`] of them, and `expression` in each
    /// advertised language the backend implements, answering
    /// `invalidArguments` when "`expression` is not a valid query in its
    /// language".
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(None)`: no search.
    fn graph_search_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<GraphSearchCapability>, Self::Error>> + Send
    {
        let _ = (caller, account_id);
        async { Ok(None) }
    }

    /// The head of node `node_id`'s edit log and how far it has been
    /// compacted ([`EditLog`]). A node with no op, or one that does not
    /// exist, has `EditLog::default()`.
    ///
    /// `GraphNode/set` checks `snapshotSeq` against the head ("a value
    /// greater than the head of the edit log" is `invalidProperties`),
    /// `GraphNodeOp/set` checks `baseSeq` against it, and
    /// `GraphNodeOp/query` answers `opsCompacted` when `compacted_through`
    /// exceeds `afterSeq`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(EditLog::default())`: an empty, uncompacted log.
    fn edit_log(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        node_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<EditLog, Self::Error>> + Send {
        let _ = (caller, account_id, node_id);
        async { Ok(EditLog::default()) }
    }

    /// The id of `account_id`'s projected Graph
    /// (draft-atwood-jmap-graph-projection-00 §Account-Level Capability
    /// Object: "An account has at most one projected Graph"), or `None`.
    ///
    /// "The server rejects a `GraphNode/set` that sets `editFormat` on such
    /// a node with a `forbidden` SetError" (draft-atwood-jmap-graph-edit-00
    /// §GraphNode Extensions).
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(None)`: no projection.
    fn projected_graph_id(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send {
        let _ = (caller, account_id);
        async { Ok(None) }
    }

    /// Hand a `GraphCursorEvent` from an accepted `GraphNode/cursor` call to
    /// the WebSocket transport (draft-atwood-jmap-graph-edit-00
    /// §GraphCursorEvent), which sends it to the connections for which
    /// [`crate::should_deliver`] holds.
    ///
    /// "Servers SHOULD rate-limit the method per user per node, and MAY
    /// discard calls above the limit while still returning success": a
    /// backend applies its limit here and drops the event silently. The
    /// hook cannot fail the call.
    ///
    /// # Default implementation
    ///
    /// Does nothing: no WebSocket transport.
    fn publish_cursor(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        event: &GraphCursorEvent,
    ) -> impl std::future::Future<Output = ()> + Send {
        let _ = (caller, account_id, event);
        async {}
    }

    /// `true` if the recipient `caller` has blocked Principal `sender_id`.
    ///
    /// "When JMAP Chat is also available, a server SHOULD NOT deliver a
    /// cursor event to a user who has blocked the sender"
    /// (draft-atwood-jmap-graph-edit-00 §GraphCursorEvent).
    /// [`crate::should_deliver`] consults it for cursor events only: "Ops
    /// are shared document state ... so they are always delivered." A
    /// backend without Chat returns `false`.
    ///
    /// # Default implementation
    ///
    /// Returns `Ok(false)`: nobody is blocked (fail-open, bd:JMAP-bghk).
    fn is_sender_blocked(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        sender_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let _ = (caller, account_id, sender_id);
        async { Ok(false) }
    }

    /// Per-backend capability probe: `true` if this backend supports the
    /// JMAP object type `O` for `account_id`. Not called by the handlers;
    /// consumers use it when assembling their Session capabilities.
    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool;
}

//! Mail and Contacts binding for the JMAP Graph method handlers: the rules
//! of draft-atwood-jmap-graph-projection-00 for presenting an account's
//! RFC 8621 and RFC 9610 objects as one projected Graph under
//! `urn:ietf:params:jmap:graph:projection`.
//!
//! `jmap-graph-server` knows nothing of Mail or Contacts. A deployment that
//! serves a projected Graph implements `jmap_graph_server::GraphBackend`
//! over its own store of projected nodes and edges and calls these
//! functions from it and from its Mail and Contacts write paths:
//!
//! | Where | Call |
//! |---|---|
//! | A source object is created or changed | [`Projectable::references`], [`object_uri`], [`projected_node`], [`projected_edge`] |
//! | `GraphBackend::get_objects::<Graph>` for the projected Graph | [`projected_graph_rights`] |
//! | `GraphBackend::query_objects::<GraphNode>` | [`node_matches`] |
//! | Any GraphNode or GraphEdge method, per caller | [`visible_ids`] |
//! | `GraphBackend::projected_graph_id` | the id it advertises as `graphId` |
//!
//! The deployment owns node and edge ids and their `created` times: "A
//! node's id SHOULD remain the same for as long as its source object
//! exists" (§Nodes), and "When a source object is destroyed, its node is
//! destroyed", which the `GraphBackend::destroy_object` cascade turns into
//! destroyed edges. A changed reference is a destroyed edge and a new one:
//! "The server does not update an edge it derives" (§Edges).
//!
//! # Projected types
//!
//! [`Projectable`] is implemented for the seven data types whose object
//! URI type names the draft registers (§JMAP Object URI Type
//! Registrations): Mailbox, Thread, Email, Identity, EmailSubmission,
//! AddressBook and ContactCard. It is not implemented for Graph, GraphNode
//! or GraphEdge: "The server MUST NOT project the Graph, GraphNode, or
//! GraphEdge data types."

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use jmap_contacts_types::{AddressBook, ContactCard};
use jmap_graph_types::query::GraphNodeFilterCondition;
use jmap_graph_types::{GraphEdge, GraphNode, GraphRights};
use jmap_mail_types::{Email, EmailSubmission, Identity, Mailbox, Thread};
use jmap_server::{GetObject, JmapBackend};
use jmap_types::{Id, JmapUri, JmapUriError, UTCDate};
use serde_json::json;

/// Capability URI for `urn:ietf:params:jmap:graph:projection`.
pub use jmap_graph_types::JMAP_GRAPH_PROJECTION_URI;

/// The GraphNode extension property naming a node's source object
/// (draft-atwood-jmap-graph-projection-00 §objectUri).
pub const OBJECT_URI_PROPERTY: &str = "objectUri";

/// A reference from a source object to another object of the same
/// account, from which one GraphEdge is derived
/// (draft-atwood-jmap-graph-projection-00 §Edges).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The edge `type`: "the data type name of the referencing object ...
    /// followed by `/`, followed by the property name", e.g.
    /// `Email/mailboxIds`.
    pub edge_type: String,
    /// The JMAP Object URI type name of the referenced object, e.g.
    /// `mailbox`.
    pub target_uri_type: &'static str,
    /// The referenced object's id.
    pub target_id: Id,
}

impl Reference {
    fn new<O: Projectable>(property: &str, target_uri_type: &'static str, target_id: &Id) -> Self {
        Self {
            edge_type: format!("{}/{property}", O::TYPE_NAME),
            target_uri_type,
            target_id: target_id.clone(),
        }
    }

    /// The object URI of the referenced object in `account_id`, by which
    /// the deployment finds the edge's `toId` node.
    ///
    /// "An edge is produced only when the referenced object is itself
    /// projected": when no node has this `objectUri`, the reference
    /// produces no edge.
    ///
    /// # Errors
    ///
    /// [`JmapUriError`] when `account_id` or the target id is not a valid
    /// RFC 8620 `Id`.
    pub fn target_uri(&self, account_id: &Id) -> Result<JmapUri, JmapUriError> {
        JmapUri::new(
            self.target_uri_type,
            account_id.clone(),
            self.target_id.clone(),
        )
    }
}

/// A data type a projected Graph may contain
/// (draft-atwood-jmap-graph-projection-00 §Which Objects Are Projected):
/// one with "a type name registered in the 'JMAP Object URI Types'
/// registry".
pub trait Projectable: GetObject {
    /// The registered JMAP Object URI type name, e.g. `email`.
    const URI_TYPE: &'static str;

    /// The source object's `id`, or `None` when a partial object lacks it.
    fn source_id(&self) -> Option<&Id>;

    /// The references the object's top-level properties hold, one per
    /// combination of property and referenced id (§Edges): an `Id` or
    /// non-null `Id|null` gives one, an `Id[]` one per element, an `Id[T]`
    /// one per key.
    ///
    /// Only properties whose definition names an object of a projected
    /// data type count: blob ids (`Email.blobId`,
    /// `EmailSubmission.dsnBlobIds`, `mdnBlobIds`) "produce no edge", and
    /// "Properties nested inside other values ... do not produce edges".
    fn references(&self) -> Vec<Reference>;
}

impl Projectable for Mailbox {
    const URI_TYPE: &'static str = "mailbox";

    fn source_id(&self) -> Option<&Id> {
        Some(&self.id)
    }

    fn references(&self) -> Vec<Reference> {
        self.parent_id
            .iter()
            .map(|id| Reference::new::<Self>("parentId", Mailbox::URI_TYPE, id))
            .collect()
    }
}

impl Projectable for Thread {
    const URI_TYPE: &'static str = "thread";

    fn source_id(&self) -> Option<&Id> {
        Some(&self.id)
    }

    fn references(&self) -> Vec<Reference> {
        let mut seen = BTreeSet::new();
        self.email_ids
            .iter()
            .filter(|id| seen.insert(*id))
            .map(|id| Reference::new::<Self>("emailIds", Email::URI_TYPE, id))
            .collect()
    }
}

impl Projectable for Email {
    const URI_TYPE: &'static str = "email";

    fn source_id(&self) -> Option<&Id> {
        Some(&self.id)
    }

    fn references(&self) -> Vec<Reference> {
        std::iter::once(Reference::new::<Self>(
            "threadId",
            Thread::URI_TYPE,
            &self.thread_id,
        ))
        .chain(
            self.mailbox_ids
                .keys()
                .map(|id| Reference::new::<Self>("mailboxIds", Mailbox::URI_TYPE, id)),
        )
        .collect()
    }
}

impl Projectable for Identity {
    const URI_TYPE: &'static str = "identity";

    fn source_id(&self) -> Option<&Id> {
        Some(&self.id)
    }

    fn references(&self) -> Vec<Reference> {
        Vec::new()
    }
}

impl Projectable for EmailSubmission {
    const URI_TYPE: &'static str = "emailsubmission";

    fn source_id(&self) -> Option<&Id> {
        Some(&self.id)
    }

    fn references(&self) -> Vec<Reference> {
        vec![
            Reference::new::<Self>("identityId", Identity::URI_TYPE, &self.identity_id),
            Reference::new::<Self>("emailId", Email::URI_TYPE, &self.email_id),
            Reference::new::<Self>("threadId", Thread::URI_TYPE, &self.thread_id),
        ]
    }
}

impl Projectable for AddressBook {
    const URI_TYPE: &'static str = "addressbook";

    fn source_id(&self) -> Option<&Id> {
        Some(&self.id)
    }

    /// ponytail: `shareWith` (`Id[AddressBookRights]|null`) names
    /// Principals, which this crate does not project, so it gives no
    /// reference. Upgrade path: implement `Projectable` for Principal
    /// (type name `principal`, registered by
    /// draft-atwood-jmap-chat-calendars-00) and add the `shareWith` keys.
    fn references(&self) -> Vec<Reference> {
        Vec::new()
    }
}

impl Projectable for ContactCard {
    const URI_TYPE: &'static str = "contactcard";

    fn source_id(&self) -> Option<&Id> {
        self.id.as_ref()
    }

    fn references(&self) -> Vec<Reference> {
        self.address_book_ids
            .iter()
            .flat_map(|ids| ids.keys())
            .map(|id| Reference::new::<Self>("addressBookIds", AddressBook::URI_TYPE, id))
            .collect()
    }
}

/// The object URI of the `O` with id `id` in `account_id`
/// (draft-atwood-jmap-graph-projection-00 §objectUri), e.g.
/// `jmap:email:A1:M42`.
///
/// # Errors
///
/// [`JmapUriError`] when either id is not a valid RFC 8620 `Id`.
pub fn object_uri<O: Projectable>(account_id: &Id, id: &Id) -> Result<JmapUri, JmapUriError> {
    JmapUri::new(O::URI_TYPE, account_id.clone(), id.clone())
}

/// The GraphNode `id` in projected Graph `graph_id` for the source object
/// `object_uri`, added to the projection at `added`
/// (draft-atwood-jmap-graph-projection-00 §Nodes).
///
/// `annotations` is `{}`; `blobId`, `mediaType` and `uri` are `null`,
/// "even when the source object refers to a blob"; `created` and `updated`
/// are "the time the node was added to the projection"; `createdBy` and
/// `updatedBy` are `null`. `objectUri` is in [`GraphNode::extra`].
pub fn projected_node(id: &Id, graph_id: &Id, object_uri: &JmapUri, added: &UTCDate) -> GraphNode {
    serde_json::from_value(json!({
        "id": id,
        "graphId": graph_id,
        OBJECT_URI_PROPERTY: object_uri.to_string(),
        "annotations": {},
        "blobId": null,
        "mediaType": null,
        "uri": null,
        "created": added,
        "updated": added,
        "createdBy": null,
        "updatedBy": null,
    }))
    .expect("a GraphNode built from typed ids and dates always deserializes")
}

/// The GraphEdge `id` in projected Graph `graph_id` from node `from_id` to
/// node `to_id` for `reference`, added to the projection at `added`
/// (draft-atwood-jmap-graph-projection-00 §Edges).
///
/// `type` is [`Reference::edge_type`]; `annotations` is `{}`; `created`
/// and `updated` are "the time the edge was added to the projection";
/// `createdBy` and `updatedBy` are `null`.
pub fn projected_edge(
    id: &Id,
    graph_id: &Id,
    from_id: &Id,
    to_id: &Id,
    reference: &Reference,
    added: &UTCDate,
) -> GraphEdge {
    serde_json::from_value(json!({
        "id": id,
        "graphId": graph_id,
        "fromId": from_id,
        "toId": to_id,
        "type": reference.edge_type,
        "annotations": {},
        "created": added,
        "updated": added,
        "createdBy": null,
        "updatedBy": null,
    }))
    .expect("a GraphEdge built from typed ids and dates always deserializes")
}

/// The node's `objectUri`, or `None`: "Always `null` for nodes that are not
/// in a projected Graph" (draft-atwood-jmap-graph-projection-00
/// §objectUri).
pub fn node_object_uri(node: &GraphNode) -> Option<&str> {
    node.extra
        .get(OBJECT_URI_PROPERTY)
        .and_then(serde_json::Value::as_str)
}

/// `true` if `node` matches `cond`: it is in the condition's Graph and,
/// when `objectUri` is given, "The node's `objectUri` is one of these
/// values" (draft-atwood-jmap-graph-projection-00 §GraphNode/query
/// Filter).
///
/// Use it in place of `jmap_graph_server::query::node_matches`, which
/// serves no projected Graph and so matches no node for an `objectUri`
/// condition. The graph-search properties are evaluated separately, as
/// there.
pub fn node_matches(cond: &GraphNodeFilterCondition, node: &GraphNode) -> bool {
    node.graph_id == cond.graph_id
        && cond.object_uri.as_ref().is_none_or(|uris| {
            node_object_uri(node).is_some_and(|uri| uris.iter().any(|u| u == uri))
        })
}

/// The projected Graph's `myRights` for every user who can see it
/// (draft-atwood-jmap-graph-projection-00 §Writes): `mayRead` true,
/// `mayAdmin` false, and `mayWrite` false because no write is accepted.
///
/// ponytail: the projection accepts no writes ("or accept no writes at
/// all"). With these rights the `jmap-graph-server` handlers reject every
/// GraphNode and GraphEdge `/set` and every change to the Graph's `name`,
/// `annotations` and `shareWith`, and its destroy, with `forbidden` ("a
/// write the server does not accept is rejected with a `forbidden`
/// SetError"), while "Each user may change their own `isSubscribed`". The
/// backend's own rights re-check sees the same `myRights`. Upgrade path: a
/// deployment that maps writes onto native methods returns `mayWrite: true`
/// and applies them with native rights checks (§Writes to Source Objects).
pub fn projected_graph_rights() -> GraphRights {
    let mut rights = GraphRights::default();
    rights.may_read = true;
    rights
}

/// The subset of `ids` of type `O` the caller can see in the projection:
/// "A node is visible to a user if and only if the user could fetch its
/// source object with that data type's native `/get` method"
/// (draft-atwood-jmap-graph-projection-00 §Visibility).
///
/// Calls `backend`'s native `get_objects` with the caller's context, so
/// visibility follows current native rights, not a cached snapshot
/// (§No Widening of Access). "An edge is visible to a user if and only if
/// both of its nodes are visible to that user"; an invisible node or edge
/// is `notFound` on `/get` and absent from `/query`.
///
/// # Errors
///
/// The backend's error from `get_objects`.
pub async fn visible_ids<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    ids: &[Id],
) -> Result<Vec<Id>, B::Error>
where
    O: Projectable + Send + Sync,
    B: JmapBackend,
{
    let (found, _not_found) = backend
        .get_objects::<O>(caller, account_id, Some(ids), None)
        .await?;
    Ok(found
        .iter()
        .filter_map(Projectable::source_id)
        .cloned()
        .collect())
}

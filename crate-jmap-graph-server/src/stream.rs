//! Graph edit stream subscription state (draft-atwood-jmap-graph-edit-00
//! §WebSocket Stream).
//!
//! A WebSocket transport keeps one [`GraphEditSubscription`] per
//! connection, starting from [`GraphEditSubscription::default`] (inactive).
//! Each `GraphEditStreamEnable` frame goes through
//! [`GraphEditSubscription::replace_with`], which validates the frame and
//! replaces the whole subscription. A `GraphEditStreamDisable` frame resets
//! it to the default: it "succeeds silently whether or not a subscription
//! is active" (§GraphEditStreamDisable).
//!
//! # Rights are checked at delivery
//!
//! `nodeIds` is kept as sent. "Ids in `nodeIds` that do not identify a node
//! the user may read produce no events but remain in the subscription;
//! rights are evaluated at each delivery", so whether a node produces
//! events is decided per event by [`should_deliver`], not here.
//!
//! # Delivery
//!
//! The backend hands a `GraphNodeOpEvent` to the transport when it
//! sequences an op ([`GraphBackend::create_object`]) and a
//! `GraphCursorEvent` from [`GraphBackend::publish_cursor`]. Before
//! sending either to a connection, the transport asks [`should_deliver`].
//! Ops sent upstream are ordinary `GraphNodeOp/set` requests: "No other
//! upstream message is defined for ops."
//!
//! The §GraphCursorEvent rate limits are SHOULD rules for the transport or
//! deployment; nothing here counts or times events.

use jmap_graph_types::{Graph, GraphNode};
use jmap_graph_types::{GraphEditEvent, GraphEditStreamEnable, DATA_TYPE_CURSORS, DATA_TYPE_OPS};
use jmap_types::{Id, JmapError};

use crate::{GraphBackend, JmapBackend};

/// Capability URI of draft-atwood-jmap-chat-00, which gates the
/// blocked-sender rule.
const JMAP_CHAT_URI: &str = "urn:ietf:params:jmap:chat";

/// The active filters of an enabled subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEditStreamFilter {
    node_ids: Vec<Id>,
    ops: bool,
    cursors: bool,
}

impl GraphEditStreamFilter {
    /// The subscribed node ids, in the order sent.
    pub fn node_ids(&self) -> &[Id] {
        &self.node_ids
    }

    /// Whether `"ops"` was requested.
    pub fn ops(&self) -> bool {
        self.ops
    }

    /// Whether `"cursors"` was requested.
    pub fn cursors(&self) -> bool {
        self.cursors
    }
}

/// The Graph edit subscription of one WebSocket connection.
///
/// The default value is inactive, which is the state before the first
/// `GraphEditStreamEnable` and after a `GraphEditStreamDisable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphEditSubscription {
    active: Option<GraphEditStreamFilter>,
}

impl GraphEditSubscription {
    /// Replaces this subscription with the one in `enable`: "A new
    /// `GraphEditStreamEnable` replaces the previous subscription in its
    /// entirety." "Unrecognized values alongside recognized ones are
    /// ignored."
    ///
    /// # Errors
    ///
    /// "If `dataTypes` is empty or contains no recognized value, the server
    /// responds with a `RequestError` whose `type` is
    /// `urn:ietf:params:jmap:error:invalidArguments` and leaves the current
    /// subscription unchanged": an `invalidArguments` [`JmapError`], with
    /// `self` unchanged. The transport sends it as an RFC 8887 §4.3.4
    /// `RequestError` frame with the full URN as `type` and
    /// [`GraphEditStreamEnable::id`] as `requestId` when the client sent
    /// one; the returned [`JmapError`] carries the short name.
    pub fn replace_with(&mut self, enable: &GraphEditStreamEnable) -> Result<(), JmapError> {
        let has = |t: &str| enable.data_types.iter().any(|d| d.as_str() == t);
        let (ops, cursors) = (has(DATA_TYPE_OPS), has(DATA_TYPE_CURSORS));
        if !ops && !cursors {
            return Err(JmapError::invalid_arguments(
                "dataTypes contains no recognized value",
            ));
        }
        self.active = Some(GraphEditStreamFilter {
            node_ids: enable.node_ids.clone(),
            ops,
            cursors,
        });
        Ok(())
    }

    /// The active filters, or `None` when no subscription is enabled.
    pub fn filter(&self) -> Option<&GraphEditStreamFilter> {
        self.active.as_ref()
    }
}

/// Whether `event` may be delivered on a connection whose subscription is
/// `subscription`.
///
/// `caller` is the connection's authenticated identity, the recipient,
/// and the event's `accountId` the account its node is read from ("An id
/// matches that id in every account the user can access; events carry
/// `accountId` to tell accounts apart"). The transport calls this at
/// delivery time, for each event and connection.
///
/// Delivery needs an active subscription that lists the event's node and
/// requested its category (`"ops"` for `GraphNodeOpEvent`, `"cursors"`
/// for `GraphCursorEvent`); an unrecognized `@type` is never delivered.
/// The recipient must have `mayRead` on the node's Graph "at the time of
/// delivery". A `GraphCursorEvent` is further not delivered to the
/// sender's own connections (its `senderId` is the recipient's
/// [`JmapBackend::principal_id`]) or to a recipient who blocked the sender
/// ([`GraphBackend::is_sender_blocked`]). Op events of a blocked user are
/// delivered: "Ops are shared document state".
///
/// `recipient_account_id` is the recipient's own account. The block rule
/// applies only "When {{JMAP-CHAT}} is also available"
/// (draft-atwood-jmap-graph-edit-00 at 29e9a41, L370), that is when it
/// advertises `urn:ietf:params:jmap:chat` ([`GraphBackend::has_capability`]),
/// as in Scene and VTC. The event's `accountId` is the node's hosting
/// account, which may be shared, so it is not the one checked.
///
/// # Errors
///
/// Returns the backend's error if reading the node, its Graph or the
/// block status fails.
pub async fn should_deliver<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    recipient_account_id: &Id,
    subscription: &GraphEditSubscription,
    event: &GraphEditEvent,
) -> Result<bool, B::Error> {
    let Some(filter) = subscription.filter() else {
        return Ok(false);
    };
    let (account_id, node_id, graph_id) = match event {
        GraphEditEvent::Op(e) if filter.ops => {
            (&e.account_id, &e.op.node_id, Some(e.op.graph_id.clone()))
        }
        GraphEditEvent::Cursor(e) if filter.cursors => (&e.account_id, &e.node_id, None),
        _ => return Ok(false),
    };
    if !filter.node_ids.contains(node_id) {
        return Ok(false);
    }
    let graph_id = match graph_id {
        Some(g) => g,
        None => {
            let (nodes, _) = backend
                .get_objects::<GraphNode>(
                    caller,
                    account_id,
                    Some(std::slice::from_ref(node_id)),
                    None,
                )
                .await?;
            match nodes.into_iter().next() {
                Some(n) => n.graph_id,
                None => return Ok(false),
            }
        }
    };
    let (graphs, _) = backend
        .get_objects::<Graph>(
            caller,
            account_id,
            Some(std::slice::from_ref(&graph_id)),
            None,
        )
        .await?;
    if !graphs.first().is_some_and(|g| g.my_rights.may_read) {
        return Ok(false);
    }
    let GraphEditEvent::Cursor(e) = event else {
        return Ok(true);
    };
    let Some(sender) = &e.sender_id else {
        return Ok(true);
    };
    if <B as JmapBackend>::principal_id(caller) == Some(sender) {
        return Ok(false);
    }
    Ok(
        !backend.has_capability(caller, recipient_account_id, JMAP_CHAT_URI)
            || !backend.is_sender_blocked(caller, sender).await?,
    )
}

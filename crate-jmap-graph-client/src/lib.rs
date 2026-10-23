//! JMAP Graph client support (draft-atwood-jmap-graph-00).
//!
//! [`JmapGraphExt`] binds the `/get`, `/changes`, `/set`, `/query` and
//! `/queryChanges` methods of `Graph`, `GraphNode` and `GraphEdge`
//! (draft-atwood-jmap-graph-00 §Methods) on top of `jmap-base-client`.
//! Wire types come from `jmap-graph-types`, including the
//! draft-atwood-jmap-graph-projection-00 GraphNode `objectUri` filter.
//!
//! draft-atwood-jmap-graph-edit-00 adds an edit log of ops to a GraphNode:
//! [`SessionClient`] binds `GraphNodeOp/get`, `/changes`, the create-only
//! `/set`, `/query` and `/queryChanges`, and `GraphNode/cursor`, each with
//! `urn:ietf:params:jmap:graph:edit` in `using`. [`GraphEditWsExt`] sends
//! `GraphEditStreamEnable` and `GraphEditStreamDisable` on a
//! [`jmap_base_client::WsSession`] and types the `GraphNodeOpEvent` and
//! `GraphCursorEvent` frames it receives.
//!
//! One traversal hop (draft §Traversal) is a `GraphEdge/query` from the
//! current frontier followed by a `GraphEdge/get`:
//!
//! ```rust,no_run
//! use jmap_graph_client::JmapGraphExt;
//! use jmap_graph_types::GraphEdgeFilterCondition;
//! use jmap_types::Id;
//!
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let gc = client.with_graph_session(session);
//! let mut cond = GraphEdgeFilterCondition::new(Id::from("G1"));
//! cond.from_id = Some(vec![Id::from("N1")]);
//! let edges = gc.graph_edge_query(&cond, None, &jmap_base_client::QueryParams::default()).await?;
//! let edges = gc.graph_edge_get(Some(&edges.ids), None).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Re-exported types
//!
//! [`ClientError`] and the shared method response types (`GetResponse`,
//! `SetResponse`, `ChangesResponse`, `QueryResponse` and the rest) are
//! re-exports from `jmap-base-client` and `jmap-types`, not types defined
//! here. The `jmap-*` workspace crates are versioned and released together:
//! depend on the same version of this crate, `jmap-base-client` and
//! `jmap-types`.

#![forbid(unsafe_code)]

mod methods;
mod ws;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse,
    SessionClient, SetError, SetResponse,
};
pub use ws::{GraphEditWsExt, GraphEditWsFrame, parse_graph_edit_ws_frame};

/// Extension trait adding JMAP Graph methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_graph_client::JmapGraphExt;`
/// This trait is **sealed**: the crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`], which keeps adding methods to the
/// trait a non-breaking change.
pub trait JmapGraphExt: sealed::Sealed {
    /// Bind this client to a JMAP session for use with Graph methods.
    ///
    /// The returned [`SessionClient`] captures the session at construction
    /// time. After re-fetching the session, construct a new `SessionClient`.
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts any [`jmap_base_client::Session`] and never
    /// fails. Each method call on the returned [`SessionClient`] returns
    /// [`ClientError::InvalidSession`] when the session has no primary
    /// account for `urn:ietf:params:jmap:graph`. Pre-check with
    /// [`session.primary_account_id("urn:ietf:params:jmap:graph")`](jmap_base_client::Session::primary_account_id)
    /// to guard at the binding site.
    fn with_graph_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapGraphExt for jmap_base_client::JmapClient {
    fn with_graph_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
            account_id: None,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapGraphExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}

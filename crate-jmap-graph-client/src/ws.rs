//! Graph edit op and cursor events over the JMAP WebSocket
//! (draft-atwood-jmap-graph-edit-00 §WebSocket Stream; line numbers cite
//! jmap-chat-spec 6bcd9d8).
//!
//! [`parse_graph_edit_ws_frame`] lifts the `GraphNodeOpEvent` and
//! `GraphCursorEvent` frames out of a base-client [`WsFrame`];
//! [`GraphEditWsExt`] adds receive and subscription methods to
//! [`WsSession`]. Ops are sent as ordinary JMAP requests containing
//! `GraphNodeOp/set` calls: "No other upstream message is defined for
//! ops" (L289).

use jmap_base_client::{ClientError, WsFrame, WsSession};
use jmap_graph_types::{
    GraphCursorEvent, GraphEditEvent, GraphEditStreamDisable, GraphEditStreamEnable,
    GraphEditStreamMessage, GraphNodeOpEvent,
};
use serde::Deserialize as _;

/// A frame from the JMAP WebSocket, with the Graph edit events typed.
///
/// `#[non_exhaustive]`, so new Graph edit event types stay semver-additive.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum GraphEditWsFrame {
    /// `GraphNodeOpEvent`: an op sequenced on a subscribed node (L313-L328).
    Op(GraphNodeOpEvent),
    /// `GraphCursorEvent`: another user's cursor in a subscribed node
    /// (L349-L370).
    Cursor(GraphCursorEvent),
    /// Any other frame, unchanged: `StateChange`, `Response`,
    /// `RequestError` and other extensions' events. A Graph edit event
    /// whose body does not parse also arrives here, as
    /// [`WsFrame::Unknown`] with its `@type` and raw JSON.
    Other(WsFrame),
}

/// Promote a base-client [`WsFrame`] to a [`GraphEditWsFrame`].
///
/// A [`WsFrame::Unknown`] whose `@type` is `GraphNodeOpEvent` or
/// `GraphCursorEvent` and whose body parses becomes the matching typed
/// variant; every other frame becomes [`GraphEditWsFrame::Other`]. One
/// malformed frame never fails the connection.
pub fn parse_graph_edit_ws_frame(frame: WsFrame) -> GraphEditWsFrame {
    let WsFrame::Unknown { raw, .. } = &frame else {
        return GraphEditWsFrame::Other(frame);
    };
    match GraphEditEvent::deserialize(raw) {
        Ok(GraphEditEvent::Op(e)) => GraphEditWsFrame::Op(e),
        Ok(GraphEditEvent::Cursor(e)) => GraphEditWsFrame::Cursor(e),
        _ => GraphEditWsFrame::Other(frame),
    }
}

/// Graph edit stream methods on [`WsSession`].
///
/// Import this trait to use: `use jmap_graph_client::GraphEditWsExt;`
///
/// This trait is **sealed**: only [`WsSession`] implements it, so adding
/// methods is not a breaking change. The method names carry `graph_edit`
/// so they do not collide with other extensions' WebSocket traits in
/// scope.
pub trait GraphEditWsExt: sealed::Sealed {
    /// Receive the next frame from the server as a [`GraphEditWsFrame`].
    ///
    /// Returns `None` when the server has cleanly closed the connection
    /// and `Some(Err(..))` on a transport failure, as
    /// [`WsSession::next_frame`] does.
    fn next_graph_edit_frame(
        &mut self,
    ) -> impl std::future::Future<Output = Option<Result<GraphEditWsFrame, ClientError>>> + Send;

    /// Send a `GraphEditStreamEnable` frame, starting or replacing the
    /// subscription on this connection (L291-L307).
    ///
    /// "Success is silent; there is no acknowledgment", and the
    /// subscription "takes effect before the server processes any later
    /// message on the same connection" (L307), so send it before loading
    /// or catching up on a node. A rejected frame is answered with a
    /// `RequestError`, which arrives as [`GraphEditWsFrame::Other`].
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_graph_edit_stream_enable(
        &mut self,
        enable: &GraphEditStreamEnable,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;

    /// Send a `GraphEditStreamDisable` frame, ending the subscription
    /// (L309-L311). It "succeeds silently whether or not a subscription is
    /// active".
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_graph_edit_stream_disable(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
}

mod sealed {
    /// Seals [`super::GraphEditWsExt`]; see its rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::WsSession {}
}

impl GraphEditWsExt for WsSession {
    async fn next_graph_edit_frame(&mut self) -> Option<Result<GraphEditWsFrame, ClientError>> {
        self.next_frame()
            .await
            .map(|r| r.map(parse_graph_edit_ws_frame))
    }

    async fn send_graph_edit_stream_enable(
        &mut self,
        enable: &GraphEditStreamEnable,
    ) -> Result<(), ClientError> {
        send_json(self, &GraphEditStreamMessage::Enable(enable.clone())).await
    }

    async fn send_graph_edit_stream_disable(&mut self) -> Result<(), ClientError> {
        send_json(
            self,
            &GraphEditStreamMessage::Disable(GraphEditStreamDisable::default()),
        )
        .await
    }
}

/// Serialize `frame` and send it as one text frame.
async fn send_json(
    ws: &mut WsSession,
    frame: &(impl serde::Serialize + Sync),
) -> Result<(), ClientError> {
    let text = serde_json::to_string(frame).map_err(ClientError::from_serialize)?;
    ws.send_text(text).await
}

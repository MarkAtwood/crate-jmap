//! VTC ephemeral events over the JMAP WebSocket
//! (draft-atwood-jmap-vtc-wss-00).
//!
//! [`parse_vtc_ws_frame`] lifts the VTC event frames out of a base-client
//! [`WsFrame`]; [`VtcWsExt`] adds VTC receive and send methods to
//! [`WsSession`].

use jmap_base_client::{ClientError, WsFrame, WsSession};
use jmap_vtc_types::{
    VTC_EVENT_TYPES, VtcEvent, VtcStreamDisable, VtcStreamEnable, VtcStreamMessage,
};
use serde::Deserialize as _;

/// A frame from the JMAP WebSocket, with the VTC ephemeral events typed.
///
/// `#[non_exhaustive]`, so new frame kinds stay semver-additive.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum VtcWsFrame {
    /// One of the eight VTC event types of §Event Types. Never
    /// [`VtcEvent::Unknown`]: an unrecognized `@type` arrives as
    /// [`VtcWsFrame::Other`].
    Event(VtcEvent),
    /// Any other frame, unchanged.
    ///
    /// VTC events "are interleaved with `StateChange` and `Response`
    /// frames on the same WebSocket connection", so those arrive here for
    /// the caller to handle, as does the `RequestError` that answers a
    /// rejected `VTCStreamEnable`; a client "SHOULD silently ignore" an
    /// unrecognized `@type`. A VTC event whose body does not parse also
    /// arrives here, as [`WsFrame::Unknown`] with its `@type` and raw JSON.
    Other(WsFrame),
}

/// Promote a base-client [`WsFrame`] to a [`VtcWsFrame`].
///
/// A [`WsFrame::Unknown`] whose `@type` is one of [`VTC_EVENT_TYPES`] and
/// whose body parses becomes [`VtcWsFrame::Event`]; every other frame
/// becomes [`VtcWsFrame::Other`]. One malformed frame never fails the
/// connection.
pub fn parse_vtc_ws_frame(frame: WsFrame) -> VtcWsFrame {
    let WsFrame::Unknown { type_name, raw } = &frame else {
        return VtcWsFrame::Other(frame);
    };
    if !VTC_EVENT_TYPES.contains(&type_name.as_str()) {
        return VtcWsFrame::Other(frame);
    }
    match VtcEvent::deserialize(raw) {
        Ok(event) => VtcWsFrame::Event(event),
        Err(_) => VtcWsFrame::Other(frame),
    }
}

/// VTC ephemeral-event methods on [`WsSession`].
///
/// Import this trait to use: `use jmap_vtc_client::VtcWsExt;`
///
/// This trait is **sealed**: only [`WsSession`] implements it, so adding
/// methods is not a breaking change. The method names carry `vtc` so they
/// do not collide with the Chat and Scene WebSocket traits when several
/// are in scope.
pub trait VtcWsExt: sealed::Sealed {
    /// Receive the next frame from the server as a [`VtcWsFrame`].
    ///
    /// Returns `None` when the server has cleanly closed the connection
    /// and `Some(Err(..))` on a transport failure, as
    /// [`WsSession::next_frame`] does.
    fn next_vtc_frame(
        &mut self,
    ) -> impl std::future::Future<Output = Option<Result<VtcWsFrame, ClientError>>> + Send;

    /// Send a `VTCStreamEnable` frame, starting or replacing the VTC event
    /// subscription on this connection.
    ///
    /// "A subsequent `VTCStreamEnable` message replaces the previous
    /// ephemeral subscription in its entirety", so pass the full desired
    /// state. The subscription is bound to the connection and cancelled
    /// when it closes: "A reconnecting client sends a new
    /// `VTCStreamEnable` to re-establish its subscription."
    ///
    /// Success is silent; a rejection arrives later as a `RequestError` in
    /// [`VtcWsFrame::Other`], carrying [`VtcStreamEnable::id`] as its
    /// `requestId` ([`WsFrame::request_id`]).
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_vtc_stream_enable(
        &mut self,
        enable: &VtcStreamEnable,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;

    /// Send a `VTCStreamDisable` frame, cancelling the VTC event delivery
    /// that `VTCStreamEnable` set up on this connection. VTC events
    /// subscribed through `ChatStreamEnable` are unaffected.
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_vtc_stream_disable(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
}

mod sealed {
    /// Seals [`super::VtcWsExt`]; see its rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::WsSession {}
}

impl VtcWsExt for WsSession {
    async fn next_vtc_frame(&mut self) -> Option<Result<VtcWsFrame, ClientError>> {
        self.next_frame().await.map(|r| r.map(parse_vtc_ws_frame))
    }

    async fn send_vtc_stream_enable(
        &mut self,
        enable: &VtcStreamEnable,
    ) -> Result<(), ClientError> {
        send_json(self, &VtcStreamMessage::Enable(enable.clone())).await
    }

    async fn send_vtc_stream_disable(&mut self) -> Result<(), ClientError> {
        send_json(
            self,
            &VtcStreamMessage::Disable(VtcStreamDisable::default()),
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

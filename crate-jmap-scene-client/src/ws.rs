//! Scene ephemeral events over the JMAP WebSocket
//! (draft-atwood-jmap-scene-wss-00).
//!
//! [`parse_scene_ws_frame`] lifts the Scene event frames out of a
//! base-client [`WsFrame`]; [`SceneWsExt`] adds Scene receive and send
//! methods to [`WsSession`].

use jmap_base_client::{ClientError, WsFrame, WsSession};
use jmap_scene_types::{
    SceneAvatarEvent, SceneEvent, SceneInteractionEvent, SceneObjectEvent, SceneStreamDisable,
    SceneStreamEnable, SceneStreamMessage, SCENE_EVENT_TYPES,
};
use serde::Deserialize as _;

/// A frame from the JMAP WebSocket, with the Scene ephemeral events typed.
///
/// `#[non_exhaustive]`, so new Scene event types stay semver-additive.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum SceneWsFrame {
    /// `SceneAvatarEvent`: an avatar entered, left or was ejected.
    Avatar(SceneAvatarEvent),
    /// `SceneObjectEvent`: an object was created, updated or destroyed.
    Object(SceneObjectEvent),
    /// `SceneInteractionEvent`: a user interacted with an object.
    Interaction(SceneInteractionEvent),
    /// Any other frame, unchanged.
    ///
    /// Scene events "are interleaved with `StateChange`, `Response`, and
    /// other ephemeral event frames (VTC, Chat) on the same WebSocket
    /// connection", so those arrive here for the caller to handle; a
    /// client "SHOULD silently ignore" an unrecognized `@type`. A Scene
    /// event whose body does not parse also arrives here, as
    /// [`WsFrame::Unknown`] with its `@type` and raw JSON.
    Other(WsFrame),
}

/// Promote a base-client [`WsFrame`] to a [`SceneWsFrame`].
///
/// A [`WsFrame::Unknown`] whose `@type` is `SceneAvatarEvent`,
/// `SceneObjectEvent` or `SceneInteractionEvent` and whose body parses
/// becomes the matching typed variant; every other frame becomes
/// [`SceneWsFrame::Other`]. One malformed frame never fails the
/// connection.
pub fn parse_scene_ws_frame(frame: WsFrame) -> SceneWsFrame {
    let WsFrame::Unknown { type_name, raw } = &frame else {
        return SceneWsFrame::Other(frame);
    };
    if !SCENE_EVENT_TYPES.contains(&type_name.as_str()) {
        return SceneWsFrame::Other(frame);
    }
    match SceneEvent::deserialize(raw) {
        Ok(SceneEvent::Avatar(e)) => SceneWsFrame::Avatar(e),
        Ok(SceneEvent::Object(e)) => SceneWsFrame::Object(e),
        Ok(SceneEvent::Interaction(e)) => SceneWsFrame::Interaction(e),
        _ => SceneWsFrame::Other(frame),
    }
}

/// Scene ephemeral-event methods on [`WsSession`].
///
/// Import this trait to use: `use jmap_scene_client::SceneWsExt;`
///
/// This trait is **sealed**: only [`WsSession`] implements it, so adding
/// methods is not a breaking change. The method names carry `scene` so
/// they do not collide with `jmap_chat_client::ChatWsExt` when both are
/// in scope.
pub trait SceneWsExt: sealed::Sealed {
    /// Receive the next frame from the server as a [`SceneWsFrame`].
    ///
    /// Returns `None` when the server has cleanly closed the connection
    /// and `Some(Err(..))` on a transport failure, as
    /// [`WsSession::next_frame`] does.
    fn next_scene_frame(
        &mut self,
    ) -> impl std::future::Future<Output = Option<Result<SceneWsFrame, ClientError>>> + Send;

    /// Send a `SceneStreamEnable` frame, starting or replacing the Scene
    /// event subscription on this connection.
    ///
    /// A server that rejects it answers with a `RequestError`, which
    /// arrives as [`SceneWsFrame::Other`].
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_scene_stream_enable(
        &mut self,
        enable: &SceneStreamEnable,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;

    /// Send a `SceneStreamDisable` frame, cancelling all Scene event
    /// delivery on this connection.
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_scene_stream_disable(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;

    /// Send a `SceneInteractionEvent` frame for the server to validate
    /// and relay.
    ///
    /// The server ignores its `userId` and relays the event with the
    /// authenticated user's. A frame that fails the server's checks is
    /// discarded: no error reaches the sender.
    ///
    /// # Errors
    ///
    /// [`ClientError`] on a serialization or transport failure.
    fn send_scene_interaction(
        &mut self,
        interaction: &SceneInteractionEvent,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
}

mod sealed {
    /// Seals [`super::SceneWsExt`]; see its rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::WsSession {}
}

impl SceneWsExt for WsSession {
    async fn next_scene_frame(&mut self) -> Option<Result<SceneWsFrame, ClientError>> {
        self.next_frame().await.map(|r| r.map(parse_scene_ws_frame))
    }

    async fn send_scene_stream_enable(
        &mut self,
        enable: &SceneStreamEnable,
    ) -> Result<(), ClientError> {
        send_json(self, &SceneStreamMessage::Enable(enable.clone())).await
    }

    async fn send_scene_stream_disable(&mut self) -> Result<(), ClientError> {
        send_json(
            self,
            &SceneStreamMessage::Disable(SceneStreamDisable::default()),
        )
        .await
    }

    async fn send_scene_interaction(
        &mut self,
        interaction: &SceneInteractionEvent,
    ) -> Result<(), ClientError> {
        send_json(self, &SceneEvent::Interaction(interaction.clone())).await
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

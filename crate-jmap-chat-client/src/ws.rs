//! WebSocket frame types and extension trait for JMAP Chat.
//!
//! Wraps [`WsFrame`] with chat-specific variants for
//! [`ChatTypingEvent`] and
//! [`ChatPresenceEvent`], which arrive as
//! [`WsFrame::Unknown`] on the base transport.
//!
//! Spec: draft-atwood-jmap-chat-wss-00

use jmap_base_client::{ClientError, WsFrame, WsSession};
use jmap_chat_types::{ChatPresenceEvent, ChatStreamEnable, ChatTypingEvent, EphemeralMessage};
use serde::Deserialize as _;

/// A frame from the JMAP WebSocket, with the Chat ephemeral events typed.
///
/// `#[non_exhaustive]`, so new Chat event types stay semver-additive.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ChatWsFrame {
    /// `ChatTypingEvent`: a typing indicator.
    /// Delivered only after a `ChatStreamEnable` subscription has been sent.
    ChatTyping(ChatTypingEvent),
    /// `ChatPresenceEvent`: a presence update.
    /// Delivered only after a `ChatStreamEnable` subscription has been sent.
    ChatPresence(ChatPresenceEvent),
    /// Any other frame, unchanged.
    ///
    /// Chat events "are interleaved with `StateChange` and `Response`
    /// frames on the same WebSocket connection"
    /// (draft-atwood-jmap-chat-wss-00 L165), so those arrive here typed,
    /// as does the `RequestError` that reports a failed
    /// `ChatStreamEnable` (L192); a client "SHOULD silently ignore" an
    /// unrecognized `@type` (L304). A Chat event whose body does not
    /// parse also arrives here, as [`WsFrame::Unknown`] with its `@type`
    /// and raw JSON.
    Other(WsFrame),
}

/// Promote a base-client [`WsFrame`] to a [`ChatWsFrame`].
///
/// A [`WsFrame::Unknown`] whose `@type` is `ChatTypingEvent` or
/// `ChatPresenceEvent` and whose body parses becomes the matching typed
/// variant; every other frame becomes [`ChatWsFrame::Other`]. One
/// malformed frame never fails the connection.
pub fn parse_chat_ws_frame(frame: WsFrame) -> ChatWsFrame {
    let WsFrame::Unknown { type_name, raw } = &frame else {
        return ChatWsFrame::Other(frame);
    };
    let parsed = match type_name.as_str() {
        "ChatTypingEvent" => ChatTypingEvent::deserialize(raw)
            .map(ChatWsFrame::ChatTyping)
            .ok(),
        "ChatPresenceEvent" => ChatPresenceEvent::deserialize(raw)
            .map(ChatWsFrame::ChatPresence)
            .ok(),
        _ => None,
    };
    parsed.unwrap_or(ChatWsFrame::Other(frame))
}

/// Extension trait adding JMAP Chat ephemeral-event methods to [`WsSession`].
///
/// Import this trait to use: `use jmap_chat_client::ChatWsExt;`
///
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::WsSession`]. Sealing prevents downstream
/// divergence and keeps adding methods to the trait a non-breaking
/// change.
pub trait ChatWsExt: sealed::Sealed {
    /// Receive the next frame from the server, interpreted as a [`ChatWsFrame`].
    ///
    /// Returns `None` when the server has cleanly closed the connection.
    /// Returns `Some(Err(...))` on transport failure; do not call again after
    /// a transport error. A server close with code 1008 returns
    /// [`ClientError::WsClosed`]: credentials expired
    /// (draft-atwood-jmap-chat-wss-00 §Authentication and Identity), so
    /// refresh them before reconnecting.
    fn next_chat_frame(
        &mut self,
    ) -> impl std::future::Future<Output = Option<Result<ChatWsFrame, ClientError>>> + Send;

    /// Subscribe to ephemeral events (typing indicators and/or presence updates).
    ///
    /// Sends a `ChatStreamEnable` frame to the server. A subsequent call replaces
    /// the prior subscription entirely; re-send after every reconnect because
    /// subscriptions are session-scoped.
    ///
    /// Success is silent; failure arrives later as a `RequestError` frame.
    /// Set [`ChatStreamEnable::id`] to correlate it: the error carries that
    /// value as its `requestId`, readable with
    /// [`jmap_base_client::WsFrame::request_id`] on the
    /// [`ChatWsFrame::Other`] frame. A client that sends
    /// several control messages in succession SHOULD give each an `id`
    /// (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable, L186 and L192).
    ///
    /// Spec: draft-atwood-jmap-chat-wss-00
    fn send_stream_enable(
        &mut self,
        enable: &ChatStreamEnable,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;

    /// Stop all ephemeral event delivery.
    ///
    /// Sends a `ChatStreamDisable` frame. The server MUST stop delivery silently
    /// even if no subscription is active.
    ///
    /// Spec: draft-atwood-jmap-chat-wss-00
    fn send_stream_disable(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), ClientError>> + Send;
}

mod sealed {
    /// Sealing-trait for [`super::ChatWsExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::WsSession {}
}

impl ChatWsExt for WsSession {
    async fn next_chat_frame(&mut self) -> Option<Result<ChatWsFrame, ClientError>> {
        self.next_frame().await.map(|r| r.map(parse_chat_ws_frame))
    }

    async fn send_stream_enable(&mut self, enable: &ChatStreamEnable) -> Result<(), ClientError> {
        // Wrap in EphemeralMessage::Enable so the @type discriminant is included
        // in the serialized output (ChatStreamEnable itself has no @type field).
        let msg = EphemeralMessage::Enable(enable.clone());
        // serde_json::to_string failure on a typed Serialize value is an
        // internal-invariant bug (the struct is built locally, not caller
        // input). Surface as ClientError::Parse to preserve the structured
        // serde_json::Error rather than dropping it into a String via
        // InvalidArgument.
        let text = serde_json::to_string(&msg).map_err(ClientError::from_parse)?;
        self.send_text(text).await
    }

    async fn send_stream_disable(&mut self) -> Result<(), ClientError> {
        // ChatStreamDisable is #[non_exhaustive]; construct via deserialization
        // of a hardcoded literal. A failure here is an internal-invariant bug
        // (the literal is built byte-for-byte above), so ClientError::Parse
        // preserves the structured error for debugging without conflating it
        // with caller-supplied InvalidArgument.
        let msg: EphemeralMessage = serde_json::from_str(r#"{"@type":"ChatStreamDisable"}"#)
            .map_err(ClientError::from_parse)?;
        let text = serde_json::to_string(&msg).map_err(ClientError::from_parse)?;
        self.send_text(text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_chat_types::Presence;
    use jmap_types::Id;

    /// Helper: build a WsFrame::Unknown with the given type_name and raw JSON.
    fn unknown_frame(type_name: &str, json: &str) -> WsFrame {
        let raw = serde_json::from_str(json).expect("test fixture must be valid JSON");
        WsFrame::Unknown {
            type_name: type_name.to_owned(),
            raw,
        }
    }

    /// Oracle: StateChange is forwarded unchanged in `Other`.
    /// Wire format from RFC 8620 §7.1.1.
    #[test]
    fn parse_state_change_forwarded() {
        // Construct via JSON because StateChange is #[non_exhaustive].
        let sc: jmap_base_client::StateChange =
            serde_json::from_str(r#"{"changed":{"acc1":{"Chat":"s1"}}}"#)
                .expect("test fixture must deserialize");
        let frame = parse_chat_ws_frame(WsFrame::StateChange(sc));
        match frame {
            ChatWsFrame::Other(WsFrame::StateChange(got)) => {
                assert_eq!(
                    got.changed
                        .get(&Id::from("acc1"))
                        .and_then(|m| m.get("Chat"))
                        .map(|s| s.as_ref()),
                    Some("s1")
                );
            }
            other => panic!("expected StateChange, got {other:?}"),
        }
    }

    /// Oracle: Unknown { ChatTypingEvent } with valid JSON parses to ChatTyping.
    /// Wire format from draft-atwood-jmap-chat-wss-00.
    #[test]
    fn parse_chat_typing_event_valid() {
        let frame = unknown_frame(
            "ChatTypingEvent",
            r#"{"@type":"ChatTypingEvent","accountId":"a1","chatId":"c1","senderId":"u1","typing":true}"#,
        );
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::ChatTyping(evt) => {
                assert_eq!(evt.chat_id.as_ref(), "c1");
                assert_eq!(evt.sender_id, Id::from("u1"));
                assert!(evt.typing);
            }
            other => panic!("expected ChatTyping, got {other:?}"),
        }
    }

    /// Oracle: Unknown { ChatPresenceEvent } with valid JSON parses to ChatPresence.
    /// Wire format from draft-atwood-jmap-chat-wss-00.
    #[test]
    fn parse_chat_presence_event_valid() {
        let frame = unknown_frame(
            "ChatPresenceEvent",
            r#"{"@type":"ChatPresenceEvent","accountId":"a1","contactId":"u2","presence":"away"}"#,
        );
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::ChatPresence(evt) => {
                assert_eq!(evt.contact_id.as_ref(), "u2");
                assert_eq!(evt.presence, Presence::Away);
            }
            other => panic!("expected ChatPresence, got {other:?}"),
        }
    }

    /// Oracle: Unknown { ChatTypingEvent } with malformed JSON degrades to
    /// `Other(Unknown)` with its raw JSON intact. A bad server frame must
    /// not kill the entire WebSocket session.
    #[test]
    fn parse_chat_typing_malformed_keeps_raw() {
        let frame = WsFrame::Unknown {
            type_name: "ChatTypingEvent".to_owned(),
            raw: serde_json::json!({"missing": "required fields"}),
        };
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::Other(WsFrame::Unknown { type_name, raw }) => {
                assert_eq!(type_name, "ChatTypingEvent");
                assert_eq!(raw, serde_json::json!({"missing": "required fields"}));
            }
            other => panic!("expected Other(Unknown), got {other:?}"),
        }
    }

    /// Oracle: Unknown with unrecognized type_name is forwarded unchanged.
    /// draft-atwood-jmap-chat-wss-00 L304: a client "SHOULD silently ignore"
    /// an unrecognized `@type`, so the caller gets the frame to drop.
    #[test]
    fn parse_unknown_type_forwarded() {
        let frame = unknown_frame("FutureEvent", r#"{"@type":"FutureEvent","foo":"bar"}"#);
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::Other(WsFrame::Unknown { type_name, raw }) => {
                assert_eq!(type_name, "FutureEvent");
                assert_eq!(raw["foo"], "bar");
            }
            other => panic!("expected Other(Unknown), got {other:?}"),
        }
    }

    /// Oracle: draft-atwood-jmap-chat-wss-00 L186 and L194 — a
    /// `ChatStreamEnable` with an empty `dataTypes` gets "a `RequestError`
    /// frame ... with a `type` of
    /// `"urn:ietf:params:jmap:error:invalidArguments"`" that "carries this
    /// value as its `requestId`". The frame below is hand-written from that
    /// text and RFC 8887 §4.3.4; the caller must see it typed.
    #[test]
    fn parse_request_error_forwarded_typed() {
        let err: jmap_base_client::WsRequestError = serde_json::from_str(
            r#"{"@type":"RequestError","requestId":"enable-1",
                "type":"urn:ietf:params:jmap:error:invalidArguments","status":400}"#,
        )
        .expect("test fixture must deserialize");
        match parse_chat_ws_frame(WsFrame::RequestError(err)) {
            ChatWsFrame::Other(frame @ WsFrame::RequestError(_)) => {
                assert_eq!(frame.request_id(), Some("enable-1"));
                let WsFrame::RequestError(e) = frame else {
                    unreachable!("matched RequestError above")
                };
                assert_eq!(
                    e.problem_type,
                    "urn:ietf:params:jmap:error:invalidArguments"
                );
                assert_eq!(e.status, Some(400));
            }
            other => panic!("expected Other(RequestError), got {other:?}"),
        }
    }

    /// Oracle: send_stream_enable serializes to a ChatStreamEnable frame with @type.
    /// Wire format from draft-atwood-jmap-chat-wss-00.
    #[test]
    fn send_stream_enable_serializes_with_at_type() {
        let enable: ChatStreamEnable = serde_json::from_str(r#"{"dataTypes":["typing"]}"#)
            .expect("test fixture must deserialize");
        let msg = EphemeralMessage::Enable(enable);
        let text = serde_json::to_string(&msg).expect("must serialize");
        assert!(
            text.contains("\"@type\":\"ChatStreamEnable\""),
            "ChatStreamEnable frame must include @type discriminant; got: {text}"
        );
        assert!(
            text.contains("\"dataTypes\""),
            "ChatStreamEnable frame must include dataTypes field; got: {text}"
        );
    }

    /// Oracle: send_stream_disable serializes to a ChatStreamDisable frame with @type.
    /// Wire format from draft-atwood-jmap-chat-wss-00.
    #[test]
    fn send_stream_disable_serializes_with_at_type() {
        // ChatStreamDisable is #[non_exhaustive]; construct via deserialization.
        let msg: EphemeralMessage =
            serde_json::from_str(r#"{"@type":"ChatStreamDisable"}"#).expect("must deserialize");
        let text = serde_json::to_string(&msg).expect("must serialize");
        assert!(
            text.contains("\"@type\":\"ChatStreamDisable\""),
            "ChatStreamDisable frame must include @type discriminant; got: {text}"
        );
    }
}

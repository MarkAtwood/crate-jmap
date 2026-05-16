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

/// A parsed frame from the JMAP Chat WebSocket, including chat-specific variants.
///
/// Marked `#[non_exhaustive]` because the spec may define additional `@type`
/// values in future revisions.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ChatWsFrame {
    /// RFC 8620 §7.1 StateChange — one or more object types have changed state.
    StateChange(jmap_base_client::StateChange),
    /// RFC 8887 Response — reply to a JMAP request sent on this connection.
    Response(jmap_types::JmapResponse),
    /// Ephemeral typing indicator (draft-atwood-jmap-chat-wss-00).
    /// Delivered only after a `ChatStreamEnable` subscription has been sent.
    ChatTyping(ChatTypingEvent),
    /// Ephemeral presence update (draft-atwood-jmap-chat-wss-00).
    /// Delivered only after a `ChatStreamEnable` subscription has been sent.
    ChatPresence(ChatPresenceEvent),
    /// Unrecognized `@type` — ignored per forward-compatibility rules
    /// (clients SHOULD ignore unknown message types per RFC 8887 §4.3.1).
    ///
    /// Also produced when a known chat type fails to deserialize — `type_name`
    /// will be `"ChatTypingEvent"` or `"ChatPresenceEvent"` in that case.
    Unknown {
        /// The unknown-frame discriminant value. Possible sources:
        /// - The verbatim `@type` string from the underlying
        ///   [`WsFrame::Unknown`] when the chat-side parser does not
        ///   recognise it (typical: future chat-extension events).
        /// - `"<no @type>"` (the literal string) when the underlying
        ///   `WsFrame::Unknown` itself reported an absent `@type` field —
        ///   that sentinel originates in `jmap-base-client` and
        ///   propagates through unchanged.
        /// - `"ChatTypingEvent"` or `"ChatPresenceEvent"` when a known
        ///   chat-event type failed payload deserialization (see the
        ///   parent variant's doc comment above).
        /// - `"<unknown>"` when the underlying [`WsFrame`] is a future
        ///   non-`Unknown` variant added by `jmap-base-client` after
        ///   `parse_chat_ws_frame` was last updated.
        ///
        /// Callers wishing to distinguish "server sent JSON without
        /// `@type`" from "this parser doesn't recognise the value"
        /// must match on the literal strings; the field intentionally
        /// flattens both cases into one `String` to keep the variant
        /// shape stable across future spec edits.
        type_name: String,
    },
}

/// Promote a base-client [`WsFrame`] to a [`ChatWsFrame`].
///
/// - `StateChange` and `Response` are forwarded unchanged.
/// - `Unknown { type_name: "ChatTypingEvent", raw }` → tries `serde_json::from_value`
///   into [`ChatTypingEvent`]; on failure produces `Unknown` (not an error).
/// - `Unknown { type_name: "ChatPresenceEvent", raw }` → same for [`ChatPresenceEvent`].
/// - All other `Unknown` frames are forwarded as `ChatWsFrame::Unknown`.
pub fn parse_chat_ws_frame(frame: WsFrame) -> ChatWsFrame {
    match frame {
        WsFrame::StateChange(sc) => ChatWsFrame::StateChange(sc),
        WsFrame::Response(r) => ChatWsFrame::Response(r),
        WsFrame::Unknown { type_name, raw } => match type_name.as_str() {
            "ChatTypingEvent" => serde_json::from_value::<ChatTypingEvent>(raw)
                .map(ChatWsFrame::ChatTyping)
                .unwrap_or_else(|_| ChatWsFrame::Unknown {
                    type_name: "ChatTypingEvent".to_owned(),
                }),
            "ChatPresenceEvent" => serde_json::from_value::<ChatPresenceEvent>(raw)
                .map(ChatWsFrame::ChatPresence)
                .unwrap_or_else(|_| ChatWsFrame::Unknown {
                    type_name: "ChatPresenceEvent".to_owned(),
                }),
            _ => ChatWsFrame::Unknown { type_name },
        },
        // WsFrame is #[non_exhaustive]: forward any future base-client variants as Unknown.
        _ => ChatWsFrame::Unknown {
            type_name: "<unknown>".to_owned(),
        },
    }
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
    /// a transport error.
    fn next_chat_frame(
        &mut self,
    ) -> impl std::future::Future<Output = Option<Result<ChatWsFrame, ClientError>>> + Send;

    /// Subscribe to ephemeral events (typing indicators and/or presence updates).
    ///
    /// Sends a `ChatStreamEnable` frame to the server. A subsequent call replaces
    /// the prior subscription entirely; re-send after every reconnect because
    /// subscriptions are session-scoped.
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
        let text = serde_json::to_string(&msg).map_err(ClientError::Parse)?;
        self.send_text(text).await
    }

    async fn send_stream_disable(&mut self) -> Result<(), ClientError> {
        // ChatStreamDisable is #[non_exhaustive]; construct via deserialization
        // of a hardcoded literal. A failure here is an internal-invariant bug
        // (the literal is built byte-for-byte above), so ClientError::Parse
        // preserves the structured error for debugging without conflating it
        // with caller-supplied InvalidArgument.
        let msg: EphemeralMessage =
            serde_json::from_str(r#"{"@type":"ChatStreamDisable"}"#).map_err(ClientError::Parse)?;
        let text = serde_json::to_string(&msg).map_err(ClientError::Parse)?;
        self.send_text(text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_chat_types::SenderId;
    use jmap_types::Id;

    /// Helper: build a WsFrame::Unknown with the given type_name and raw JSON.
    fn unknown_frame(type_name: &str, json: &str) -> WsFrame {
        let raw = serde_json::from_str(json).expect("test fixture must be valid JSON");
        WsFrame::Unknown {
            type_name: type_name.to_owned(),
            raw,
        }
    }

    /// Oracle: StateChange is forwarded unchanged.
    /// Wire format from RFC 8620 §7.1.1.
    #[test]
    fn parse_state_change_forwarded() {
        // Construct via JSON because StateChange is #[non_exhaustive].
        let sc: jmap_base_client::StateChange =
            serde_json::from_str(r#"{"changed":{"acc1":{"Chat":"s1"}}}"#)
                .expect("test fixture must deserialize");
        let frame = parse_chat_ws_frame(WsFrame::StateChange(sc));
        match frame {
            ChatWsFrame::StateChange(got) => {
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
            r#"{"@type":"ChatTypingEvent","chatId":"c1","senderId":"u1","typing":true}"#,
        );
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::ChatTyping(evt) => {
                assert_eq!(evt.chat_id.as_ref(), "c1");
                assert_eq!(evt.sender_id, SenderId::Contact("u1".to_owned()));
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
            r#"{"@type":"ChatPresenceEvent","contactId":"u2","presence":"away"}"#,
        );
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::ChatPresence(evt) => {
                assert_eq!(evt.contact_id.as_ref(), "u2");
                assert_eq!(evt.presence, "away");
            }
            other => panic!("expected ChatPresence, got {other:?}"),
        }
    }

    /// Oracle: Unknown { ChatTypingEvent } with malformed JSON degrades to Unknown.
    /// A bad server frame must not kill the entire WebSocket session.
    #[test]
    fn parse_chat_typing_malformed_degrades_to_unknown() {
        let frame = WsFrame::Unknown {
            type_name: "ChatTypingEvent".to_owned(),
            raw: serde_json::json!({"missing": "required fields"}),
        };
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::Unknown { type_name } => {
                assert_eq!(type_name, "ChatTypingEvent");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    /// Oracle: Unknown with unrecognized type_name is forwarded as Unknown.
    /// RFC 8887 §4.3.1: clients SHOULD ignore unknown message types.
    #[test]
    fn parse_unknown_type_forwarded() {
        let frame = unknown_frame("FutureEvent", r#"{"@type":"FutureEvent","foo":"bar"}"#);
        let got = parse_chat_ws_frame(frame);
        match got {
            ChatWsFrame::Unknown { type_name } => assert_eq!(type_name, "FutureEvent"),
            other => panic!("expected Unknown, got {other:?}"),
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

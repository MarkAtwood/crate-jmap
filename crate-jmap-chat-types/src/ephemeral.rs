//! WebSocket ephemeral message types for real-time events.

use crate::clearable::{some_clearable, Clearable};
use crate::presence::Presence;
use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An ephemeral event category in [`ChatStreamEnable::data_types`]
/// (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable `dataTypes`).
///
/// "This specification defines `"typing"` and `"presence"`. It also
/// defines two companion values: `"vtc"` [...] recognized only when the
/// server advertises `urn:ietf:params:jmap:vtc:websocket`, and `"scene"`
/// [...] recognized only when the server advertises
/// `urn:ietf:params:jmap:scene:websocket`." Any other string deserializes
/// to [`Other`](Self::Other) and serializes back unchanged.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EphemeralDataType {
    /// `"typing"`: typing indicators.
    Typing,
    /// `"presence"`: presence changes.
    Presence,
    /// `"vtc"`: VTC events (draft-atwood-jmap-vtc-wss-00).
    Vtc,
    /// `"scene"`: spatial events (draft-atwood-jmap-scene-wss-00).
    Scene,
    /// Any value this version does not recognize.
    ///
    /// **Forging caveat**: `Other("typing".into())` is unequal to
    /// [`Typing`](Self::Typing) on `PartialEq` but serializes to the same
    /// wire string. Reserve `Other(s)` for unrecognized strings; compare
    /// wire equality via [`as_str`](Self::as_str).
    Other(String),
}

impl EphemeralDataType {
    /// The wire string for this data type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Typing => "typing",
            Self::Presence => "presence",
            Self::Vtc => "vtc",
            Self::Scene => "scene",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl_string_enum!(EphemeralDataType, "an ephemeral data type string",
    "typing" => Typing,
    "presence" => Presence,
    "vtc" => Vtc,
    "scene" => Scene,
);

/// Client→server: subscribe to ephemeral events for selected data types.
///
/// `chat_ids: None` means all chats; `contact_ids: None` means all contacts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamEnable {
    /// Data-type tags to stream (draft-atwood-jmap-chat-wss-00 §7.1).
    ///
    /// See [`EphemeralDataType`]; `"vtc"` and `"scene"` are recognized
    /// only when the server advertises the companion WebSocket capability.
    /// A request containing ONLY unrecognized values MUST be rejected by
    /// the server with a `RequestError`. Unrecognized values appearing
    /// alongside recognized values MUST be silently ignored.
    pub data_types: Vec<EphemeralDataType>,
    /// Chats to filter on; `None` (or JSON `null`) means all chats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_ids: Option<Vec<Id>>,
    /// Contacts to filter on; `None` (or JSON `null`) means all contacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_ids: Option<Vec<Id>>,
    /// Client-chosen identifier for this message; `None` omits it.
    ///
    /// "A `RequestError` caused by this message carries this value as its
    /// `requestId`" (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable, L186),
    /// so a client can match a failure to the frame that caused it with
    /// `WsFrame::request_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Client→server: unsubscribe from ephemeral events.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ChatStreamDisable {
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Server→client: a contact is typing (or stopped typing) in a chat.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTypingEvent {
    /// The account that contains the Chat (draft-atwood-jmap-chat-wss-00
    /// §ChatTypingEvent). Chat ids are unique only within an account, so
    /// this tells apart same-id Chats in different accounts.
    pub account_id: Id,
    /// The chat in which typing is occurring.
    pub chat_id: Id,
    /// Sender identity: the accountId for the account owner,
    /// otherwise the typing user's `ChatContact.id`.
    pub sender_id: Id,
    /// `true` if the contact started typing; `false` if they stopped.
    pub typing: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Server→client: a contact's presence state has changed.
///
/// For `status_text` and `status_emoji`:
/// - `None` = field absent → no change
/// - `Some(Clearable::Clear)` = JSON `null` → clear the value
/// - `Some(Clearable::Set(v))` = JSON string → set to `v`
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPresenceEvent {
    /// The account that contains the ChatContact
    /// (draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent). ChatContact ids
    /// are unique only within an account, so this tells apart same-id
    /// contacts in different accounts.
    pub account_id: Id,
    /// The contact whose presence changed.
    pub contact_id: Id,
    /// New presence state. Typed identically to the [`Presence`]
    /// field on `ChatContact` and `PresenceStatus`; the
    /// `Presence::Other(String)` arm preserves any future wire
    /// vocabulary verbatim for round-trip fidelity.
    pub presence: Presence,
    /// When the contact was last active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<UTCDate>,
    /// Free-text status message; `null` clears it, absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub status_text: Option<Clearable<String>>,
    /// Status emoji; `null` clears it, absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub status_emoji: Option<Clearable<String>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Wrapper enum for all WebSocket ephemeral messages.
///
/// The `@type` JSON field acts as the discriminant on the wire.
///
/// # Forward compatibility
///
/// Unrecognised `@type` values deserialize to
/// [`EphemeralMessage::Unknown`] rather than producing an error,
/// allowing forward-compatible clients to receive frames defined by
/// future draft revisions. Unlike a unit-shaped fallback, the
/// `Unknown` variant captures both the original discriminant string
/// and the full payload object, so the frame can be re-serialised
/// verbatim (byte-equivalent up to JSON whitespace and field order
/// normalisation per `serde_json`).
///
/// This is what lets federation relays, multi-tab session bridges,
/// push-to-websocket bridges, and the workspace's own
/// `jmap-testjig` forward unrecognised WSS frames without silent
/// corruption.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum EphemeralMessage {
    /// The `ChatStreamEnable` message (draft-atwood-jmap-chat-wss-00 §7.1).
    Enable(ChatStreamEnable),
    /// The `ChatStreamDisable` message (draft-atwood-jmap-chat-wss-00 §7.2).
    Disable(ChatStreamDisable),
    /// The `ChatTypingEvent` message (draft-atwood-jmap-chat-wss-00 §7.3).
    Typing(ChatTypingEvent),
    /// The `ChatPresenceEvent` message (draft-atwood-jmap-chat-wss-00 §7.4).
    Presence(ChatPresenceEvent),
    /// Any `@type` not recognized by this version of the library.
    ///
    /// `type_name` carries the verbatim wire value of the `@type`
    /// field. `payload` carries every other top-level field of the
    /// frame, preserved verbatim for round-trip fidelity.
    Unknown {
        /// The original wire value of the `@type` discriminant.
        type_name: String,
        /// All other top-level fields of the frame, preserved
        /// verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

impl Serialize for EphemeralMessage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Build a serde_json::Value with `@type` set, then serialise
        // it. This keeps the wire format byte-equivalent to what the
        // previous `#[serde(tag = "@type")]` derive emitted for the
        // four known variants, while letting Unknown re-emit its
        // captured `type_name` + `payload` verbatim.
        let value: serde_json::Value = match self {
            EphemeralMessage::Enable(e) => {
                inject_type_tag(e, "ChatStreamEnable").map_err(serde::ser::Error::custom)?
            }
            EphemeralMessage::Disable(e) => {
                inject_type_tag(e, "ChatStreamDisable").map_err(serde::ser::Error::custom)?
            }
            EphemeralMessage::Typing(e) => {
                inject_type_tag(e, "ChatTypingEvent").map_err(serde::ser::Error::custom)?
            }
            EphemeralMessage::Presence(e) => {
                inject_type_tag(e, "ChatPresenceEvent").map_err(serde::ser::Error::custom)?
            }
            EphemeralMessage::Unknown { type_name, payload } => {
                let mut map = payload.clone();
                // Insert the wire @type tag. If the captured payload
                // unexpectedly contains an "@type" key (a future
                // mistake by a caller mutating Unknown.payload by
                // hand), it gets overwritten so the variant's
                // type_name remains authoritative.
                map.insert(
                    "@type".to_owned(),
                    serde_json::Value::String(type_name.clone()),
                );
                serde_json::Value::Object(map)
            }
        };
        value.serialize(s)
    }
}

impl<'de> Deserialize<'de> for EphemeralMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Two-phase: deserialise to a Map, dispatch on @type, then
        // re-deserialise the same Map into the concrete payload type
        // for known variants. Unknown carries the entire payload
        // (minus @type) for round-trip preservation.
        let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        let type_val = map
            .remove("@type")
            .ok_or_else(|| D::Error::missing_field("@type"))?;
        let type_name = match type_val {
            serde_json::Value::String(s) => s,
            other => {
                return Err(D::Error::custom(format!(
                    "@type must be a string, found {other}"
                )));
            }
        };

        // Re-deserialise the remaining map into the known payload
        // type. The map no longer contains @type, so the payload's
        // `extra` catch-all is not polluted with the discriminant.
        match type_name.as_str() {
            "ChatStreamEnable" => {
                let payload = from_map::<ChatStreamEnable, D>(map)?;
                Ok(EphemeralMessage::Enable(payload))
            }
            "ChatStreamDisable" => {
                let payload = from_map::<ChatStreamDisable, D>(map)?;
                Ok(EphemeralMessage::Disable(payload))
            }
            "ChatTypingEvent" => {
                let payload = from_map::<ChatTypingEvent, D>(map)?;
                Ok(EphemeralMessage::Typing(payload))
            }
            "ChatPresenceEvent" => {
                let payload = from_map::<ChatPresenceEvent, D>(map)?;
                Ok(EphemeralMessage::Presence(payload))
            }
            _ => Ok(EphemeralMessage::Unknown {
                type_name,
                payload: map,
            }),
        }
    }
}

/// Serialise a payload struct, then materialise it as a JSON object
/// and inject the `@type` tag. Used by [`EphemeralMessage`]'s
/// `Serialize` impl for the known variants.
fn inject_type_tag<T: Serialize>(
    payload: &T,
    type_name: &str,
) -> Result<serde_json::Value, serde_json::Error> {
    let value = serde_json::to_value(payload)?;
    match value {
        serde_json::Value::Object(mut map) => {
            map.insert(
                "@type".to_owned(),
                serde_json::Value::String(type_name.to_owned()),
            );
            Ok(serde_json::Value::Object(map))
        }
        other => Err(serde_json::Error::custom(format!(
            "EphemeralMessage payload must serialise to a JSON object, got {other}"
        ))),
    }
}

/// Deserialise a payload struct from an already-decoded JSON map.
/// Used by [`EphemeralMessage`]'s `Deserialize` impl for the known
/// variants. The error is funnelled through the outer deserialiser's
/// `Error` type so the failure mode is consistent with native serde
/// errors.
fn from_map<'de, T, D>(map: serde_json::Map<String, serde_json::Value>) -> Result<T, D::Error>
where
    T: serde::de::DeserializeOwned,
    D: Deserializer<'de>,
{
    serde_json::from_value(serde_json::Value::Object(map)).map_err(D::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_event_roundtrip() {
        let json = r#"{"@type":"ChatTypingEvent","accountId":"a1","chatId":"c1","senderId":"alice","typing":true}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Typing(e) => {
                assert_eq!(e.chat_id, Id::from("c1"));
                assert_eq!(e.sender_id, Id::from("alice"));
                assert!(e.typing);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn presence_event_null_status_text() {
        // null → Some(Clearable::Clear)
        let json = r#"{"@type":"ChatPresenceEvent","accountId":"a1","contactId":"u1","presence":"away","statusText":null}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(e.presence, Presence::Away);
                assert_eq!(e.status_text, Some(Clearable::Clear));
                assert_eq!(e.status_emoji, None); // absent
            }
            _ => panic!("wrong variant"),
        }
    }

    /// Oracle: known spec-defined wire values for `presence` route to
    /// the corresponding `Presence` variant, in parity with
    /// `ChatContact.presence` and `PresenceStatus.presence`. An
    /// unknown wire value preserves the original string via
    /// `Presence::Other(_)` for round-trip fidelity.
    #[test]
    fn presence_event_typed_presence() {
        let known = [
            ("online", Presence::Online),
            ("away", Presence::Away),
            ("busy", Presence::Busy),
            ("invisible", Presence::Invisible),
            ("offline", Presence::Offline),
        ];
        for (wire, expected) in known {
            let json = format!(
                r#"{{"@type":"ChatPresenceEvent","accountId":"a1","contactId":"u1","presence":"{wire}"}}"#
            );
            let msg: EphemeralMessage = serde_json::from_str(&json).unwrap();
            match msg {
                EphemeralMessage::Presence(e) => {
                    assert_eq!(e.presence, expected, "wire value {wire:?}");
                }
                _ => panic!("wrong variant for wire value {wire:?}"),
            }
        }

        // Unknown wire value preserved verbatim via Other(_).
        let json = r#"{"@type":"ChatPresenceEvent","accountId":"a1","contactId":"u1","presence":"do-not-disturb"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(e.presence, Presence::Other("do-not-disturb".to_owned()));
                let back = serde_json::to_value(&e).unwrap();
                assert_eq!(back["presence"], "do-not-disturb");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn presence_event_set_status_text() {
        // value → Some(Clearable::Set(...))
        let json = r#"{"@type":"ChatPresenceEvent","accountId":"a1","contactId":"u1","presence":"online","statusText":"In a meeting"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(
                    e.status_text,
                    Some(Clearable::Set("In a meeting".to_owned()))
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn presence_event_absent_status() {
        // field absent → None
        let json =
            r#"{"@type":"ChatPresenceEvent","accountId":"a1","contactId":"u1","presence":"busy"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(e.status_text, None);
                assert_eq!(e.status_emoji, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn stream_enable_null_chat_ids() {
        let json = r#"{"@type":"ChatStreamEnable","dataTypes":["typing"],"chatIds":null,"contactIds":null}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Enable(e) => {
                assert_eq!(e.data_types, vec![EphemeralDataType::Typing]);
                assert_eq!(e.chat_ids, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn stream_disable_roundtrip() {
        let json = r#"{"@type":"ChatStreamDisable"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, EphemeralMessage::Disable(_)));
        let out = serde_json::to_string(&msg).unwrap();
        assert_eq!(out, json);
    }

    /// Oracle: hand-built JSON value representing a future-spec frame
    /// that this crate version does not recognise. Asserts both
    /// (a) deserialise routes to `Unknown` with `type_name` carrying
    ///     the original wire tag verbatim and `payload` carrying every
    ///     other top-level field, AND
    /// (b) serialise emits a JSON object whose `@type` matches the
    ///     stored `type_name` and whose other top-level fields equal
    ///     the captured `payload`.
    ///
    /// Regression-protects against the previous unit-variant shape,
    /// which discarded the wire tag and payload on deserialise and
    /// emitted `{"@type":"Unknown"}` on serialise — a silent
    /// data-loss failure mode for federation relays and bridge
    /// implementations that forward unrecognised frames.
    #[test]
    fn unknown_variant_round_trips_type_name_and_payload() {
        let input_value = serde_json::json!({
            "@type": "ChatVideoCallStart",
            "callId": "v1",
            "ringingAt": "2026-01-02T09:00:00Z",
            "participants": ["alice", "bob"],
            "nested": {"foo": 42}
        });

        let msg: EphemeralMessage = serde_json::from_value(input_value.clone()).unwrap();

        // (a) Variant shape carries the wire tag and the full payload.
        match &msg {
            EphemeralMessage::Unknown { type_name, payload } => {
                assert_eq!(type_name, "ChatVideoCallStart");
                assert_eq!(payload.get("callId"), Some(&serde_json::json!("v1")));
                assert_eq!(
                    payload.get("ringingAt"),
                    Some(&serde_json::json!("2026-01-02T09:00:00Z"))
                );
                assert_eq!(
                    payload.get("participants"),
                    Some(&serde_json::json!(["alice", "bob"]))
                );
                assert_eq!(payload.get("nested"), Some(&serde_json::json!({"foo": 42})));
                // @type must NOT appear in payload — it lives in
                // type_name. This is what lets the consumer mutate
                // payload without the discriminant being ambiguous.
                assert!(payload.get("@type").is_none());
            }
            other => panic!("expected EphemeralMessage::Unknown, got {other:?}"),
        }

        // (b) Serialise round-trips the @type back into the JSON
        // object alongside every captured payload field. Compare as
        // serde_json::Value to be tolerant of field ordering.
        let out_value = serde_json::to_value(&msg).unwrap();
        assert_eq!(out_value, input_value);
    }

    /// Oracle: an unknown frame whose payload is `{}` (just the
    /// `@type` tag and nothing else) still routes to `Unknown` with
    /// an empty `payload` map and round-trips byte-equivalently.
    #[test]
    fn unknown_variant_empty_payload_round_trips() {
        let input_value = serde_json::json!({"@type": "ChatFutureBareFrame"});
        let msg: EphemeralMessage = serde_json::from_value(input_value.clone()).unwrap();
        match &msg {
            EphemeralMessage::Unknown { type_name, payload } => {
                assert_eq!(type_name, "ChatFutureBareFrame");
                assert!(payload.is_empty());
            }
            other => panic!("expected EphemeralMessage::Unknown, got {other:?}"),
        }
        let out_value = serde_json::to_value(&msg).unwrap();
        assert_eq!(out_value, input_value);
    }

    /// Oracle: a known frame still round-trips byte-equivalently
    /// after the hand-rolled (de)serialiser replaced the
    /// `#[serde(tag = "@type")]` derive. This is the
    /// regression-protection test for the wire format of the four
    /// recognised variants.
    #[test]
    fn known_variant_round_trips_byte_equivalent() {
        let input_value = serde_json::json!({
            "@type": "ChatTypingEvent",
            "accountId": "a1",
            "chatId": "c1",
            "senderId": "alice",
            "typing": true
        });
        let msg: EphemeralMessage = serde_json::from_value(input_value.clone()).unwrap();
        assert!(matches!(msg, EphemeralMessage::Typing(_)));
        let out_value = serde_json::to_value(&msg).unwrap();
        assert_eq!(out_value, input_value);
    }

    /// Oracle: a missing `@type` field surfaces a deserialise error
    /// rather than routing to a silent fallback variant. This
    /// matches the behaviour of the previous `#[serde(tag = "@type")]`
    /// derive and is what consumers expect for a malformed frame.
    #[test]
    fn missing_type_tag_is_error() {
        let input = r#"{"chatId":"c1"}"#;
        let res: Result<EphemeralMessage, _> = serde_json::from_str(input);
        assert!(res.is_err(), "expected error for missing @type tag");
    }

    /// Oracle: a non-string `@type` value is a malformed frame and
    /// surfaces as a deserialise error rather than routing to
    /// Unknown with a stringified non-string value.
    #[test]
    fn non_string_type_tag_is_error() {
        let input = r#"{"@type":42}"#;
        let res: Result<EphemeralMessage, _> = serde_json::from_str(input);
        assert!(res.is_err(), "expected error for non-string @type tag");
    }

    /// Oracle: the draft-atwood-jmap-chat-wss-00 §ChatTypingEvent example
    /// frame (jmap-chat-spec 6bcd9d8, wss:241-249), copied verbatim.
    /// wss:229-230: "`accountId` (String): The id of the account that
    /// contains the Chat." It is a typed field, not an extension.
    #[test]
    fn typing_event_spec_example_carries_account_id() {
        let frame = serde_json::json!({
            "@type": "ChatTypingEvent",
            "accountId": "u1",
            "chatId": "01J3XKZQN4MWVT8PPBEHTJ3HX0",
            "senderId": "c8xq2m",
            "typing": true
        });
        let msg: EphemeralMessage = serde_json::from_value(frame.clone()).unwrap();
        match &msg {
            EphemeralMessage::Typing(e) => {
                assert_eq!(e.account_id, Id::from("u1"));
                assert!(e.extra.is_empty());
            }
            other => panic!("expected Typing, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(&msg).unwrap(), frame);
    }

    /// Oracle: the draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent
    /// example frame (jmap-chat-spec 6bcd9d8, wss:286-296), copied
    /// verbatim. wss:266-267: "`accountId` (String): The id of the account
    /// that contains the ChatContact."
    #[test]
    fn presence_event_spec_example_carries_account_id() {
        let frame = serde_json::json!({
            "@type": "ChatPresenceEvent",
            "accountId": "u1",
            "contactId": "c8xq2m",
            "presence": "away",
            "lastActiveAt": "2026-04-26T12:00:00Z",
            "statusText": "In a meeting",
            "statusEmoji": null
        });
        let msg: EphemeralMessage = serde_json::from_value(frame.clone()).unwrap();
        match &msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(e.account_id, Id::from("u1"));
                assert!(e.extra.is_empty());
            }
            other => panic!("expected Presence, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(&msg).unwrap(), frame);
    }

    /// Oracle: wss:229 and wss:266 list `accountId` without "optional",
    /// so a frame lacking it is malformed.
    #[test]
    fn events_without_account_id_are_rejected() {
        let typing = r#"{"chatId":"c1","senderId":"alice","typing":true}"#;
        assert!(serde_json::from_str::<ChatTypingEvent>(typing).is_err());
        let presence = r#"{"contactId":"u1","presence":"away"}"#;
        assert!(serde_json::from_str::<ChatPresenceEvent>(presence).is_err());
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `ChatStreamEnable.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_stream_enable_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "dataTypes": ["typing"],
            "acmeCorpStreamPriority": "high"
        });
        let e: ChatStreamEnable = serde_json::from_value(raw).unwrap();
        assert_eq!(
            e.extra
                .get("acmeCorpStreamPriority")
                .and_then(|v| v.as_str()),
            Some("high")
        );
        let back = serde_json::to_value(&e).unwrap();
        assert_eq!(back["acmeCorpStreamPriority"], "high");
    }

    /// wss 6bcd9d8 L185-186: "`id` (String, optional): A client-specified
    /// identifier for this message." It is a typed field, not an extra, and
    /// is omitted from the wire when absent.
    #[test]
    fn chat_stream_enable_id_is_typed_and_optional() {
        let json = r#"{"@type":"ChatStreamEnable","dataTypes":["typing"],"id":"R1"}"#;
        let EphemeralMessage::Enable(e) = serde_json::from_str(json).unwrap() else {
            panic!("expected Enable");
        };
        assert_eq!(e.id.as_deref(), Some("R1"));
        assert!(e.extra.is_empty());
        let back = serde_json::to_value(EphemeralMessage::Enable(e.clone())).unwrap();
        assert_eq!(
            back,
            serde_json::json!({"@type": "ChatStreamEnable", "dataTypes": ["typing"], "id": "R1"})
        );

        let mut e = e;
        e.id = None;
        let back = serde_json::to_value(&e).unwrap();
        assert_eq!(back, serde_json::json!({"dataTypes": ["typing"]}));
    }

    /// wss 33702a5 L185: "`id` (String, optional)". A non-string `id` is a
    /// malformed frame, not a vendor extra.
    #[test]
    fn chat_stream_enable_non_string_id_rejected() {
        let json = r#"{"@type":"ChatStreamEnable","dataTypes":["typing"],"id":7}"#;
        assert!(serde_json::from_str::<EphemeralMessage>(json).is_err());
    }

    /// `ChatStreamDisable.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_stream_disable_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "acmeCorpReason": "user-quit"
        });
        let d: ChatStreamDisable = serde_json::from_value(raw).unwrap();
        assert_eq!(
            d.extra.get("acmeCorpReason").and_then(|v| v.as_str()),
            Some("user-quit")
        );
        let back = serde_json::to_value(&d).unwrap();
        assert_eq!(back["acmeCorpReason"], "user-quit");
    }

    /// `ChatTypingEvent.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_typing_event_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "a1",
            "chatId": "c1",
            "senderId": "alice",
            "typing": true,
            "acmeCorpClientId": "web-3"
        });
        let e: ChatTypingEvent = serde_json::from_value(raw).unwrap();
        assert_eq!(
            e.extra.get("acmeCorpClientId").and_then(|v| v.as_str()),
            Some("web-3")
        );
        let back = serde_json::to_value(&e).unwrap();
        assert_eq!(back["acmeCorpClientId"], "web-3");
    }

    /// `ChatPresenceEvent.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_presence_event_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "a1",
            "contactId": "u1",
            "presence": "online",
            "acmeCorpDeviceClass": "mobile"
        });
        let e: ChatPresenceEvent = serde_json::from_value(raw).unwrap();
        assert_eq!(
            e.extra.get("acmeCorpDeviceClass").and_then(|v| v.as_str()),
            Some("mobile")
        );
        let back = serde_json::to_value(&e).unwrap();
        assert_eq!(back["acmeCorpDeviceClass"], "mobile");
    }
}

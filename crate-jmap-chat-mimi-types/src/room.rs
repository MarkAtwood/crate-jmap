//! draft-atwood-jmap-chat-mimi-00 §Rooms — the `Chat.roomId` extension
//! field and the arguments of `Room/create`, `Room/update`, `Room/send` and
//! `Room/unbind`.
//!
//! None of the four is a standard RFC 8620 method. The `Room/update` and
//! `Room/send` responses are decoded from the hub's MIMI responses
//! (draft-ietf-mimi-protocol-06 §5.3, §5.4), with the response codes
//! written as their MIMI code names. Fields the draft marks "When
//! `responseCode` is ..." are `Option`; deserialize does not check that
//! they are present exactly for that code, so check before use.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::Base64Url;

/// The `roomId` of a Chat, read from the Chat's `extra` map.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Chat.roomId: "`roomId` (String,
/// immutable, server-set, optional): The MIMI room ID of the room this
/// Chat is bound to. Absent for Chats not bound to a MIMI room." An
/// unbound Chat "keeps its `roomId`", so `Some` does not mean the Chat is
/// still bound.
///
/// `roomId` is an extension field, so `jmap_chat_types::Chat` keeps it in
/// `extra`; pass `&chat.extra`. Returns `None` when `roomId` is absent or
/// is not a string.
///
/// # Example
///
/// ```
/// let extra = serde_json::json!({
///     "roomId": "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a"
/// });
/// assert_eq!(
///     jmap_chat_mimi_types::chat_room_id(extra.as_object().unwrap()),
///     Some("mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a")
/// );
/// ```
pub fn chat_room_id(extra: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
    extra.get("roomId")?.as_str()
}

/// The `responseCode` of a `Room/update` response: the
/// UpdateResponseCode of draft-ietf-mimi-protocol-06 §5.3, written as its
/// name.
///
/// `Other` preserves any code name this version does not know, for
/// lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UpdateResponseCode {
    /// The UpdateRequest was accepted and will be distributed.
    Success,
    /// The hub is using a different epoch.
    WrongEpoch,
    /// Policy or authorization prevented the hub from accepting the
    /// UpdateRequest.
    NotAllowed,
    /// At least one proposal is invalid.
    InvalidProposal,
    /// A code name not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(UpdateResponseCode, "an UpdateResponseCode name",
    "success" => Success,
    "wrongEpoch" => WrongEpoch,
    "notAllowed" => NotAllowed,
    "invalidProposal" => InvalidProposal,
);

/// The `responseCode` of a `Room/send` response: the `statusCode`
/// (SubmitResponseCode) of draft-ietf-mimi-protocol-06 §5.4, written as
/// its name.
///
/// `Other` preserves any code name this version does not know, for
/// lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubmitResponseCode {
    /// The hub accepted the message.
    Accepted,
    /// Policy or authorization prevented the hub from accepting the
    /// message.
    NotAllowed,
    /// The hub is using a newer epoch.
    EpochTooOld,
    /// A code name not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(SubmitResponseCode, "a SubmitResponseCode name",
    "accepted" => Accepted,
    "notAllowed" => NotAllowed,
    "epochTooOld" => EpochTooOld,
);

/// Request arguments of `Room/create`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/create. All three arguments
/// are required.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::RoomCreateRequest;
///
/// let req = RoomCreateRequest::new(
///     jmap_types::Id::new_validated("u1")?,
///     "AAEABEAAEJr1".parse()?,
///     "QAABAQ".parse()?,
/// );
/// assert_eq!(
///     serde_json::to_string(&req)?,
///     r#"{"accountId":"u1","groupInfo":"AAEABEAAEJr1","ratchetTree":"QAABAQ"}"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomCreateRequest {
    /// The account to use.
    pub account_id: Id,
    /// An MLSMessage (RFC 9420 §6) of wire format `mls_group_info`
    /// carrying the group's GroupInfo, without the `ratchet_tree`
    /// extension.
    pub group_info: Base64Url,
    /// The group's RatchetTree (RFC 9420 §12.4.3.3).
    pub ratchet_tree: Base64Url,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomCreateRequest {
    /// A request from its three required arguments, with no extras.
    pub fn new(account_id: Id, group_info: Base64Url, ratchet_tree: Base64Url) -> Self {
        Self {
            account_id,
            group_info,
            ratchet_tree,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `Room/create`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/create. All three arguments
/// are required.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomCreateResponse {
    /// The account used.
    pub account_id: Id,
    /// The room ID.
    pub room_id: String,
    /// The id of the new Chat.
    pub chat_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomCreateResponse {
    /// A response from its three required arguments, with no extras.
    pub fn new(account_id: Id, room_id: String, chat_id: Id) -> Self {
        Self {
            account_id,
            room_id,
            chat_id,
            extra: serde_json::Map::new(),
        }
    }
}

/// Request arguments of `Room/update`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/update. All three arguments
/// are required.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomUpdateRequest {
    /// The account to use.
    pub account_id: Id,
    /// The `roomId` of a Chat in this account.
    pub room_id: String,
    /// An UpdateRequest (draft-ietf-mimi-protocol-06 §5.3).
    pub request: Base64Url,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomUpdateRequest {
    /// A request from its three required arguments, with no extras.
    pub fn new(account_id: Id, room_id: String, request: Base64Url) -> Self {
        Self {
            account_id,
            room_id,
            request,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `Room/update`, decoded from the hub's
/// UpdateRoomResponse.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/update. `accountId`,
/// `responseCode` and `errorDescription` are required; each other field
/// is present only for one `responseCode`.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::{RoomUpdateResponse, UpdateResponseCode};
///
/// let resp: RoomUpdateResponse = serde_json::from_str(r#"{
///     "accountId": "u1",
///     "responseCode": "wrongEpoch",
///     "errorDescription": "",
///     "currentEpoch": 7
/// }"#)?;
/// assert_eq!(resp.response_code, UpdateResponseCode::WrongEpoch);
/// assert_eq!(resp.current_epoch, Some(7));
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomUpdateResponse {
    /// The account used.
    pub account_id: Id,
    /// The hub's response code.
    pub response_code: UpdateResponseCode,
    /// The hub's `errorDescription`, possibly empty.
    pub error_description: String,
    /// When `response_code` is [`UpdateResponseCode::Success`]: the hub's
    /// `accepted_timestamp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<UTCDate>,
    /// When `response_code` is [`UpdateResponseCode::WrongEpoch`]: the
    /// hub's current epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_epoch: Option<u64>,
    /// When `response_code` is [`UpdateResponseCode::InvalidProposal`]:
    /// the ProposalRefs (RFC 9420 §5.2) of the invalid proposals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_proposals: Option<Vec<Base64Url>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomUpdateResponse {
    /// A response from its three required arguments, with no conditional
    /// fields and no extras.
    pub fn new(
        account_id: Id,
        response_code: UpdateResponseCode,
        error_description: String,
    ) -> Self {
        Self {
            account_id,
            response_code,
            error_description,
            accepted_at: None,
            current_epoch: None,
            invalid_proposals: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Request arguments of `Room/send`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/send. All three arguments
/// are required.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSendRequest {
    /// The account to use.
    pub account_id: Id,
    /// The `roomId` of a Chat in this account.
    pub room_id: String,
    /// An MLSMessage (RFC 9420 §6) of wire format PrivateMessage and
    /// content type application, whose plaintext is MIMI content.
    pub message: Base64Url,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomSendRequest {
    /// A request from its three required arguments, with no extras.
    pub fn new(account_id: Id, room_id: String, message: Base64Url) -> Self {
        Self {
            account_id,
            room_id,
            message,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `Room/send`, decoded from the
/// SubmitMessageResponse.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/send. `accountId` and
/// `responseCode` are required; each other field is present only for one
/// `responseCode`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSendResponse {
    /// The account used.
    pub account_id: Id,
    /// The hub's status code.
    pub response_code: SubmitResponseCode,
    /// When `response_code` is [`SubmitResponseCode::Accepted`]: the
    /// hub's `accepted_timestamp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<UTCDate>,
    /// When `response_code` is [`SubmitResponseCode::EpochTooOld`]: the
    /// hub's current epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_epoch: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomSendResponse {
    /// A response from its two required arguments, with no conditional
    /// fields and no extras.
    pub fn new(account_id: Id, response_code: SubmitResponseCode) -> Self {
        Self {
            account_id,
            response_code,
            accepted_at: None,
            current_epoch: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Request arguments of `Room/unbind`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/unbind. Both arguments are
/// required.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomUnbindRequest {
    /// The account to use.
    pub account_id: Id,
    /// The `roomId` of a Chat in this account.
    pub room_id: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomUnbindRequest {
    /// A request from its two required arguments, with no extras.
    pub fn new(account_id: Id, room_id: String) -> Self {
        Self {
            account_id,
            room_id,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `Room/unbind`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §Room/unbind: the server "returns
/// `accountId` (Id): The account used."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomUnbindResponse {
    /// The account used.
    pub account_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RoomUnbindResponse {
    /// A response from its one required argument, with no extras.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::{json, Value};

    // The draft's §Examples payloads are shortened with "...". These tests
    // drop the "..." and keep the prefixes, which are themselves valid
    // unpadded base64url: "AAEABEAAEJr1" and "AAEAAQAQmvVK" and
    // "AAEAAgAQmvVK" (12 characters each), "QAABAQ" (6 characters, final
    // 'Q' = 010000 with pad bits 0000).
    const ROOM: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";
    // The KeyPackageRef of draft §Conventions, a ProposalRef-shaped
    // HashReference.
    const REF: &str = "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";

    fn id(s: &str) -> Id {
        Id::new_validated(s).unwrap()
    }

    /// Asserts `raw` deserializes into `T` and serializes back unchanged.
    fn round_trip<T: DeserializeOwned + Serialize>(raw: &Value) -> T {
        let v: T = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(&serde_json::to_value(&v).unwrap(), raw);
        v
    }

    /// Asserts removing each of `fields` from `raw` fails to deserialize.
    fn each_required<T: DeserializeOwned>(raw: &Value, fields: &[&str]) {
        for field in fields {
            let mut raw = raw.clone();
            raw.as_object_mut().unwrap().remove(*field);
            assert!(
                serde_json::from_value::<T>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    fn create_request() -> Value {
        json!({ "accountId": "u1", "groupInfo": "AAEABEAAEJr1", "ratchetTree": "QAABAQ" })
    }

    fn create_response() -> Value {
        json!({ "accountId": "u1", "roomId": ROOM, "chatId": "01JA8Z3K6V2Q4R5S7T9W0XYZAB" })
    }

    fn update_request() -> Value {
        json!({ "accountId": "u1", "roomId": ROOM, "request": "AAEAAQAQmvVK" })
    }

    fn update_response() -> Value {
        json!({
            "accountId": "u1",
            "responseCode": "success",
            "errorDescription": "",
            "acceptedAt": "2026-10-18T17:02:11.482Z"
        })
    }

    fn send_request() -> Value {
        json!({ "accountId": "u1", "roomId": ROOM, "message": "AAEAAgAQmvVK" })
    }

    fn send_response() -> Value {
        json!({ "accountId": "u1", "responseCode": "epochTooOld", "currentEpoch": 4 })
    }

    fn unbind_request() -> Value {
        json!({ "accountId": "u2", "roomId": ROOM })
    }

    #[test]
    fn chat_room_id_reads_chat_extra() {
        // Oracle: draft §Chat.roomId: "When this capability is present,
        // the Chat object has one more field: `roomId` (String, ...,
        // optional)"; "A Chat with a `roomId` has kind `\"group\"`" and its
        // `members` "is the empty list". Hand-written Chat per
        // draft-atwood-jmap-chat-00 §Chat.
        let chat = json!({
            "id": "01JA8Z3K6V2Q4R5S7T9W0XYZAB",
            "kind": "group",
            "createdAt": "2026-10-18T17:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "muted": false,
            "receiveTypingIndicators": true,
            "name": "",
            "members": [],
            "roomId": ROOM
        });
        let chat: jmap_chat_types::Chat = serde_json::from_value(chat).unwrap();
        assert_eq!(chat_room_id(&chat.extra), Some(ROOM));
    }

    #[test]
    fn chat_room_id_absent_or_not_string() {
        // Oracle: draft §Chat.roomId: "Absent for Chats not bound to a
        // MIMI room"; the field is a String.
        assert_eq!(chat_room_id(&serde_json::Map::new()), None);
        let extra = json!({ "roomId": 42 });
        assert_eq!(chat_room_id(extra.as_object().unwrap()), None);
    }

    #[test]
    fn create_matches_draft_example() {
        // Oracle: draft §Examples, "Alice's client creates a room".
        let req: RoomCreateRequest = round_trip(&create_request());
        assert_eq!(req.account_id.as_ref(), "u1");
        assert_eq!(req.group_info.as_str(), "AAEABEAAEJr1");
        assert_eq!(req.ratchet_tree.as_str(), "QAABAQ");
        let built = RoomCreateRequest::new(
            id("u1"),
            "AAEABEAAEJr1".parse().unwrap(),
            "QAABAQ".parse().unwrap(),
        );
        assert_eq!(serde_json::to_value(built).unwrap(), create_request());

        let resp: RoomCreateResponse = round_trip(&create_response());
        assert_eq!(resp.room_id, ROOM);
        assert_eq!(resp.chat_id.as_ref(), "01JA8Z3K6V2Q4R5S7T9W0XYZAB");
        let built =
            RoomCreateResponse::new(id("u1"), ROOM.into(), id("01JA8Z3K6V2Q4R5S7T9W0XYZAB"));
        assert_eq!(serde_json::to_value(built).unwrap(), create_response());
    }

    #[test]
    fn update_matches_draft_example() {
        // Oracle: draft §Examples, "Alice's client commits Bob's addition
        // and the hub accepts it".
        let req: RoomUpdateRequest = round_trip(&update_request());
        assert_eq!(req.room_id, ROOM);
        assert_eq!(req.request.as_str(), "AAEAAQAQmvVK");
        let built = RoomUpdateRequest::new(id("u1"), ROOM.into(), "AAEAAQAQmvVK".parse().unwrap());
        assert_eq!(serde_json::to_value(built).unwrap(), update_request());

        let resp: RoomUpdateResponse = round_trip(&update_response());
        assert_eq!(resp.response_code, UpdateResponseCode::Success);
        assert_eq!(resp.error_description, "");
        assert_eq!(
            resp.accepted_at.as_ref().map(AsRef::as_ref),
            Some("2026-10-18T17:02:11.482Z")
        );
        assert_eq!(resp.current_epoch, None);
        assert_eq!(resp.invalid_proposals, None);
        let mut built = RoomUpdateResponse::new(id("u1"), UpdateResponseCode::Success, "".into());
        built.accepted_at = Some("2026-10-18T17:02:11.482Z".into());
        assert_eq!(serde_json::to_value(built).unwrap(), update_response());
    }

    #[test]
    fn update_invalid_proposal_round_trips() {
        // Oracle: draft §Room/update: "`invalidProposals` (String[]): When
        // `responseCode` is `\"invalidProposal\"`: the ProposalRefs of the
        // invalid proposals."
        let raw = json!({
            "accountId": "u1",
            "responseCode": "invalidProposal",
            "errorDescription": "bad add",
            "invalidProposals": [REF]
        });
        let resp: RoomUpdateResponse = round_trip(&raw);
        assert_eq!(resp.response_code, UpdateResponseCode::InvalidProposal);
        assert_eq!(resp.invalid_proposals.unwrap()[0].as_str(), REF);
        let mut padded = raw.clone();
        padded["invalidProposals"] = json!(["AA=="]);
        assert!(serde_json::from_value::<RoomUpdateResponse>(padded).is_err());
    }

    #[test]
    fn send_matches_draft_example() {
        // Oracle: draft §Examples, "A client sends a message in a stale
        // epoch".
        let req: RoomSendRequest = round_trip(&send_request());
        assert_eq!(req.room_id, ROOM);
        assert_eq!(req.message.as_str(), "AAEAAgAQmvVK");
        let built = RoomSendRequest::new(id("u1"), ROOM.into(), "AAEAAgAQmvVK".parse().unwrap());
        assert_eq!(serde_json::to_value(built).unwrap(), send_request());

        let resp: RoomSendResponse = round_trip(&send_response());
        assert_eq!(resp.response_code, SubmitResponseCode::EpochTooOld);
        assert_eq!(resp.current_epoch, Some(4));
        assert_eq!(resp.accepted_at, None);
        let mut built = RoomSendResponse::new(id("u1"), SubmitResponseCode::EpochTooOld);
        built.current_epoch = Some(4);
        assert_eq!(serde_json::to_value(built).unwrap(), send_response());
    }

    #[test]
    fn unbind_matches_draft_example() {
        // Oracle: draft §Examples, "Bob's client processes a commit that
        // removes Bob's last client"; §Room/unbind response is `accountId`.
        let req: RoomUnbindRequest = round_trip(&unbind_request());
        assert_eq!(req.account_id.as_ref(), "u2");
        assert_eq!(req.room_id, ROOM);
        assert_eq!(
            serde_json::to_value(RoomUnbindRequest::new(id("u2"), ROOM.into())).unwrap(),
            unbind_request()
        );
        let raw = json!({ "accountId": "u2" });
        let resp: RoomUnbindResponse = round_trip(&raw);
        assert_eq!(resp.account_id.as_ref(), "u2");
        assert_eq!(
            serde_json::to_value(RoomUnbindResponse::new(id("u2"))).unwrap(),
            raw
        );
    }

    #[test]
    fn required_arguments_rejected_when_missing() {
        // Oracle: draft §Room/create, §Room/update, §Room/send and
        // §Room/unbind list these arguments with no "optional" or "When".
        each_required::<RoomCreateRequest>(
            &create_request(),
            &["accountId", "groupInfo", "ratchetTree"],
        );
        each_required::<RoomCreateResponse>(&create_response(), &["accountId", "roomId", "chatId"]);
        each_required::<RoomUpdateRequest>(&update_request(), &["accountId", "roomId", "request"]);
        each_required::<RoomUpdateResponse>(
            &update_response(),
            &["accountId", "responseCode", "errorDescription"],
        );
        each_required::<RoomSendRequest>(&send_request(), &["accountId", "roomId", "message"]);
        each_required::<RoomSendResponse>(&send_response(), &["accountId", "responseCode"]);
        each_required::<RoomUnbindRequest>(&unbind_request(), &["accountId", "roomId"]);
        each_required::<RoomUnbindResponse>(&json!({ "accountId": "u2" }), &["accountId"]);
    }

    #[test]
    fn padded_byte_strings_rejected() {
        // Oracle: draft §Conventions: byte strings are the "unpadded
        // base64url encoding".
        for (raw, field) in [
            (create_request(), "groupInfo"),
            (create_request(), "ratchetTree"),
        ] {
            let mut raw = raw;
            raw[field] = json!("AA==");
            assert!(serde_json::from_value::<RoomCreateRequest>(raw).is_err());
        }
        let mut raw = update_request();
        raw["request"] = json!("AA==");
        assert!(serde_json::from_value::<RoomUpdateRequest>(raw).is_err());
        let mut raw = send_request();
        raw["message"] = json!("AA==");
        assert!(serde_json::from_value::<RoomSendRequest>(raw).is_err());
    }

    #[test]
    fn update_codes_match_mimi_names() {
        // Oracle: draft-ietf-mimi-protocol-06 §5.3 `enum { success(0),
        // wrongEpoch(1), notAllowed(2), invalidProposal(3), (255) }
        // UpdateResponseCode`.
        let cases = [
            ("success", UpdateResponseCode::Success),
            ("wrongEpoch", UpdateResponseCode::WrongEpoch),
            ("notAllowed", UpdateResponseCode::NotAllowed),
            ("invalidProposal", UpdateResponseCode::InvalidProposal),
        ];
        for (name, code) in cases {
            assert_eq!(
                serde_json::from_value::<UpdateResponseCode>(json!(name)).unwrap(),
                code
            );
            assert_eq!(serde_json::to_value(&code).unwrap(), json!(name));
        }
    }

    #[test]
    fn submit_codes_match_mimi_names() {
        // Oracle: draft-ietf-mimi-protocol-06 §5.4 `enum { accepted(0),
        // notAllowed(1), epochTooOld(2), (255) } SubmitResponseCode`, and
        // draft §Room/send: `"accepted"`, `"notAllowed"`, or
        // `"epochTooOld"`.
        let cases = [
            ("accepted", SubmitResponseCode::Accepted),
            ("notAllowed", SubmitResponseCode::NotAllowed),
            ("epochTooOld", SubmitResponseCode::EpochTooOld),
        ];
        for (name, code) in cases {
            assert_eq!(
                serde_json::from_value::<SubmitResponseCode>(json!(name)).unwrap(),
                code
            );
            assert_eq!(serde_json::to_value(&code).unwrap(), json!(name));
        }
    }

    #[test]
    fn unknown_codes_round_trip_as_other() {
        let u: UpdateResponseCode = serde_json::from_value(json!("vendorQuota")).unwrap();
        assert_eq!(u, UpdateResponseCode::Other("vendorQuota".into()));
        assert_eq!(serde_json::to_value(&u).unwrap(), json!("vendorQuota"));
        let s: SubmitResponseCode = serde_json::from_value(json!("vendorQuota")).unwrap();
        assert_eq!(s, SubmitResponseCode::Other("vendorQuota".into()));
        assert_eq!(serde_json::to_value(&s).unwrap(), json!("vendorQuota"));
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = update_response();
        raw["vendor.example:hub"] = json!("a.example");
        let resp: RoomUpdateResponse = round_trip(&raw);
        assert_eq!(
            resp.extra.get("vendor.example:hub"),
            Some(&json!("a.example"))
        );
        let mut raw = send_request();
        raw["vendor.example:x"] = json!(1);
        round_trip::<RoomSendRequest>(&raw);
        let mut raw = unbind_request();
        raw["vendor.example:x"] = json!(1);
        round_trip::<RoomUnbindRequest>(&raw);
    }
}

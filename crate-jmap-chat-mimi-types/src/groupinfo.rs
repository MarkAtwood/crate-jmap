//! draft-atwood-jmap-chat-mimi-00 §GroupInfo/fetch — the request and
//! response arguments of `GroupInfo/fetch`.
//!
//! The client passes a signed MIMI GroupInfoRequest and the server returns
//! the GroupInfoResponse `status` as its name plus "The whole
//! GroupInfoResponse, unchanged" (draft-ietf-mimi-protocol-06 §5.6). The
//! GroupInfo and ratchet tree inside it are encrypted to the client, so the
//! server passes them through.

use jmap_types::{Id, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::Base64Url;

/// The `status` of a `GroupInfo/fetch` response: the GroupInfoCode of
/// draft-ietf-mimi-protocol-06 §5.6, "written as its name".
///
/// `Other` preserves any code name this version does not know, for
/// lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GroupInfoCode {
    /// The GroupInfo and ratchet tree were provided as requested.
    Success,
    /// The requester was not authorized to access the GroupInfo. A hub
    /// may also use it in place of `noSuchRoom`.
    NotAuthorized,
    /// The requested room does not exist.
    NoSuchRoom,
    /// A code name not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(GroupInfoCode, "a GroupInfoCode name",
    "success" => Success,
    "notAuthorized" => NotAuthorized,
    "noSuchRoom" => NoSuchRoom,
);

/// Request arguments of `GroupInfo/fetch`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §GroupInfo/fetch. All three
/// arguments are required.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::GroupInfoFetchRequest;
///
/// let req = GroupInfoFetchRequest::new(
///     jmap_types::Id::new_validated("u1")?,
///     "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a".into(),
///     "AAEAAQAgsJ4f".parse()?,
/// );
/// assert_eq!(
///     serde_json::to_string(&req)?,
///     r#"{"accountId":"u1","roomId":"mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a","request":"AAEAAQAgsJ4f"}"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfoFetchRequest {
    /// The account to use.
    pub account_id: Id,
    /// The `roomId` of a Chat in this account.
    pub room_id: String,
    /// A GroupInfoRequest (draft-ietf-mimi-protocol-06 §5.6) signed by the
    /// client.
    pub request: Base64Url,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GroupInfoFetchRequest {
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

/// Response arguments of `GroupInfo/fetch`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §GroupInfo/fetch. All three
/// arguments are required. `response` is the GroupInfoResponse as the hub
/// signed it; the client "MUST verify the response signature with
/// `hub_sender`" before it joins.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::{GroupInfoCode, GroupInfoFetchResponse};
///
/// let resp: GroupInfoFetchResponse = serde_json::from_str(r#"{
///     "accountId": "u1",
///     "status": "noSuchRoom",
///     "response": "AAEbbWltaTov"
/// }"#)?;
/// assert_eq!(resp.status, GroupInfoCode::NoSuchRoom);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfoFetchResponse {
    /// The account used.
    pub account_id: Id,
    /// The GroupInfoResponse `status`.
    pub status: GroupInfoCode,
    /// The whole GroupInfoResponse, unchanged.
    pub response: Base64Url,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GroupInfoFetchResponse {
    /// A response from its three required arguments, with no extras.
    pub fn new(account_id: Id, status: GroupInfoCode, response: Base64Url) -> Self {
        Self {
            account_id,
            status,
            response,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // The draft's §Examples payloads are shortened with "...". These tests
    // drop the "..." and keep the prefixes, which are themselves valid
    // unpadded base64url of 12 characters each: "AAEAAQAgsJ4f" and
    // "AAEbbWltaTov".
    const REQ: &str = "AAEAAQAgsJ4f";
    const RESP: &str = "AAEbbWltaTov";
    const ROOM: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";

    fn draft_request() -> serde_json::Value {
        json!({ "accountId": "u1", "roomId": ROOM, "request": REQ })
    }

    fn draft_response() -> serde_json::Value {
        json!({ "accountId": "u1", "status": "success", "response": RESP })
    }

    #[test]
    fn request_matches_draft_example() {
        // Oracle: draft §Examples, "A new client of Alice fetches GroupInfo
        // before an external join".
        let raw = draft_request();
        let req: GroupInfoFetchRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(req.account_id.as_ref(), "u1");
        assert_eq!(req.room_id, ROOM);
        assert_eq!(req.request.as_str(), REQ);
        assert_eq!(serde_json::to_value(&req).unwrap(), raw);
        let built = GroupInfoFetchRequest::new(
            Id::new_validated("u1").unwrap(),
            ROOM.into(),
            REQ.parse().unwrap(),
        );
        assert_eq!(serde_json::to_value(built).unwrap(), raw);
    }

    #[test]
    fn response_matches_draft_example() {
        // Oracle: draft §Examples GroupInfo/fetch response.
        let raw = draft_response();
        let resp: GroupInfoFetchResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(resp.account_id.as_ref(), "u1");
        assert_eq!(resp.status, GroupInfoCode::Success);
        assert_eq!(resp.response.as_str(), RESP);
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);
        let built = GroupInfoFetchResponse::new(
            Id::new_validated("u1").unwrap(),
            GroupInfoCode::Success,
            RESP.parse().unwrap(),
        );
        assert_eq!(serde_json::to_value(built).unwrap(), raw);
    }

    #[test]
    fn each_argument_required() {
        // Oracle: draft §GroupInfo/fetch lists the request arguments
        // accountId, roomId, request and the response arguments accountId,
        // status, response, none marked optional.
        for field in ["accountId", "roomId", "request"] {
            let mut raw = draft_request();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<GroupInfoFetchRequest>(raw).is_err(),
                "missing request {field} must be rejected"
            );
        }
        for field in ["accountId", "status", "response"] {
            let mut raw = draft_response();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<GroupInfoFetchResponse>(raw).is_err(),
                "missing response {field} must be rejected"
            );
        }
    }

    #[test]
    fn padded_byte_strings_rejected() {
        // Oracle: draft §Conventions: byte strings are the "unpadded
        // base64url encoding".
        let mut raw = draft_request();
        raw["request"] = json!("AA==");
        assert!(serde_json::from_value::<GroupInfoFetchRequest>(raw).is_err());
        let mut raw = draft_response();
        raw["response"] = json!("AA==");
        assert!(serde_json::from_value::<GroupInfoFetchResponse>(raw).is_err());
    }

    #[test]
    fn codes_match_mimi_names() {
        // Oracle: draft §GroupInfo/fetch: "(`\"success\"`,
        // `\"notAuthorized\"`, or `\"noSuchRoom\"`)", the names of
        // draft-ietf-mimi-protocol-06 §5.6 `enum { reserved(0); success(1);
        // notAuthorized(2); noSuchRoom(3); (255) } GroupInfoCode`.
        let cases = [
            ("success", GroupInfoCode::Success),
            ("notAuthorized", GroupInfoCode::NotAuthorized),
            ("noSuchRoom", GroupInfoCode::NoSuchRoom),
        ];
        for (name, code) in cases {
            assert_eq!(
                serde_json::from_value::<GroupInfoCode>(json!(name)).unwrap(),
                code
            );
            assert_eq!(serde_json::to_value(&code).unwrap(), json!(name));
        }
    }

    #[test]
    fn unknown_code_round_trips_as_other() {
        let c: GroupInfoCode = serde_json::from_value(json!("vendorRateLimited")).unwrap();
        assert_eq!(c, GroupInfoCode::Other("vendorRateLimited".into()));
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            json!("vendorRateLimited")
        );
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = draft_request();
        raw["vendor.example:x"] = json!(1);
        let req: GroupInfoFetchRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(req.extra.get("vendor.example:x"), Some(&json!(1)));
        assert_eq!(serde_json::to_value(&req).unwrap(), raw);

        let mut raw = draft_response();
        raw["vendor.example:hub"] = json!("a.example");
        let resp: GroupInfoFetchResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            resp.extra.get("vendor.example:hub"),
            Some(&json!("a.example"))
        );
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);
    }
}

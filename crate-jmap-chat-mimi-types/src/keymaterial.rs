//! draft-atwood-jmap-chat-mimi-00 §KeyMaterial/fetch — the request and
//! response arguments of `KeyMaterial/fetch`.
//!
//! The method is not a standard RFC 8620 method: the client passes a
//! signed MIMI KeyMaterialRequest and the server returns the arguments
//! "decoded from the KeyMaterialResponse" (draft-ietf-mimi-protocol-06
//! §5.2). The status codes are written as their MIMI code names.

use jmap_types::{Id, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::Base64Url;

/// The `userStatus` of a `KeyMaterial/fetch` response: the
/// KeyMaterialUserCode of draft-ietf-mimi-protocol-06 §5.2, "written as
/// its name".
///
/// `Other` preserves any code name this version does not know, for
/// lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyMaterialUserCode {
    /// Key material was provided for every client of the target user.
    Success,
    /// Key material was provided for at least one client of the target
    /// user.
    PartialSuccess,
    /// The providers or the target user's clients do not support the
    /// requested protocol.
    IncompatibleProtocol,
    /// No client could supply key material compatible with the request's
    /// `requiredCapabilities`.
    NoCompatibleMaterial,
    /// The target provider does not know the target user.
    UserUnknown,
    /// The requester has no consent to fetch the target user's key
    /// material.
    NoConsent,
    /// The target user does not consent for this room.
    NoConsentForThisRoom,
    /// The target user existed and has been deleted.
    UserDeleted,
    /// A code name not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(KeyMaterialUserCode, "a KeyMaterialUserCode name",
    "success" => Success,
    "partialSuccess" => PartialSuccess,
    "incompatibleProtocol" => IncompatibleProtocol,
    "noCompatibleMaterial" => NoCompatibleMaterial,
    "userUnknown" => UserUnknown,
    "noConsent" => NoConsent,
    "noConsentForThisRoom" => NoConsentForThisRoom,
    "userDeleted" => UserDeleted,
);

/// The `clientStatus` of a [`ClientKeyMaterial`]: the
/// KeyMaterialClientCode of draft-ietf-mimi-protocol-06 §5.2, written as
/// its name.
///
/// `Other` preserves any code name this version does not know, for
/// lossless round-trip.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyMaterialClientCode {
    /// Key material was provided for the client.
    Success,
    /// The client had no key material available.
    KeyMaterialExhausted,
    /// The client had no key material compatible with the request's
    /// `requiredCapabilities`.
    NothingCompatible,
    /// A code name not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(KeyMaterialClientCode, "a KeyMaterialClientCode name",
    "success" => Success,
    "keyMaterialExhausted" => KeyMaterialExhausted,
    "nothingCompatible" => NothingCompatible,
);

/// Request arguments of `KeyMaterial/fetch`.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §KeyMaterial/fetch. All three
/// arguments are required.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::KeyMaterialFetchRequest;
///
/// let req = KeyMaterialFetchRequest::new(
///     jmap_types::Id::new_validated("u1")?,
///     "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a".into(),
///     "AAEYbWltaTov".parse()?,
/// );
/// assert_eq!(
///     serde_json::to_string(&req)?,
///     r#"{"accountId":"u1","roomId":"mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a","request":"AAEYbWltaTov"}"#
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyMaterialFetchRequest {
    /// The account to use.
    pub account_id: Id,
    /// The `roomId` of a Chat in this account bound to the room the
    /// target user is to be added to.
    pub room_id: String,
    /// A KeyMaterialRequest (draft-ietf-mimi-protocol-06 §5.2) signed by
    /// the client.
    pub request: Base64Url,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl KeyMaterialFetchRequest {
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

/// One entry of [`KeyMaterialFetchResponse::clients`], decoded from a
/// MIMI ClientKeyMaterial.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §KeyMaterial/fetch: "`keyPackage`
/// (String, present when `clientStatus` is `\"success\"`), and
/// `clientCapabilities` (String, the MLS Capabilities, present when the
/// provider included it)". Deserialize does not check that `keyPackage`
/// is present exactly when `client_status` is
/// [`KeyMaterialClientCode::Success`]; check it before use.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientKeyMaterial {
    /// The client's status code.
    pub client_status: KeyMaterialClientCode,
    /// The URI of the client.
    pub client_uri: String,
    /// The client's KeyPackage (RFC 9420 §10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_package: Option<Base64Url>,
    /// The client's MLS Capabilities (RFC 9420 §7.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<Base64Url>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ClientKeyMaterial {
    /// An entry with no `keyPackage`, no `clientCapabilities` and no
    /// extras.
    pub fn new(client_status: KeyMaterialClientCode, client_uri: String) -> Self {
        Self {
            client_status,
            client_uri,
            key_package: None,
            client_capabilities: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `KeyMaterial/fetch`, decoded from a MIMI
/// KeyMaterialResponse.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §KeyMaterial/fetch. All four
/// arguments are required. draft-ietf-mimi-protocol-06 §5.2 forbids
/// populating the clients list for user codes other than `success`,
/// `partialSuccess` and `noCompatibleMaterial`, so `clients` is then
/// empty.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::{KeyMaterialFetchResponse, KeyMaterialUserCode};
///
/// let resp: KeyMaterialFetchResponse = serde_json::from_str(r#"{
///     "accountId": "u1",
///     "userStatus": "userUnknown",
///     "userUri": "mimi://b.example/u/bob",
///     "clients": []
/// }"#)?;
/// assert_eq!(resp.user_status, KeyMaterialUserCode::UserUnknown);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyMaterialFetchResponse {
    /// The account used.
    pub account_id: Id,
    /// The user status code.
    pub user_status: KeyMaterialUserCode,
    /// The target user URI.
    pub user_uri: String,
    /// One entry per ClientKeyMaterial.
    pub clients: Vec<ClientKeyMaterial>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl KeyMaterialFetchResponse {
    /// A response from its four required arguments, with no extras.
    pub fn new(
        account_id: Id,
        user_status: KeyMaterialUserCode,
        user_uri: String,
        clients: Vec<ClientKeyMaterial>,
    ) -> Self {
        Self {
            account_id,
            user_status,
            user_uri,
            clients,
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
    // unpadded base64url: "AAEYbWltaTov" (12 characters) and
    // "AAEAAkBBy2c" (11 characters, final 'c' = 011100 with pad bits 00).
    const REQ: &str = "AAEYbWltaTov";
    const KP: &str = "AAEAAkBBy2c";
    const ROOM: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";

    fn draft_response() -> serde_json::Value {
        json!({
            "accountId": "u1",
            "userStatus": "partialSuccess",
            "userUri": "mimi://b.example/u/bob",
            "clients": [
                { "clientStatus": "success",
                  "clientUri": "mimi://b.example/d/ClientB1",
                  "keyPackage": KP },
                { "clientStatus": "keyMaterialExhausted",
                  "clientUri": "mimi://b.example/d/ClientB2" }
            ]
        })
    }

    #[test]
    fn request_matches_draft_example() {
        // Oracle: draft §Examples, "Alice's client fetches KeyPackages for
        // Bob".
        let raw = json!({ "accountId": "u1", "roomId": ROOM, "request": REQ });
        let req: KeyMaterialFetchRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(req.account_id.as_ref(), "u1");
        assert_eq!(req.room_id, ROOM);
        assert_eq!(req.request.as_str(), REQ);
        assert_eq!(serde_json::to_value(&req).unwrap(), raw);
        let built = KeyMaterialFetchRequest::new(
            Id::new_validated("u1").unwrap(),
            ROOM.into(),
            REQ.parse().unwrap(),
        );
        assert_eq!(serde_json::to_value(built).unwrap(), raw);
    }

    #[test]
    fn each_request_argument_required() {
        // Oracle: draft §KeyMaterial/fetch lists accountId, roomId and
        // request with no "optional".
        for field in ["accountId", "roomId", "request"] {
            let mut raw = json!({ "accountId": "u1", "roomId": ROOM, "request": REQ });
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<KeyMaterialFetchRequest>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn padded_request_rejected() {
        // Oracle: draft §Conventions: byte strings are the "unpadded
        // base64url encoding".
        let raw = json!({ "accountId": "u1", "roomId": ROOM, "request": "AA==" });
        assert!(serde_json::from_value::<KeyMaterialFetchRequest>(raw).is_err());
    }

    #[test]
    fn response_matches_draft_example() {
        // Oracle: draft §Examples KeyMaterial/fetch response.
        let raw = draft_response();
        let resp: KeyMaterialFetchResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(resp.account_id.as_ref(), "u1");
        assert_eq!(resp.user_status, KeyMaterialUserCode::PartialSuccess);
        assert_eq!(resp.user_uri, "mimi://b.example/u/bob");
        assert_eq!(resp.clients.len(), 2);
        let b1 = &resp.clients[0];
        assert_eq!(b1.client_status, KeyMaterialClientCode::Success);
        assert_eq!(b1.client_uri, "mimi://b.example/d/ClientB1");
        assert_eq!(b1.key_package.as_ref().unwrap().as_str(), KP);
        assert_eq!(b1.client_capabilities, None);
        let b2 = &resp.clients[1];
        assert_eq!(
            b2.client_status,
            KeyMaterialClientCode::KeyMaterialExhausted
        );
        assert_eq!(b2.client_uri, "mimi://b.example/d/ClientB2");
        assert_eq!(b2.key_package, None);
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);

        let mut built_b1 = ClientKeyMaterial::new(
            KeyMaterialClientCode::Success,
            "mimi://b.example/d/ClientB1".into(),
        );
        built_b1.key_package = Some(KP.parse().unwrap());
        let built = KeyMaterialFetchResponse::new(
            Id::new_validated("u1").unwrap(),
            KeyMaterialUserCode::PartialSuccess,
            "mimi://b.example/u/bob".into(),
            vec![
                built_b1,
                ClientKeyMaterial::new(
                    KeyMaterialClientCode::KeyMaterialExhausted,
                    "mimi://b.example/d/ClientB2".into(),
                ),
            ],
        );
        assert_eq!(serde_json::to_value(built).unwrap(), raw);
    }

    #[test]
    fn each_response_argument_required() {
        // Oracle: draft §KeyMaterial/fetch response lists accountId,
        // userStatus, userUri and clients with no "optional".
        for field in ["accountId", "userStatus", "userUri", "clients"] {
            let mut raw = draft_response();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<KeyMaterialFetchResponse>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn client_status_and_uri_required() {
        for field in ["clientStatus", "clientUri"] {
            let mut raw = json!({ "clientStatus": "success", "clientUri": "mimi://b.example/d/C" });
            raw.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<ClientKeyMaterial>(raw).is_err());
        }
    }

    #[test]
    fn client_capabilities_round_trips() {
        // Oracle: draft §KeyMaterial/fetch: `clientCapabilities` (String,
        // the MLS Capabilities, present when the provider included it);
        // MIMI-protocol-06 §5.2 carries it for `nothingCompatible`.
        let raw = json!({
            "clientStatus": "nothingCompatible",
            "clientUri": "mimi://b.example/d/ClientB3",
            "clientCapabilities": "AAEC"
        });
        let c: ClientKeyMaterial = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(c.client_status, KeyMaterialClientCode::NothingCompatible);
        assert_eq!(c.client_capabilities.as_ref().unwrap().as_str(), "AAEC");
        assert_eq!(serde_json::to_value(&c).unwrap(), raw);
    }

    #[test]
    fn padded_client_byte_strings_rejected() {
        for field in ["keyPackage", "clientCapabilities"] {
            let mut raw = json!({ "clientStatus": "success", "clientUri": "mimi://b.example/d/C" });
            raw[field] = json!("AA==");
            assert!(serde_json::from_value::<ClientKeyMaterial>(raw).is_err());
        }
    }

    #[test]
    fn user_codes_match_mimi_names() {
        // Oracle: draft-ietf-mimi-protocol-06 §5.2 `enum { success(0);
        // partialSuccess(1); incompatibleProtocol(2);
        // noCompatibleMaterial(3); userUnknown(4); noConsent(5);
        // noConsentForThisRoom(6); userDeleted(7); (255) }
        // KeyMaterialUserCode`.
        let cases = [
            ("success", KeyMaterialUserCode::Success),
            ("partialSuccess", KeyMaterialUserCode::PartialSuccess),
            (
                "incompatibleProtocol",
                KeyMaterialUserCode::IncompatibleProtocol,
            ),
            (
                "noCompatibleMaterial",
                KeyMaterialUserCode::NoCompatibleMaterial,
            ),
            ("userUnknown", KeyMaterialUserCode::UserUnknown),
            ("noConsent", KeyMaterialUserCode::NoConsent),
            (
                "noConsentForThisRoom",
                KeyMaterialUserCode::NoConsentForThisRoom,
            ),
            ("userDeleted", KeyMaterialUserCode::UserDeleted),
        ];
        for (name, code) in cases {
            assert_eq!(
                serde_json::from_value::<KeyMaterialUserCode>(json!(name)).unwrap(),
                code
            );
            assert_eq!(serde_json::to_value(&code).unwrap(), json!(name));
        }
    }

    #[test]
    fn client_codes_match_mimi_names() {
        // Oracle: draft-ietf-mimi-protocol-06 §5.2 `enum { success(0);
        // keyMaterialExhausted(1), nothingCompatible(2), (255) }
        // KeyMaterialClientCode`.
        let cases = [
            ("success", KeyMaterialClientCode::Success),
            (
                "keyMaterialExhausted",
                KeyMaterialClientCode::KeyMaterialExhausted,
            ),
            (
                "nothingCompatible",
                KeyMaterialClientCode::NothingCompatible,
            ),
        ];
        for (name, code) in cases {
            assert_eq!(
                serde_json::from_value::<KeyMaterialClientCode>(json!(name)).unwrap(),
                code
            );
            assert_eq!(serde_json::to_value(&code).unwrap(), json!(name));
        }
    }

    #[test]
    fn unknown_codes_round_trip_as_other() {
        let u: KeyMaterialUserCode = serde_json::from_value(json!("vendorRateLimited")).unwrap();
        assert_eq!(u, KeyMaterialUserCode::Other("vendorRateLimited".into()));
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            json!("vendorRateLimited")
        );
        let c: KeyMaterialClientCode = serde_json::from_value(json!("vendorRevoked")).unwrap();
        assert_eq!(c, KeyMaterialClientCode::Other("vendorRevoked".into()));
        assert_eq!(serde_json::to_value(&c).unwrap(), json!("vendorRevoked"));
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = draft_response();
        raw["vendor.example:hub"] = json!("a.example");
        raw["clients"][0]["vendor.example:label"] = json!("phone");
        let resp: KeyMaterialFetchResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            resp.extra.get("vendor.example:hub"),
            Some(&json!("a.example"))
        );
        assert_eq!(
            resp.clients[0].extra.get("vendor.example:label"),
            Some(&json!("phone"))
        );
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);

        let raw =
            json!({ "accountId": "u1", "roomId": ROOM, "request": REQ, "vendor.example:x": 1 });
        let req: KeyMaterialFetchRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&req).unwrap(), raw);
    }
}

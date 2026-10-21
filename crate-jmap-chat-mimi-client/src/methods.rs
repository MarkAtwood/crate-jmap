//! The draft-atwood-jmap-chat-mimi-00 §KeyPackages, §KeyMaterial/fetch
//! and §Rooms method bindings on [`SessionClient`].
//!
//! Each method builds its arguments from the `jmap-chat-mimi-types`
//! request type, sends a single-method request, and decodes the matching
//! response type. `KeyPackage/get` and `KeyPackage/set` are standard
//! RFC 8620 methods; the others are not.

use std::collections::BTreeMap;

use jmap_chat_mimi_types::{
    Base64Url, GroupInfoFetchRequest, GroupInfoFetchResponse, KeyMaterialFetchRequest,
    KeyMaterialFetchResponse, KeyPackage, KeyPackageCreate, KeyPackageCreated, MimiCapability,
    RoomCreateRequest, RoomCreateResponse, RoomSendRequest, RoomSendResponse, RoomUnbindRequest,
    RoomUnbindResponse, RoomUpdateRequest, RoomUpdateResponse, JMAP_CHAT_MIMI_URI,
};
use jmap_types::{GetResponse, Id, SetResponse};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// The call-id embedded in every single-method request this crate sends.
const CALL_ID: &str = "r1";

/// The `using` array of every request. The MIMI capability "is meaningful
/// only when `urn:ietf:params:jmap:chat` is also present" (draft
/// §Capability).
const USING_MIMI: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:chat",
    JMAP_CHAT_MIMI_URI,
];

/// A `JmapClient` bound to a JMAP session, carrying the MIMI provider
/// methods.
///
/// Obtain via [`JmapMimiExt::with_mimi_session`](crate::JmapMimiExt::with_mimi_session).
/// Every method uses the session's primary account for
/// `urn:ietf:params:jmap:chat:mimi`.
///
/// # Errors
///
/// Every method returns
/// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
/// when the session has no primary account for
/// `urn:ietf:params:jmap:chat:mimi`, and otherwise any error of
/// [`JmapClient::call`](jmap_base_client::JmapClient::call) or
/// [`extract_response`](jmap_base_client::extract_response), including
/// [`MethodError`](jmap_base_client::ClientError::MethodError) for the
/// draft's `invalidArguments` and `serverUnavailable`.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            // The inner JmapClient is not Debug.
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .finish()
    }
}

impl SessionClient {
    /// Borrow the underlying [`JmapClient`](jmap_base_client::JmapClient).
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// Borrow the captured [`Session`](jmap_base_client::Session).
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// The primary account id for `urn:ietf:params:jmap:chat:mimi`.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// when the session has no such primary account.
    pub fn mimi_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        self.session
            .primary_account_id(JMAP_CHAT_MIMI_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:chat:mimi".into(),
                )
            })
    }

    /// The account-level `urn:ietf:params:jmap:chat:mimi` capability object
    /// of the primary MIMI account (draft §Account-Level Capability Object),
    /// whose `userUri` [`account_has_left`](crate::account_has_left) takes.
    ///
    /// Returns `Ok(None)` when the account carries no such capability.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// as for [`Self::mimi_account_id`], or
    /// [`ClientError::Parse`](jmap_base_client::ClientError::Parse) when the
    /// capability object is malformed.
    pub fn mimi_capability(&self) -> Result<Option<MimiCapability>, jmap_base_client::ClientError> {
        let account_id = self.mimi_account_id()?;
        match self.session.accounts.get(account_id) {
            Some(account) => account.account_extension_capability(JMAP_CHAT_MIMI_URI),
            None => Ok(None),
        }
    }

    /// `KeyPackage/get` (draft §KeyPackage/get, RFC 8620 §5.1): fetch the
    /// account's KeyPackage records by id, or all of them when `ids` is
    /// `None`.
    ///
    /// Every property is requested: [`KeyPackage`] has no optional field,
    /// so a partial object would not decode.
    pub async fn key_package_get(
        &self,
        ids: Option<&[Id]>,
    ) -> Result<GetResponse<KeyPackage>, jmap_base_client::ClientError> {
        let mut args = serde_json::json!({ "accountId": self.mimi_account_id()? });
        if let Some(ids) = ids {
            args["ids"] =
                serde_json::to_value(ids).map_err(jmap_base_client::ClientError::from_serialize)?;
        }
        self.call("KeyPackage/get", args).await
    }

    /// `KeyPackage/set` (draft §KeyPackage/set, RFC 8620 §5.3): publish
    /// the KeyPackages in `create`, keyed by creation id, and destroy the
    /// records in `destroy`. An empty `create` or `destroy` is omitted.
    ///
    /// There is no `update`: the server "MUST reject every update". A
    /// rejected create (`invalidProperties`, `overQuota`) is in the
    /// response's `not_created`, not an error here.
    pub async fn key_package_set(
        &self,
        create: &BTreeMap<String, KeyPackageCreate>,
        destroy: &[Id],
    ) -> Result<SetResponse<KeyPackageCreated>, jmap_base_client::ClientError> {
        let mut args = serde_json::json!({ "accountId": self.mimi_account_id()? });
        if !create.is_empty() {
            args["create"] = serde_json::to_value(create)
                .map_err(jmap_base_client::ClientError::from_serialize)?;
        }
        if !destroy.is_empty() {
            args["destroy"] = serde_json::to_value(destroy)
                .map_err(jmap_base_client::ClientError::from_serialize)?;
        }
        self.call("KeyPackage/set", args).await
    }

    /// `KeyMaterial/fetch` (draft §KeyMaterial/fetch): obtain KeyPackages
    /// for another user's clients, to add that user to the room bound to a
    /// Chat in this account.
    ///
    /// `request` is a KeyMaterialRequest signed by the client. The
    /// provider's verdict is in the response's `user_status` and each
    /// client's `client_status`.
    pub async fn key_material_fetch(
        &self,
        room_id: &str,
        request: Base64Url,
    ) -> Result<KeyMaterialFetchResponse, jmap_base_client::ClientError> {
        let account_id = self.account()?;
        self.call(
            "KeyMaterial/fetch",
            KeyMaterialFetchRequest::new(account_id, room_id.to_owned(), request),
        )
        .await
    }

    /// `Room/create` (draft §Room/create): create a room with the server
    /// as hub, and a Chat bound to it, from the client's one-member MLS
    /// group.
    ///
    /// `group_info` is an MLSMessage of wire format `mls_group_info`
    /// without the `ratchet_tree` extension; `ratchet_tree` is the group's
    /// ratchet tree.
    pub async fn room_create(
        &self,
        group_info: Base64Url,
        ratchet_tree: Base64Url,
    ) -> Result<RoomCreateResponse, jmap_base_client::ClientError> {
        let account_id = self.account()?;
        self.call(
            "Room/create",
            RoomCreateRequest::new(account_id, group_info, ratchet_tree),
        )
        .await
    }

    /// `Room/update` (draft §Room/update): submit an UpdateRequest (a
    /// proposal or commit) for the room bound to a Chat in this account.
    ///
    /// The hub's verdict is in the response's `responseCode`; a refusal by
    /// the hub is not an error here.
    pub async fn room_update(
        &self,
        room_id: &str,
        request: Base64Url,
    ) -> Result<RoomUpdateResponse, jmap_base_client::ClientError> {
        let account_id = self.account()?;
        self.call(
            "Room/update",
            RoomUpdateRequest::new(account_id, room_id.to_owned(), request),
        )
        .await
    }

    /// `Room/send` (draft §Room/send): submit an application message, an
    /// MLSMessage of wire format PrivateMessage, to the room's hub.
    ///
    /// The hub's verdict is in the response's `responseCode`.
    pub async fn room_send(
        &self,
        room_id: &str,
        message: Base64Url,
    ) -> Result<RoomSendResponse, jmap_base_client::ClientError> {
        let account_id = self.account()?;
        self.call(
            "Room/send",
            RoomSendRequest::new(account_id, room_id.to_owned(), message),
        )
        .await
    }

    /// `Room/unbind` (draft §Room/unbind): tell the server that the
    /// account has left the room's MLS group.
    ///
    /// Call it after processing a commit for which
    /// [`account_has_left`](crate::account_has_left) holds. The server does
    /// not check this against the MLS group: unbinding while other clients
    /// of the account remain in the group cuts them off from the room
    /// (draft §Security Considerations).
    pub async fn room_unbind(
        &self,
        room_id: &str,
    ) -> Result<RoomUnbindResponse, jmap_base_client::ClientError> {
        let account_id = self.account()?;
        self.call(
            "Room/unbind",
            RoomUnbindRequest::new(account_id, room_id.to_owned()),
        )
        .await
    }

    /// `GroupInfo/fetch` (draft §GroupInfo/fetch): obtain the GroupInfo
    /// and ratchet tree of the room's MLS group for an external join.
    ///
    /// `request` is a GroupInfoRequest signed by the client. Check the
    /// returned `response` with
    /// [`verify_group_info_response`](crate::verify_group_info_response)
    /// and the decrypted GroupInfo with
    /// [`group_info_names_hub_sender`](crate::group_info_names_hub_sender)
    /// before joining.
    pub async fn group_info_fetch(
        &self,
        room_id: &str,
        request: Base64Url,
    ) -> Result<GroupInfoFetchResponse, jmap_base_client::ClientError> {
        let account_id = self.account()?;
        self.call(
            "GroupInfo/fetch",
            GroupInfoFetchRequest::new(account_id, room_id.to_owned(), request),
        )
        .await
    }

    fn account(&self) -> Result<Id, jmap_base_client::ClientError> {
        Ok(Id::from(self.mimi_account_id()?))
    }

    async fn call<A: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        args: A,
    ) -> Result<R, jmap_base_client::ClientError> {
        let args =
            serde_json::to_value(args).map_err(jmap_base_client::ClientError::from_serialize)?;
        let using = USING_MIMI.iter().map(|&s| s.to_owned()).collect();
        let req = jmap_types::JmapRequest::new(
            using,
            vec![jmap_types::Invocation::new(method, args, CALL_ID)],
            None,
        );
        let resp = self
            .client
            .call(self.session.api_url.as_str(), &req)
            .await?;
        jmap_base_client::extract_response(&resp, CALL_ID)
    }
}

/// Compile-time assertion that [`SessionClient`] is `Send + Sync`.
#[allow(dead_code)]
fn _assert_session_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionClient>();
}

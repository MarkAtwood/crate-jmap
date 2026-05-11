//! Miscellaneous JMAP Chat method implementations on SessionClient.
//!
//! Covers the smaller object surfaces — `ReadPosition/*`, `PresenceStatus/*`,
//! and `PushSubscription/*` (RFC 8620 §7.2 plus the JMAP Chat Push extension,
//! draft-atwood-jmap-chat-push-00).

use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, GetResponse, PresenceStatusPatch, PushSubscriptionCreateInput,
    PushSubscriptionCreateResponse, PushSubscriptionPatch, SetResponse,
};

impl super::SessionClient {
    /// Fetch ReadPosition objects by IDs (JMAP Chat §5 ReadPosition/get).
    ///
    /// If `ids` is `None`, returns all ReadPosition records for the account.
    /// The server creates one ReadPosition per Chat automatically.
    pub async fn read_position_get(
        &self,
        ids: Option<&[Id]>,
    ) -> Result<GetResponse<jmap_chat_types::ReadPosition>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` when None — see the matching comment on `chat_get` for
        // the rationale. ReadPosition/get has no `properties` parameter.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        let req = super::build_request("ReadPosition/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update the read position for a Chat (JMAP Chat §5 ReadPosition/set).
    ///
    /// `read_position_id` is the server-assigned ReadPosition.id (from
    /// `read_position_get`). `last_read_message_id` is the Message.id of the
    /// most recent message read. The server updates `lastReadAt` and
    /// recomputes `Chat.unreadCount`.
    ///
    /// `create` and `destroy` are forbidden by the spec; only `update` is issued.
    pub async fn read_position_update(
        &self,
        read_position_id: &Id,
        last_read_message_id: &Id,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "update": {
                read_position_id.as_ref(): { "lastReadMessageId": last_read_message_id }
            },
        });
        let req = super::build_request("ReadPosition/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch the singleton PresenceStatus record (JMAP Chat §5 PresenceStatus/get).
    ///
    /// Per spec there is exactly one PresenceStatus per account; `ids: null`
    /// retrieves it.
    pub async fn presence_status_get(
        &self,
    ) -> Result<GetResponse<jmap_chat_types::PresenceStatus>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": None::<&[Id]>,
        });
        let req = super::build_request("PresenceStatus/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ReadPosition records since `since_state` (JMAP Chat §5 ReadPosition/changes).
    ///
    /// `max_changes` may be `None` to let the server choose the limit (RFC 8620 §5.2).
    pub async fn read_position_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "read_position_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("ReadPosition/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update the PresenceStatus record (JMAP Chat §5 PresenceStatus/set).
    ///
    /// Only `update` is issued; `create` and `destroy` are forbidden by the spec.
    /// Fields absent from `patch` (i.e. `Patch::Keep` or `None`) are omitted from
    /// the patch and left unchanged server-side.
    pub async fn presence_status_update(
        &self,
        id: &Id,
        patch: &PresenceStatusPatch<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut patch_map = serde_json::Map::new();
        if let Some(p) = &patch.presence {
            patch_map.insert(
                "presence".into(),
                serde_json::to_value(p).map_err(jmap_base_client::ClientError::Parse)?,
            );
        }
        if let Some(entry) = patch
            .status_text
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("statusText".into(), entry);
        }
        if let Some(entry) = patch
            .status_emoji
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("statusEmoji".into(), entry);
        }
        if let Some(entry) = patch
            .expires_at
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("expiresAt".into(), entry);
        }
        if let Some(rs) = patch.receipt_sharing {
            patch_map.insert("receiptSharing".into(), rs.into());
        }
        // Wrap the constructed map in a PatchObject (RFC 8620 §5.3) before
        // serializing. Wire bytes are unchanged because PatchObject is
        // #[serde(transparent)]; the typed boundary documents the contract.
        let patch_value = serde_json::Value::Object(PatchObject::from_map(patch_map).into_inner());
        let args = serde_json::json!({
            "accountId": account_id,
            "update": { id.as_ref(): patch_value },
        });
        let req = super::build_request("PresenceStatus/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to PresenceStatus records since `since_state` (JMAP Chat §5 PresenceStatus/changes).
    ///
    /// `max_changes` may be `None` to let the server choose the limit (RFC 8620 §5.2).
    pub async fn presence_status_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "presence_status_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("PresenceStatus/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a PushSubscription with the optional `chatPush` extension
    /// (RFC 8620 §7.2 / draft-atwood-jmap-chat-push-00 §3).
    ///
    /// PushSubscriptions are account-independent: no `accountId` is included
    /// in the request (RFC 8620 §7.2). When `input.chat_push` is `Some`, the
    /// `using` array includes `urn:ietf:params:jmap:chat:push` (RFC 8620 §3.3:
    /// capabilities MUST only be declared when used); otherwise `urn:ietf:params:jmap:core`
    /// alone is used.
    ///
    /// This method issues a `create` operation only. To extend `expires`, set
    /// the verification code, change `types`, or update `chatPush`, use
    /// [`push_subscription_update`](Self::push_subscription_update). To
    /// unsubscribe, use [`push_subscription_destroy`](Self::push_subscription_destroy).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
    pub async fn push_subscription_create(
        &self,
        input: &PushSubscriptionCreateInput<'_>,
    ) -> Result<PushSubscriptionCreateResponse, jmap_base_client::ClientError> {
        if input.device_client_id.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "push_subscription_create: device_client_id may not be empty".into(),
            ));
        }
        if input.url.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "push_subscription_create: url may not be empty".into(),
            ));
        }
        // PushSubscriptions are not account-scoped; use api_url without session_parts().
        let api_url = self.api_url();
        let client_id = super::resolve_client_id(input.client_id);
        let mut create_obj = serde_json::json!({
            "deviceClientId": input.device_client_id,
            "url": input.url,
        });
        if let Some(exp) = input.expires {
            create_obj["expires"] = exp.as_ref().into();
        }
        if let Some(types) = input.types {
            create_obj["types"] = serde_json::Value::Array(
                types.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let has_chat_push = input.chat_push.is_some();
        if let Some(cp) = input.chat_push {
            let mut seen = std::collections::HashSet::new();
            for (account_id, _) in cp {
                if !seen.insert(account_id) {
                    return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                        "push_subscription_create: duplicate accountId '{}' in chat_push",
                        account_id
                    )));
                }
            }
            let mut chat_push_map = serde_json::Map::new();
            for (account_id, config) in cp {
                chat_push_map.insert(
                    account_id.as_ref().to_owned(),
                    serde_json::to_value(config).map_err(jmap_base_client::ClientError::Parse)?,
                );
            }
            create_obj["chatPush"] = serde_json::Value::Object(chat_push_map);
        }
        let args = serde_json::json!({
            "create": { client_id: create_obj }
        });
        // RFC 8620 §3.3: only declare the chatPush capability when it is actually used.
        let using = if has_chat_push {
            super::USING_CHAT_PUSH
        } else {
            super::USING_CORE
        };
        let req = super::build_request("PushSubscription/set", args, using);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update a PushSubscription (RFC 8620 §7.2.2 `PushSubscription/set` update).
    ///
    /// Issues a `PushSubscription/set` request with only the `update` sub-map
    /// populated. RFC 8620 §7.2 declares `url`, `keys`, and `deviceClientId`
    /// immutable; to change those, destroy the subscription and create a new
    /// one. The patchable properties are exposed via [`PushSubscriptionPatch`]:
    /// `verificationCode`, `expires`, `types`, and the JMAP Chat Push
    /// extension's `chatPush`.
    ///
    /// PushSubscriptions are not account-scoped (RFC 8620 §7.2): no
    /// `accountId` is sent. When the patch touches `chat_push` or
    /// `clear_chat_push`, the `using` array includes
    /// `urn:ietf:params:jmap:chat:push`; otherwise only
    /// `urn:ietf:params:jmap:core` is declared (RFC 8620 §3.3).
    ///
    /// Returns [`jmap_base_client::ClientError::InvalidArgument`] if `id` is
    /// empty, if both `patch.types` and `patch.clear_types` are set, or if
    /// both `patch.chat_push` and `patch.clear_chat_push` are set.
    pub async fn push_subscription_update(
        &self,
        id: &Id,
        patch: &PushSubscriptionPatch<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: typed &Id does not prevent empty Id values.
        if id.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "push_subscription_update: id may not be empty".into(),
            ));
        }
        if patch.types.is_some() && patch.clear_types {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "push_subscription_update: types and clear_types are mutually exclusive".into(),
            ));
        }
        if patch.chat_push.is_some() && patch.clear_chat_push {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "push_subscription_update: chat_push and clear_chat_push are mutually exclusive"
                    .into(),
            ));
        }

        let api_url = self.api_url();
        let mut patch_map = serde_json::Map::new();
        if let Some(code) = patch.verification_code {
            patch_map.insert("verificationCode".into(), code.into());
        }
        if let Some(entry) = patch
            .expires
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("expires".into(), entry);
        }
        if let Some(types) = patch.types {
            patch_map.insert(
                "types".into(),
                serde_json::Value::Array(
                    types.iter().copied().map(serde_json::Value::from).collect(),
                ),
            );
        } else if patch.clear_types {
            patch_map.insert("types".into(), serde_json::Value::Null);
        }
        if let Some(cp) = patch.chat_push {
            let mut seen = std::collections::HashSet::new();
            for (account_id, _) in cp {
                if !seen.insert(account_id) {
                    return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                        "push_subscription_update: duplicate accountId '{}' in chat_push",
                        account_id
                    )));
                }
            }
            let mut chat_push_map = serde_json::Map::new();
            for (account_id, config) in cp {
                chat_push_map.insert(
                    account_id.as_ref().to_owned(),
                    serde_json::to_value(config).map_err(jmap_base_client::ClientError::Parse)?,
                );
            }
            patch_map.insert("chatPush".into(), serde_json::Value::Object(chat_push_map));
        } else if patch.clear_chat_push {
            patch_map.insert("chatPush".into(), serde_json::Value::Null);
        }

        let patch_value = serde_json::Value::Object(PatchObject::from_map(patch_map).into_inner());
        let args = serde_json::json!({
            "update": { id.as_ref(): patch_value }
        });
        let using = if patch.chat_push.is_some() || patch.clear_chat_push {
            super::USING_CHAT_PUSH
        } else {
            super::USING_CORE
        };
        let req = super::build_request("PushSubscription/set", args, using);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy one or more PushSubscriptions (RFC 8620 §7.2.2 `PushSubscription/set` destroy).
    ///
    /// Issues a `PushSubscription/set` request with only the `destroy` array
    /// populated. PushSubscriptions are not account-scoped (RFC 8620 §7.2):
    /// no `accountId` is sent. Only `urn:ietf:params:jmap:core` is declared
    /// — destroying never requires the chatPush capability since it is a
    /// property-blind operation.
    ///
    /// Returns [`jmap_base_client::ClientError::InvalidArgument`] if `ids` is
    /// empty (a destroy call with no ids would be a no-op round-trip).
    ///
    /// Clients SHOULD NOT destroy a PushSubscription they did not create —
    /// RFC 8620 §7.2 reserves that to clients that recognise the
    /// `deviceClientId`. This client does not enforce that rule; the server
    /// may reject the call.
    pub async fn push_subscription_destroy(
        &self,
        ids: &[Id],
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "push_subscription_destroy: ids may not be empty".into(),
            ));
        }
        let api_url = self.api_url();
        let args = serde_json::json!({
            "destroy": ids,
        });
        let req = super::build_request("PushSubscription/set", args, super::USING_CORE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

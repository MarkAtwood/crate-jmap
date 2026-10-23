//! JMAP Chat — Message/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_CHAT)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.
//!
//! SPECIAL: `message_create` additionally inspects `SetResponse.not_created` for
//! [`SetErrorType::RateLimit`](jmap_types::SetErrorType::RateLimit) (wire `"rateLimit"`)
//! and surfaces it as `ClientError::RateLimited`.

use jmap_types::{Filter, Id, PatchObject, State};
use serde::Serialize;

use super::{
    ChangesResponse, GetResponse, MessageCreateInput, MessagePatch, PartialObject,
    QueryChangesResponse, QueryResponse, ReactionChange, SetResponse,
};

/// Reject a `sender_reaction_id` that is empty or contains RFC 6901
/// JSON Pointer special characters (`/` or `~`).
///
/// Shared by both `ReactionChange::Add` and `ReactionChange::Remove`
/// arms of `message_update`; see the rustdoc on
/// [`ReactionChange`](super::ReactionChange) for the underlying
/// JSON-Pointer construction rule.
fn validate_sender_reaction_id(id: &str) -> Result<(), jmap_base_client::ClientError> {
    if id.is_empty() {
        return Err(jmap_base_client::ClientError::InvalidArgument(
            "message_update: sender_reaction_id may not be empty".into(),
        ));
    }
    if id.contains('/') || id.contains('~') {
        return Err(jmap_base_client::ClientError::InvalidArgument(
            "message_update: sender_reaction_id must not contain '/' or '~' \
             (RFC 6901 JSON Pointer special characters)"
                .into(),
        ));
    }
    Ok(())
}

impl super::SessionClient {
    /// Fetch Message objects by IDs (RFC 8620 §5.1 / JMAP Chat §Message/get).
    ///
    /// Pass `ids: None` to fetch every Message in the account
    /// (RFC 8620 §5.1: "If null, then all records of the data type are
    /// returned"); the server bounds the result with `maxObjectsInGet` and
    /// `requestTooLarge`. Pass `properties: None` to return all fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn message_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::Message>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `chat_get` for the rationale.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice)
                .map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        let req = super::build_request("Message/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Message IDs within a Chat (RFC 8620 §5.5 / JMAP Chat §Message/query).
    ///
    /// `filter` is an RFC 8620 §5.5 `Filter`: a bare condition
    /// (`Filter::Condition`) or a `FilterOperator` combining sub-filters
    /// with `AND` / `OR` / `NOT`. The condition type `C` is normally
    /// [`MessageFilterCondition`](jmap_chat_types::MessageFilterCondition); use
    /// `jmap_metadata_types::WithMetadata<MessageFilterCondition>` to add the
    /// draft-ietf-jmap-metadata-04 §3.6 `metadata*` conditions. Per spec
    /// the filter must include a `chatId` condition unless it includes
    /// `hasMention: true`; servers return `unsupportedFilter` otherwise.
    ///
    /// `sort: None` omits `sort`, so the server applies the
    /// draft-atwood-jmap-chat-00 §Message/query default: `receivedAt`
    /// ascending when the filter has `chatId`, descending otherwise. To get
    /// the N most recent messages of a chat with `position: 0, limit: N`,
    /// pass a descending `receivedAt` comparator. The spec forbids ordering
    /// by `sentAt`.
    ///
    /// `params` selects the result window
    /// ([`QueryParams`](jmap_base_client::QueryParams), RFC 8620 §5.5);
    /// when `params.anchor` is set the server ignores `params.position`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `filter` is `None`: an unfiltered query has neither `chatId`
    ///   nor `hasMention: true`, so the server must reject it.
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing `filter` or `sort` fails (a caller-supplied condition
    ///   type `C` whose `Serialize` impl errors).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::message_get`]. RFC 8620
    ///   §5.5 defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    #[allow(clippy::too_many_arguments)] // RFC 8620 §5.5 /query args
    pub async fn message_query<C: Serialize>(
        &self,
        filter: Option<&Filter<C>>,
        sort: Option<&[jmap_chat_types::MessageComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = message_query_args(account_id, filter, sort, params)?;
        let req = super::build_request("Message/query", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Message objects since `since_state` (RFC 8620 §5.2 / Message/changes).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful and would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::message_get`].
    pub async fn message_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "message_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Message/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create (send) a new Message (RFC 8620 §5.3 / JMAP Chat §Message/set).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
    /// The server maps the creation key to the server-assigned Message id in
    /// `SetResponse.created`.
    ///
    /// # Rate limiting
    ///
    /// A channel in slow mode rejects the create with the RFC 8620 §5.3
    /// `rateLimit` SetError in `not_created` (draft-atwood-jmap-chat-00
    /// §Chat `slowModeSeconds`). This method returns
    /// `Err(ClientError::RateLimited)` for it. `retry_after` is the
    /// `serverRetryAfter` timestamp when the server supplies one; RFC 8620
    /// does not define that field, so when it is absent `retry_after` is
    /// the time the response was received and the caller should apply its
    /// own backoff, as for an HTTP 429 without `Retry-After`.
    ///
    /// # Return value
    ///
    /// Returns `Err(ClientError::RateLimited)` when the server returns a
    /// `rateLimit` set error for the creation key.
    ///
    /// For all other server-side rejections (e.g., `invalidProperties`, `forbidden`),
    /// this method returns `Ok(set_resp)` with the error recorded in
    /// `set_resp.not_created`. **Callers MUST inspect `not_created` on every `Ok`
    /// response to confirm the message was actually created.**
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing the typed `body_type` enum or the `attachments`,
    ///   `mentions`, `broadcastMentions` or `actions` arrays fails
    ///   (pathological conditions only).
    /// - [`ClientError::RateLimited`](jmap_base_client::ClientError::RateLimited)
    ///   if the server rejects the message with
    ///   [`SetErrorType::RateLimit`](jmap_types::SetErrorType::RateLimit)
    ///   (wire `"rateLimit"`). The `retry_after` field carries the
    ///   `serverRetryAfter` deadline, or the receive time when it is absent.
    /// - [`ClientError::UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse)
    ///   if the server emits a `rateLimit` SetError whose
    ///   `serverRetryAfter` is not a valid `UTCDate`.
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::message_get`]. All other
    ///   per-creation failures (e.g. `invalidProperties`, `forbidden`)
    ///   appear in [`SetResponse::not_created`] on a successful
    ///   [`Ok`] response (see the "Return value" note above).
    pub async fn message_create(
        &self,
        input: &MessageCreateInput<'_>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let client_id = super::resolve_client_id(input.client_id);
        // Borrow as &str so we can use it both as the json! key and as the
        // not_created lookup key without moving the String.
        let client_id_str: &str = &client_id;
        let mut create_obj = serde_json::json!({
            "chatId": input.chat_id,
            "body": input.body,
            "bodyType": serde_json::to_value(&input.body_type)
                .map_err(jmap_base_client::ClientError::from_parse)?,
            "sentAt": input.sent_at.as_ref(),
        });
        if let Some(rt) = input.reply_to {
            create_obj["replyTo"] = rt.as_ref().into();
        }
        if let Some(v) = input.attachments {
            create_obj["attachments"] =
                serde_json::to_value(v).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(v) = input.mentions {
            create_obj["mentions"] =
                serde_json::to_value(v).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(v) = input.broadcast_mentions {
            create_obj["broadcastMentions"] =
                serde_json::to_value(v).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(v) = input.actions {
            create_obj["actions"] =
                serde_json::to_value(v).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(v) = input.thread_root_id {
            create_obj["threadRootId"] = v.as_ref().into();
        }
        if let Some(v) = input.topic_id {
            create_obj["topicId"] = v.into();
        }
        if let Some(v) = input.subject {
            create_obj["subject"] = v.into();
        }
        if let Some(v) = input.sender_expires_at {
            create_obj["senderExpiresAt"] = v.as_ref().into();
        }
        if let Some(v) = input.burn_on_read {
            create_obj["burnOnRead"] = v.into();
        }
        if let Some(v) = input.expires_after_read_seconds {
            create_obj["expiresAfterReadSeconds"] = v.into();
        }
        let mut args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id_str: create_obj },
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("Message/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        let set_resp: SetResponse<PartialObject> =
            jmap_base_client::extract_response(&resp, super::CALL_ID)?;
        // Check for server-side rate limiting on the creation key.
        if let Some(err) = set_resp.not_created.get(client_id_str)
            && err.error_type == jmap_types::SetErrorType::RateLimit
        {
            let retry_after = match super::server_retry_after(err) {
                Ok(Some(t)) => t,
                Ok(None) => jmap_types::UTCDate::from(
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                ),
                Err(super::ServerRetryAfterError::Malformed(raw)) => {
                    return Err(jmap_base_client::ClientError::UnexpectedResponse(format!(
                        "rateLimit SetError has malformed serverRetryAfter: {raw}"
                    )));
                }
            };
            return Err(jmap_base_client::ClientError::RateLimited { retry_after });
        }
        Ok(set_resp)
    }

    /// Update Message properties (RFC 8620 §5.3 / JMAP Chat §4.5 Message/set).
    ///
    /// Issues an `update` operation patching only the fields present in `patch`.
    /// Supports body edits (author-only), reaction changes (JSON Pointer patch on
    /// `reactions` map), read-receipt updates (`readAt`), and chat-level deletion
    /// (`deletedAt` / `deletedForAll`).
    ///
    /// If all optional fields are `None`, an empty patch object is sent. RFC 8620
    /// §5.3 permits this; the server treats it as a no-op but still returns the
    /// object in `updated`.
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if any [`ReactionChange`] entry carries an empty
    ///   `sender_reaction_id`, or one containing `/` or `~`
    ///   (RFC 6901 JSON Pointer reserved characters that would
    ///   misinterpret the patch path), or if `read_disposition` is set
    ///   without `read_at`.
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing the typed `body_type` or `read_disposition` enums
    ///   fails (pathological conditions only).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::message_get`]. /set update
    ///   errors appear in [`SetResponse::not_updated`] rather than
    ///   as [`Err`].
    pub async fn message_update(
        &self,
        id: &Id,
        patch: &MessagePatch<'_>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        // draft-atwood-jmap-chat-00 §Message `readDisposition`: "A client
        // supplies it only in the same `Message/set` update that sets
        // `readAt`".
        if patch.read_disposition.is_some() && patch.read_at.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "message_update: read_disposition requires read_at".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut patch_map = serde_json::Map::new();
        if let Some(b) = patch.body {
            patch_map.insert("body".into(), b.into());
        }
        if let Some(bt) = &patch.body_type {
            patch_map.insert(
                "bodyType".into(),
                serde_json::to_value(bt).map_err(jmap_base_client::ClientError::from_parse)?,
            );
        }
        if let Some(ra) = patch.read_at {
            patch_map.insert("readAt".into(), ra.as_ref().into());
        }
        if let Some(rd) = &patch.read_disposition {
            patch_map.insert(
                "readDisposition".into(),
                serde_json::to_value(rd).map_err(jmap_base_client::ClientError::from_parse)?,
            );
        }
        if let Some(da) = patch.deleted_at {
            patch_map.insert("deletedAt".into(), da.as_ref().into());
        }
        if let Some(dfa) = patch.deleted_for_all {
            patch_map.insert("deletedForAll".into(), dfa.into());
        }
        if let Some(broadcast_mentions) = &patch.broadcast_mentions {
            patch_map.insert(
                "broadcastMentions".into(),
                serde_json::to_value(broadcast_mentions)
                    .expect("BroadcastMention serializes infallibly"),
            );
        }
        if let Some(mentions) = &patch.mentions {
            patch_map.insert(
                "mentions".into(),
                serde_json::to_value(mentions).expect("Mention serializes infallibly"),
            );
        }
        for change in patch.reaction_changes.unwrap_or(&[]) {
            match change {
                ReactionChange::Add {
                    sender_reaction_id,
                    emoji,
                    sent_at,
                } => {
                    validate_sender_reaction_id(sender_reaction_id)?;
                    patch_map.insert(
                        format!("reactions/{sender_reaction_id}"),
                        serde_json::json!({"emoji": emoji, "sentAt": sent_at.as_ref()}),
                    );
                }
                ReactionChange::Remove { sender_reaction_id } => {
                    validate_sender_reaction_id(sender_reaction_id)?;
                    patch_map.insert(
                        format!("reactions/{sender_reaction_id}"),
                        serde_json::Value::Null,
                    );
                }
            }
        }
        // Wrap the constructed map in a PatchObject (RFC 8620 §5.3) before
        // serializing. Wire bytes are unchanged because PatchObject is
        // #[serde(transparent)]; the typed boundary documents the contract.
        let patch_value = serde_json::Value::Object(PatchObject::from_map(patch_map).into_inner());
        let mut args = serde_json::json!({
            "accountId": account_id,
            "update": { id.as_ref(): patch_value },
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("Message/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy Message objects (RFC 8620 §5.3 / Message/set destroy).
    ///
    /// Permanently removes the listed message IDs from the account.
    /// `ids` must be non-empty; the guard fires before any network call.
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `ids` is empty (caller-precondition guard).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::message_get`]. /set destroy
    ///   errors appear in [`SetResponse::not_destroyed`] rather
    ///   than as [`Err`].
    pub async fn message_destroy(
        &self,
        ids: &[Id],
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "message_destroy: ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "destroy": ids,
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("Message/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Message since `since_query_state`
    /// (RFC 8620 §5.6 / Message/queryChanges).
    ///
    /// Returns which message IDs were removed from or added to the query
    /// result set since the given state. `max_changes` may be `None`.
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `Message/query` call that returned `since_query_state` —
    /// RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// `filter` is an RFC 8620 §5.5 `Filter`: a bare condition
    /// (`Filter::Condition`) or a `FilterOperator` combining sub-filters
    /// with `AND` / `OR` / `NOT`. The condition type `C` is normally
    /// [`MessageFilterCondition`](jmap_chat_types::MessageFilterCondition); use
    /// `jmap_metadata_types::WithMetadata<MessageFilterCondition>` to add the
    /// draft-ietf-jmap-metadata-04 §3.6 `metadata*` conditions.
    /// Pass `None` to omit it; name the type when you do (`None::<&MessageFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing `filter` fails (a caller-supplied condition type `C`
    ///   whose `Serialize` impl errors).
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::message_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::message_get`]. RFC 8620
    ///   §5.6 also defines `cannotCalculateChanges` (returned when the
    ///   server cannot honour the request given the supplied filter /
    ///   sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn message_query_changes<C: Serialize>(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&Filter<C>>,
        sort: Option<&[jmap_chat_types::MessageComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `chat_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "message_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] =
                serde_json::to_value(f).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(s) = sort {
            args["sort"] =
                serde_json::to_value(s).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] =
                serde_json::to_value(uti).map_err(jmap_base_client::ClientError::from_parse)?;
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req = super::build_request("Message/queryChanges", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

/// Build `Message/query` arguments for `account_id` (shared with
/// [`super::batch::Batch::message_query_get`]); see
/// [`super::SessionClient::message_query`] for the `None` defaults and
/// errors.
pub(super) fn message_query_args<C: Serialize>(
    account_id: &str,
    filter: Option<&Filter<C>>,
    sort: Option<&[jmap_chat_types::MessageComparator]>,
    params: &jmap_base_client::QueryParams,
) -> Result<serde_json::Value, jmap_base_client::ClientError> {
    let Some(filter) = filter else {
        return Err(jmap_base_client::ClientError::InvalidArgument(
            "message_query: a filter scoped by chat_id or has_mention=true must be provided".into(),
        ));
    };
    let mut args = params.to_arguments(account_id);
    args["filter"] =
        serde_json::to_value(filter).map_err(jmap_base_client::ClientError::from_parse)?;
    if let Some(s) = sort {
        args["sort"] =
            serde_json::to_value(s).map_err(jmap_base_client::ClientError::from_parse)?;
    }
    Ok(args)
}

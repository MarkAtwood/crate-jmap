//! The `jmap-chat-server` hook traits on [`SlackBridge`]
//! (docs/design/chat-backend-split.md): the write path to Slack in
//! [`ChatStore`], refusals for what Slack has no form for, and
//! `Delegate*` impls to the embedded [`MemoryBackend`] for the rest.

use std::collections::HashSet;
use std::time::Duration;

use jmap_chat_server::memory::{MemoryBackend, MemoryError};
use jmap_chat_server::{
    ChatLimits, ChatModeration, ChatRestrictions, ChatSettings, ChatSpaces, ChatStore,
    DelegateChatAccessPolicy, DelegateChatEvents, DelegateChatMessageLifecycle,
    DelegateChatResolvers, DelegateSpacePolicy, InviteJoinRefusal, OpResult,
};
use jmap_chat_types::{
    BodyType, BroadcastScope, Chat, ChatBan, ChatPermission, Message, Space, SpaceMetadataPatch,
    SpacePatchOp,
};
use jmap_server::{
    BackendSetError, DelegateJmapAccounts, DelegateJmapReads, JmapObject, JmapReads, SetError,
    SetErrorType, SetObject,
};
use jmap_types::{Id, PatchObject, UTCDate};
use serde_json::{Value, json};

use crate::bridge::{MapState, MsgEntry, SlackBridge};
use crate::client::SlackError;
use crate::mrkdwn::{self, RenderError};
use crate::time::{now_utc, utc_from_unix};

type SetResult<T> = Result<T, BackendSetError<MemoryError>>;

/// The body types the bridge can send: `text/plain` and
/// `application/jmap-chat-rich`.
pub const SUPPORTED_BODY_TYPES: &[&str] = &["text/plain", "application/jmap-chat-rich"];

/// Slack truncates `text` past 40,000 characters
/// (<https://docs.slack.dev/reference/methods/chat.postMessage/>).
pub const MAX_BODY_BYTES: u64 = 40_000;

/// Chat preferences that stay local (not shared with peers, per
/// draft-atwood-jmap-chat-00 §Chat), the only Chat/set update keys the
/// bridge accepts.
const LOCAL_CHAT_KEYS: &[&str] = &[
    "muted",
    "muteUntil",
    "receiveTypingIndicators",
    "receiptSharing",
    "messageExpirySeconds",
    "metadata",
    "privateMetadata",
];

/// Types kept only in the local store: no Slack counterpart is written.
const LOCAL_TYPES: &[&str] = &["ReadPosition", "ChatContact", "PresenceStatus"];

fn refuse(error_type: SetErrorType, description: impl Into<String>) -> SetError {
    SetError::new(error_type).with_description(description)
}

fn refused<T>(error_type: SetErrorType, description: impl Into<String>) -> SetResult<T> {
    Err(BackendSetError::SetError(Box::new(refuse(
        error_type,
        description,
    ))))
}

fn invalid<T>(properties: &[&str], description: impl Into<String>) -> SetResult<T> {
    Err(BackendSetError::SetError(Box::new(
        refuse(SetErrorType::InvalidProperties, description)
            .with_properties(properties.iter().copied()),
    )))
}

fn store_fail(what: &str) -> BackendSetError<MemoryError> {
    BackendSetError::Other(MemoryError::new(what))
}

/// The SetError a failed Slack call surfaces as (README "How failures
/// surface"). Transport failures, 5xx and unknown Slack codes are
/// [`BackendSetError::Other`], which the handlers report as `serverFail`.
pub(crate) fn slack_set_error(e: &SlackError) -> BackendSetError<MemoryError> {
    let typed = |t: SetErrorType, code: &str| {
        BackendSetError::SetError(Box::new(refuse(t, format!("slack: {code}"))))
    };
    match e {
        SlackError::RateLimited { retry_after, .. } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                + *retry_after
                + Duration::from_secs(1);
            let at = utc_from_unix(i64::try_from(now.as_secs()).unwrap_or(i64::MAX), 0);
            BackendSetError::SetError(Box::new(
                refuse(SetErrorType::RateLimit, "slack: rate limited")
                    .with_extra("serverRetryAfter", json!(at)),
            ))
        }
        SlackError::WrongToken { .. } => typed(SetErrorType::Forbidden, "wrong token type"),
        SlackError::Api { code, .. } => match code.as_str() {
            "channel_not_found"
            | "not_in_channel"
            | "is_archived"
            | "restricted_action"
            | "restricted_action_read_only_channel"
            | "restricted_action_thread_only_channel"
            | "restricted_action_non_threadable_channel"
            | "cant_update_message"
            | "cant_delete_message"
            | "edit_window_closed"
            | "missing_scope"
            | "not_allowed_token_type"
            | "posting_to_general_channel_denied"
            | "ekm_access_denied"
            | "team_access_not_granted"
            | "no_permission"
            | "access_denied"
            | "invalid_auth"
            | "not_authed"
            | "account_inactive"
            | "token_revoked"
            | "token_expired" => typed(SetErrorType::Forbidden, code),
            "message_not_found" | "thread_not_found" => typed(SetErrorType::NotFound, code),
            "msg_too_long" | "too_many_attachments" | "msg_blocks_too_long" => {
                typed(SetErrorType::TooLarge, code)
            }
            "too_many_reactions" | "too_many_emoji" => typed(SetErrorType::OverQuota, code),
            "ratelimited" | "rate_limited" => typed(SetErrorType::RateLimit, code),
            "no_text" => BackendSetError::SetError(Box::new(
                refuse(SetErrorType::InvalidProperties, "slack: no_text").with_properties(["body"]),
            )),
            "invalid_name" => BackendSetError::SetError(Box::new(
                refuse(SetErrorType::InvalidProperties, "slack: invalid_name")
                    .with_properties(["reactions"]),
            )),
            other => BackendSetError::Other(MemoryError::new(format!("slack: {other}"))),
        },
        other => BackendSetError::Other(MemoryError::new(other.to_string())),
    }
}

fn render_error(e: &RenderError) -> BackendSetError<MemoryError> {
    let property = match e {
        RenderError::UnsupportedBodyType => "bodyType",
        _ => "body",
    };
    BackendSetError::SetError(Box::new(
        refuse(SetErrorType::InvalidProperties, e.to_string()).with_properties([property]),
    ))
}

fn ok_codes(r: Result<(), SlackError>, ok: &[&str]) -> Result<(), SlackError> {
    match r {
        Err(SlackError::Api { ref code, .. }) if ok.contains(&code.as_str()) => Ok(()),
        other => other,
    }
}

impl SlackBridge {
    fn check_account(&self, account_id: &Id) -> SetResult<()> {
        if account_id == self.account_id() {
            Ok(())
        } else {
            refused(SetErrorType::Forbidden, "not the bridged account")
        }
    }

    async fn stored_message(&self, id: &Id) -> SetResult<Message> {
        let (found, _) = self
            .inner
            .get_objects::<Message>(&(), self.account_id(), Some(std::slice::from_ref(id)), None)
            .await
            .map_err(BackendSetError::Other)?;
        match found.into_iter().next() {
            Some(m) => Ok(m),
            None => refused(SetErrorType::NotFound, "no such Message"),
        }
    }

    /// The Slack `thread_ts` for a create's `threadRootId` / `replyTo`.
    fn thread_ts(st: &MapState, channel: &str, msg: &Message) -> SetResult<Option<String>> {
        let (target, property) = match (&msg.thread_root_id, &msg.reply_to) {
            (Some(t), _) => (t, "threadRootId"),
            (None, Some(r)) => (r, "replyTo"),
            (None, None) => return Ok(None),
        };
        let Some((c, ts)) = st.message_keys.get(target) else {
            return invalid(&[property], "the target Message is not in Slack");
        };
        if c != channel {
            return invalid(&[property], "the target Message is in another Chat");
        }
        let root = st
            .messages
            .get(&(c.clone(), ts.clone()))
            .and_then(|e| e.thread_ts.clone())
            .unwrap_or_else(|| ts.clone());
        Ok(Some(root))
    }

    async fn post_message<F>(&self, account_id: &Id, build: F) -> SetResult<(Id, Message)>
    where
        F: FnOnce(&Id) -> Message + Send,
    {
        self.check_account(account_id)?;
        let mut st = self.shared.state.lock().await;
        let id = self.inner.mint_id();
        let mut msg = build(&id);
        let Some(channel) = st.chat_channels.get(&msg.chat_id).cloned() else {
            return invalid(&["chatId"], "no such Chat");
        };
        if st.chats.get(&channel).is_some_and(|c| c.archived) {
            return refused(
                SetErrorType::Forbidden,
                "the Slack conversation is archived",
            );
        }
        // Fields Slack cannot carry: refuse rather than drop them.
        let mut unsupported = Vec::new();
        if !msg.attachments.is_empty() {
            unsupported.push("attachments");
        }
        if !msg.actions.is_empty() {
            unsupported.push("actions");
        }
        if msg.sender_expires_at.is_some() {
            unsupported.push("senderExpiresAt");
        }
        if msg.burn_on_read == Some(true) {
            unsupported.push("burnOnRead");
        }
        if msg.expires_after_read_seconds.is_some() {
            unsupported.push("expiresAfterReadSeconds");
        }
        if msg.topic_id.is_some() {
            unsupported.push("topicId");
        }
        if !unsupported.is_empty() {
            return invalid(&unsupported, "not representable in Slack");
        }
        let thread_ts = Self::thread_ts(&st, &channel, &msg)?;
        let mentions: Vec<Id> = msg.mentions.iter().map(|m| m.id.clone()).collect();
        let scopes: Vec<BroadcastScope> = msg
            .broadcast_mentions
            .iter()
            .map(|b| b.scope.clone())
            .collect();
        let (text, mrkdwn) = mrkdwn::from_jmap(&msg.body, &msg.body_type, &mentions, &scopes, &*st)
            .map_err(|e| render_error(&e))?;
        let posted = self
            .shared
            .client
            .chat_post_message(&channel, &text, thread_ts.as_deref(), mrkdwn)
            .await
            .map_err(|e| slack_set_error(&e))?;
        // Slack accepted it: the message is delivered.
        msg.delivery_state = jmap_chat_types::DeliveryState::Delivered;
        msg.delivered_at = Some(now_utc());
        let stored = self.inner.insert_message(account_id, msg).await?;
        st.message_keys
            .insert(id.clone(), (channel.clone(), posted.ts.clone()));
        st.messages.insert(
            (channel, posted.ts),
            MsgEntry {
                id: id.clone(),
                text,
                edited: None,
                thread_ts,
                deleted: false,
            },
        );
        Ok((id, stored))
    }

    async fn update_message(&self, account_id: &Id, id: &Id, patch: PatchObject) -> SetResult<()> {
        self.check_account(account_id)?;
        let mut st = self.shared.state.lock().await;
        let Some((channel, ts)) = st.message_keys.get(id).cloned() else {
            // Not bridged (already destroyed on the Slack side): local only.
            ChatStore::update_object::<Message>(&self.inner, &(), account_id, id, patch).await?;
            return Ok(());
        };
        let current = self.stored_message(id).await?;
        let map = patch.as_map();
        if map.contains_key("reactions") {
            return invalid(&["reactions"], "change reactions with reactions/<id> keys");
        }
        let deletes_for_all = map.get("deletedAt").is_some_and(|v| !v.is_null())
            && map.get("deletedForAll") == Some(&Value::Bool(true));
        let edits = ["body", "bodyType", "mentions", "broadcastMentions"]
            .iter()
            .any(|k| map.contains_key(*k));
        // Validate everything before the first Slack call.
        let mut adds = Vec::new();
        let mut removes = Vec::new();
        for (key, value) in map {
            let Some(rid) = key.strip_prefix("reactions/") else {
                continue;
            };
            if value.is_null() {
                let Some(r) = current.reactions.get(rid) else {
                    continue;
                };
                let Some(name) = crate::emoji::to_slack(&r.emoji) else {
                    return invalid(&[key.as_str()], "emoji has no Slack name");
                };
                removes.push((rid.to_owned(), name));
            } else {
                let emoji = value
                    .get("emoji")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let Some(name) = crate::emoji::to_slack(emoji) else {
                    return invalid(&[key.as_str()], "emoji has no Slack name");
                };
                adds.push((rid.to_owned(), name));
            }
        }
        let edit_text = if edits && !deletes_for_all {
            let body = map
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or(&current.body)
                .to_owned();
            let body_type: BodyType = match map.get("bodyType") {
                Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
                    BackendSetError::SetError(Box::new(
                        refuse(SetErrorType::InvalidProperties, "bodyType")
                            .with_properties(["bodyType"]),
                    ))
                })?,
                None => current.body_type.clone(),
            };
            let mentions: Vec<Id> = match map.get("mentions") {
                Some(v) => serde_json::from_value::<Vec<jmap_chat_types::Mention>>(v.clone())
                    .map_err(|_| store_fail("mentions"))?
                    .into_iter()
                    .map(|m| m.id)
                    .collect(),
                None => current.mentions.iter().map(|m| m.id.clone()).collect(),
            };
            let scopes: Vec<BroadcastScope> = match map.get("broadcastMentions") {
                Some(v) => {
                    serde_json::from_value::<Vec<jmap_chat_types::BroadcastMention>>(v.clone())
                        .map_err(|_| store_fail("broadcastMentions"))?
                        .into_iter()
                        .map(|b| b.scope)
                        .collect()
                }
                None => current
                    .broadcast_mentions
                    .iter()
                    .map(|b| b.scope.clone())
                    .collect(),
            };
            let (text, _) = mrkdwn::from_jmap(&body, &body_type, &mentions, &scopes, &*st)
                .map_err(|e| render_error(&e))?;
            Some(text)
        } else {
            None
        };

        let client = &self.shared.client;
        if deletes_for_all {
            ok_codes(
                client.chat_delete(&channel, &ts).await,
                &["message_not_found"],
            )
            .map_err(|e| slack_set_error(&e))?;
        } else if let Some(text) = &edit_text {
            client
                .chat_update(&channel, &ts, text)
                .await
                .map_err(|e| slack_set_error(&e))?;
        }
        let owner = self.owner_user().to_owned();
        for (_, name) in &adds {
            ok_codes(
                client.reactions_add(&channel, &ts, name).await,
                &["already_reacted"],
            )
            .map_err(|e| slack_set_error(&e))?;
        }
        for (_, name) in &removes {
            ok_codes(
                client.reactions_remove(&channel, &ts, name).await,
                &["no_reaction"],
            )
            .map_err(|e| slack_set_error(&e))?;
        }

        ChatStore::update_object::<Message>(&self.inner, &(), account_id, id, patch).await?;
        let key = (channel, ts);
        if let Some(entry) = st.messages.get_mut(&key) {
            if let Some(text) = edit_text {
                entry.text = text;
            }
            if deletes_for_all {
                entry.deleted = true;
            }
        }
        for (rid, name) in adds {
            st.reactions
                .insert((id.clone(), owner.clone(), name), Id::from(rid.as_str()));
        }
        for (rid, _) in removes {
            st.reactions.retain(|_, v| v.as_ref() != rid);
        }
        Ok(())
    }

    async fn delete_message(&self, account_id: &Id, id: &Id) -> SetResult<()> {
        self.check_account(account_id)?;
        let mut st = self.shared.state.lock().await;
        if let Some((channel, ts)) = st.message_keys.get(id).cloned() {
            let already = st
                .messages
                .get(&(channel.clone(), ts.clone()))
                .is_some_and(|e| e.deleted);
            if !already {
                ok_codes(
                    self.shared.client.chat_delete(&channel, &ts).await,
                    &["message_not_found"],
                )
                .map_err(|e| slack_set_error(&e))?;
            }
            ChatStore::destroy_object::<Message>(&self.inner, &(), account_id, id).await?;
            st.messages.remove(&(channel, ts));
            st.message_keys.remove(id);
            st.reactions.retain(|(mid, _, _), _| mid != id);
            return Ok(());
        }
        ChatStore::destroy_object::<Message>(&self.inner, &(), account_id, id).await
    }
}

impl DelegateJmapAccounts for SlackBridge {
    type Target = MemoryBackend;
    fn jmap_accounts(&self) -> &MemoryBackend {
        &self.inner
    }
}

impl DelegateJmapReads for SlackBridge {
    type Target = MemoryBackend;
    fn jmap_reads(&self) -> &MemoryBackend {
        &self.inner
    }
}

/// The write path: Message creates, edits, deletes and reactions go to
/// Slack first and are stored only when Slack accepts them; local-only
/// types go to the store; everything else is refused.
impl ChatStore for SlackBridge {
    async fn create_object<O, F>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> SetResult<(Id, O)>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        if LOCAL_TYPES.contains(&O::TYPE_NAME) {
            return self
                .inner
                .create_object(caller, account_id, create_id, build)
                .await;
        }
        refused(
            SetErrorType::Forbidden,
            format!("{} creation is not bridged to Slack", O::TYPE_NAME),
        )
    }

    async fn create_message<F>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        build: F,
    ) -> SetResult<(Id, Message)>
    where
        F: FnOnce(&Id) -> Message + Send,
    {
        self.post_message(account_id, build).await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> SetResult<Option<O>> {
        match O::TYPE_NAME {
            "Message" => {
                let patch: PatchObject = serde_json::to_value(&patch)
                    .and_then(serde_json::from_value)
                    .map_err(|_| store_fail("patch"))?;
                self.update_message(account_id, id, patch).await?;
                Ok(None)
            }
            // Chat updates are limited to local preferences by
            // `ChatRestrictions::may_update_chat`.
            t if t == Chat::TYPE_NAME || LOCAL_TYPES.contains(&t) => {
                self.inner
                    .update_object::<O>(caller, account_id, id, patch)
                    .await
            }
            t => refused(
                SetErrorType::Forbidden,
                format!("{t} changes are not bridged to Slack"),
            ),
        }
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> SetResult<()> {
        match O::TYPE_NAME {
            "Message" => self.delete_message(account_id, id).await,
            t if LOCAL_TYPES.contains(&t) => {
                self.inner.destroy_object::<O>(caller, account_id, id).await
            }
            t => refused(
                SetErrorType::Forbidden,
                format!("{t} destruction is not bridged to Slack; leave or archive it in Slack"),
            ),
        }
    }

    fn supports_type<O: JmapObject>(&self, _caller: &(), _account_id: &Id) -> bool {
        matches!(
            O::TYPE_NAME,
            "Chat" | "Message" | "Space" | "ChatContact" | "ReadPosition" | "PresenceStatus"
        )
    }
}

/// Space structure follows Slack: every Space/set patch is refused.
impl ChatSpaces for SlackBridge {
    async fn generate_invite_code(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<String, MemoryError> {
        self.inner.generate_invite_code(caller, account_id).await
    }

    async fn join_space_with_invite(
        &self,
        _caller: &(),
        _account_id: &Id,
        _invite_id: &Id,
        _joined_at: &UTCDate,
    ) -> Result<Result<(), InviteJoinRefusal>, MemoryError> {
        // No SpaceInvite can exist (their create is refused).
        Err(MemoryError::new("Space/join is not bridged to Slack"))
    }

    async fn apply_space_patch(
        &self,
        _caller: &(),
        _account_id: &Id,
        _space_id: &Id,
        _ops: Vec<SpacePatchOp>,
    ) -> SetResult<Vec<OpResult>> {
        refused(
            SetErrorType::Forbidden,
            "Space members, roles, categories and channels follow Slack",
        )
    }

    async fn apply_space_metadata_patch(
        &self,
        _caller: &(),
        _account_id: &Id,
        _space_id: &Id,
        _patch: SpaceMetadataPatch,
    ) -> SetResult<Option<Space>> {
        refused(
            SetErrorType::Forbidden,
            "the Space follows the Slack workspace",
        )
    }

    fn space_extension_properties(&self, caller: &(), account_id: &Id) -> &'static [&'static str] {
        self.inner.space_extension_properties(caller, account_id)
    }

    fn protect_last_admin(&self, caller: &(), account_id: &Id) -> bool {
        self.inner.protect_last_admin(caller, account_id)
    }

    async fn everyone_permissions(
        &self,
        caller: &(),
        account_id: &Id,
        space: &Space,
    ) -> Result<HashSet<ChatPermission>, MemoryError> {
        self.inner
            .everyone_permissions(caller, account_id, space)
            .await
    }
}

/// Archived conversations take no new Messages; Chat/set updates are
/// limited to local preferences.
impl ChatRestrictions for SlackBridge {
    async fn may_create_message(
        &self,
        _caller: &(),
        _account_id: &Id,
        chat: &Chat,
    ) -> Result<Result<(), SetError>, MemoryError> {
        let st = self.shared.state.lock().await;
        let archived = st
            .chat_channels
            .get(&chat.id)
            .and_then(|c| st.chats.get(c))
            .is_some_and(|c| c.archived);
        Ok(if archived {
            Err(refuse(
                SetErrorType::Forbidden,
                "the Slack conversation is archived",
            ))
        } else {
            Ok(())
        })
    }

    async fn may_update_chat(
        &self,
        _caller: &(),
        _account_id: &Id,
        _chat: &Chat,
        patch_keys: &[&str],
    ) -> Result<Result<(), SetError>, MemoryError> {
        let foreign: Vec<&str> = patch_keys
            .iter()
            .copied()
            .filter(|k| {
                let top = k.split('/').next().unwrap_or(k);
                !LOCAL_CHAT_KEYS.contains(&top)
            })
            .collect();
        Ok(if foreign.is_empty() {
            Ok(())
        } else {
            Err(refuse(
                SetErrorType::Forbidden,
                "only local Chat preferences can change",
            )
            .with_properties(foreign))
        })
    }
}

/// Slack moderation is not bridged.
impl ChatModeration for SlackBridge {
    async fn create_chat_ban<F>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _create_id: &str,
        _build: F,
    ) -> SetResult<(Id, ChatBan)>
    where
        F: FnOnce(&Id) -> ChatBan + Send,
    {
        refused(SetErrorType::Forbidden, "ChatBan is not bridged to Slack")
    }

    async fn update_chat_ban(
        &self,
        _caller: &(),
        _account_id: &Id,
        _id: &Id,
        _patch: PatchObject,
    ) -> SetResult<Option<ChatBan>> {
        refused(SetErrorType::Forbidden, "ChatBan is not bridged to Slack")
    }

    async fn destroy_chat_ban(&self, _caller: &(), _account_id: &Id, _id: &Id) -> SetResult<()> {
        refused(SetErrorType::Forbidden, "ChatBan is not bridged to Slack")
    }

    async fn kick_chat_members(
        &self,
        _caller: &(),
        _account_id: &Id,
        _chat: &Chat,
        _user_ids: &[Id],
    ) -> Result<Result<(), SetError>, MemoryError> {
        Ok(Err(refuse(
            SetErrorType::Forbidden,
            "removing members is not bridged to Slack",
        )))
    }
}

/// Limits for what Slack carries; `ownerUserId` is `slack:<team>:<user>`.
impl ChatSettings for SlackBridge {
    fn limits(&self, _caller: &(), _account_id: &Id) -> ChatLimits {
        ChatLimits::default()
            .with_supported_body_types(SUPPORTED_BODY_TYPES)
            .with_encrypted_body_types(&[])
            .with_supports_threads(true)
            .with_max_body_bytes(MAX_BODY_BYTES)
            .with_max_attachments_per_message(0)
    }

    fn maintains_unread_count(&self) -> bool {
        self.inner.maintains_unread_count()
    }

    async fn owner_user_id(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<Option<String>, MemoryError> {
        Ok((account_id == self.account_id()).then(|| self.owner_identity()))
    }
}

macro_rules! delegate {
    ($($tr:ident => $acc:ident),* $(,)?) => {$(
        impl $tr for SlackBridge {
            type Target = MemoryBackend;
            fn $acc(&self) -> &MemoryBackend {
                &self.inner
            }
        }
    )*};
}

delegate!(
    DelegateSpacePolicy => space_policy,
    DelegateChatAccessPolicy => chat_access_policy,
    DelegateChatMessageLifecycle => chat_message_lifecycle,
    DelegateChatEvents => chat_events,
    DelegateChatResolvers => chat_resolvers,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(e: BackendSetError<MemoryError>) -> Option<SetErrorType> {
        match e {
            BackendSetError::SetError(se) => Some(se.error_type),
            _ => None,
        }
    }

    fn api(code: &str) -> SlackError {
        SlackError::Api {
            method: "chat.postMessage".to_owned(),
            code: code.to_owned(),
        }
    }

    /// Error codes from the chat.postMessage, chat.update, chat.delete and
    /// reactions.add reference pages.
    #[test]
    fn slack_codes_map_to_set_errors() {
        assert_eq!(
            kind(slack_set_error(&api("not_in_channel"))),
            Some(SetErrorType::Forbidden)
        );
        assert_eq!(
            kind(slack_set_error(&api("is_archived"))),
            Some(SetErrorType::Forbidden)
        );
        assert_eq!(
            kind(slack_set_error(&api("cant_update_message"))),
            Some(SetErrorType::Forbidden)
        );
        assert_eq!(
            kind(slack_set_error(&api("message_not_found"))),
            Some(SetErrorType::NotFound)
        );
        assert_eq!(
            kind(slack_set_error(&api("msg_too_long"))),
            Some(SetErrorType::TooLarge)
        );
        assert_eq!(
            kind(slack_set_error(&api("too_many_reactions"))),
            Some(SetErrorType::OverQuota)
        );
        assert_eq!(
            kind(slack_set_error(&api("no_text"))),
            Some(SetErrorType::InvalidProperties)
        );
        assert_eq!(kind(slack_set_error(&api("something_new"))), None);
        let limited = SlackError::RateLimited {
            method: "chat.postMessage".to_owned(),
            retry_after: Duration::from_secs(30),
        };
        let BackendSetError::SetError(se) = slack_set_error(&limited) else {
            panic!("rateLimit is a SetError");
        };
        assert_eq!(se.error_type, SetErrorType::RateLimit);
        let after = se
            .extra
            .get("serverRetryAfter")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(UTCDate::new_validated(after).is_ok(), "{after}");
    }
}

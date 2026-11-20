//! The bridge: one Slack workspace mirrored into a JMAP account held in an
//! embedded [`MemoryBackend`], kept current by initial sync and Events API
//! events.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use jmap_chat_server::ChatStore;
use jmap_chat_server::memory::{MemoryBackend, MemoryError};
use jmap_chat_types::{
    BroadcastMention, Chat, ChatContact, ChatKind, ChatMember, ChatMemberRole, DeliveryState,
    Mention, Message, Space,
};
use jmap_server::{BackendSetError, JmapReads};
use jmap_types::{Id, PatchObject, UTCDate};
use serde::Deserialize as _;
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

use crate::client::{Conversation, SlackClient, SlackError, SlackMessage, SlackUser, all_pages};
use crate::mrkdwn::{self, Directory};
use crate::time::{now_utc, utc_from_ts, utc_from_unix};
use crate::token::TokenKind;

/// The conversation types [`BridgeConfig::new`] syncs.
pub const DEFAULT_CONVERSATION_TYPES: &str = "public_channel,private_channel,mpim,im";

/// Message subtypes the bridge stores; every other subtype (`channel_join`,
/// `channel_topic`, `bot_add`, …) is a Slack system message and is skipped
/// (<https://docs.slack.dev/reference/events/message/>).
const STORED_SUBTYPES: &[&str] = &[
    "bot_message",
    "thread_broadcast",
    "me_message",
    "file_share",
];

/// How a [`SlackBridge`] is set up.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    /// The JMAP account the workspace appears in.
    pub account_id: Id,
    /// The Space name; `auth.test`'s workspace name when `None`.
    pub space_name: Option<String>,
    /// The `conversations.list` `types`.
    pub conversation_types: String,
    /// The `conversations.history` `oldest` bound of the initial sync, a
    /// Slack `ts`; all history when `None`.
    pub history_oldest: Option<String>,
    /// Whether the initial sync fetches thread replies with
    /// `conversations.replies`.
    pub backfill_threads: bool,
}

impl BridgeConfig {
    /// Sync [`DEFAULT_CONVERSATION_TYPES`], all history and threads into
    /// `account_id`.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            space_name: None,
            conversation_types: DEFAULT_CONVERSATION_TYPES.to_owned(),
            history_oldest: None,
            backfill_threads: true,
        }
    }

    /// Name the Space `name`.
    pub fn with_space_name(self, name: impl Into<String>) -> Self {
        Self {
            space_name: Some(name.into()),
            ..self
        }
    }

    /// Sync these `conversations.list` types.
    pub fn with_conversation_types(self, types: impl Into<String>) -> Self {
        Self {
            conversation_types: types.into(),
            ..self
        }
    }

    /// Fetch history from `oldest` (a Slack `ts`) only.
    pub fn with_history_oldest(self, oldest: impl Into<String>) -> Self {
        Self {
            history_oldest: Some(oldest.into()),
            ..self
        }
    }

    /// Fetch thread replies in the initial sync, or not.
    pub fn with_thread_backfill(self, backfill: bool) -> Self {
        Self {
            backfill_threads: backfill,
            ..self
        }
    }
}

/// A failure of [`SlackBridge::connect`], [`SlackBridge::initial_sync`] or
/// [`SlackBridge::apply_event`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// A Web API call failed.
    #[error(transparent)]
    Slack(#[from] SlackError),
    /// The embedded store refused a write.
    #[error("local store: {0}")]
    Store(String),
    /// The client holds an app-level token; the bridge needs a bot or user
    /// token.
    #[error("the bridge needs a bot or user token, not an app-level token")]
    AppToken,
}

fn store_err(e: BackendSetError<MemoryError>) -> BridgeError {
    match e {
        BackendSetError::SetError(se) => BridgeError::Store(se.error_type.to_string()),
        BackendSetError::Other(e) => BridgeError::Store(e.description().to_owned()),
        _ => BridgeError::Store("store error".to_owned()),
    }
}

fn read_err(e: MemoryError) -> BridgeError {
    BridgeError::Store(e.description().to_owned())
}

/// A conversation the initial sync could not read.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedConversation {
    /// The Slack conversation id.
    pub channel: String,
    /// Slack's error code (`not_in_channel`, `missing_scope`, …).
    pub error: String,
}

/// What an initial sync stored.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// ChatContacts created.
    pub contacts: usize,
    /// Chats created.
    pub conversations: usize,
    /// Messages created or updated.
    pub messages: usize,
    /// Conversations whose history could not be read.
    pub skipped: Vec<SkippedConversation>,
}

/// What [`SlackBridge::apply_event`] did.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOutcome {
    /// The JMAP state changed.
    Applied,
    /// The event matched what the store already holds (for example the
    /// echo of a write made through JMAP).
    Unchanged,
    /// The event has no JMAP form, or names something the bridge does not
    /// hold.
    Ignored,
}

#[derive(Debug, Clone)]
pub(crate) struct ChatEntry {
    pub(crate) id: Id,
    pub(crate) kind: ChatKind,
    pub(crate) archived: bool,
    pub(crate) name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct MsgEntry {
    pub(crate) id: Id,
    /// The last text Slack holds (or that the bridge sent).
    pub(crate) text: String,
    /// The `edited.ts` last seen.
    pub(crate) edited: Option<String>,
    /// The thread parent `ts`, for replies.
    pub(crate) thread_ts: Option<String>,
    pub(crate) deleted: bool,
}

/// The Slack ↔ JMAP id mapping.
#[derive(Debug, Default)]
pub(crate) struct MapState {
    /// Slack user or bot id → ChatContact id (accountId for the owner).
    pub(crate) users: HashMap<String, Id>,
    pub(crate) contact_users: HashMap<Id, String>,
    pub(crate) names: HashMap<String, String>,
    /// Space members other than the owner, with their join time.
    pub(crate) space_members: Vec<(Id, UTCDate)>,
    pub(crate) uncategorized: Vec<Id>,
    pub(crate) chats: HashMap<String, ChatEntry>,
    pub(crate) chat_channels: HashMap<Id, String>,
    pub(crate) messages: HashMap<(String, String), MsgEntry>,
    pub(crate) message_keys: HashMap<Id, (String, String)>,
    /// (Message id, Slack user, reaction name) → senderReactionId.
    pub(crate) reactions: HashMap<(Id, String, String), Id>,
}

impl Directory for MapState {
    fn contact(&self, slack_user: &str) -> Option<(Id, String)> {
        let id = self.users.get(slack_user)?;
        let name = self
            .names
            .get(slack_user)
            .cloned()
            .unwrap_or_else(|| slack_user.to_owned());
        Some((id.clone(), name))
    }

    fn slack_user(&self, id: &Id) -> Option<String> {
        self.contact_users.get(id).cloned()
    }

    fn channel_name(&self, channel: &str) -> Option<String> {
        self.chats.get(channel).and_then(|c| c.name.clone())
    }
}

pub(crate) struct Shared {
    pub(crate) client: SlackClient,
    pub(crate) config: BridgeConfig,
    pub(crate) team_id: String,
    pub(crate) owner_user: String,
    pub(crate) space_id: Id,
    pub(crate) state: Mutex<MapState>,
}

/// One Slack workspace as a JMAP Chat account: a `jmap-chat-server`
/// [`ChatBackend`](jmap_chat_server::ChatBackend) over an embedded
/// [`MemoryBackend`].
///
/// Cloning is cheap and shares all state.
#[derive(Clone)]
pub struct SlackBridge {
    pub(crate) inner: MemoryBackend<()>,
    pub(crate) shared: Arc<Shared>,
}

/// Shows ids only, never the token.
impl fmt::Debug for SlackBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlackBridge")
            .field("client", &self.shared.client)
            .field("account_id", &self.shared.config.account_id)
            .field("team_id", &self.shared.team_id)
            .field("space_id", &self.shared.space_id)
            .finish_non_exhaustive()
    }
}

fn value_of<T: serde::Serialize>(v: &T) -> Result<Value, BridgeError> {
    serde_json::to_value(v).map_err(|_| BridgeError::Store("serialize".to_owned()))
}

fn from_value<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, BridgeError> {
    serde_json::from_value(v).map_err(|_| BridgeError::Store("deserialize".to_owned()))
}

impl SlackBridge {
    /// Check the token with `auth.test` and create the account and its
    /// Space. The token's user is the account owner: their messages have
    /// the accountId as `senderId`.
    ///
    /// # Errors
    ///
    /// [`BridgeError::AppToken`] for an app-level token, a
    /// [`BridgeError::Slack`] when `auth.test` fails.
    pub async fn connect(client: SlackClient, config: BridgeConfig) -> Result<Self, BridgeError> {
        if client.token_kind() == TokenKind::App {
            return Err(BridgeError::AppToken);
        }
        let auth = client.auth_test().await?;
        let inner = MemoryBackend::<()>::default().with_account(&config.account_id);
        let space_id = inner.mint_id();
        let now = now_utc();
        let owner = from_value(json!({
            "id": config.account_id, "roleIds": [], "joinedAt": now,
        }))?;
        let name = config
            .space_name
            .clone()
            .or_else(|| auth.team.clone())
            .unwrap_or_else(|| auth.team_id.clone());
        let mut space = Space::new(
            space_id.clone(),
            name,
            Vec::new(),
            vec![owner],
            Vec::new(),
            Vec::new(),
            now,
            false,
            false,
            1,
        );
        space.description = auth.url.clone();
        inner
            .insert_object(&config.account_id, space)
            .await
            .map_err(store_err)?;
        let mut state = MapState::default();
        state
            .users
            .insert(auth.user_id.clone(), config.account_id.clone());
        state
            .contact_users
            .insert(config.account_id.clone(), auth.user_id.clone());
        Ok(Self {
            inner,
            shared: Arc::new(Shared {
                client,
                config,
                team_id: auth.team_id,
                owner_user: auth.user_id,
                space_id,
                state: Mutex::new(state),
            }),
        })
    }

    /// The embedded store, for draining its outbound and lifecycle events
    /// and for push state. Writes made on it directly are not sent to
    /// Slack.
    pub fn memory(&self) -> &MemoryBackend<()> {
        &self.inner
    }

    /// The JMAP account.
    pub fn account_id(&self) -> &Id {
        &self.shared.config.account_id
    }

    /// The workspace's Space.
    pub fn space_id(&self) -> &Id {
        &self.shared.space_id
    }

    /// The Slack workspace id.
    pub fn team_id(&self) -> &str {
        &self.shared.team_id
    }

    /// The Slack user id of the account owner.
    pub fn owner_user(&self) -> &str {
        &self.shared.owner_user
    }

    /// The owner's identity string (`ownerUserId`): `slack:<team>:<user>`.
    pub fn owner_identity(&self) -> String {
        self.identity(&self.shared.owner_user)
    }

    fn identity(&self, slack_id: &str) -> String {
        format!("slack:{}:{slack_id}", self.shared.team_id)
    }

    fn account(&self) -> &Id {
        &self.shared.config.account_id
    }

    /// Run the initial sync: `users.list`, `conversations.list`, then each
    /// conversation's `conversations.history` (and `conversations.replies`
    /// for threads), every collection paginated and every call through the
    /// client's 429 handling. Running it again reconciles: known messages
    /// are updated in place, not duplicated.
    ///
    /// Holds the bridge mapping for the whole run, so JMAP writes wait.
    ///
    /// # Errors
    ///
    /// A [`BridgeError`] for a failed `users.list` or `conversations.list`
    /// or a store failure. A conversation whose history Slack refuses is
    /// listed in [`SyncReport::skipped`] instead.
    pub async fn initial_sync(&self) -> Result<SyncReport, BridgeError> {
        let client = &self.shared.client;
        let mut report = SyncReport::default();
        let users =
            all_pages(|cursor| async move { client.users_list(cursor.as_deref()).await }).await?;
        let mut st = self.shared.state.lock().await;
        for user in &users {
            if user.id == self.shared.owner_user {
                st.names.insert(user.id.clone(), user.display().to_owned());
                continue;
            }
            if self.upsert_contact(&mut st, user).await? {
                report.contacts += 1;
            }
        }
        self.write_space(&st).await?;

        let types = self.shared.config.conversation_types.as_str();
        let conversations =
            all_pages(
                |cursor| async move { client.conversations_list(types, cursor.as_deref()).await },
            )
            .await?;
        for conv in &conversations {
            let known = st.chats.contains_key(&conv.id);
            self.ensure_chat(&mut st, conv).await?;
            if !known {
                report.conversations += 1;
            }
            match self.backfill(&mut st, &conv.id).await {
                Ok(n) => report.messages += n,
                Err(BridgeError::Slack(SlackError::Api { code, .. })) => {
                    report.skipped.push(SkippedConversation {
                        channel: conv.id.clone(),
                        error: code,
                    });
                }
                Err(e) => return Err(e),
            }
        }
        Ok(report)
    }

    async fn backfill(&self, st: &mut MapState, channel: &str) -> Result<usize, BridgeError> {
        let client = &self.shared.client;
        let oldest = self.shared.config.history_oldest.as_deref();
        let mut history = all_pages(|cursor| async move {
            client
                .conversations_history(channel, oldest, cursor.as_deref())
                .await
        })
        .await?;
        // conversations.history is newest first; store oldest first so the
        // minted ULIDs follow Slack's order.
        history.reverse();
        let mut n = 0;
        for m in &history {
            if self.upsert_message(st, channel, m).await? == EventOutcome::Applied {
                n += 1;
            }
            let is_parent = m.reply_count.is_some_and(|c| c > 0)
                && m.thread_ts.as_deref().is_none_or(|t| t == m.ts);
            if is_parent && self.shared.config.backfill_threads {
                let ts = m.ts.as_str();
                let replies = all_pages(|cursor| async move {
                    client
                        .conversations_replies(channel, ts, cursor.as_deref())
                        .await
                })
                .await?;
                for r in replies.iter().filter(|r| r.ts != m.ts) {
                    if self.upsert_message(st, channel, r).await? == EventOutcome::Applied {
                        n += 1;
                    }
                }
            }
        }
        Ok(n)
    }

    // -----------------------------------------------------------------
    // Contacts and the Space
    // -----------------------------------------------------------------

    /// Create or refresh the ChatContact of `user`; `true` when created.
    async fn upsert_contact(
        &self,
        st: &mut MapState,
        user: &SlackUser,
    ) -> Result<bool, BridgeError> {
        let login = user.display().to_owned();
        st.names.insert(user.id.clone(), login.clone());
        if let Some(id) = st.users.get(&user.id).cloned() {
            if id != *self.account() {
                let patch =
                    PatchObject::from_map(Map::from_iter([("login".to_owned(), json!(login))]));
                self.inner
                    .update_object::<ChatContact>(&(), self.account(), &id, patch)
                    .await
                    .map_err(store_err)?;
            }
            return Ok(false);
        }
        let now = now_utc();
        let id = self.inner.mint_id();
        let contact = ChatContact::new(
            id.clone(),
            self.identity(&user.id),
            login,
            now.clone(),
            now.clone(),
            false,
        );
        self.inner
            .insert_object(self.account(), contact)
            .await
            .map_err(store_err)?;
        st.users.insert(user.id.clone(), id.clone());
        st.contact_users.insert(id.clone(), user.id.clone());
        if !user.deleted {
            st.space_members.push((id, now));
        }
        Ok(true)
    }

    async fn ensure_user(&self, st: &mut MapState, slack_user: &str) -> Result<Id, BridgeError> {
        if let Some(id) = st.users.get(slack_user) {
            return Ok(id.clone());
        }
        let user = self.shared.client.users_info(slack_user).await?;
        self.upsert_contact(st, &user).await?;
        self.write_space(st).await?;
        st.users
            .get(slack_user)
            .cloned()
            .ok_or_else(|| BridgeError::Store("contact not recorded".to_owned()))
    }

    /// The ChatContact of a bot that posts without a user id.
    async fn ensure_bot(
        &self,
        st: &mut MapState,
        bot_id: &str,
        username: Option<&str>,
    ) -> Result<Id, BridgeError> {
        if let Some(id) = st.users.get(bot_id) {
            return Ok(id.clone());
        }
        let now = now_utc();
        let id = self.inner.mint_id();
        let login = username.filter(|u| !u.trim().is_empty()).unwrap_or(bot_id);
        let contact = ChatContact::new(
            id.clone(),
            self.identity(bot_id),
            login,
            now.clone(),
            now,
            false,
        );
        self.inner
            .insert_object(self.account(), contact)
            .await
            .map_err(store_err)?;
        st.users.insert(bot_id.to_owned(), id.clone());
        st.contact_users.insert(id.clone(), bot_id.to_owned());
        st.names.insert(bot_id.to_owned(), login.to_owned());
        Ok(id)
    }

    /// Write the Space's `members`, `memberCount` and
    /// `uncategorizedChannelIds` from the mapping.
    async fn write_space(&self, st: &MapState) -> Result<(), BridgeError> {
        let (spaces, _) = self
            .inner
            .get_objects::<Space>(
                &(),
                self.account(),
                Some(std::slice::from_ref(&self.shared.space_id)),
                None,
            )
            .await
            .map_err(read_err)?;
        let Some(space) = spaces.into_iter().next() else {
            return Err(BridgeError::Store("Space missing".to_owned()));
        };
        let owner = space
            .members
            .iter()
            .find(|m| m.id == *self.account())
            .map(value_of)
            .transpose()?;
        let mut members: Vec<Value> = owner.into_iter().collect();
        for (id, joined) in &st.space_members {
            members.push(json!({ "id": id, "roleIds": [], "joinedAt": joined }));
        }
        let new_members = Value::Array(members);
        let new_uncat = value_of(&st.uncategorized)?;
        if value_of(&space.members)? == new_members
            && value_of(&space.uncategorized_channel_ids)? == new_uncat
        {
            return Ok(());
        }
        let count = new_members.as_array().map_or(0, Vec::len);
        let patch = PatchObject::from_map(Map::from_iter([
            ("memberCount".to_owned(), json!(count)),
            ("members".to_owned(), new_members),
            ("uncategorizedChannelIds".to_owned(), new_uncat),
        ]));
        self.inner
            .update_object::<Space>(&(), self.account(), &self.shared.space_id, patch)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    // -----------------------------------------------------------------
    // Conversations
    // -----------------------------------------------------------------

    async fn ensure_chat(&self, st: &mut MapState, conv: &Conversation) -> Result<Id, BridgeError> {
        if let Some(entry) = st.chats.get_mut(&conv.id) {
            entry.archived = conv.is_archived;
            let id = entry.id.clone();
            if conv.name.is_some() && entry.kind == ChatKind::Channel && entry.name != conv.name {
                let name = conv.name.clone();
                entry.name = name.clone();
                self.patch_chat(&id, "name", json!(name)).await?;
            }
            return Ok(id);
        }
        let id = self.inner.mint_id();
        let created = utc_from_unix(conv.created, 0);
        let (kind, mut chat) = if conv.is_im {
            let Some(other) = conv.user.as_deref() else {
                return Err(BridgeError::Store("IM without a user".to_owned()));
            };
            let contact = self.ensure_user(st, other).await?;
            let mut chat = Chat::new(
                id.clone(),
                ChatKind::Direct,
                created,
                0,
                Vec::new(),
                false,
                true,
            );
            chat.contact_id = Some(contact);
            (ChatKind::Direct, chat)
        } else if conv.is_group_dm() {
            let client = &self.shared.client;
            let channel = conv.id.as_str();
            let member_ids = all_pages(|cursor| async move {
                client
                    .conversations_members(channel, cursor.as_deref())
                    .await
            })
            .await?;
            let mut members = Vec::new();
            let mut names = Vec::new();
            for m in &member_ids {
                let mid = self.ensure_user(st, m).await?;
                if *m != self.shared.owner_user {
                    names.push(st.names.get(m).cloned().unwrap_or_else(|| m.clone()));
                }
                members.push(ChatMember::new(
                    mid,
                    ChatMemberRole::Member,
                    created.clone(),
                ));
            }
            let mut chat = Chat::new(
                id.clone(),
                ChatKind::Group,
                created,
                0,
                Vec::new(),
                false,
                true,
            );
            chat.name = Some(if names.is_empty() {
                conv.name.clone().unwrap_or_else(|| conv.id.clone())
            } else {
                names.join(", ")
            });
            chat.members = Some(members);
            (ChatKind::Group, chat)
        } else {
            let mut chat = Chat::new(
                id.clone(),
                ChatKind::Channel,
                created,
                0,
                Vec::new(),
                false,
                true,
            );
            chat.name = Some(conv.name.clone().unwrap_or_else(|| conv.id.clone()));
            chat.space_id = Some(self.shared.space_id.clone());
            chat.topic = conv
                .topic
                .as_ref()
                .map(|t| t.value.clone())
                .filter(|t| !t.is_empty());
            (ChatKind::Channel, chat)
        };
        if kind == ChatKind::Channel {
            chat.position = Some(st.uncategorized.len() as u64);
        }
        let name = chat.name.clone();
        self.inner
            .insert_object(self.account(), chat)
            .await
            .map_err(store_err)?;
        st.chats.insert(
            conv.id.clone(),
            ChatEntry {
                id: id.clone(),
                kind: kind.clone(),
                archived: conv.is_archived,
                name,
            },
        );
        st.chat_channels.insert(id.clone(), conv.id.clone());
        if kind == ChatKind::Channel {
            st.uncategorized.push(id.clone());
            self.write_space(st).await?;
        }
        Ok(id)
    }

    async fn patch_chat(&self, id: &Id, key: &str, value: Value) -> Result<(), BridgeError> {
        let patch = PatchObject::from_map(Map::from_iter([(key.to_owned(), value)]));
        self.inner
            .update_object::<Chat>(&(), self.account(), id, patch)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn chat_for(&self, st: &mut MapState, channel: &str) -> Result<Id, BridgeError> {
        if let Some(c) = st.chats.get(channel) {
            return Ok(c.id.clone());
        }
        let conv = self.shared.client.conversations_info(channel).await?;
        self.ensure_chat(st, &conv).await
    }

    /// Remove a conversation the owner can no longer see, with its
    /// Messages.
    async fn remove_chat(
        &self,
        st: &mut MapState,
        channel: &str,
    ) -> Result<EventOutcome, BridgeError> {
        let Some(entry) = st.chats.remove(channel) else {
            return Ok(EventOutcome::Ignored);
        };
        st.chat_channels.remove(&entry.id);
        let keys: Vec<(String, String)> = st
            .messages
            .keys()
            .filter(|(c, _)| c == channel)
            .cloned()
            .collect();
        for key in keys {
            if let Some(m) = st.messages.remove(&key) {
                st.message_keys.remove(&m.id);
                st.reactions.retain(|(mid, _, _), _| *mid != m.id);
                match self
                    .inner
                    .destroy_object::<Message>(&(), self.account(), &m.id)
                    .await
                {
                    Ok(()) => {}
                    Err(BackendSetError::SetError(_)) => {} // already gone
                    Err(e) => return Err(store_err(e)),
                }
            }
        }
        self.inner
            .destroy_object::<Chat>(&(), self.account(), &entry.id)
            .await
            .map_err(store_err)?;
        if entry.kind == ChatKind::Channel {
            st.uncategorized.retain(|c| *c != entry.id);
            self.write_space(st).await?;
        }
        Ok(EventOutcome::Applied)
    }

    // -----------------------------------------------------------------
    // Messages
    // -----------------------------------------------------------------

    async fn sender_of(
        &self,
        st: &mut MapState,
        m: &SlackMessage,
    ) -> Result<Option<Id>, BridgeError> {
        if let Some(user) = m.user.as_deref() {
            return self.ensure_user(st, user).await.map(Some);
        }
        if let Some(bot) = m.bot_id.as_deref() {
            return self
                .ensure_bot(st, bot, m.username.as_deref())
                .await
                .map(Some);
        }
        Ok(None)
    }

    /// Make sure every `<@U…>` in `text` has a ChatContact, so mentions
    /// resolve. A user Slack will not describe stays an unresolved mention.
    async fn ensure_mentioned(&self, st: &mut MapState, text: &str) {
        let mut rest = text;
        while let Some(i) = rest.find("<@") {
            let after = &rest[i + 2..];
            let end = after.find(['>', '|']).unwrap_or(after.len());
            let user = &after[..end];
            if !user.is_empty() && !st.users.contains_key(user) {
                // Best effort: an unknown user becomes plain text.
                let _ = self.ensure_user(st, user).await;
            }
            rest = &after[end..];
        }
    }

    async fn reaction_sender(&self, st: &mut MapState, user: &str) -> Result<Id, BridgeError> {
        self.ensure_user(st, user).await
    }

    /// Store a Slack message as a Message, or reconcile the stored one.
    pub(crate) async fn upsert_message(
        &self,
        st: &mut MapState,
        channel: &str,
        m: &SlackMessage,
    ) -> Result<EventOutcome, BridgeError> {
        if m.subtype
            .as_deref()
            .is_some_and(|s| !STORED_SUBTYPES.contains(&s))
        {
            return Ok(EventOutcome::Ignored);
        }
        let key = (channel.to_owned(), m.ts.clone());
        if let Some(entry) = st.messages.get(&key).cloned() {
            return self.reconcile(st, channel, &entry, m).await;
        }
        let Some(sender) = self.sender_of(st, m).await? else {
            return Ok(EventOutcome::Ignored);
        };
        let chat_id = self.chat_for(st, channel).await?;
        self.ensure_mentioned(st, &m.text).await;
        let converted = mrkdwn::to_jmap(&m.text, &*st);
        let sent = utc_from_ts(&m.ts).unwrap_or_else(now_utc);
        let id = self.inner.mint_id();
        let mut msg = Message::new(
            id.clone(),
            id.clone(),
            sender,
            chat_id.clone(),
            converted.body,
            converted.body_type,
            sent.clone(),
            sent.clone(),
            DeliveryState::Delivered,
        );
        msg.mentions = converted.mentions.into_iter().map(Mention::new).collect();
        msg.broadcast_mentions = converted
            .broadcast_mentions
            .into_iter()
            .map(BroadcastMention::new)
            .collect();
        let thread_ts = m.thread_ts.clone().filter(|t| *t != m.ts);
        if let Some(root) = thread_ts
            .as_ref()
            .and_then(|t| st.messages.get(&(channel.to_owned(), t.clone())))
        {
            msg.reply_to = Some(root.id.clone());
            msg.thread_root_id = Some(root.id.clone());
        }
        msg.edited_at = m.edited.as_ref().and_then(|e| utc_from_ts(&e.ts));
        let mut reaction_keys = Vec::new();
        for r in &m.reactions {
            for user in &r.users {
                let sender = self.reaction_sender(st, user).await?;
                let rid = self.inner.mint_id();
                let reaction = from_value(json!({
                    "emoji": crate::emoji::from_slack(&r.name),
                    "senderId": sender,
                    "sentAt": sent,
                }))?;
                msg.reactions.insert(rid.to_string(), reaction);
                reaction_keys.push(((id.clone(), user.clone(), r.name.clone()), rid));
            }
        }
        self.inner
            .insert_object(self.account(), msg)
            .await
            .map_err(store_err)?;
        st.reactions.extend(reaction_keys);
        st.message_keys.insert(id.clone(), key.clone());
        st.messages.insert(
            key,
            MsgEntry {
                id,
                text: m.text.clone(),
                edited: m.edited.as_ref().map(|e| e.ts.clone()),
                thread_ts,
                deleted: false,
            },
        );
        self.patch_chat(&chat_id, "lastMessageAt", json!(sent))
            .await?;
        Ok(EventOutcome::Applied)
    }

    async fn reconcile(
        &self,
        st: &mut MapState,
        channel: &str,
        entry: &MsgEntry,
        m: &SlackMessage,
    ) -> Result<EventOutcome, BridgeError> {
        if entry.deleted {
            return Ok(EventOutcome::Unchanged);
        }
        let mut patch = Map::new();
        let edited = m.edited.as_ref().map(|e| e.ts.clone());
        // A body changes only on a Slack edit (a new `edited.ts`) whose text
        // is not the text this bridge sent: unfurls and reply-count
        // updates also arrive as `message_changed`, and the echo of a
        // JMAP edit carries the text the bridge sent.
        if edited != entry.edited && m.text != entry.text {
            self.ensure_mentioned(st, &m.text).await;
            let c = mrkdwn::to_jmap(&m.text, &*st);
            patch.insert("body".to_owned(), json!(c.body));
            patch.insert("bodyType".to_owned(), value_of(&c.body_type)?);
            let mentions: Vec<Mention> = c.mentions.into_iter().map(Mention::new).collect();
            patch.insert("mentions".to_owned(), value_of(&mentions)?);
            let bms: Vec<BroadcastMention> = c
                .broadcast_mentions
                .into_iter()
                .map(BroadcastMention::new)
                .collect();
            patch.insert("broadcastMentions".to_owned(), value_of(&bms)?);
            if let Some(at) = edited.as_deref().and_then(utc_from_ts) {
                patch.insert("editedAt".to_owned(), json!(at));
            }
        }
        // Reactions: add what Slack lists and the store lacks; remove what
        // the store holds and Slack no longer lists (unless Slack's user
        // list for that emoji is truncated).
        let mut wanted: Vec<(String, String)> = Vec::new();
        let mut truncated: Vec<&str> = Vec::new();
        for r in &m.reactions {
            if (r.users.len() as u64) < r.count {
                truncated.push(&r.name);
            }
            for u in &r.users {
                wanted.push((u.clone(), r.name.clone()));
            }
        }
        let sent = utc_from_ts(&m.ts).unwrap_or_else(now_utc);
        let mut added = Vec::new();
        for (user, name) in &wanted {
            let k = (entry.id.clone(), user.clone(), name.clone());
            if !st.reactions.contains_key(&k) {
                let sender = self.reaction_sender(st, user).await?;
                let rid = self.inner.mint_id();
                patch.insert(
                    format!("reactions/{rid}"),
                    json!({ "emoji": crate::emoji::from_slack(name), "senderId": sender, "sentAt": sent }),
                );
                added.push((k, rid));
            }
        }
        let removed: Vec<(Id, String, String)> = st
            .reactions
            .keys()
            .filter(|(mid, u, n)| {
                *mid == entry.id
                    && !truncated.contains(&n.as_str())
                    && !wanted.iter().any(|(wu, wn)| wu == u && wn == n)
            })
            .cloned()
            .collect();
        for k in &removed {
            if let Some(rid) = st.reactions.get(k) {
                patch.insert(format!("reactions/{rid}"), Value::Null);
            }
        }
        if let Some(e) = st.messages.get_mut(&(channel.to_owned(), m.ts.clone())) {
            e.edited = edited;
            e.text = m.text.clone();
        }
        if patch.is_empty() {
            return Ok(EventOutcome::Unchanged);
        }
        self.inner
            .update_object::<Message>(&(), self.account(), &entry.id, PatchObject::from_map(patch))
            .await
            .map_err(store_err)?;
        st.reactions.extend(added);
        for k in removed {
            st.reactions.remove(&k);
        }
        Ok(EventOutcome::Applied)
    }

    async fn tombstone(
        &self,
        st: &mut MapState,
        channel: &str,
        ts: &str,
    ) -> Result<EventOutcome, BridgeError> {
        let Some(entry) = st.messages.get_mut(&(channel.to_owned(), ts.to_owned())) else {
            return Ok(EventOutcome::Ignored);
        };
        if entry.deleted {
            return Ok(EventOutcome::Unchanged);
        }
        entry.deleted = true;
        let id = entry.id.clone();
        st.reactions.retain(|(mid, _, _), _| *mid != id);
        // draft-atwood-jmap-chat-00 §Message `deletedAt`: "When set, `body`
        // is empty, `attachments` is empty, and `editHistory` is absent."
        let patch = PatchObject::from_map(Map::from_iter([
            ("deletedAt".to_owned(), json!(now_utc())),
            ("deletedForAll".to_owned(), json!(true)),
            ("body".to_owned(), json!("")),
            ("attachments".to_owned(), json!([])),
            ("mentions".to_owned(), json!([])),
            ("broadcastMentions".to_owned(), json!([])),
            ("reactions".to_owned(), json!({})),
        ]));
        self.inner
            .update_object::<Message>(&(), self.account(), &id, patch)
            .await
            .map_err(store_err)?;
        Ok(EventOutcome::Applied)
    }

    // -----------------------------------------------------------------
    // Events API
    // -----------------------------------------------------------------

    /// Apply one Events API event: the `event` object of an
    /// `event_callback` (see [`crate::events`]).
    ///
    /// | Event | Effect |
    /// |---|---|
    /// | `message` (no subtype, `bot_message`, `thread_broadcast`, `me_message`, `file_share`) | Message created (a known `ts` is reconciled) |
    /// | `message` / `message_changed` | body, mentions, `editedAt` and reactions reconciled |
    /// | `message` / `message_deleted` | tombstone: `deletedAt`, `deletedForAll`, empty body |
    /// | `reaction_added` / `reaction_removed` | `reactions/<senderReactionId>` added / removed |
    /// | `channel_created` | channel Chat created, listed in the Space |
    /// | `channel_rename`, `group_rename` | Chat `name` |
    /// | `channel_archive`, `group_archive` / `…_unarchive` | new Messages refused / allowed again |
    /// | `channel_deleted`, `group_deleted` | Chat and its Messages destroyed |
    /// | `member_joined_channel` | owner: the conversation is imported with its history; others: ChatContact, group `members` |
    /// | `member_left_channel` | owner: Chat destroyed; others: group `members` |
    /// | `team_join`, `user_change` | ChatContact created / `login` refreshed |
    ///
    /// Everything else is [`EventOutcome::Ignored`].
    ///
    /// # Errors
    ///
    /// A [`BridgeError`] when a lookup call (`users.info`,
    /// `conversations.info`, …) or a store write fails. The event can be
    /// applied again; a full resync with [`initial_sync`](Self::initial_sync)
    /// also repairs the state.
    pub async fn apply_event(&self, event: &Value) -> Result<EventOutcome, BridgeError> {
        let s = |k: &str| event.get(k).and_then(Value::as_str);
        let mut st = self.shared.state.lock().await;
        match s("type") {
            Some("message") => {
                let Some(channel) = s("channel") else {
                    return Ok(EventOutcome::Ignored);
                };
                match s("subtype") {
                    Some("message_changed") => {
                        let Some(m) = event
                            .get("message")
                            .and_then(|m| SlackMessage::deserialize(m).ok())
                        else {
                            return Ok(EventOutcome::Ignored);
                        };
                        let key = (channel.to_owned(), m.ts.clone());
                        match st.messages.get(&key).cloned() {
                            Some(entry) => self.reconcile(&mut st, channel, &entry, &m).await,
                            None => self.upsert_message(&mut st, channel, &m).await,
                        }
                    }
                    Some("message_deleted") => match s("deleted_ts") {
                        Some(ts) => self.tombstone(&mut st, channel, ts).await,
                        None => Ok(EventOutcome::Ignored),
                    },
                    _ => match SlackMessage::deserialize(event) {
                        Ok(m) => self.upsert_message(&mut st, channel, &m).await,
                        Err(_) => Ok(EventOutcome::Ignored),
                    },
                }
            }
            Some(kind @ ("reaction_added" | "reaction_removed")) => {
                let item = event.get("item");
                let field = |k: &str| item.and_then(|i| i.get(k)).and_then(Value::as_str);
                let (Some("message"), Some(channel), Some(ts), Some(user), Some(name)) = (
                    field("type"),
                    field("channel"),
                    field("ts"),
                    s("user"),
                    s("reaction"),
                ) else {
                    return Ok(EventOutcome::Ignored);
                };
                let at = s("event_ts").and_then(utc_from_ts).unwrap_or_else(now_utc);
                self.apply_reaction(
                    &mut st,
                    channel,
                    ts,
                    user,
                    name,
                    kind == "reaction_added",
                    at,
                )
                .await
            }
            Some("channel_created") => {
                let Some(mut conv) = event
                    .get("channel")
                    .and_then(|c| Conversation::deserialize(c).ok())
                else {
                    return Ok(EventOutcome::Ignored);
                };
                conv.is_channel = true;
                if st.chats.contains_key(&conv.id) {
                    return Ok(EventOutcome::Unchanged);
                }
                self.ensure_chat(&mut st, &conv).await?;
                Ok(EventOutcome::Applied)
            }
            Some("channel_rename" | "group_rename") => {
                let c = event.get("channel");
                let (Some(id), Some(name)) = (
                    c.and_then(|c| c.get("id")).and_then(Value::as_str),
                    c.and_then(|c| c.get("name")).and_then(Value::as_str),
                ) else {
                    return Ok(EventOutcome::Ignored);
                };
                let Some(entry) = st.chats.get_mut(id) else {
                    return Ok(EventOutcome::Ignored);
                };
                if entry.name.as_deref() == Some(name) || entry.kind != ChatKind::Channel {
                    return Ok(EventOutcome::Unchanged);
                }
                entry.name = Some(name.to_owned());
                let chat_id = entry.id.clone();
                self.patch_chat(&chat_id, "name", json!(name)).await?;
                Ok(EventOutcome::Applied)
            }
            Some(
                kind @ ("channel_archive" | "group_archive" | "channel_unarchive"
                | "group_unarchive"),
            ) => {
                let archived = kind.ends_with("_archive");
                match s("channel").and_then(|c| st.chats.get_mut(c)) {
                    Some(entry) if entry.archived != archived => {
                        entry.archived = archived;
                        Ok(EventOutcome::Applied)
                    }
                    Some(_) => Ok(EventOutcome::Unchanged),
                    None => Ok(EventOutcome::Ignored),
                }
            }
            Some("channel_deleted" | "group_deleted") => match s("channel") {
                Some(channel) => self.remove_chat(&mut st, channel).await,
                None => Ok(EventOutcome::Ignored),
            },
            Some("member_joined_channel") => {
                let (Some(user), Some(channel)) = (s("user"), s("channel")) else {
                    return Ok(EventOutcome::Ignored);
                };
                if user == self.shared.owner_user {
                    if st.chats.contains_key(channel) {
                        return Ok(EventOutcome::Unchanged);
                    }
                    let conv = self.shared.client.conversations_info(channel).await?;
                    self.ensure_chat(&mut st, &conv).await?;
                    self.backfill(&mut st, channel).await?;
                    return Ok(EventOutcome::Applied);
                }
                let contact = self.ensure_user(&mut st, user).await?;
                self.change_group_member(&mut st, channel, &contact, true)
                    .await
            }
            Some("member_left_channel") => {
                let (Some(user), Some(channel)) = (s("user"), s("channel")) else {
                    return Ok(EventOutcome::Ignored);
                };
                if user == self.shared.owner_user {
                    return self.remove_chat(&mut st, channel).await;
                }
                let Some(contact) = st.users.get(user).cloned() else {
                    return Ok(EventOutcome::Ignored);
                };
                self.change_group_member(&mut st, channel, &contact, false)
                    .await
            }
            Some("team_join" | "user_change") => {
                let Some(user) = event
                    .get("user")
                    .and_then(|u| SlackUser::deserialize(u).ok())
                else {
                    return Ok(EventOutcome::Ignored);
                };
                if user.id == self.shared.owner_user {
                    st.names.insert(user.id.clone(), user.display().to_owned());
                    return Ok(EventOutcome::Unchanged);
                }
                self.upsert_contact(&mut st, &user).await?;
                self.write_space(&st).await?;
                Ok(EventOutcome::Applied)
            }
            _ => Ok(EventOutcome::Ignored),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_reaction(
        &self,
        st: &mut MapState,
        channel: &str,
        ts: &str,
        user: &str,
        name: &str,
        added: bool,
        at: UTCDate,
    ) -> Result<EventOutcome, BridgeError> {
        let Some(entry) = st
            .messages
            .get(&(channel.to_owned(), ts.to_owned()))
            .cloned()
        else {
            return Ok(EventOutcome::Ignored);
        };
        if entry.deleted {
            return Ok(EventOutcome::Ignored);
        }
        let k = (entry.id.clone(), user.to_owned(), name.to_owned());
        let (patch, rid) = match (added, st.reactions.get(&k).cloned()) {
            (true, Some(_)) | (false, None) => return Ok(EventOutcome::Unchanged),
            (true, None) => {
                let sender = self.reaction_sender(st, user).await?;
                let rid = self.inner.mint_id();
                let v = json!({ "emoji": crate::emoji::from_slack(name), "senderId": sender, "sentAt": at });
                (Map::from_iter([(format!("reactions/{rid}"), v)]), rid)
            }
            (false, Some(rid)) => (
                Map::from_iter([(format!("reactions/{rid}"), Value::Null)]),
                rid,
            ),
        };
        self.inner
            .update_object::<Message>(&(), self.account(), &entry.id, PatchObject::from_map(patch))
            .await
            .map_err(store_err)?;
        if added {
            st.reactions.insert(k, rid);
        } else {
            st.reactions.remove(&k);
        }
        Ok(EventOutcome::Applied)
    }

    async fn change_group_member(
        &self,
        st: &mut MapState,
        channel: &str,
        contact: &Id,
        join: bool,
    ) -> Result<EventOutcome, BridgeError> {
        let Some(entry) = st.chats.get(channel).filter(|c| c.kind == ChatKind::Group) else {
            return Ok(EventOutcome::Ignored);
        };
        let (chats, _) = self
            .inner
            .get_objects::<Chat>(
                &(),
                self.account(),
                Some(std::slice::from_ref(&entry.id)),
                None,
            )
            .await
            .map_err(read_err)?;
        let Some(chat) = chats.into_iter().next() else {
            return Ok(EventOutcome::Ignored);
        };
        let mut members = chat.members.unwrap_or_default();
        let present = members.iter().any(|m| m.id == *contact);
        match (join, present) {
            (true, false) => members.push(ChatMember::new(
                contact.clone(),
                ChatMemberRole::Member,
                now_utc(),
            )),
            (false, true) => members.retain(|m| m.id != *contact),
            _ => return Ok(EventOutcome::Unchanged),
        }
        let id = entry.id.clone();
        self.patch_chat(&id, "members", value_of(&members)?).await?;
        Ok(EventOutcome::Applied)
    }
}

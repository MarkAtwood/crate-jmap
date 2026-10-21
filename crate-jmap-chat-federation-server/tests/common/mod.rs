//! The deployment backend the `Peer/*` handler tests run over, and the
//! hand-written JSON records they seed.

#![allow(dead_code)] // Each test binary uses a subset of the helpers.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use jmap_chat_federation_server::{
    handle_peer_deliver, handle_peer_group_update, handle_peer_presence, handle_peer_receipt,
    handle_peer_retract, handle_peer_subscribe_presence, handle_peer_typing,
    handle_peer_unsubscribe_presence, AuthenticatedPeer, FederationBackend,
    FederationPresenceBackend, FetchedBlob,
};
use jmap_chat_federation_types::{PeerAttachment, PeerAvatar};
use jmap_chat_server::memory::{MemoryBackend, MemoryError};
use jmap_chat_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, ChatBackend,
    ChatLimits, GetObject, InviteJoinRefusal, JmapBackend, JmapObject, OpResult,
    QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult, SetError,
    SetErrorType, SetObject, SpacePatchOp,
};
use jmap_chat_types::{
    Chat, ChatContact, ChatPresenceEvent, ChatTypingEvent, Message, Space, SpaceMetadataPatch,
};
use jmap_peer_auth::PeerDomain;
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};

pub const ACCOUNT: &str = "A1";
pub const OWNER: &str = "owner@home.example";
pub const PEER_DOMAIN: &str = "peer.example";
pub const BOB: &str = "bob@peer.example";
pub const BOB_ID: &str = "contact-bob";
pub const CAROL: &str = "carol@other.example";
pub const CAROL_ID: &str = "contact-carol";
/// Two chat ids in ULID form; `LOW` sorts before `HIGH` byte by byte.
pub const LOW: &str = "01J0000000000000000000000A";
pub const HIGH: &str = "01J0000000000000000000000B";
pub const GROUP: &str = "01J000000000000000000000G1";
pub const CHANNEL: &str = "01J000000000000000000000C1";
pub const SPACE: &str = "space-1";
pub const PAST: &str = "2000-01-01T00:00:00Z";
pub const FUTURE: &str = "2999-01-01T00:00:00Z";
pub const FETCH_URL: &str = "https://peer.example/blob/1";
/// FIPS 180-2 Appendix B.1: SHA-256("abc").
pub const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

pub fn hex32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex fixture");
    }
    out
}

/// A deployment backend: chat storage in a `MemoryBackend`, plus the
/// federation hooks with test-controlled state.
pub struct Fed {
    pub chat: MemoryBackend,
    pub peer: Option<AuthenticatedPeer>,
    pub everyone: HashSet<jmap_chat_types::ChatPermission>,
    pub blobs: HashMap<String, FetchedBlob>,
    pub fetched: Mutex<Vec<String>>,
    pub next_id: AtomicU64,
    /// Events passed to `forward_typing`, in order.
    pub typed: Mutex<Vec<ChatTypingEvent>>,
    /// Presence subscriptions: subscriber identity to granted TTL.
    pub subscriptions: Mutex<HashMap<String, u64>>,
    /// Events passed to `forward_presence`, in order.
    pub presences: Mutex<Vec<ChatPresenceEvent>>,
    /// The most ChatContacts the account may hold; a create beyond it
    /// fails with `rateLimit`. `None` is no limit.
    pub contact_cap: Option<usize>,
}

impl Fed {
    /// Account [`ACCOUNT`] with owner [`OWNER`], called by the peer at
    /// [`PEER_DOMAIN`] whose identity is [`BOB`]. Bob is a known
    /// ChatContact. "abc" is fetchable at [`FETCH_URL`].
    pub fn new() -> Self {
        let chat = MemoryBackend::new();
        chat.register_account(&Id::from(ACCOUNT));
        chat.set_owner_user_id_for_test(&Id::from(ACCOUNT), OWNER);
        let fed = Self {
            chat,
            peer: Some(AuthenticatedPeer::new(
                PeerDomain::parse(PEER_DOMAIN).expect("domain fixture"),
                BOB,
            )),
            everyone: HashSet::new(),
            blobs: HashMap::from([(
                FETCH_URL.to_owned(),
                FetchedBlob::new(Id::from("local-blob-1"), 3, hex32(ABC_SHA256)),
            )]),
            fetched: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(0),
            typed: Mutex::new(Vec::new()),
            subscriptions: Mutex::new(HashMap::new()),
            presences: Mutex::new(Vec::new()),
            contact_cap: None,
        };
        fed.seed("ChatContact", contact(BOB_ID, BOB, false));
        fed
    }

    pub fn seed(&self, type_name: &'static str, value: Value) {
        let id = value["id"].as_str().expect("fixture id").to_owned();
        self.chat
            .insert_object_for_test(type_name, ACCOUNT, &id, value);
    }

    pub async fn all<O: GetObject + Send + Sync>(&self) -> Vec<O> {
        self.chat
            .get_objects::<O>(&(), &Id::from(ACCOUNT), None, None)
            .await
            .expect("read")
            .0
    }

    pub async fn messages(&self) -> Vec<Message> {
        self.all::<Message>().await
    }

    pub async fn contacts(&self) -> Vec<ChatContact> {
        self.all::<ChatContact>().await
    }

    pub async fn chats(&self) -> Vec<Chat> {
        self.all::<Chat>().await
    }

    pub async fn deliver(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_deliver(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub async fn receipt(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_receipt(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub async fn typing(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_typing(self, &(), args).await.map(|o| o.primary)
    }

    pub async fn retract(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_retract(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub async fn group_update(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_group_update(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub async fn subscribe_presence(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_subscribe_presence(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub async fn unsubscribe_presence(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_unsubscribe_presence(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub async fn presence(&self, args: Value) -> Result<Value, JmapError> {
        handle_peer_presence(self, &(), args)
            .await
            .map(|o| o.primary)
    }

    pub fn subscriptions(&self) -> HashMap<String, u64> {
        self.subscriptions.lock().expect("lock").clone()
    }

    pub fn presences(&self) -> Vec<ChatPresenceEvent> {
        self.presences.lock().expect("lock").clone()
    }

    pub fn typed(&self) -> Vec<ChatTypingEvent> {
        self.typed.lock().expect("lock").clone()
    }
}

pub fn contact(id: &str, identity: &str, blocked: bool) -> Value {
    json!({
        "id": id,
        "identity": identity,
        "login": identity,
        "firstSeenAt": PAST,
        "lastSeenAt": PAST,
        "blocked": blocked,
    })
}

pub fn direct_chat(id: &str, contact_id: &str) -> Value {
    json!({
        "id": id,
        "kind": "direct",
        "createdAt": PAST,
        "unreadCount": 0,
        "pinnedMessageIds": [],
        "muted": false,
        "receiveTypingIndicators": true,
        "contactId": contact_id,
    })
}

pub fn group_chat(members: &[&str], closed: bool) -> Value {
    let mut chat = json!({
        "id": GROUP,
        "kind": "group",
        "createdAt": PAST,
        "unreadCount": 0,
        "pinnedMessageIds": [],
        "muted": false,
        "receiveTypingIndicators": true,
        "name": "g",
        "members": members
            .iter()
            .map(|m| json!({"id": m, "role": "member", "joinedAt": PAST}))
            .collect::<Vec<_>>(),
    });
    if closed {
        chat["closedAt"] = json!(PAST);
    }
    chat
}

pub fn channel(slow_mode_seconds: u64, overrides: Value) -> Value {
    json!({
        "id": CHANNEL,
        "kind": "channel",
        "createdAt": PAST,
        "unreadCount": 0,
        "pinnedMessageIds": [],
        "muted": false,
        "receiveTypingIndicators": true,
        "name": "c",
        "spaceId": SPACE,
        "slowModeSeconds": slow_mode_seconds,
        "permissionOverrides": overrides,
    })
}

/// A Space with role `poster` holding `role_permissions`, and Bob as a
/// member holding `poster`.
pub fn space(role_permissions: &[&str], closed: bool) -> Value {
    let mut space = json!({
        "id": SPACE,
        "name": "s",
        "roles": [{"id": "poster", "name": "poster", "permissions": role_permissions, "position": 1}],
        "members": [{"id": BOB_ID, "roleIds": ["poster"], "joinedAt": PAST}],
        "categories": [],
        "uncategorizedChannelIds": [CHANNEL],
        "createdAt": PAST,
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "memberCount": 1,
    });
    if closed {
        space["closedAt"] = json!(PAST);
    }
    space
}

pub fn stored_message(
    id: &str,
    sender_msg_id: &str,
    chat_id: &str,
    sender_id: &str,
    received_at: &str,
) -> Value {
    json!({
        "id": id,
        "senderMsgId": sender_msg_id,
        "senderId": sender_id,
        "chatId": chat_id,
        "body": "old",
        "bodyType": "text/plain",
        "attachments": [],
        "mentions": [],
        "broadcastMentions": [],
        "actions": [],
        "reactions": {},
        "sentAt": PAST,
        "receivedAt": received_at,
        "deliveryState": "delivered",
    })
}

pub fn assert_error(result: Result<Value, JmapError>, error_type: &str) {
    match result {
        Err(e) => assert_eq!(e.error_type.to_string(), error_type, "{e:?}"),
        Ok(v) => panic!("expected {error_type}, got {v}"),
    }
}

impl JmapBackend for Fed {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, caller: &(), account_id: &Id) -> Result<bool, MemoryError> {
        self.chat.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        self.chat
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        self.chat.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        self.chat
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        self.chat
            .query_objects::<O>(caller, account_id, params)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        self.chat
            .query_changes::<O>(caller, account_id, params)
            .await
    }
}

impl ChatBackend for Fed {
    async fn create_object<O, F>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<MemoryError>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        if let (Some(cap), true) = (self.contact_cap, O::TYPE_NAME == ChatContact::TYPE_NAME) {
            if self.contacts().await.len() >= cap {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::RateLimit,
                )));
            }
        }
        self.chat
            .create_object(caller, account_id, create_id, build)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        self.chat
            .update_object::<O>(caller, account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        self.chat.destroy_object::<O>(caller, account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self, caller: &(), account_id: &Id) -> bool {
        self.chat.supports_type::<O>(caller, account_id)
    }

    fn generate_invite_code(&self) -> String {
        self.chat.generate_invite_code()
    }

    async fn join_space_with_invite(
        &self,
        caller: &(),
        account_id: &Id,
        invite_id: &Id,
        joined_at: &UTCDate,
    ) -> Result<Result<(), InviteJoinRefusal>, MemoryError> {
        self.chat
            .join_space_with_invite(caller, account_id, invite_id, joined_at)
            .await
    }

    async fn read_and_burn_message(
        &self,
        caller: &(),
        account_id: &Id,
        message_id: &Id,
        patch: PatchObject,
    ) -> Result<(), BackendSetError<MemoryError>> {
        self.chat
            .read_and_burn_message(caller, account_id, message_id, patch)
            .await
    }

    async fn apply_space_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<MemoryError>> {
        self.chat
            .apply_space_patch(caller, account_id, space_id, ops)
            .await
    }

    async fn apply_space_metadata_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        patch: SpaceMetadataPatch,
    ) -> Result<Option<Space>, BackendSetError<MemoryError>> {
        self.chat
            .apply_space_metadata_patch(caller, account_id, space_id, patch)
            .await
    }

    fn limits(&self, caller: &(), account_id: &Id) -> ChatLimits {
        self.chat.limits(caller, account_id)
    }

    async fn owner_user_id(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<Option<String>, MemoryError> {
        self.chat.owner_user_id(caller, account_id).await
    }
}

impl FederationBackend for Fed {
    fn authenticated_peer(&self, _caller: &()) -> Option<AuthenticatedPeer> {
        self.peer.clone()
    }

    fn everyone_permissions(
        &self,
        _caller: &(),
        _account_id: &Id,
        _space: &Space,
    ) -> HashSet<jmap_chat_types::ChatPermission> {
        self.everyone.clone()
    }

    fn new_message_id(&self) -> Id {
        Id::from(format!("fresh-{}", self.next_id.fetch_add(1, Ordering::Relaxed)).as_str())
    }

    async fn fetch_attachment(
        &self,
        _caller: &(),
        _account_id: &Id,
        attachment: &PeerAttachment,
    ) -> Result<Result<FetchedBlob, JmapError>, MemoryError> {
        self.fetched
            .lock()
            .expect("lock")
            .push(attachment.fetch_url.clone());
        Ok(self
            .blobs
            .get(&attachment.fetch_url)
            .cloned()
            .ok_or_else(|| JmapError::invalid_arguments("fetch refused")))
    }

    async fn fetch_avatar(
        &self,
        _caller: &(),
        _account_id: &Id,
        avatar: &PeerAvatar,
    ) -> Option<FetchedBlob> {
        self.fetched
            .lock()
            .expect("lock")
            .push(avatar.fetch_url.clone());
        self.blobs.get(&avatar.fetch_url).cloned()
    }

    async fn create_chat(
        &self,
        _caller: &(),
        account_id: &Id,
        chat: Chat,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let value = serde_json::to_value(&chat).expect("Chat serializes");
        self.chat
            .insert_object_for_test("Chat", account_id.as_ref(), chat.id.as_ref(), value);
        Ok(())
    }

    async fn move_direct_chat(
        &self,
        caller: &(),
        account_id: &Id,
        from: &Id,
        to: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let (chats, _) = self
            .chat
            .get_objects::<Chat>(caller, account_id, Some(std::slice::from_ref(from)), None)
            .await
            .map_err(BackendSetError::Other)?;
        let moved: Vec<Message> = self
            .messages()
            .await
            .into_iter()
            .filter(|m| &m.chat_id == from)
            .collect();
        self.chat
            .destroy_object::<Chat>(caller, account_id, from)
            .await?;
        for chat in chats {
            let mut value = serde_json::to_value(&chat).expect("Chat serializes");
            value["id"] = json!(to);
            self.chat
                .insert_object_for_test("Chat", account_id.as_ref(), to.as_ref(), value);
        }
        for m in moved {
            let mut value = serde_json::to_value(&m).expect("Message serializes");
            value["chatId"] = json!(to);
            self.chat
                .insert_object_for_test("Message", account_id.as_ref(), m.id.as_ref(), value);
        }
        Ok(())
    }

    async fn forward_typing(&self, _caller: &(), _account_id: &Id, event: &ChatTypingEvent) {
        self.typed.lock().expect("lock").push(event.clone());
    }
}

/// The deployment's subscription policy: default 600 s, at most 3600 s.
pub const DEFAULT_TTL: u64 = 600;
pub const MAX_TTL: u64 = 3600;

impl FederationPresenceBackend for Fed {
    async fn subscribe_presence(
        &self,
        _caller: &(),
        _account_id: &Id,
        subscriber_id: &str,
        ttl: Option<u64>,
    ) -> Result<u64, MemoryError> {
        let granted = ttl.map_or(DEFAULT_TTL, |t| t.min(MAX_TTL));
        self.subscriptions
            .lock()
            .expect("lock")
            .insert(subscriber_id.to_owned(), granted);
        Ok(granted)
    }

    async fn unsubscribe_presence(
        &self,
        _caller: &(),
        _account_id: &Id,
        subscriber_id: &str,
    ) -> Result<(), MemoryError> {
        self.subscriptions
            .lock()
            .expect("lock")
            .remove(subscriber_id);
        Ok(())
    }

    async fn forward_presence(&self, _caller: &(), _account_id: &Id, event: &ChatPresenceEvent) {
        self.presences.lock().expect("lock").push(event.clone());
    }
}

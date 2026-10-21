//! In-memory reference implementation of [`MimiBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` backs this crate's integration tests and is a readable
//! example for anyone writing a real backend. All state lives in
//! `HashMap`s behind a `std::sync::Mutex`; nothing is persisted.
//!
//! # Caller context
//!
//! `CallerCtx` is `()`: every rule here is per account, not per caller.
//!
//! # Changes and query
//!
//! draft-atwood-jmap-chat-mimi-00 defines `KeyPackage/get` and
//! `KeyPackage/set` only, so the backend keeps a state counter and no
//! change log: `get_changes` reports `cannotCalculateChanges`, and the
//! query methods report `unknownMethod`.
//!
//! # Handed-out KeyPackages
//!
//! `claim_key_packages` keeps the KeyPackageRef of every KeyPackage it
//! returns, and the KeyPackageRef reuse check covers those refs too.
//! ponytail: the refs are kept forever, which the draft allows ("until a
//! Welcome naming it has been delivered or its leaf node lifetime has
//! passed" is a lower bound); a long-running backend drops them once the
//! Welcome is delivered or the lifetime passes.
//!
//! The `clientUri` of a client is `{userUri}/client/{n}`, numbered in the
//! order its signature key was bound and stored with the binding, so it
//! stays the same for that signature key.
//!
//! # Rooms
//!
//! `create_room` keeps each room it hosts and creates, in place of a
//! `jmap-chat-server` store, a Chat record holding only the fields
//! §Room/create and §Chat.roomId fix (`id`, `kind`, `name`, `roomId`,
//! `members`); `room_chat` reads it back.
//!
//! # FanoutMessages
//!
//! `store_fanout` keeps, per room Chat, its bound flag and its Messages
//! (`id`, `chatId`, `bodyType`, `body`, `senderMsgId`, `sentAt`,
//! `senderId`), and per account the hub ChatContacts (`id`, `identity`,
//! `login`, `blocked`). `room_chat_id`, `room_messages` and `hub_contact`
//! read them back; `block_contact` stands in for a `ChatContact/set` that
//! sets `blocked`.
//!
//! # Hub
//!
//! `mimi_request` stands in for the MIMI transport and for hub processing:
//! it records each request, readable with `hub_requests`, and answers with
//! the next reply queued by `queue_hub_reply`, or `HubUnavailable` when
//! none is queued. `local_key_material_account` names a local account
//! only for a room `create_room` made.
//!
//! # Concurrency
//!
//! The `await_holding_lock` lint is denied module-wide: no guard is held
//! across an `.await`.

#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use jmap_chat_mimi_types::{
    ClientKeyMaterial, KeyMaterialClientCode, KeyPackage, MimiCapability, MIMI_FANOUT_BODY_TYPE,
};
use jmap_types::{Id, JmapError, State};
use openmls::prelude::OpenMlsCrypto;
use serde_json::json;

use crate::backend::{
    group_id_in_use, key_package_over_quota, key_package_ref_in_use, room_id_in_use,
    room_signature_key_not_bound, signature_key_bound_elsewhere, BackendChangesError,
    BackendQueryError, BackendSetError, ChangesResult, GetObject, HubReply, HubUnavailable,
    JmapBackend, JmapObject, MimiBackend, MimiEndpoint, NewKeyPackage, NewRoom, QueryChangesResult,
    QueryObject, QueryResult, SetError, SetErrorType,
};
use crate::fanout::{Fanout, FanoutRecipients};
use crate::handout::{KeyMaterialRequirements, KeyPackageFit};

/// One stored KeyPackage record and the signature key of its client.
struct Record {
    key_package: KeyPackage,
    signature_key: Vec<u8>,
}

/// A room this server hosts.
struct Room {
    group_id: Vec<u8>,
}

/// A Chat bound, or once bound, to a room.
struct RoomChat {
    account: String,
    room_id: String,
    bound: bool,
    chat: serde_json::Value,
    messages: Vec<serde_json::Value>,
    /// The FanoutMessage `message` octets of each stored Message.
    mls_messages: Vec<Vec<u8>>,
}

impl RoomChat {
    /// Whether the Chat already stores `fanout`: a Message with its
    /// `senderMsgId` or a FanoutMessage with byte-identical `message`.
    fn holds(&self, fanout: &Fanout) -> bool {
        self.messages
            .iter()
            .any(|m| m["senderMsgId"] == fanout.sender_msg_id.as_str())
            || self.mls_messages.contains(&fanout.message)
    }
}

/// The account a signature key is bound to, and its client's `clientUri`.
struct Binding {
    account: String,
    client_uri: String,
}

#[derive(Default)]
struct Inner {
    /// Registered accounts and their MIMI capability object.
    accounts: HashMap<String, MimiCapability>,
    /// `account_id` → `id → record`.
    records: HashMap<String, HashMap<Id, Record>>,
    /// `account_id` → KeyPackage state counter.
    states: HashMap<String, u64>,
    /// Signature key → the account it is bound to.
    bindings: HashMap<Vec<u8>, Binding>,
    /// KeyPackageRef of every KeyPackage handed out → its account.
    handed_out: HashMap<String, String>,
    /// Room ID → hosted room.
    rooms: HashMap<String, Room>,
    /// Chat id → a room Chat.
    room_chats: HashMap<Id, RoomChat>,
    /// `(account_id, hub provider URI)` → the hub's ChatContact.
    hub_contacts: HashMap<(String, String), serde_json::Value>,
    /// Replies `mimi_request` returns, in order.
    hub_replies: VecDeque<Result<HubReply, HubUnavailable>>,
    /// Every `mimi_request`, in order.
    hub_requests: Vec<HubRequest>,
    /// Source of record ids.
    next_id: u64,
}

/// One request [`MemoryBackend`] received through `mimi_request`.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubRequest {
    /// The account the request was sent for.
    pub account_id: Id,
    /// The room.
    pub room_id: String,
    /// The endpoint.
    pub endpoint: MimiEndpoint,
    /// The request body.
    pub body: Vec<u8>,
}

impl Inner {
    fn state(&self, account_id: &Id) -> u64 {
        self.states.get(account_id.as_ref()).copied().unwrap_or(0)
    }

    fn next(&mut self, prefix: &str) -> Id {
        self.next_id += 1;
        Id::from(format!("{prefix}{}", self.next_id).as_str())
    }

    /// Create a Chat in `account` bound to `room_id`, per §Chat.roomId.
    fn create_room_chat(&mut self, account: &str, room_id: &str, name: &str) -> Id {
        let chat_id = self.next("chat");
        let chat = json!({
            "id": chat_id,
            "kind": "group",
            "name": name,
            "roomId": room_id,
            "members": [],
        });
        self.room_chats.insert(
            chat_id.clone(),
            RoomChat {
                account: account.to_owned(),
                room_id: room_id.to_owned(),
                bound: true,
                chat,
                messages: Vec::new(),
                mls_messages: Vec::new(),
            },
        );
        chat_id
    }

    fn room_chat_id(&self, account: &str, room_id: &str) -> Option<Id> {
        self.room_chats
            .iter()
            .find(|(_, c)| c.account == account && c.room_id == room_id)
            .map(|(id, _)| id.clone())
    }

    fn hub_blocked(&self, account: &str, hub_uri: &str) -> bool {
        self.hub_contacts
            .get(&(account.to_owned(), hub_uri.to_owned()))
            .is_some_and(|c| c["blocked"] == true)
    }

    /// The id of `account`'s ChatContact for `hub_uri`, created if absent.
    fn hub_contact_id(&mut self, account: &str, hub_uri: &str) -> serde_json::Value {
        let key = (account.to_owned(), hub_uri.to_owned());
        if let Some(contact) = self.hub_contacts.get(&key) {
            return contact["id"].clone();
        }
        let id = self.next("contact");
        let contact = json!({
            "id": id,
            "identity": hub_uri,
            "login": hub_uri,
            "blocked": false,
        });
        self.hub_contacts.insert(key, contact);
        json!(id)
    }

    fn bump(&mut self, account_id: &Id) {
        *self
            .states
            .entry(account_id.as_ref().to_owned())
            .or_insert(0) += 1;
    }
}

/// A fully in-memory implementation of [`MimiBackend`] over the MLS
/// crypto provider `C`.
///
/// `Clone` is a cheap `Arc` clone: every clone shares the same state.
pub struct MemoryBackend<C> {
    crypto: Arc<C>,
    inner: Arc<Mutex<Inner>>,
}

impl<C> Clone for MemoryBackend<C> {
    fn clone(&self) -> Self {
        Self {
            crypto: Arc::clone(&self.crypto),
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<C> MemoryBackend<C> {
    /// An empty backend with no accounts that verifies with `crypto`.
    pub fn new(crypto: C) -> Self {
        Self {
            crypto: Arc::new(crypto),
            inner: Arc::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock means a test already panicked mid-mutation.
        self.inner.lock().expect("MemoryBackend lock poisoned")
    }

    /// Register `account_id` with the MIMI capability object it advertises.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id, capability: MimiCapability) {
        self.lock()
            .accounts
            .insert(account_id.as_ref().to_owned(), capability);
    }

    /// The room Chat with id `chat_id`, if any.
    #[doc(hidden)]
    pub fn room_chat(&self, chat_id: &Id) -> Option<serde_json::Value> {
        self.lock().room_chats.get(chat_id).map(|c| c.chat.clone())
    }

    /// The id of the Chat in `account_id` whose `roomId` is `room_id`.
    #[doc(hidden)]
    pub fn room_chat_id(&self, account_id: &Id, room_id: &str) -> Option<Id> {
        self.lock().room_chat_id(account_id.as_ref(), room_id)
    }

    /// The Messages of the room Chat `chat_id`, in the order stored.
    #[doc(hidden)]
    pub fn room_messages(&self, chat_id: &Id) -> Vec<serde_json::Value> {
        self.lock()
            .room_chats
            .get(chat_id)
            .map(|c| c.messages.clone())
            .unwrap_or_default()
    }

    /// `account_id`'s ChatContact for the hub `hub_uri`, if it exists.
    #[doc(hidden)]
    pub fn hub_contact(&self, account_id: &Id, hub_uri: &str) -> Option<serde_json::Value> {
        self.lock()
            .hub_contacts
            .get(&(account_id.as_ref().to_owned(), hub_uri.to_owned()))
            .cloned()
    }

    /// Queue the answer to the next `mimi_request`.
    #[doc(hidden)]
    pub fn queue_hub_reply(&self, reply: Result<HubReply, HubUnavailable>) {
        self.lock().hub_replies.push_back(reply);
    }

    /// Every `mimi_request` so far, in order.
    #[doc(hidden)]
    pub fn hub_requests(&self) -> Vec<HubRequest> {
        self.lock().hub_requests.clone()
    }

    /// Set `blocked` on `account_id`'s ChatContact for the hub `hub_uri`,
    /// creating the ChatContact if it does not exist.
    #[doc(hidden)]
    pub fn block_contact(&self, account_id: &Id, hub_uri: &str) {
        let mut inner = self.lock();
        inner.hub_contact_id(account_id.as_ref(), hub_uri);
        if let Some(contact) = inner
            .hub_contacts
            .get_mut(&(account_id.as_ref().to_owned(), hub_uri.to_owned()))
        {
            contact["blocked"] = json!(true);
        }
    }
}

/// Opaque storage-layer error returned by [`MemoryBackend`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    /// A [`MemoryError`] with a human-readable description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Human-readable description of the failure.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryBackend error: {}", self.description)
    }
}

impl std::error::Error for MemoryError {}

impl<C: Send + Sync + 'static> JmapBackend for MemoryBackend<C> {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.lock().accounts.contains_key(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        let inner = self.lock();
        let records = inner
            .records
            .get(account_id.as_ref())
            .filter(|_| O::TYPE_NAME == KeyPackage::TYPE_NAME);
        let decode = |r: &Record| {
            serde_json::to_value(&r.key_package)
                .and_then(O::deserialize)
                .map_err(|e| MemoryError::new(format!("decode {}: {e}", O::TYPE_NAME)))
        };
        match ids {
            None => {
                let list = records
                    .into_iter()
                    .flat_map(HashMap::values)
                    .map(decode)
                    .collect::<Result<_, _>>()?;
                Ok((list, Vec::new()))
            }
            Some(ids) => {
                let mut found = Vec::new();
                let mut missing = Vec::new();
                for id in ids {
                    match records.and_then(|m| m.get(id)) {
                        Some(r) => found.push(decode(r)?),
                        None => missing.push(id.clone()),
                    }
                }
                Ok((found, missing))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        Ok(State::from(self.lock().state(account_id).to_string()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _since_state: &State,
        _max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        Err(BackendChangesError::CannotCalculate)
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _limit: Option<u64>,
        _position: i64,
        _anchor: Option<&Id>,
        _anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        Err(BackendQueryError::Method(JmapError::unknown_method()))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        Err(BackendChangesError::CannotCalculate)
    }
}

impl<C: OpenMlsCrypto + Send + Sync + 'static> MimiBackend for MemoryBackend<C> {
    type Crypto = C;

    fn mls_crypto(&self) -> &C {
        &self.crypto
    }

    async fn mimi_capability(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<MimiCapability, MemoryError> {
        self.lock()
            .accounts
            .get(account_id.as_ref())
            .cloned()
            .ok_or_else(|| MemoryError::new("unknown account"))
    }

    async fn create_key_package(
        &self,
        _caller: &(),
        account_id: &Id,
        key_package: NewKeyPackage,
        max_key_packages: u64,
    ) -> Result<Id, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let account = account_id.as_ref();

        if inner
            .bindings
            .get(&key_package.signature_key)
            .is_some_and(|bound| bound.account != account)
        {
            return Err(signature_key_bound_elsewhere().into());
        }
        if inner
            .records
            .values()
            .flat_map(HashMap::values)
            .any(|r| r.key_package.key_package_ref == key_package.key_package_ref)
            || inner
                .handed_out
                .contains_key(key_package.key_package_ref.as_str())
        {
            return Err(key_package_ref_in_use().into());
        }

        let records = inner.records.get(account);
        let replaced = records
            .into_iter()
            .flat_map(HashMap::iter)
            .find(|(_, r)| {
                key_package.last_resort
                    && r.key_package.last_resort
                    && r.signature_key == key_package.signature_key
            })
            .map(|(id, _)| id.clone());
        let stored = records.map_or(0, HashMap::len) - usize::from(replaced.is_some());
        if stored as u64 >= max_key_packages {
            return Err(key_package_over_quota().into());
        }

        inner.next_id += 1;
        let id = Id::from(format!("kp{}", inner.next_id).as_str());
        let record = serde_json::from_value(json!({
            "id": id,
            "keyPackage": key_package.key_package,
            "keyPackageRef": key_package.key_package_ref,
            "lastResort": key_package.last_resort,
        }))
        .map_err(|e| BackendSetError::Other(MemoryError::new(format!("encode KeyPackage: {e}"))))?;
        if !inner.bindings.contains_key(&key_package.signature_key) {
            let user_uri = inner
                .accounts
                .get(account)
                .map(|c| c.user_uri.clone())
                .ok_or_else(|| BackendSetError::Other(MemoryError::new("unknown account")))?;
            let client_uri = format!("{user_uri}/client/{}", inner.bindings.len() + 1);
            inner.bindings.insert(
                key_package.signature_key.clone(),
                Binding {
                    account: account.to_owned(),
                    client_uri,
                },
            );
        }
        let records = inner.records.entry(account.to_owned()).or_default();
        if let Some(old) = replaced {
            records.remove(&old);
        }
        records.insert(
            id.clone(),
            Record {
                key_package: record,
                signature_key: key_package.signature_key,
            },
        );
        inner.bump(account_id);
        Ok(id)
    }

    async fn claim_key_packages(
        &self,
        _caller: &(),
        account_id: &Id,
        requirements: &KeyMaterialRequirements,
    ) -> Result<Vec<ClientKeyMaterial>, MemoryError> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let account = account_id.as_ref();
        let mut clients: Vec<(&Vec<u8>, &Binding)> = inner
            .bindings
            .iter()
            .filter(|(_, b)| b.account == account)
            .collect();
        clients.sort_by(|a, b| a.1.client_uri.cmp(&b.1.client_uri));
        let records = inner.records.entry(account.to_owned()).or_default();
        let mut out = Vec::with_capacity(clients.len());
        let mut destroyed = false;
        for (signature_key, binding) in clients {
            let mut status = KeyMaterialClientCode::KeyMaterialExhausted;
            let mut last_resort = None;
            let mut fresh = None;
            for (id, r) in records
                .iter()
                .filter(|(_, r)| &r.signature_key == signature_key)
            {
                match requirements.fit(&*self.crypto, &r.key_package.key_package) {
                    KeyPackageFit::Compatible if r.key_package.last_resort => {
                        last_resort = Some(r.key_package.clone());
                    }
                    KeyPackageFit::Compatible => {
                        fresh = Some(id.clone());
                        break;
                    }
                    KeyPackageFit::Incompatible => {
                        status = KeyMaterialClientCode::NothingCompatible
                    }
                    KeyPackageFit::Unavailable => {}
                }
            }
            let handed = match fresh {
                Some(id) => {
                    destroyed = true;
                    records.remove(&id).map(|r| r.key_package)
                }
                None => last_resort,
            };
            let mut entry = ClientKeyMaterial::new(status, binding.client_uri.clone());
            if let Some(kp) = handed {
                inner
                    .handed_out
                    .insert(kp.key_package_ref.as_str().to_owned(), account.to_owned());
                entry.client_status = KeyMaterialClientCode::Success;
                entry.key_package = Some(kp.key_package);
            }
            out.push(entry);
        }
        if destroyed {
            inner.bump(account_id);
        }
        Ok(out)
    }

    async fn create_room(
        &self,
        _caller: &(),
        account_id: &Id,
        room: NewRoom,
    ) -> Result<Result<Id, JmapError>, MemoryError> {
        let mut inner = self.lock();
        if !inner
            .bindings
            .get(&room.signature_key)
            .is_some_and(|bound| bound.account == account_id.as_ref())
        {
            return Ok(Err(room_signature_key_not_bound()));
        }
        if inner.rooms.contains_key(&room.room_id) {
            return Ok(Err(room_id_in_use()));
        }
        if inner.rooms.values().any(|r| r.group_id == room.group_id) {
            return Ok(Err(group_id_in_use()));
        }
        let chat_id = inner.create_room_chat(account_id.as_ref(), &room.room_id, &room.room_name);
        inner.rooms.insert(
            room.room_id,
            Room {
                group_id: room.group_id,
            },
        );
        Ok(Ok(chat_id))
    }

    async fn store_fanout(
        &self,
        room_id: &str,
        hub_uri: &str,
        fanout: &Fanout,
    ) -> Result<(), MemoryError> {
        let mut inner = self.lock();
        let chats: Vec<Id> = match &fanout.recipients {
            FanoutRecipients::Room => inner
                .room_chats
                .iter()
                .filter(|(_, c)| c.room_id == room_id && c.bound)
                .map(|(id, _)| id.clone())
                .collect(),
            FanoutRecipients::Welcome(refs) => {
                let mut accounts: Vec<String> = refs
                    .iter()
                    .filter_map(|r| inner.handed_out.get(r.as_str()).cloned())
                    .collect();
                accounts.sort();
                accounts.dedup();
                accounts.retain(|a| !inner.hub_blocked(a, hub_uri));
                accounts
                    .iter()
                    .map(|account| match inner.room_chat_id(account, room_id) {
                        Some(id) => {
                            if let Some(chat) = inner.room_chats.get_mut(&id) {
                                if !chat.holds(fanout) {
                                    chat.bound = true;
                                }
                            }
                            id
                        }
                        None => inner.create_room_chat(account, room_id, ""),
                    })
                    .collect()
            }
        };
        for chat_id in chats {
            let Some(chat) = inner.room_chats.get(&chat_id) else {
                continue;
            };
            let account = chat.account.clone();
            if chat.holds(fanout) || inner.hub_blocked(&account, hub_uri) {
                continue;
            }
            let sender_id = inner.hub_contact_id(&account, hub_uri);
            let id = inner.next("msg");
            let message = json!({
                "id": id,
                "chatId": chat_id,
                "bodyType": MIMI_FANOUT_BODY_TYPE,
                "body": fanout.body,
                "senderMsgId": fanout.sender_msg_id,
                "sentAt": fanout.sent_at,
                "senderId": sender_id,
            });
            if let Some(chat) = inner.room_chats.get_mut(&chat_id) {
                chat.messages.push(message);
                chat.mls_messages.push(fanout.message.clone());
            }
        }
        Ok(())
    }

    async fn unbind_room(
        &self,
        _caller: &(),
        account_id: &Id,
        room_id: &str,
    ) -> Result<bool, MemoryError> {
        let mut inner = self.lock();
        let Some(id) = inner.room_chat_id(account_id.as_ref(), room_id) else {
            return Ok(false);
        };
        if let Some(chat) = inner.room_chats.get_mut(&id) {
            chat.bound = false;
        }
        Ok(true)
    }

    async fn room_bound(
        &self,
        _caller: &(),
        account_id: &Id,
        room_id: &str,
    ) -> Result<bool, MemoryError> {
        Ok(self
            .lock()
            .room_chats
            .values()
            .any(|c| c.account == account_id.as_ref() && c.room_id == room_id && c.bound))
    }

    async fn local_key_material_account(
        &self,
        _caller: &(),
        room_id: &str,
        target_user: &str,
    ) -> Result<Option<Id>, MemoryError> {
        let inner = self.lock();
        if !inner.rooms.contains_key(room_id) {
            return Ok(None);
        }
        Ok(inner
            .accounts
            .iter()
            .find(|(_, c)| c.user_uri == target_user)
            .map(|(id, _)| Id::from(id.as_str())))
    }

    async fn mimi_request(
        &self,
        _caller: &(),
        account_id: &Id,
        room_id: &str,
        endpoint: &MimiEndpoint,
        body: &[u8],
    ) -> Result<Result<HubReply, HubUnavailable>, MemoryError> {
        let mut inner = self.lock();
        inner.hub_requests.push(HubRequest {
            account_id: account_id.clone(),
            room_id: room_id.to_owned(),
            endpoint: endpoint.clone(),
            body: body.to_vec(),
        });
        Ok(inner.hub_replies.pop_front().unwrap_or(Err(HubUnavailable)))
    }

    async fn destroy_key_package(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let removed = inner
            .records
            .get_mut(account_id.as_ref())
            .and_then(|m| m.remove(id));
        if removed.is_none() {
            return Err(SetError::new(SetErrorType::NotFound).into());
        }
        inner.bump(account_id);
        Ok(())
    }
}

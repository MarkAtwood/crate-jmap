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
//! # Concurrency
//!
//! The `await_holding_lock` lint is denied module-wide: no guard is held
//! across an `.await`.

#![deny(clippy::await_holding_lock)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use jmap_chat_mimi_types::{ClientKeyMaterial, KeyMaterialClientCode, KeyPackage, MimiCapability};
use jmap_types::{Id, JmapError, State};
use openmls::prelude::OpenMlsCrypto;
use serde_json::json;

use crate::backend::{
    group_id_in_use, key_package_over_quota, key_package_ref_in_use, room_id_in_use,
    room_signature_key_not_bound, signature_key_bound_elsewhere, BackendChangesError,
    BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
    MimiBackend, NewKeyPackage, NewRoom, QueryChangesResult, QueryObject, QueryResult, SetError,
    SetErrorType,
};
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
    /// Chat id → the Chat created for a room.
    room_chats: HashMap<Id, serde_json::Value>,
    /// Source of record ids.
    next_id: u64,
}

impl Inner {
    fn state(&self, account_id: &Id) -> u64 {
        self.states.get(account_id.as_ref()).copied().unwrap_or(0)
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

    /// The Chat `create_room` created with id `chat_id`, if any.
    #[doc(hidden)]
    pub fn room_chat(&self, chat_id: &Id) -> Option<serde_json::Value> {
        self.lock().room_chats.get(chat_id).cloned()
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
        inner.next_id += 1;
        let chat_id = Id::from(format!("chat{}", inner.next_id).as_str());
        let chat = json!({
            "id": chat_id,
            "kind": "group",
            "name": room.room_name,
            "roomId": room.room_id,
            "members": [],
        });
        inner.room_chats.insert(chat_id.clone(), chat);
        inner.rooms.insert(
            room.room_id,
            Room {
                group_id: room.group_id,
            },
        );
        Ok(Ok(chat_id))
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

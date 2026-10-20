//! In-memory reference implementation of [`VtcBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` backs this crate's integration tests and is a readable
//! example for anyone writing a real backend. All state lives in
//! `HashMap`s behind a `std::sync::Mutex`; nothing is persisted, and no
//! ring notification is actually delivered. A `VTCRecordingStateEvent` is
//! recorded, not sent; `recording_events_for_test` returns them.
//!
//! # Caller context
//!
//! `CallerCtx` is the caller's VTC userId (`String`). It is what
//! [`VtcBackend::caller_user_id`] returns and what
//! [`VtcBackend::is_moderator`] checks.
//!
//! # Query
//!
//! `VTCCall/query`, `VTCRecording/query` and `VTCLivestream/query` evaluate
//! the filter and sort with the [`crate::query`] functions.
//! `/queryChanges` is not implemented: results say
//! `canCalculateChanges: false` and `/queryChanges` returns
//! `cannotCalculateChanges` (RFC 8620 §5.6).
//!
//! # Concurrency
//!
//! The `await_holding_lock` lint is denied module-wide: no guard is held
//! across an `.await`.

#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use jmap_server::{apply_patch_object, resolve_query_start};
use jmap_types::{Id, State};
use jmap_vtc_types::query::{
    VtcCallComparator, VtcCallFilter, VtcLivestreamFilter, VtcMediaComparator, VtcRecordingFilter,
};
use jmap_vtc_types::{
    CallRole, CallState, VtcCall, VtcCapability, VtcLivestream, VtcParticipant, VtcRecording,
    VtcRecordingStateEvent,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::call_already_active_on_chat;
use crate::query::{
    call_matches, compare_calls, compare_livestreams, compare_recordings, livestream_matches,
    recording_matches,
};
use crate::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    VtcBackend,
};

/// How one id changed in one state step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

type Key = (&'static str, String);

#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → stored object`.
    objects: HashMap<Key, HashMap<Id, Value>>,
    /// `(type_name, account_id)` → state counter.
    states: HashMap<Key, u64>,
    /// `(type_name, account_id)` → `(state after the change, id, change)`.
    log: HashMap<Key, Vec<(u64, Id, Change)>>,
    /// Registered accounts and their VTC capability object.
    accounts: HashMap<String, VtcCapability>,
    /// `(account_id, capability URI)` pairs advertised besides VTC.
    advertised: HashSet<(String, String)>,
    /// Every `VTCRecordingStateEvent` delivered, in order.
    recording_events: Vec<VtcRecordingStateEvent>,
}

impl Inner {
    fn objects(&self, type_name: &'static str, account_id: &Id) -> Option<&HashMap<Id, Value>> {
        self.objects
            .get(&(type_name, account_id.as_ref().to_owned()))
    }

    fn objects_mut(&mut self, type_name: &'static str, account_id: &Id) -> &mut HashMap<Id, Value> {
        self.objects
            .entry((type_name, account_id.as_ref().to_owned()))
            .or_default()
    }

    fn state(&self, type_name: &'static str, account_id: &Id) -> u64 {
        self.states
            .get(&(type_name, account_id.as_ref().to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// Advance the state of `type_name` once and log `changes` under it.
    fn commit(&mut self, type_name: &'static str, account_id: &Id, changes: Vec<(Id, Change)>) {
        let key = (type_name, account_id.as_ref().to_owned());
        let state = self.states.entry(key.clone()).or_insert(0);
        *state += 1;
        let state = *state;
        self.log
            .entry(key)
            .or_default()
            .extend(changes.into_iter().map(|(id, c)| (state, id, c)));
    }

    fn calls(&self, account_id: &Id) -> impl Iterator<Item = VtcCall> + '_ {
        self.objects(VtcCall::TYPE_NAME, account_id)
            .into_iter()
            .flat_map(HashMap::values)
            .filter_map(|v| VtcCall::deserialize(v).ok())
    }

    fn participants(&self, account_id: &Id) -> impl Iterator<Item = VtcParticipant> + '_ {
        self.all(account_id)
    }

    /// Every stored object of type `O` that decodes.
    fn all<O: JmapObject>(&self, account_id: &Id) -> impl Iterator<Item = O> + '_ {
        self.objects(O::TYPE_NAME, account_id)
            .into_iter()
            .flat_map(HashMap::values)
            .filter_map(|v| O::deserialize(v).ok())
    }
}

/// A fully in-memory implementation of [`VtcBackend`].
///
/// `Clone` is a cheap `Arc` clone: every clone shares the same state.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// An empty backend with no accounts.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock means a test already panicked mid-mutation.
        self.inner.lock().expect("MemoryBackend lock poisoned")
    }

    /// Register `account_id` with the VTC capability object it advertises.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id, capability: VtcCapability) {
        self.lock()
            .accounts
            .insert(account_id.as_ref().to_owned(), capability);
    }

    /// Advertise the companion capability `uri` (e.g.
    /// `urn:ietf:params:jmap:chat`) on `account_id`.
    #[doc(hidden)]
    pub fn advertise(&self, account_id: &Id, uri: &str) {
        self.lock()
            .advertised
            .insert((account_id.as_ref().to_owned(), uri.to_owned()));
    }

    /// The `VTCRecordingStateEvent`s delivered so far, in order.
    #[doc(hidden)]
    pub fn recording_events_for_test(&self) -> Vec<VtcRecordingStateEvent> {
        self.lock().recording_events.clone()
    }

    /// Store `value` as object `id` of `type_name` without validation or a
    /// state change, to seed states the methods cannot reach.
    #[doc(hidden)]
    pub fn insert_object_for_test(
        &self,
        type_name: &'static str,
        account_id: &Id,
        id: &str,
        value: Value,
    ) {
        self.lock()
            .objects_mut(type_name, account_id)
            .insert(Id::from(id), value);
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

fn not_found<E>() -> BackendSetError<E> {
    BackendSetError::SetError(SetError::new(SetErrorType::NotFound))
}

fn decode<O: JmapObject>(v: &Value) -> Result<O, MemoryError> {
    O::deserialize(v).map_err(|e| MemoryError::new(format!("decode {}: {e}", O::TYPE_NAME)))
}

/// A VTCParticipant record for a ring call, which nobody has joined yet.
fn ring_participant(call_id: &Id, user_id: &str, role: CallRole) -> Value {
    let response = (role == CallRole::Participant).then_some("pending");
    json!({
        "id": ulid::Ulid::new().to_string(),
        "callId": call_id,
        "userId": user_id,
        "displayName": user_id,
        "role": role,
        "joinedAt": null,
        "leftAt": null,
        "callResponse": response,
        "joinMethod": "webrtc",
        "gatewayData": null,
        "lobbyState": null,
        "mediaState": { "audio": false, "video": false, "screen": false, "raisedHand": false },
        "speakerTimeMs": 0,
        "kickedBy": null,
        "e2eeFingerprint": null,
        "unmuteRequested": false
    })
}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = String;

    async fn account_exists(&self, _caller: &String, account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.lock().accounts.contains_key(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        let inner = self.lock();
        let map = inner.objects(O::TYPE_NAME, account_id);
        match ids {
            None => {
                let list = map
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
                    match map.and_then(|m| m.get(id)) {
                        Some(v) => found.push(decode(v)?),
                        None => missing.push(id.clone()),
                    }
                }
                Ok((found, missing))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        Ok(State::from(
            self.lock().state(O::TYPE_NAME, account_id).to_string(),
        ))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        let inner = self.lock();
        let since: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;
        let current = inner.state(O::TYPE_NAME, account_id);
        if since > current {
            return Err(BackendChangesError::CannotCalculate);
        }

        // RFC 8620 §5.2: each id appears once, in the bucket of its net
        // change since `since_state`. A create followed by an update stays
        // created; a later destroy or re-create overrides.
        let mut net: Vec<(Id, Change)> = Vec::new();
        for (_, id, change) in inner
            .log
            .get(&(O::TYPE_NAME, account_id.as_ref().to_owned()))
            .into_iter()
            .flatten()
            .filter(|(s, _, _)| *s > since)
        {
            match net.iter_mut().find(|(seen, _)| seen == id) {
                Some((_, prev)) if *prev == Change::Created && *change == Change::Updated => {}
                Some((_, prev)) => *prev = *change,
                None => net.push((id.clone(), *change)),
            }
        }
        if max_changes.is_some_and(|max| net.len() as u64 > max) {
            return Err(BackendChangesError::TooManyChanges {
                limit: max_changes.unwrap_or_default(),
            });
        }
        let bucket = |want: Change| -> Vec<Id> {
            net.iter()
                .filter(|(_, c)| *c == want)
                .map(|(id, _)| id.clone())
                .collect()
        };
        Ok(ChangesResult::new(
            bucket(Change::Created),
            bucket(Change::Updated),
            bucket(Change::Destroyed),
            false,
            State::from(current.to_string()),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        // `O` is one of the queryable VTC types, so its filter and
        // comparators re-decode as that type's.
        let recode = |v: Result<Value, serde_json::Error>| {
            v.map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))
        };
        let filter = filter
            .map(|f| recode(serde_json::to_value(f)))
            .transpose()?;
        let sort = match sort {
            Some(s) => recode(serde_json::to_value(s))?,
            None => Value::Array(Vec::new()),
        };

        let inner = self.lock();
        let ids: Vec<Id> = match O::TYPE_NAME {
            VtcCall::TYPE_NAME => {
                let filter: Option<VtcCallFilter> = filter.as_ref().map(recode_as).transpose()?;
                let sort: Vec<VtcCallComparator> = recode_as(&sort)?;
                let attended: HashSet<(Id, String)> = inner
                    .participants(account_id)
                    .filter_map(|p| p.user_id.map(|u| (p.call_id, u)))
                    .collect();
                let mut calls: Vec<VtcCall> = inner
                    .calls(account_id)
                    .filter(|c| {
                        filter.as_ref().is_none_or(|f| {
                            call_matches(f, c, &|u: &str| {
                                attended.contains(&(c.id.clone(), u.to_owned()))
                            })
                        })
                    })
                    .collect();
                calls.sort_by(|a, b| compare_calls(a, b, &sort));
                calls.into_iter().map(|c| c.id).collect()
            }
            VtcRecording::TYPE_NAME => {
                let filter: Option<VtcRecordingFilter> =
                    filter.as_ref().map(recode_as).transpose()?;
                let sort: Vec<VtcMediaComparator> = recode_as(&sort)?;
                let mut list: Vec<VtcRecording> = inner
                    .all(account_id)
                    .filter(|r| filter.as_ref().is_none_or(|f| recording_matches(f, r)))
                    .collect();
                list.sort_by(|a, b| compare_recordings(a, b, &sort));
                list.into_iter().map(|r| r.id).collect()
            }
            VtcLivestream::TYPE_NAME => {
                let filter: Option<VtcLivestreamFilter> =
                    filter.as_ref().map(recode_as).transpose()?;
                let sort: Vec<VtcMediaComparator> = recode_as(&sort)?;
                let mut list: Vec<VtcLivestream> = inner
                    .all(account_id)
                    .filter(|l| filter.as_ref().is_none_or(|f| livestream_matches(f, l)))
                    .collect();
                list.sort_by(|a, b| compare_livestreams(a, b, &sort));
                list.into_iter().map(|l| l.id).collect()
            }
            other => {
                return Err(BackendQueryError::Other(MemoryError::new(format!(
                    "query of {other} is not implemented"
                ))))
            }
        };

        let total = ids.len() as u64;
        let start = resolve_query_start(&ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let page = ids
            .into_iter()
            .skip(start)
            .take(limit.map_or(usize::MAX, |n| n as usize))
            .collect();
        Ok(QueryResult::new(
            page,
            start as u64,
            Some(total),
            State::from(inner.state(O::TYPE_NAME, account_id).to_string()),
            false,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &String,
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

fn recode_as<T: serde::de::DeserializeOwned>(
    v: &Value,
) -> Result<T, BackendQueryError<MemoryError>> {
    T::deserialize(v).map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))
}

impl VtcBackend for MemoryBackend {
    async fn vtc_capability(
        &self,
        _caller: &String,
        account_id: &Id,
    ) -> Result<VtcCapability, MemoryError> {
        self.lock()
            .accounts
            .get(account_id.as_ref())
            .cloned()
            .ok_or_else(|| MemoryError::new("account not registered"))
    }

    fn has_capability(&self, _caller: &String, account_id: &Id, uri: &str) -> bool {
        self.lock()
            .advertised
            .contains(&(account_id.as_ref().to_owned(), uri.to_owned()))
    }

    async fn caller_user_id(
        &self,
        caller: &String,
        _account_id: &Id,
    ) -> Result<String, MemoryError> {
        Ok(caller.clone())
    }

    async fn create_call(
        &self,
        _caller: &String,
        account_id: &Id,
        mut call: VtcCall,
        ring_targets: &[String],
    ) -> Result<VtcCall, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        if !inner.accounts.contains_key(account_id.as_ref()) {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::Forbidden,
            )));
        }
        if let Some(Some(chat_id)) = &call.chat_id {
            let busy = inner.calls(account_id).any(|c| {
                c.chat_id.as_ref().and_then(Option::as_ref) == Some(chat_id)
                    && matches!(c.state, CallState::Active | CallState::Ringing)
            });
            if busy {
                return Err(BackendSetError::SetError(call_already_active_on_chat()));
            }
        }

        let id = Id::from(ulid::Ulid::new().to_string());
        call.id = id.clone();
        call.join_uri = format!("https://media.example.invalid/room/{id}");
        let mut participants = Vec::new();
        if call.state == CallState::Creating {
            participants.push(ring_participant(
                &id,
                &call.initiator_id,
                CallRole::Moderator,
            ));
            participants.extend(
                ring_targets
                    .iter()
                    .map(|t| ring_participant(&id, t, CallRole::Participant)),
            );
            call.state = CallState::Ringing;
        }

        let parent = match call.parent_call_id.as_ref() {
            Some(pid) => {
                let stored = inner
                    .objects(VtcCall::TYPE_NAME, account_id)
                    .and_then(|m| m.get(pid))
                    .ok_or_else(|| {
                        BackendSetError::SetError(
                            SetError::new(SetErrorType::InvalidProperties)
                                .with_properties(["parentCallId"]),
                        )
                    })?;
                let mut parent: VtcCall = decode(stored).map_err(BackendSetError::Other)?;
                parent.breakout_room_ids.push(id.clone());
                Some(parent)
            }
            None => None,
        };

        let encode =
            |v: &VtcCall| serde_json::to_value(v).map_err(|e| MemoryError::new(e.to_string()));
        let stored = encode(&call).map_err(BackendSetError::Other)?;
        let parent_stored = parent
            .as_ref()
            .map(encode)
            .transpose()
            .map_err(BackendSetError::Other)?;
        let participant_ids: Vec<Id> = participants
            .iter()
            .map(|p| decode::<VtcParticipant>(p).map(|p| p.id))
            .collect::<Result<_, _>>()
            .map_err(BackendSetError::Other)?;

        let calls = inner.objects_mut(VtcCall::TYPE_NAME, account_id);
        calls.insert(id.clone(), stored);
        let mut changes = vec![(id, Change::Created)];
        if let (Some(parent), Some(value)) = (parent, parent_stored) {
            calls.insert(parent.id.clone(), value);
            changes.push((parent.id, Change::Updated));
        }
        inner.commit(VtcCall::TYPE_NAME, account_id, changes);

        if !participants.is_empty() {
            let store = inner.objects_mut(VtcParticipant::TYPE_NAME, account_id);
            for (pid, p) in participant_ids.iter().zip(participants) {
                store.insert(pid.clone(), p);
            }
            inner.commit(
                VtcParticipant::TYPE_NAME,
                account_id,
                participant_ids
                    .into_iter()
                    .map(|pid| (pid, Change::Created))
                    .collect(),
            );
        }
        Ok(call)
    }

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<MemoryError>> {
        let mut value = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?;
        let mut inner = self.lock();
        // "The call MUST be in state `"active"`", rechecked under the lock.
        if matches!(
            O::TYPE_NAME,
            VtcRecording::TYPE_NAME | VtcLivestream::TYPE_NAME
        ) {
            let call_id = value.get("callId").and_then(Value::as_str).map(Id::from);
            let active = call_id.is_some_and(|cid| {
                inner
                    .calls(account_id)
                    .any(|c| c.id == cid && c.state == CallState::Active)
            });
            if !active {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::InvalidProperties).with_properties(["callId"]),
                ));
            }
        }
        let id = Id::from(ulid::Ulid::new().to_string());
        value["id"] = json!(id);
        let stored: O = decode(&value).map_err(BackendSetError::Other)?;
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .insert(id.clone(), value);
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Created)],
        );
        Ok((id, stored))
    }

    async fn deliver_recording_state_event(
        &self,
        _caller: &String,
        _account_id: &Id,
        event: VtcRecordingStateEvent,
    ) {
        self.lock().recording_events.push(event);
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let mut current = inner
            .objects(O::TYPE_NAME, account_id)
            .and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(not_found)?;

        let patch = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?;
        let (Value::Object(patch), Value::Object(base)) = (patch, &mut current) else {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::InvalidPatch,
            )));
        };
        apply_patch_object(base, &patch).map_err(|e| {
            BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
            )
        })?;
        if O::deserialize(&current).is_err() {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_description(format!("patched {} is invalid", O::TYPE_NAME)),
            ));
        }

        let mut changes = vec![(id.clone(), Change::Updated)];
        // A breakout room that ends leaves its parent's breakoutRoomIds.
        let ended_child = (O::TYPE_NAME == VtcCall::TYPE_NAME)
            .then(|| VtcCall::deserialize(&current).ok())
            .flatten()
            .filter(|c| c.state == CallState::Ended)
            .and_then(|c| c.parent_call_id);
        let calls = inner.objects_mut(O::TYPE_NAME, account_id);
        if let Some(parent_id) = ended_child {
            if let Some(Value::Array(rooms)) = calls
                .get_mut(&parent_id)
                .and_then(|p| p.get_mut("breakoutRoomIds"))
            {
                rooms.retain(|r| r.as_str() != Some(id.as_ref()));
                changes.push((parent_id, Change::Updated));
            }
        }
        calls.insert(id.clone(), current);
        inner.commit(O::TYPE_NAME, account_id, changes);
        Ok(None)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let calls = inner.objects_mut(O::TYPE_NAME, account_id);
        let removed = calls.remove(id).ok_or_else(not_found)?;
        let mut changes = vec![(id.clone(), Change::Destroyed)];
        // A destroyed breakout room leaves its parent's breakoutRoomIds.
        let parent_id = (O::TYPE_NAME == VtcCall::TYPE_NAME)
            .then(|| VtcCall::deserialize(&removed).ok())
            .flatten()
            .and_then(|c| c.parent_call_id);
        if let Some(parent_id) = parent_id {
            if let Some(Value::Array(rooms)) = calls
                .get_mut(&parent_id)
                .and_then(|p| p.get_mut("breakoutRoomIds"))
            {
                rooms.retain(|r| r.as_str() != Some(id.as_ref()));
                changes.push((parent_id, Change::Updated));
            }
        }
        inner.commit(O::TYPE_NAME, account_id, changes);
        Ok(())
    }

    async fn is_moderator(
        &self,
        caller: &String,
        account_id: &Id,
        call: &VtcCall,
    ) -> Result<bool, MemoryError> {
        // "The call initiator receives `"moderator"` by default."
        Ok(*caller == call.initiator_id
            || self.lock().participants(account_id).any(|p| {
                p.call_id == call.id
                    && p.user_id.as_ref() == Some(caller)
                    && p.role == CallRole::Moderator
            }))
    }

    fn supports_type<O: JmapObject>(&self, _caller: &String, _account_id: &Id) -> bool {
        true
    }
}

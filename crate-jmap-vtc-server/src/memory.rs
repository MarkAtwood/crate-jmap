//! In-memory reference implementation of [`VtcBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` backs this crate's integration tests and is a readable
//! example for anyone writing a real backend. All state lives in
//! `HashMap`s behind a `std::sync::Mutex`; nothing is persisted, and no
//! ring notification is actually delivered. A `VTCRecordingStateEvent` is
//! recorded, not sent; `recording_events_for_test` returns them. So are the
//! events of [`VtcBackend::deliver_event`]; `events_for_test` returns
//! them, and no dial-out call is placed.
//!
//! # Caller context
//!
//! `CallerCtx` is the caller's VTC userId (`String`). It is what
//! [`VtcBackend::caller_user_id`] returns and what
//! [`VtcBackend::is_moderator`] checks.
//!
//! # Access
//!
//! Every caller may access every call, except a `(userId, call)` pair
//! made inaccessible with `deny_call_access`: the open default of
//! bd:JMAP-bghk. `get_objects` and `query_objects` hide an inaccessible
//! call and its participants, recordings and livestreams, and the
//! participants of a call with `e2eeEnabled: true` from a user with no
//! record in it, as [`VtcBackend`] §Per-call access requires.
//!
//! # Query
//!
//! `VTCCall/query`, `VTCParticipant/query`, `VTCRecording/query` and
//! `VTCLivestream/query` evaluate the filter and sort with the
//! [`crate::query`] functions.
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

use jmap_server::{apply_patch_object, limit_or_max, resolve_query_start};
use jmap_types::{Id, State, UTCDate};
use jmap_vtc_types::{
    CallRole, CallState, VtcCall, VtcCapability, VtcEvent, VtcLivestream, VtcParticipant,
    VtcRecording, VtcRecordingStateEvent,
};
use jmap_vtc_types::{
    VtcCallComparator, VtcCallFilter, VtcLivestreamFilter, VtcMediaComparator,
    VtcParticipantComparator, VtcParticipantFilter, VtcRecordingFilter,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::query::{
    call_matches, compare_calls, compare_livestreams, compare_participants, compare_recordings,
    livestream_matches, participant_matches, recording_matches,
};
use crate::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
    SetError, SetErrorType, SetObject, VtcBackend,
};
use crate::{StatefulObject, call_already_active_on_chat};

/// How one id changed in one state step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

/// `type_name` → `account_id` → `V`. Nested rather than tuple-keyed so a
/// lookup borrows the `&str` account id instead of allocating a `String`.
type Keyed<V> = HashMap<&'static str, HashMap<String, V>>;

#[derive(Default)]
struct Inner {
    /// `type_name` → `account_id` → `id → stored object`.
    objects: Keyed<HashMap<Id, Value>>,
    /// `type_name` → `account_id` → state counter.
    states: Keyed<u64>,
    /// `type_name` → `account_id` → `(state after the change, id, change)`.
    log: Keyed<Vec<(u64, Id, Change)>>,
    /// Registered accounts and their VTC capability object.
    accounts: HashMap<String, VtcCapability>,
    /// `(account_id, capability URI)` pairs advertised besides VTC.
    advertised: HashSet<(String, String)>,
    /// Every `VTCRecordingStateEvent` delivered, in order.
    recording_events: Vec<VtcRecordingStateEvent>,
    /// Every [`VtcBackend::deliver_event`] event, in order.
    events: Vec<VtcEvent>,
    /// `(blocking userId, blocked userId)` pairs.
    blocked: HashSet<(String, String)>,
    /// `(userId, call id)` pairs whose call the user may not access.
    denied: HashSet<(String, String)>,
}

impl Inner {
    /// [`VtcBackend::update_object`] under the held lock.
    // Same error type as `VtcBackend::update_object`, which it implements.
    fn update<O: SetObject>(
        &mut self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        let mut current = self
            .objects(O::TYPE_NAME, account_id)
            .and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(not_found)?;

        let patch = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize patch", &e)))?;
        let (Value::Object(patch), Value::Object(base)) = (patch, &mut current) else {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::InvalidPatch,
            ))));
        };
        apply_patch_object(base, &patch).map_err(|e| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
            ))
        })?;
        if O::deserialize(&current).is_err() {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_description(format!("patched {} is invalid", O::TYPE_NAME)),
            )));
        }
        let active = match (O::TYPE_NAME == VtcParticipant::TYPE_NAME)
            .then(|| VtcParticipant::deserialize(&current).ok())
            .flatten()
        {
            Some(p) => {
                let count = self
                    .active_count_within_quota(account_id, &p)
                    .ok_or_else(over_quota)?;
                Some((p.call_id, count))
            }
            None => None,
        };

        let mut changes = vec![(id.clone(), Change::Updated)];
        // A breakout room that ends leaves its parent's breakoutRoomIds.
        let ended_child = (O::TYPE_NAME == VtcCall::TYPE_NAME)
            .then(|| VtcCall::deserialize(&current).ok())
            .flatten()
            .filter(|c| c.state == CallState::Ended)
            .and_then(|c| c.parent_call_id);
        let calls = self.objects_mut(O::TYPE_NAME, account_id);
        if let Some(parent_id) = &ended_child
            && let Some(Value::Array(rooms)) = calls
                .get_mut(parent_id)
                .and_then(|p| p.get_mut("breakoutRoomIds"))
        {
            rooms.retain(|r| r.as_str() != Some(id.as_ref()));
            changes.push((parent_id.clone(), Change::Updated));
        }
        calls.insert(id.clone(), current);
        self.commit(O::TYPE_NAME, account_id, changes);
        if let Some(parent_id) = ended_child {
            self.clear_breakout_assignments(account_id, &parent_id, id);
        }
        if let Some((call_id, count)) = active {
            self.set_active_count(account_id, &call_id, count);
        }
        Ok(None)
    }

    fn objects(&self, type_name: &'static str, account_id: &Id) -> Option<&HashMap<Id, Value>> {
        self.objects
            .get(type_name)
            .and_then(|m| m.get(account_id.as_ref()))
    }

    fn objects_mut(&mut self, type_name: &'static str, account_id: &Id) -> &mut HashMap<Id, Value> {
        self.objects
            .entry(type_name)
            .or_default()
            .entry(account_id.as_ref().to_owned())
            .or_default()
    }

    fn state(&self, type_name: &'static str, account_id: &Id) -> u64 {
        self.states
            .get(type_name)
            .and_then(|m| m.get(account_id.as_ref()))
            .copied()
            .unwrap_or(0)
    }

    /// Advance the state of `type_name` once and log `changes` under it.
    fn commit(&mut self, type_name: &'static str, account_id: &Id, changes: Vec<(Id, Change)>) {
        let state = self
            .states
            .entry(type_name)
            .or_default()
            .entry(account_id.as_ref().to_owned())
            .or_insert(0);
        *state += 1;
        let state = *state;
        self.log
            .entry(type_name)
            .or_default()
            .entry(account_id.as_ref().to_owned())
            .or_default()
            .extend(changes.into_iter().map(|(id, c)| (state, id, c)));
    }

    fn calls(&self, account_id: &Id) -> impl Iterator<Item = VtcCall> + '_ + use<'_> {
        self.objects(VtcCall::TYPE_NAME, account_id)
            .into_iter()
            .flat_map(HashMap::values)
            .filter_map(|v| VtcCall::deserialize(v).ok())
    }

    fn participants(&self, account_id: &Id) -> impl Iterator<Item = VtcParticipant> + '_ + use<'_> {
        self.all(account_id)
    }

    /// The `activeParticipantCount` of `candidate`'s call once `candidate`
    /// is stored, or `None` when that is a rise above the account's
    /// `maxParticipantsPerCall`.
    fn active_count_within_quota(
        &self,
        account_id: &Id,
        candidate: &VtcParticipant,
    ) -> Option<u64> {
        let in_call = |p: &VtcParticipant| p.call_id == candidate.call_id && is_active(p);
        let before = self.participants(account_id).filter(in_call).count();
        let after = self
            .participants(account_id)
            .filter(|p| p.id != candidate.id && in_call(p))
            .count()
            + usize::from(is_active(candidate));
        let max = self
            .accounts
            .get(account_id.as_ref())
            .and_then(|cap| cap.max_participants_per_call);
        (after <= before || max.is_none_or(|m| after as u64 <= m)).then_some(after as u64)
    }

    /// Store `count` as `call_id`'s `activeParticipantCount`, advancing the
    /// VTCCall state only when it changes.
    fn set_active_count(&mut self, account_id: &Id, call_id: &Id, count: u64) {
        let Some(call) = self
            .objects_mut(VtcCall::TYPE_NAME, account_id)
            .get_mut(call_id)
        else {
            return;
        };
        if call.get("activeParticipantCount") != Some(&json!(count)) {
            call["activeParticipantCount"] = json!(count);
            self.commit(
                VtcCall::TYPE_NAME,
                account_id,
                vec![(call_id.clone(), Change::Updated)],
            );
        }
    }

    /// Reset to `null` every `breakoutCallId` naming `room_id` on the
    /// participants of `parent_id`.
    fn clear_breakout_assignments(&mut self, account_id: &Id, parent_id: &Id, room_id: &Id) {
        let ids: Vec<Id> = self
            .participants(account_id)
            .filter(|p| p.call_id == *parent_id && p.breakout_call_id.as_ref() == Some(room_id))
            .map(|p| p.id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let store = self.objects_mut(VtcParticipant::TYPE_NAME, account_id);
        for id in &ids {
            if let Some(p) = store.get_mut(id) {
                p["breakoutCallId"] = Value::Null;
            }
        }
        self.commit(
            VtcParticipant::TYPE_NAME,
            account_id,
            ids.into_iter().map(|id| (id, Change::Updated)).collect(),
        );
    }

    /// `true` if `caller` may see `v`, a stored object of `type_name`
    /// ([`VtcBackend`] §Per-call access): its call is not denied to
    /// `caller`, and a participant of a call with `e2eeEnabled: true` is
    /// shown only to a user with a record in that call.
    ///
    /// ponytail: scans every participant for each E2EE participant record,
    /// O(participants²) on a full fetch. Upgrade: index records by call.
    fn visible(&self, caller: &str, account_id: &Id, type_name: &str, v: &Value) -> bool {
        let field = if type_name == VtcCall::TYPE_NAME {
            "id"
        } else {
            "callId"
        };
        let Some(call_id) = v.get(field).and_then(Value::as_str) else {
            return true;
        };
        if self
            .denied
            .contains(&(caller.to_owned(), call_id.to_owned()))
        {
            return false;
        }
        if type_name != VtcParticipant::TYPE_NAME {
            return true;
        }
        let call_id = Id::from(call_id);
        let e2ee = self
            .objects(VtcCall::TYPE_NAME, account_id)
            .and_then(|m| m.get(&call_id))
            .is_some_and(|c| c["e2eeEnabled"] == true);
        !e2ee
            || self
                .participants(account_id)
                .any(|p| p.call_id == call_id && p.user_id.as_deref() == Some(caller))
    }

    /// The `get_objects` read of `ids`, hiding what `caller` may not see
    /// ([`Self::visible`]), or nothing when `caller` is `None`.
    fn get<O: JmapObject>(
        &self,
        caller: Option<&str>,
        account_id: &Id,
        ids: Option<&[Id]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        let map = self.objects(O::TYPE_NAME, account_id);
        let shown = |v: &Value| caller.is_none_or(|c| self.visible(c, account_id, O::TYPE_NAME, v));
        match ids {
            None => {
                let list = map
                    .into_iter()
                    .flat_map(HashMap::values)
                    .filter(|v| shown(v))
                    .map(decode)
                    .collect::<Result<_, _>>()?;
                Ok((list, Vec::new()))
            }
            Some(ids) => {
                let mut found = Vec::new();
                let mut missing = Vec::new();
                for id in ids {
                    match map.and_then(|m| m.get(id)) {
                        Some(v) if shown(v) => found.push(decode(v)?),
                        _ => missing.push(id.clone()),
                    }
                }
                Ok((found, missing))
            }
        }
    }

    /// Every stored object of type `O` that decodes.
    fn all<O: JmapObject>(&self, account_id: &Id) -> impl Iterator<Item = O> + '_ + use<'_, O> {
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
#[non_exhaustive]
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

    /// Make `user` blocked by `blocker`, for
    /// [`VtcBackend::is_user_blocked`].
    #[doc(hidden)]
    pub fn block(&self, blocker: &str, user: &str) {
        self.lock()
            .blocked
            .insert((blocker.to_owned(), user.to_owned()));
    }

    /// Make call `call_id` inaccessible to `user`, as a deployment's
    /// §Chat-Bound Calls or §Standalone Calls rule would.
    #[doc(hidden)]
    pub fn deny_call_access(&self, user: &str, call_id: &str) {
        self.lock()
            .denied
            .insert((user.to_owned(), call_id.to_owned()));
    }

    /// The `VTCRecordingStateEvent`s delivered so far, in order.
    #[doc(hidden)]
    pub fn recording_events_for_test(&self) -> Vec<VtcRecordingStateEvent> {
        self.lock().recording_events.clone()
    }

    /// The [`VtcBackend::deliver_event`] events delivered so far, in order.
    #[doc(hidden)]
    pub fn events_for_test(&self) -> Vec<VtcEvent> {
        self.lock().events.clone()
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

    /// Construct a [`MemoryError`] for a `serde_json` failure during
    /// `context` (the operation that failed).
    ///
    /// Only the error's [`serde_json::error::Category`] is kept. A serde
    /// data error quotes the offending value (`invalid type: string
    /// "alice@example.com", expected u64`), and the `JmapBackend::Error`
    /// contract forbids PII and blob content in `Display` output
    /// (bd:JMAP-510h.37).
    pub(crate) fn serde(context: impl std::fmt::Display, e: &serde_json::Error) -> Self {
        Self::new(format!("{context}: serde_json {:?} error", e.classify()))
    }
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryBackend error: {}", self.description)
    }
}

impl std::error::Error for MemoryError {}

fn not_found<E>() -> BackendSetError<E> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

/// "joined and not yet left" (§VTCCall `activeParticipantCount`).
fn is_active(p: &VtcParticipant) -> bool {
    p.joined_at.is_some() && p.left_at.is_none()
}

fn over_quota<E>() -> BackendSetError<E> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::OverQuota)
            .with_description("the call has reached maxParticipantsPerCall"),
    ))
}

fn decode<O: JmapObject>(v: &Value) -> Result<O, MemoryError> {
    O::deserialize(v).map_err(|e| MemoryError::serde(format_args!("decode {}", O::TYPE_NAME), &e))
}

/// A VTCParticipant record for a ring call, joined at `joined_at`.
fn ring_participant(
    call_id: &Id,
    user_id: &str,
    role: CallRole,
    joined_at: Option<&UTCDate>,
) -> Value {
    let response = (role == CallRole::Participant).then_some("pending");
    json!({
        "id": ulid::Ulid::new().to_string(),
        "callId": call_id,
        "userId": user_id,
        "displayName": user_id,
        "role": role,
        "joinedAt": joined_at,
        "leftAt": null,
        "callResponse": response,
        "joinMethod": "webrtc",
        "gatewayData": null,
        "lobbyState": null,
        "mediaState": { "audio": false, "video": false, "screen": false, "raisedHand": false },
        "speakerTimeMs": 0,
        "kickedBy": null,
        "e2eeFingerprint": null,
        "unmuteRequested": false,
        "breakoutCallId": null
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
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        self.lock().get::<O>(Some(caller), account_id, ids)
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
            .get(O::TYPE_NAME)
            .and_then(|m| m.get(account_id.as_ref()))
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
        caller: &String,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let QueryParams {
            filter,
            sort,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
        // `O` is one of the queryable VTC types, so its filter and
        // comparators re-decode as that type's.
        let recode = |v: Result<Value, serde_json::Error>| {
            v.map_err(|e| BackendQueryError::Other(MemoryError::serde("recode query argument", &e)))
        };
        let filter = filter
            .map(|f| recode(serde_json::to_value(f)))
            .transpose()?;
        let sort = match sort {
            Some(s) => recode(serde_json::to_value(s))?,
            None => Value::Array(Vec::new()),
        };

        let inner = self.lock();
        let mut ids: Vec<Id> = match O::TYPE_NAME {
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
            VtcParticipant::TYPE_NAME => {
                let filter: Option<VtcParticipantFilter> =
                    filter.as_ref().map(recode_as).transpose()?;
                let sort: Vec<VtcParticipantComparator> = recode_as(&sort)?;
                let mut list: Vec<VtcParticipant> = inner
                    .participants(account_id)
                    .filter(|p| filter.as_ref().is_none_or(|f| participant_matches(f, p)))
                    .collect();
                list.sort_by(|a, b| compare_participants(a, b, &sort));
                list.into_iter().map(|p| p.id).collect()
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
                ))));
            }
        };
        let store = inner.objects(O::TYPE_NAME, account_id);
        ids.retain(|id| {
            store
                .and_then(|m| m.get(id))
                .is_some_and(|v| inner.visible(caller, account_id, O::TYPE_NAME, v))
        });

        let total = ids.len() as u64;
        let start = resolve_query_start(&ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let page = ids
            .into_iter()
            .skip(start)
            .take(limit_or_max(limit))
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
        _params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        Err(BackendChangesError::CannotCalculate)
    }
}

fn recode_as<T: serde::de::DeserializeOwned>(
    v: &Value,
) -> Result<T, BackendQueryError<MemoryError>> {
    T::deserialize(v)
        .map_err(|e| BackendQueryError::Other(MemoryError::serde("recode query argument", &e)))
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

    async fn create_call<F>(
        &self,
        _caller: &String,
        account_id: &Id,
        build: F,
        ring_targets: &[String],
    ) -> Result<VtcCall, BackendSetError<MemoryError>>
    where
        F: FnOnce(&Id) -> VtcCall + Send,
    {
        let id = Id::from(ulid::Ulid::new().to_string());
        let mut call = build(&id);
        let mut inner = self.lock();
        if !inner.accounts.contains_key(account_id.as_ref()) {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::Forbidden,
            ))));
        }
        if let Some(Some(chat_id)) = &call.chat_id {
            let busy = inner.calls(account_id).any(|c| {
                c.chat_id.as_ref().and_then(Option::as_ref) == Some(chat_id)
                    && matches!(c.state, CallState::Active | CallState::Ringing)
            });
            if busy {
                return Err(BackendSetError::SetError(Box::new(
                    call_already_active_on_chat(),
                )));
            }
        }

        call.join_uri = format!("https://media.example.invalid/room/{id}");
        let mut participants = Vec::new();
        if call.state == CallState::Creating {
            participants.push(ring_participant(
                &id,
                &call.initiator_id,
                CallRole::Moderator,
                Some(&call.created_at),
            ));
            participants.extend(
                ring_targets
                    .iter()
                    .map(|t| ring_participant(&id, t, CallRole::Participant, None)),
            );
            call.state = CallState::Ringing;
            call.active_participant_count = 1;
        }

        let parent = match call.parent_call_id.as_ref() {
            Some(pid) => {
                let stored = inner
                    .objects(VtcCall::TYPE_NAME, account_id)
                    .and_then(|m| m.get(pid))
                    .ok_or_else(|| {
                        BackendSetError::SetError(Box::new(
                            SetError::new(SetErrorType::InvalidProperties)
                                .with_properties(["parentCallId"]),
                        ))
                    })?;
                let mut parent: VtcCall = decode(stored).map_err(BackendSetError::Other)?;
                parent.breakout_room_ids.push(id.clone());
                Some(parent)
            }
            None => None,
        };

        let encode = |v: &VtcCall| {
            serde_json::to_value(v).map_err(|e| MemoryError::serde("serialize VtcCall", &e))
        };
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

    async fn create_object<O, F>(
        &self,
        _caller: &String,
        account_id: &Id,
        _create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<MemoryError>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        let id = Id::from(ulid::Ulid::new().to_string());
        let value = serde_json::to_value(build(&id)).map_err(|e| {
            BackendSetError::Other(MemoryError::serde("serialize created object", &e))
        })?;
        let mut inner = self.lock();
        // "MUST return `invalidProperties` naming `callId` when the call is
        // not in state `"active"`", rechecked under the lock.
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
                return Err(BackendSetError::SetError(Box::new(
                    SetError::new(SetErrorType::InvalidProperties).with_properties(["callId"]),
                )));
            }
        }
        let stored: O = decode(&value).map_err(BackendSetError::Other)?;
        let participant = (O::TYPE_NAME == VtcParticipant::TYPE_NAME)
            .then(|| decode::<VtcParticipant>(&value))
            .transpose()
            .map_err(BackendSetError::Other)?;
        let active = match &participant {
            Some(p) => {
                // §Reconnecting: one record per user per call.
                if let Some(existing) = p.user_id.as_ref().and_then(|u| {
                    inner
                        .participants(account_id)
                        .find(|q| q.call_id == p.call_id && q.user_id.as_ref() == Some(u))
                }) {
                    return Err(BackendSetError::SetError(Box::new(
                        SetError::new(SetErrorType::AlreadyExists).with_existing_id(existing.id),
                    )));
                }
                let count = inner
                    .active_count_within_quota(account_id, p)
                    .ok_or_else(over_quota)?;
                Some((p.call_id.clone(), count))
            }
            None => None,
        };
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .insert(id.clone(), value);
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Created)],
        );
        if let Some((call_id, count)) = active {
            inner.set_active_count(account_id, &call_id, count);
        }
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

    async fn deliver_event(&self, _caller: &String, _account_id: &Id, event: VtcEvent) {
        self.lock().events.push(event);
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        self.lock().update::<O>(account_id, id, patch)
    }

    async fn update_object_if<O: StatefulObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
        expected: &[O::State],
        patch: O::Patch,
    ) -> Result<Result<Option<O>, O::State>, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let Some(value) = inner
            .objects(O::TYPE_NAME, account_id)
            .and_then(|m| m.get(id))
        else {
            return Err(not_found());
        };
        let stored: O = decode(value).map_err(BackendSetError::Other)?;
        if !expected.contains(stored.state()) {
            return Ok(Err(stored.state().clone()));
        }
        inner.update::<O>(account_id, id, patch).map(Ok)
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
        if let Some(parent_id) = &parent_id
            && let Some(Value::Array(rooms)) = calls
                .get_mut(parent_id)
                .and_then(|p| p.get_mut("breakoutRoomIds"))
        {
            rooms.retain(|r| r.as_str() != Some(id.as_ref()));
            changes.push((parent_id.clone(), Change::Updated));
        }
        inner.commit(O::TYPE_NAME, account_id, changes);
        if let Some(parent_id) = parent_id {
            inner.clear_breakout_assignments(account_id, &parent_id, id);
        }
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

    async fn active_call_count(
        &self,
        _caller: &String,
        account_id: &Id,
    ) -> Result<u64, MemoryError> {
        Ok(self
            .lock()
            .calls(account_id)
            .filter(|c| c.state == CallState::Active)
            .count() as u64)
    }

    async fn get_objects_unscoped<O: GetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        self.lock().get::<O>(None, account_id, ids)
    }

    async fn is_user_blocked(&self, caller: &String, user_id: &str) -> Result<bool, MemoryError> {
        Ok(self
            .lock()
            .blocked
            .contains(&(caller.clone(), user_id.to_owned())))
    }

    fn supports_type<O: JmapObject>(&self, _caller: &String, _account_id: &Id) -> bool {
        true
    }
}

#[cfg(test)]
mod memory_error_tests {
    use super::MemoryError;

    /// The `JmapBackend::Error` contract: "Implementations MUST NOT
    /// include ... PII shaped like an email address" in `Display`
    /// output. serde_json data errors quote the offending value, so
    /// [`MemoryError::serde`] must drop the message (bd:JMAP-510h.37).
    #[test]
    fn serde_error_display_omits_offending_value() {
        let canary = "alice@example.com";
        let e = serde_json::from_value::<u64>(serde_json::Value::String(canary.into()))
            .expect_err("a string is not a u64");
        assert!(
            e.to_string().contains(canary),
            "precondition: serde_json quotes the value"
        );

        let shown = MemoryError::serde("decode", &e).to_string();
        assert!(!shown.contains(canary), "canary leaked: {shown}");
        assert!(
            shown.contains("decode") && shown.contains("Data"),
            "{shown}"
        );
    }
}

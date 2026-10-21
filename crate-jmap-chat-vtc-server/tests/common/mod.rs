//! Shared fixtures. "vtc Lnnn" cites draft-atwood-jmap-vtc-00.md and
//! "chat Lnnn" draft-atwood-jmap-chat-00.md, both at jmap-chat-spec
//! 0cd3af0. Records are hand-written JSON from the drafts' field lists.

#![allow(dead_code)]

use std::collections::HashSet;

use jmap_chat_server::memory::{MemoryBackend as ChatMemory, MemoryError as ChatError};
use jmap_chat_server::{handle_chat_changes, handle_chat_get};
use jmap_chat_vtc_server::{check_call_create, is_blocked_by_contact, sync_active_call_id};
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    HandlerOutput, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetObject,
};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::{MemoryBackend as VtcMemory, MemoryError as VtcError};
use jmap_vtc_server::{handle_vtc_call_get, handle_vtc_call_set, VtcBackend};
use jmap_vtc_types::{VtcCall, VtcCapability, VtcEvent, VtcRecordingStateEvent};
use serde_json::{json, Value};

pub const ACCOUNT: &str = "A1";
pub const OWNER: &str = "user:owner@example.com";
pub const BOB: &str = "user:bob@example.com";
pub const ALICE: &str = "user:alice@example.com";
pub const CHAT_URI: &str = "urn:ietf:params:jmap:chat";

/// A deployment backend that offers both VTC and Chat, wired to this
/// crate as its docs prescribe: VTC storage in `vtc`, Chat storage in
/// `chat`, one account [`ACCOUNT`] in each.
pub struct Bound {
    pub vtc: VtcMemory,
    pub chat: ChatMemory,
    pub everyone: HashSet<jmap_chat_types::ChatPermission>,
}

impl Bound {
    /// [`ACCOUNT`] registered in both backends with every call type
    /// enabled (capability object hand-written from vtc L148-187), the
    /// Chat capability advertised, and an empty `@everyone` floor.
    pub fn new() -> Self {
        let vtc = VtcMemory::new();
        let cap: VtcCapability = serde_json::from_value(json!({
            "mayCreateCall": true,
            "supportsRingCalls": true,
            "supportsRoomCalls": true,
            "supportsScheduledCalls": true,
            "supportsRecording": false,
            "supportsLivestream": false,
            "supportsLobby": false,
            "supportsBreakoutRooms": false,
            "supportedMediaTypes": ["audio", "video"],
            "maxParticipantsPerCall": null,
            "maxConcurrentCalls": null,
            "maxBreakoutRooms": null
        }))
        .expect("capability fixture");
        vtc.register_account(&Id::from(ACCOUNT), cap);
        vtc.advertise(&Id::from(ACCOUNT), CHAT_URI);
        let chat = ChatMemory::new();
        chat.register_account(&Id::from(ACCOUNT));
        chat.set_owner_user_id_for_test(&Id::from(ACCOUNT), OWNER);
        Self {
            vtc,
            chat,
            everyone: HashSet::new(),
        }
    }

    /// Store a Chat record `value` (its `id` is the record id).
    pub fn seed_chat_object(&self, type_name: &'static str, value: Value) {
        let id = value["id"].as_str().expect("fixture has an id").to_owned();
        self.chat
            .insert_object_for_test(type_name, ACCOUNT, &id, value);
    }

    /// Store a VTC record `value` (its `id` is the record id).
    pub fn seed_vtc_object(&self, type_name: &'static str, value: Value) {
        let id = value["id"].as_str().expect("fixture has an id").to_owned();
        self.vtc
            .insert_object_for_test(type_name, &Id::from(ACCOUNT), &id, value);
    }
}

fn chat_error(e: ChatError) -> VtcError {
    VtcError::new(e.to_string())
}

fn chat_set_error(e: BackendSetError<ChatError>) -> BackendSetError<VtcError> {
    match e {
        BackendSetError::SetError(s) => BackendSetError::SetError(s),
        BackendSetError::Other(e) => BackendSetError::Other(chat_error(e)),
        other => BackendSetError::Other(VtcError::new(other.to_string())),
    }
}

impl JmapBackend for Bound {
    type Error = VtcError;
    type CallerCtx = String;

    async fn account_exists(&self, caller: &String, account_id: &Id) -> Result<bool, VtcError> {
        self.vtc.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), VtcError> {
        self.vtc
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<State, VtcError> {
        self.vtc.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<VtcError>> {
        self.vtc
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<VtcError>> {
        self.vtc
            .query_objects::<O>(caller, account_id, params)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<VtcError>> {
        self.vtc
            .query_changes::<O>(caller, account_id, params)
            .await
    }
}

impl VtcBackend for Bound {
    async fn vtc_capability(
        &self,
        caller: &String,
        account_id: &Id,
    ) -> Result<VtcCapability, VtcError> {
        self.vtc.vtc_capability(caller, account_id).await
    }

    fn has_capability(&self, caller: &String, account_id: &Id, uri: &str) -> bool {
        self.vtc.has_capability(caller, account_id, uri)
    }

    async fn caller_user_id(&self, caller: &String, account_id: &Id) -> Result<String, VtcError> {
        self.vtc.caller_user_id(caller, account_id).await
    }

    async fn create_call(
        &self,
        caller: &String,
        account_id: &Id,
        call: VtcCall,
        ring_targets: &[String],
    ) -> Result<VtcCall, BackendSetError<VtcError>> {
        check_call_create(&self.chat, &(), account_id, &call, &self.everyone)
            .await
            .map_err(|e| BackendSetError::Other(chat_error(e)))?
            .map_err(BackendSetError::SetError)?;
        let stored = self
            .vtc
            .create_call(caller, account_id, call, ring_targets)
            .await?;
        sync_active_call_id(&self.chat, &(), account_id, &stored)
            .await
            .map_err(chat_set_error)?;
        Ok(stored)
    }

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<VtcError>> {
        self.vtc
            .create_object::<O>(caller, account_id, create_id, obj)
            .await
    }

    async fn deliver_recording_state_event(
        &self,
        caller: &String,
        account_id: &Id,
        event: VtcRecordingStateEvent,
    ) {
        self.vtc
            .deliver_recording_state_event(caller, account_id, event)
            .await
    }

    async fn deliver_event(&self, caller: &String, account_id: &Id, event: VtcEvent) {
        self.vtc.deliver_event(caller, account_id, event).await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<VtcError>> {
        let echo = self
            .vtc
            .update_object::<O>(caller, account_id, id, patch)
            .await?;
        if O::TYPE_NAME == VtcCall::TYPE_NAME {
            let (found, _) = self
                .vtc
                .get_objects::<VtcCall>(caller, account_id, Some(std::slice::from_ref(id)), None)
                .await
                .map_err(BackendSetError::Other)?;
            for call in &found {
                sync_active_call_id(&self.chat, &(), account_id, call)
                    .await
                    .map_err(chat_set_error)?;
            }
        }
        Ok(echo)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<VtcError>> {
        self.vtc.destroy_object::<O>(caller, account_id, id).await
    }

    async fn is_user_blocked(
        &self,
        _caller: &String,
        account_id: &Id,
        user_id: &str,
    ) -> Result<bool, VtcError> {
        is_blocked_by_contact(&self.chat, &(), account_id, user_id)
            .await
            .map_err(chat_error)
    }

    async fn active_call_count(&self, caller: &String, account_id: &Id) -> Result<u64, VtcError> {
        self.vtc.active_call_count(caller, account_id).await
    }

    async fn get_objects_unscoped<O: GetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
    ) -> Result<(Vec<O>, Vec<Id>), VtcError> {
        self.vtc
            .get_objects_unscoped::<O>(caller, account_id, ids)
            .await
    }

    fn supports_type<O: JmapObject>(&self, caller: &String, account_id: &Id) -> bool {
        self.vtc.supports_type::<O>(caller, account_id)
    }
}

/// Run `VTCCall/set` on [`ACCOUNT`] as `caller` with `args` (without
/// `accountId`).
pub async fn vtc_set(backend: &Bound, caller: &str, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    let HandlerOutput { primary, .. } = handle_vtc_call_set(backend, &caller.to_owned(), args)
        .await
        .expect("VTCCall/set");
    primary
}

/// The `created` entry of one `VTCCall/set` create, which must succeed.
pub async fn create_call(backend: &Bound, call: Value) -> Value {
    let resp = vtc_set(backend, OWNER, json!({ "create": { "c0": call } })).await;
    assert!(resp["notCreated"].is_null(), "create failed: {resp}");
    resp["created"]["c0"].clone()
}

/// The `notCreated` entry of one `VTCCall/set` create, which must fail.
pub async fn create_call_error(backend: &Bound, call: Value) -> Value {
    let resp = vtc_set(backend, OWNER, json!({ "create": { "c0": call } })).await;
    assert!(resp["created"].is_null(), "create must fail: {resp}");
    resp["notCreated"]["c0"].clone()
}

/// One VTCCall as `VTCCall/get` returns it.
pub async fn get_call(backend: &Bound, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_vtc_call_get(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("VTCCall/get");
    primary["list"][0].clone()
}

/// One Chat as `Chat/get` returns it.
pub async fn get_chat(backend: &Bound, id: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_chat_get(
        &backend.chat,
        &(),
        json!({ "accountId": ACCOUNT, "ids": [id] }),
    )
    .await
    .expect("Chat/get");
    primary["list"][0].clone()
}

/// The current Chat `state` string.
pub async fn chat_state(backend: &Bound) -> String {
    let HandlerOutput { primary, .. } = handle_chat_get(
        &backend.chat,
        &(),
        json!({ "accountId": ACCOUNT, "ids": [] }),
    )
    .await
    .expect("Chat/get");
    primary["state"].as_str().expect("state").to_owned()
}

/// The `updated` ids of `Chat/changes` since `since`.
pub async fn chat_updated_since(backend: &Bound, since: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_chat_changes(
        &backend.chat,
        &(),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
    )
    .await
    .expect("Chat/changes");
    primary["updated"].clone()
}

/// A group Chat `id` whose `members` are `member_ids` (chat L467: "The
/// `id` field is the participant's ChatContact.id, or the accountId for
/// the owner").
pub fn group_chat(id: &str, member_ids: &[&str]) -> Value {
    let members: Vec<Value> = member_ids
        .iter()
        .map(|m| json!({ "id": m, "role": "member", "joinedAt": "2026-01-01T00:00:00Z" }))
        .collect();
    json!({
        "id": id,
        "kind": "group",
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "members": members,
        "activeCallId": null
    })
}

/// A channel Chat `id` in Space `space_id` with `overrides` as its
/// `permissionOverrides`.
pub fn channel_chat(id: &str, space_id: &str, overrides: Value) -> Value {
    json!({
        "id": id,
        "kind": "channel",
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "spaceId": space_id,
        "permissionOverrides": overrides,
        "activeCallId": null
    })
}

/// A Space `id` with one SpaceRole `R-CALLER` holding `role_permissions`,
/// and `members` as `(member id, role ids)` pairs.
pub fn space(id: &str, role_permissions: &[&str], members: &[(&str, &[&str])]) -> Value {
    let members: Vec<Value> = members
        .iter()
        .map(|(m, roles)| json!({ "id": m, "roleIds": roles, "joinedAt": "2026-01-01T00:00:00Z" }))
        .collect();
    json!({
        "id": id,
        "name": "Space",
        "roles": [{ "id": "R-CALLER", "name": "Caller", "permissions": role_permissions, "position": 1 }],
        "members": members,
        "categories": [],
        "uncategorizedChannelIds": [],
        "createdAt": "2026-01-01T00:00:00Z",
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "memberCount": members.len()
    })
}

/// A ChatContact `id` for `identity` (chat §ChatContact field list).
pub fn contact(id: &str, identity: &str, blocked: bool) -> Value {
    json!({
        "id": id,
        "identity": identity,
        "login": identity,
        "firstSeenAt": "2026-01-01T00:00:00Z",
        "lastSeenAt": "2026-01-01T00:00:00Z",
        "blocked": blocked
    })
}

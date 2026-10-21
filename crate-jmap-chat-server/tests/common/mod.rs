//! Shared test infrastructure.
//!
//! Most of the in-memory backend used by these tests now lives in the
//! crate itself as the public reference implementation
//! [`jmap_chat_server::memory::MemoryBackend`]. This module:
//!
//! - re-exports that public reference impl (and `MemoryError`) under the
//!   historical `common::*` paths, so existing tests can use
//!   `use common::MemoryBackend;` unchanged.
//! - keeps the test-only [`FaultyBackend`] negative-path wrapper that
//!   always returns errors. This is testing scaffolding (not a reference
//!   impl) and so stays here.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(async_fn_in_trait)]

use jmap_server::HandlerOutput;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// Re-exports — keep `use common::MemoryBackend;` working for tests.
pub use jmap_chat_server::memory::{MemoryBackend, MemoryError};

use jmap_chat_server::{
    AttachmentBlob, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult,
    ChatBackend, ChatLimits, EmojiSetOp, GetObject, InviteJoinRefusal, JmapBackend, JmapObject,
    OpResult, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    SlowModeError, SpacePatchOp,
};
use jmap_types::{Id, PatchObject, State, UTCDate};

// ---------------------------------------------------------------------------
// FaultyBackend — always returns errors, for negative-path testing
// ---------------------------------------------------------------------------

/// A backend that always returns errors. Used to test the handler's error paths.
#[derive(Clone, Default)]
pub struct FaultyBackend;

impl JmapBackend for FaultyBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), _account_id: &Id) -> Result<bool, Self::Error> {
        Err(MemoryError::new("storage unavailable"))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        Err(MemoryError::new("storage unavailable"))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
    ) -> Result<State, Self::Error> {
        Err(MemoryError::new("storage unavailable"))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _since_state: &State,
        _max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        Err(BackendChangesError::Other(MemoryError::new(
            "storage unavailable",
        )))
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
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        Err(BackendQueryError::Other(MemoryError::new(
            "storage unavailable",
        )))
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
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        Err(BackendChangesError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }
}

impl ChatBackend for FaultyBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _create_id: &str,
        _obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _id: &Id,
        _patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        _account_id: &Id,
        _id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }

    fn supports_type<O: JmapObject>(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> bool {
        false
    }

    async fn join_space_with_invite(
        &self,
        _caller: &(),
        _account_id: &Id,
        _invite_id: &Id,
        _joined_at: &UTCDate,
    ) -> Result<Result<(), InviteJoinRefusal>, Self::Error> {
        Err(MemoryError::new("storage unavailable"))
    }

    async fn read_and_burn_message(
        &self,
        _caller: &(),
        _account_id: &Id,
        _message_id: &Id,
        _patch: PatchObject,
    ) -> Result<(), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }

    fn generate_invite_code(&self) -> String {
        // test-only: not a CSPRNG
        format!(
            "{:012x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                & 0xffff_ffff_ffff,
        )
    }

    async fn apply_space_patch(
        &self,
        _caller: &(),
        _account_id: &Id,
        _space_id: &Id,
        _ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }

    async fn apply_space_metadata_patch(
        &self,
        _caller: &(),
        _account_id: &Id,
        _space_id: &Id,
        _patch: jmap_chat_types::SpaceMetadataPatch,
    ) -> Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError::new(
            "storage unavailable",
        )))
    }
}

// ---------------------------------------------------------------------------
// TrackingBackend — MemoryBackend with policy hooks overridable per-test
// ---------------------------------------------------------------------------

/// A `MemoryBackend` wrapper that lets a test inject custom outcomes for
/// individual [`ChatBackend`] policy hooks (`slow_mode_check`, etc.) while
/// otherwise delegating to the reference impl for storage, change-log, and
/// all read/write methods.
///
/// The current implementation only knobs `slow_mode_check`. Future Layer B
/// beads (`is_contact_blocked`, `may_set_custom_emoji`, etc.) extend the
/// configuration surface with the same wrapper pattern rather than
/// inventing a fresh test backend per method.
#[derive(Clone, Default)]
pub struct TrackingBackend {
    inner: MemoryBackend,
    /// When `Some`, [`ChatBackend::slow_mode_check`] returns
    /// `Err(SlowModeError::new(<this>))`. When `None`, forwards to
    /// `inner` (which is a no-op).
    slow_mode_block: Option<UTCDate>,
    /// When `true`, [`ChatBackend::may_set_custom_emoji`] returns
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)))` for every op
    /// (Create/Update/Destroy). When `false`, forwards to `inner`
    /// (which returns `Ok(Ok(()))`).
    emoji_set_deny: bool,
    /// When `true`, [`ChatBackend::may_broadcast_mention`] returns
    /// `Ok(Err(SetError::new(SetErrorType::Forbidden)))`. When `false`,
    /// forwards to `inner` (which returns `Ok(Ok(()))`).
    broadcast_mention_deny: bool,
    /// When `true`, [`ChatBackend::may_view_chat`] returns `Ok(false)`.
    /// When `false`, forwards to `inner`.
    chat_view_deny: bool,
    /// Counter incremented every time
    /// [`ChatBackend::is_contact_blocked`] is invoked. Lets a test
    /// verify whether a handler consulted the predicate, which has no
    /// effect on the handler's wire response.
    is_contact_blocked_calls: Arc<AtomicU64>,
    /// When `true`, the per-Chat restriction hooks apply the
    /// draft-atwood-jmap-chat-mimi-00 rules to every Chat whose extension
    /// properties hold `roomId`. When `false`, they forward to `inner`.
    room_restricted: bool,
    /// [`JmapBackend::get_objects`] calls per object type name, so a test
    /// can count the reads a handler makes.
    get_objects_calls: Arc<Mutex<HashMap<&'static str, u64>>>,
    /// When `Some`, [`JmapBackend::get_objects`] for this type name
    /// returns an error. Other types forward to `inner`.
    get_objects_fail: Option<&'static str>,
    /// The `id` of every object passed to [`ChatBackend::create_object`],
    /// in call order.
    create_input_ids: Arc<Mutex<Vec<serde_json::Value>>>,
    /// When `Some`, [`ChatBackend::attachment_blob`] returns
    /// `Ok(Err(<this>))`.
    attachment_blob_reject: Option<SetError>,
    /// When `true`, [`ChatBackend::attachment_blob`] returns `Err`.
    attachment_blob_fail: bool,
    /// When `Some`, [`ChatBackend::filenode_attachment`] returns
    /// `Ok(<this>.map(Some))`; when `None`, it forwards to `inner`.
    filenode_attachment: Option<Result<jmap_chat_types::Attachment, SetError>>,
    /// Count of [`ChatBackend::filenode_attachment`] calls.
    filenode_attachment_calls: Arc<AtomicU64>,
}

/// The Chat/set patch keys draft-atwood-jmap-chat-mimi-00 L251 forbids on
/// a Chat with a `roomId`.
const ROOM_FORBIDDEN_CHAT_KEYS: &[&str] = &[
    "name",
    "description",
    "avatarBlobId",
    "addMembers",
    "removeMembers",
    "updateMemberRoles",
];

/// The Message properties draft-atwood-jmap-chat-mimi-00 L426 forbids
/// changing in a Chat with a `roomId`.
const ROOM_FROZEN_MESSAGE_PROPS: &[&str] = &["body", "bodyType", "reactions"];

fn is_room_chat(chat: &jmap_chat_types::Chat) -> bool {
    chat.extra.contains_key("roomId")
}

impl TrackingBackend {
    /// Fresh `TrackingBackend` with all policy hooks at their default
    /// no-op behaviour (slow-mode allows everything, emoji-set
    /// authorization allows everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure the wrapper so [`ChatBackend::slow_mode_check`] always
    /// rejects with the given `retry_after` UTCDate. The wrapped
    /// `MemoryBackend` is otherwise functional.
    pub fn with_slow_mode_blocking(retry_after: UTCDate) -> Self {
        Self {
            slow_mode_block: Some(retry_after),
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`ChatBackend::may_set_custom_emoji`]
    /// returns a `Forbidden` SetError for every op. The wrapped
    /// `MemoryBackend` is otherwise functional.
    pub fn with_emoji_set_denied() -> Self {
        Self {
            emoji_set_deny: true,
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`ChatBackend::may_broadcast_mention`]
    /// returns a `Forbidden` SetError. The wrapped `MemoryBackend` is
    /// otherwise functional.
    pub fn with_broadcast_mention_denied() -> Self {
        Self {
            broadcast_mention_deny: true,
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`ChatBackend::may_view_chat`] hides
    /// every Chat. The wrapped `MemoryBackend` is otherwise functional.
    pub fn with_chat_view_denied() -> Self {
        Self {
            chat_view_deny: true,
            ..Self::default()
        }
    }

    /// Configure the wrapper so the per-Chat restriction hooks enforce
    /// the draft-atwood-jmap-chat-mimi-00 rules for Chats with a
    /// `roomId`. The wrapped `MemoryBackend` is otherwise functional.
    pub fn with_room_restrictions() -> Self {
        Self {
            room_restricted: true,
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`JmapBackend::get_objects`] fails for
    /// objects of `type_name`. The wrapped `MemoryBackend` is otherwise
    /// functional.
    pub fn with_get_objects_failing(type_name: &'static str) -> Self {
        Self {
            get_objects_fail: Some(type_name),
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`ChatBackend::attachment_blob`] rejects
    /// every blob with `set_error`. The wrapped `MemoryBackend` is
    /// otherwise functional.
    pub fn with_attachment_blob_rejected(set_error: SetError) -> Self {
        Self {
            attachment_blob_reject: Some(set_error),
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`ChatBackend::attachment_blob`] fails
    /// with a backend error. The wrapped `MemoryBackend` is otherwise
    /// functional.
    pub fn with_attachment_blob_failing() -> Self {
        Self {
            attachment_blob_fail: true,
            ..Self::default()
        }
    }

    /// Configure the wrapper so [`ChatBackend::filenode_attachment`]
    /// resolves every `filenodeId` entry to `result`. The wrapped
    /// `MemoryBackend` is otherwise functional.
    pub fn with_filenode_attachment(result: Result<jmap_chat_types::Attachment, SetError>) -> Self {
        Self {
            filenode_attachment: Some(result),
            ..Self::default()
        }
    }

    /// Count of [`ChatBackend::filenode_attachment`] calls so far.
    pub fn filenode_attachment_call_count(&self) -> u64 {
        self.filenode_attachment_calls.load(Ordering::SeqCst)
    }

    /// Borrow the underlying `MemoryBackend` for test seeding (e.g.
    /// `register_account`).
    pub fn inner(&self) -> &MemoryBackend {
        &self.inner
    }

    /// Return the number of [`JmapBackend::get_objects`] calls for
    /// objects of `type_name`.
    pub fn get_objects_call_count(&self, type_name: &str) -> u64 {
        let calls = self.get_objects_calls.lock().expect("lock poisoned");
        calls.get(type_name).copied().unwrap_or(0)
    }

    /// Return the `id` of every object passed to
    /// [`ChatBackend::create_object`], in call order.
    pub fn create_input_ids(&self) -> Vec<serde_json::Value> {
        self.create_input_ids.lock().expect("lock poisoned").clone()
    }

    /// Return the number of times the wrapped
    /// [`ChatBackend::is_contact_blocked`] has been invoked. Used by
    /// tests that verify the handler reaches the predicate-call site.
    pub fn is_contact_blocked_call_count(&self) -> u64 {
        self.is_contact_blocked_calls.load(Ordering::SeqCst)
    }
}

impl JmapBackend for TrackingBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        *self
            .get_objects_calls
            .lock()
            .expect("lock poisoned")
            .entry(O::TYPE_NAME)
            .or_default() += 1;
        if self.get_objects_fail == Some(O::TYPE_NAME) {
            return Err(MemoryError::new("injected get_objects failure"));
        }
        self.inner
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner
            .query_objects::<O>(
                caller,
                account_id,
                filter,
                sort,
                limit,
                position,
                anchor,
                anchor_offset,
            )
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .query_changes::<O>(
                caller,
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

impl ChatBackend for TrackingBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let input = serde_json::to_value(&obj).expect("JmapObject serializes");
        self.create_input_ids
            .lock()
            .expect("lock poisoned")
            .push(input["id"].clone());
        self.inner
            .create_object::<O>(caller, account_id, create_id, obj)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        self.inner
            .update_object::<O>(caller, account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.inner.destroy_object::<O>(caller, account_id, id).await
    }

    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool {
        self.inner.supports_type::<O>(caller, account_id)
    }

    async fn join_space_with_invite(
        &self,
        caller: &(),
        account_id: &Id,
        invite_id: &Id,
        joined_at: &UTCDate,
    ) -> Result<Result<(), InviteJoinRefusal>, Self::Error> {
        self.inner
            .join_space_with_invite(caller, account_id, invite_id, joined_at)
            .await
    }

    fn generate_invite_code(&self) -> String {
        self.inner.generate_invite_code()
    }

    fn limits(&self, caller: &(), account_id: &Id) -> ChatLimits {
        self.inner.limits(caller, account_id)
    }

    async fn apply_space_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_patch(caller, account_id, space_id, ops)
            .await
    }

    async fn apply_space_metadata_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        patch: jmap_chat_types::SpaceMetadataPatch,
    ) -> Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_metadata_patch(caller, account_id, space_id, patch)
            .await
    }

    async fn slow_mode_check(
        &self,
        caller: &(),
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<(), SlowModeError> {
        match &self.slow_mode_block {
            Some(d) => Err(SlowModeError::new(d.clone())),
            None => {
                self.inner
                    .slow_mode_check(caller, account_id, chat_id)
                    .await
            }
        }
    }

    async fn may_set_custom_emoji(
        &self,
        caller: &(),
        account_id: &Id,
        target_space_id: Option<&Id>,
        op: EmojiSetOp,
    ) -> Result<Result<(), SetError>, Self::Error> {
        if self.emoji_set_deny {
            Ok(Err(SetError::new(SetErrorType::Forbidden)
                .with_description(
                    "TrackingBackend deny_set_custom_emoji: emoji authorization denied for test.",
                )))
        } else {
            self.inner
                .may_set_custom_emoji(caller, account_id, target_space_id, op)
                .await
        }
    }

    async fn may_broadcast_mention(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        chat_id: &Id,
    ) -> Result<Result<(), SetError>, Self::Error> {
        if self.broadcast_mention_deny {
            Ok(Err(SetError::new(SetErrorType::Forbidden)
                .with_description(
                    "TrackingBackend deny_broadcast_mention: mention_broadcast denied for test.",
                )))
        } else {
            self.inner
                .may_broadcast_mention(caller, account_id, space_id, chat_id)
                .await
        }
    }

    async fn may_view_chat(
        &self,
        caller: &(),
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<bool, Self::Error> {
        if self.chat_view_deny {
            Ok(false)
        } else {
            self.inner.may_view_chat(caller, account_id, chat_id).await
        }
    }

    async fn is_contact_blocked(
        &self,
        caller: &(),
        account_id: &Id,
        contact_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.is_contact_blocked_calls.fetch_add(1, Ordering::SeqCst);
        self.inner
            .is_contact_blocked(caller, account_id, contact_id)
            .await
    }

    async fn may_create_message(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
    ) -> Result<Result<(), SetError>, Self::Error> {
        if self.room_restricted && is_room_chat(chat) {
            return Ok(Err(SetError::new(SetErrorType::Forbidden)
                .with_description(
                    "messages in a MIMI room are sent with Room/send",
                )));
        }
        self.inner
            .may_create_message(caller, account_id, chat)
            .await
    }

    async fn may_update_message(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
        message: &jmap_chat_types::Message,
        patch_keys: &[&str],
    ) -> Result<Result<(), SetError>, Self::Error> {
        if self.room_restricted && is_room_chat(chat) {
            let mut changed: Vec<String> = Vec::new();
            for key in patch_keys {
                let top = key.split('/').next().unwrap_or(key);
                if ROOM_FROZEN_MESSAGE_PROPS.contains(&top) && !changed.iter().any(|c| c == top) {
                    changed.push(top.to_owned());
                }
            }
            if !changed.is_empty() {
                return Ok(Err(
                    SetError::new(SetErrorType::InvalidProperties).with_properties(changed)
                ));
            }
        }
        self.inner
            .may_update_message(caller, account_id, chat, message, patch_keys)
            .await
    }

    async fn may_update_chat(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
        patch_keys: &[&str],
    ) -> Result<Result<(), SetError>, Self::Error> {
        if self.room_restricted
            && is_room_chat(chat)
            && patch_keys
                .iter()
                .any(|k| ROOM_FORBIDDEN_CHAT_KEYS.contains(k))
        {
            return Ok(Err(SetError::new(SetErrorType::Forbidden)
                .with_description("a MIMI room is changed with Room/update")));
        }
        self.inner
            .may_update_chat(caller, account_id, chat, patch_keys)
            .await
    }

    fn retains_edit_history(&self) -> bool {
        self.inner.retains_edit_history()
    }

    async fn expire_message(
        &self,
        caller: &(),
        account_id: &Id,
        message_id: &Id,
    ) -> Result<(), Self::Error> {
        self.inner
            .expire_message(caller, account_id, message_id)
            .await
    }

    async fn read_and_burn_message(
        &self,
        caller: &(),
        account_id: &Id,
        message_id: &Id,
        patch: PatchObject,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.inner
            .read_and_burn_message(caller, account_id, message_id, patch)
            .await
    }

    async fn attachment_blob(
        &self,
        caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Result<Option<AttachmentBlob>, SetError>, Self::Error> {
        if self.attachment_blob_fail {
            return Err(MemoryError::new("injected attachment_blob failure"));
        }
        if let Some(set_error) = &self.attachment_blob_reject {
            return Ok(Err(set_error.clone()));
        }
        self.inner
            .attachment_blob(caller, account_id, blob_id)
            .await
    }

    async fn filenode_attachment(
        &self,
        caller: &(),
        account_id: &Id,
        chat: &jmap_chat_types::Chat,
        attachment: &jmap_chat_types::AttachmentCreate,
    ) -> Result<Result<Option<jmap_chat_types::Attachment>, SetError>, Self::Error> {
        self.filenode_attachment_calls
            .fetch_add(1, Ordering::SeqCst);
        match &self.filenode_attachment {
            Some(result) => Ok(result.clone().map(Some)),
            None => {
                self.inner
                    .filenode_attachment(caller, account_id, chat, attachment)
                    .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IdentityBackend — MemoryBackend wrapper exposing a resolvable caller id
// ---------------------------------------------------------------------------

/// A test backend with `CallerCtx = Id` that overrides
/// [`JmapBackend::principal_id`] to return the caller verbatim.
///
/// The reference `MemoryBackend` uses `CallerCtx = ()` and inherits
/// the default `principal_id` impl returning `None`. Its permission
/// and role-hierarchy gates resolve against the owner's member entry
/// (the accountId) either way; `IdentityBackend` exercises callers
/// whose resolved principal differs from the accountId, and the
/// `Space/get` member visibility check, which applies only when
/// `principal_id` returns `Some`.
///
/// The wrapper forwards every `ChatBackend` / `JmapBackend` method to
/// the inner `MemoryBackend` with `&()` (the inner backend remains
/// `CallerCtx = ()`).
///
/// Per the workspace AGENTS.md "Caller identity (foundation seam)"
/// section: "Backends that honor identity-dependent semantics MUST
/// override this method." `IdentityBackend` is the chat-server's
/// first such backend (test-only); production deployments will write
/// their own. See `bd:JMAP-g7wu.2.4.3`.
#[derive(Clone, Default)]
pub struct IdentityBackend {
    inner: MemoryBackend,
}

impl IdentityBackend {
    /// Fresh `IdentityBackend` wrapping an empty `MemoryBackend`.
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
        }
    }

    /// Borrow the wrapped `MemoryBackend` for test seeding (e.g.
    /// `register_account`, `insert_object_for_test`,
    /// `set_protect_last_admin_for_test`).
    pub fn inner(&self) -> &MemoryBackend {
        &self.inner
    }
}

impl JmapBackend for IdentityBackend {
    type Error = MemoryError;
    type CallerCtx = Id;

    fn principal_id(caller: &Self::CallerCtx) -> Option<&Id> {
        Some(caller)
    }

    async fn account_exists(&self, _caller: &Id, account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(&(), account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(&(), account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(&(), account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .get_changes::<O>(&(), account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner
            .query_objects::<O>(
                &(),
                account_id,
                filter,
                sort,
                limit,
                position,
                anchor,
                anchor_offset,
            )
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .query_changes::<O>(
                &(),
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

impl ChatBackend for IdentityBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        self.inner
            .create_object::<O>(&(), account_id, create_id, obj)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        self.inner
            .update_object::<O>(&(), account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.inner.destroy_object::<O>(&(), account_id, id).await
    }

    fn supports_type<O: JmapObject>(&self, _caller: &Id, account_id: &Id) -> bool {
        self.inner.supports_type::<O>(&(), account_id)
    }

    async fn join_space_with_invite(
        &self,
        _caller: &Id,
        account_id: &Id,
        invite_id: &Id,
        joined_at: &UTCDate,
    ) -> Result<Result<(), InviteJoinRefusal>, Self::Error> {
        self.inner
            .join_space_with_invite(&(), account_id, invite_id, joined_at)
            .await
    }

    fn generate_invite_code(&self) -> String {
        self.inner.generate_invite_code()
    }

    fn limits(&self, _caller: &Id, account_id: &Id) -> ChatLimits {
        self.inner.limits(&(), account_id)
    }

    fn protect_last_admin(&self, _caller: &Id, account_id: &Id) -> bool {
        self.inner.protect_last_admin(&(), account_id)
    }

    async fn apply_space_patch(
        &self,
        _caller: &Id,
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_patch(&(), account_id, space_id, ops)
            .await
    }

    async fn apply_space_metadata_patch(
        &self,
        _caller: &Id,
        account_id: &Id,
        space_id: &Id,
        patch: jmap_chat_types::SpaceMetadataPatch,
    ) -> Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_metadata_patch(&(), account_id, space_id, patch)
            .await
    }

    async fn slow_mode_check(
        &self,
        _caller: &Id,
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<(), SlowModeError> {
        self.inner.slow_mode_check(&(), account_id, chat_id).await
    }

    async fn may_set_custom_emoji(
        &self,
        _caller: &Id,
        account_id: &Id,
        target_space_id: Option<&Id>,
        op: EmojiSetOp,
    ) -> Result<Result<(), SetError>, Self::Error> {
        self.inner
            .may_set_custom_emoji(&(), account_id, target_space_id, op)
            .await
    }

    async fn may_set_pinned_messages(
        &self,
        _caller: &Id,
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<Result<(), SetError>, Self::Error> {
        self.inner
            .may_set_pinned_messages(&(), account_id, chat_id)
            .await
    }

    async fn may_set_space_ban(
        &self,
        _caller: &Id,
        account_id: &Id,
        space_id: &Id,
    ) -> Result<Result<(), SetError>, Self::Error> {
        self.inner
            .may_set_space_ban(&(), account_id, space_id)
            .await
    }

    async fn may_set_space_invite(
        &self,
        _caller: &Id,
        account_id: &Id,
        space_id: &Id,
        created_by: Option<&Id>,
    ) -> Result<Result<(), SetError>, Self::Error> {
        self.inner
            .may_set_space_invite(&(), account_id, space_id, created_by)
            .await
    }

    async fn may_get_space_invite(
        &self,
        _caller: &Id,
        account_id: &Id,
        invite: &jmap_chat_types::SpaceInvite,
    ) -> Result<bool, Self::Error> {
        self.inner
            .may_get_space_invite(&(), account_id, invite)
            .await
    }

    async fn may_get_space_ban(
        &self,
        _caller: &Id,
        account_id: &Id,
        ban: &jmap_chat_types::SpaceBan,
    ) -> Result<bool, Self::Error> {
        self.inner.may_get_space_ban(&(), account_id, ban).await
    }

    async fn may_send_message(
        &self,
        _caller: &Id,
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<Result<(), SetError>, Self::Error> {
        self.inner.may_send_message(&(), account_id, chat_id).await
    }

    async fn may_view_chat(
        &self,
        _caller: &Id,
        account_id: &Id,
        chat_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.inner.may_view_chat(&(), account_id, chat_id).await
    }

    async fn is_contact_blocked(
        &self,
        _caller: &Id,
        account_id: &Id,
        contact_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.inner
            .is_contact_blocked(&(), account_id, contact_id)
            .await
    }

    fn retains_edit_history(&self) -> bool {
        self.inner.retains_edit_history()
    }

    async fn expire_message(
        &self,
        _caller: &Id,
        account_id: &Id,
        message_id: &Id,
    ) -> Result<(), Self::Error> {
        self.inner.expire_message(&(), account_id, message_id).await
    }

    async fn read_and_burn_message(
        &self,
        _caller: &Id,
        account_id: &Id,
        message_id: &Id,
        patch: PatchObject,
    ) -> Result<(), BackendSetError<Self::Error>> {
        self.inner
            .read_and_burn_message(&(), account_id, message_id, patch)
            .await
    }
}

// ---------------------------------------------------------------------------
// InjectableBackend — MemoryBackend wrapper with per-(type, op) fault injection
// ---------------------------------------------------------------------------

/// Canary literal embedded in every fault-injected `BackendSetError::Other`
/// payload. Tests assert this string does NOT appear in the wire-format
/// `/set` response, which proves the per-id `serverFail` Value path redacts
/// backend-error Display text through
/// [`jmap_server::server_fail_value_from_backend`] rather than echoing it
/// onto the wire. Shaped like a leaked credential so a future contributor
/// who breaks the redaction sees the visible footgun in the test failure.
///
/// Mirrors the canonical jmap-mail-server `FAULTY_BACKEND_CANARY` literal.
pub const INJECTABLE_BACKEND_CANARY: &str = "BACKEND-CANARY-LEAK-DO-NOT-WIRE-7f3a2";

/// A wrapper around [`MemoryBackend`] that can fault-inject
/// `BackendSetError::Other` (or `Self::Error`) for specific
/// `(type_name, operation)` pairs. The setup writes go through to the
/// inner [`MemoryBackend`] normally; only operations matching a
/// previously-injected pair fail.
///
/// Call [`InjectableBackend::inject`] before the operation under test. The
/// first matching call returns an error whose Display contains
/// [`INJECTABLE_BACKEND_CANARY`]; the flag is cleared so subsequent calls
/// go to the inner backend normally (fire-once semantics).
///
/// Mirrors the canonical jmap-mail-server `FaultyBackend` injection
/// pattern. Lives alongside [`FaultyBackend`] (the always-fails
/// negative-path wrapper) rather than extending it because the two
/// existing `FaultyBackend` test sites depend on the always-fails
/// behaviour.
///
/// # Supported injection targets
///
/// - `(O::TYPE_NAME, "create")` — fails the next
///   [`ChatBackend::create_object`] call for type `O`.
/// - `(O::TYPE_NAME, "destroy")` — fails the next
///   [`ChatBackend::destroy_object`] call for type `O`.
/// - `(O::TYPE_NAME, "get")` — fails the next
///   [`JmapBackend::get_objects`] call for type `O`.
/// - [`InjectableBackend::queue_chat_get_fault_after_create`] — arms
///   `("Chat", "get")` after the next successful
///   [`ChatBackend::create_object`] call for type `Chat`, so the
///   handler's pre-create snapshot read succeeds and its post-create
///   race-detection re-fetch fails.
/// - `("Message", "read_and_burn")` — fails the next
///   [`ChatBackend::read_and_burn_message`] call.
/// - [`InjectableBackend::queue_chat_race_phantom`] — after the next
///   [`ChatBackend::create_object`] call for type `Chat` returns
///   successfully, insert a phantom `Direct` Chat into the inner
///   backend with a lex-smaller id and the supplied `contact_id`.
///   This makes the handler's race-detection re-fetch see a
///   duplicate, driving it into the cleanup-destroy branch. Combine
///   with `inject("Chat", "destroy")` to exercise the
///   `chat.rs:413-428` redaction site.
///
/// Other `(type, op)` pairs are not currently honored; extend this
/// wrapper if a new redaction-canary site lands.
pub struct InjectableBackend {
    pub inner: MemoryBackend,
    failures:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(&'static str, &'static str)>>>,
    /// Pending race-phantom seed: when set, the next `create_object::<Chat>`
    /// call (post-success) seeds a phantom Direct Chat into the inner
    /// backend at the supplied `(account_id, phantom_id, contact_id)`.
    /// Fire-once.
    chat_race_phantom: std::sync::Arc<std::sync::Mutex<Option<ChatRacePhantom>>>,
    /// When set, the next successful `create_object::<Chat>` arms a
    /// `("Chat", "get")` fault. Fire-once.
    chat_get_fault_after_create: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Pre-canned phantom-Chat seed: where to insert (`account_id`), what
/// id to seed it with (`phantom_id` — must be lex-smaller than the
/// next-assigned new chat id for the race-detection canonical pick
/// to select it), and the `contact_id` of the Direct chat being
/// duplicated.
struct ChatRacePhantom {
    account_id: String,
    phantom_id: String,
    contact_id: String,
}

impl InjectableBackend {
    /// Fresh `InjectableBackend` wrapping an empty [`MemoryBackend`] with
    /// no faults queued.
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            failures: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            chat_race_phantom: std::sync::Arc::new(std::sync::Mutex::new(None)),
            chat_get_fault_after_create: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
        }
    }

    /// Schedule a fault for the next call to `op` on `type_name`.
    ///
    /// Calling `inject` twice for the same `(type_name, op)` pair is a
    /// no-op — only one fault is queued; the second call is silently
    /// ignored.
    pub fn inject(&self, type_name: &'static str, op: &'static str) {
        self.failures.lock().unwrap().insert((type_name, op));
    }

    /// Schedule a phantom-Direct-Chat seed to fire after the next
    /// successful `create_object::<Chat>` call. The phantom is inserted
    /// into the inner backend (via `MemoryBackend::insert_object_for_test`)
    /// AFTER the inner create returns, so the handler's pre-create
    /// `existing_chats` snapshot does not see it, but the handler's
    /// post-create race-detection re-fetch does.
    ///
    /// `phantom_id` should be lex-smaller than the next id the inner
    /// `MemoryBackend` will assign to the about-to-be-created Chat —
    /// otherwise the race-detection code picks the new id as canonical
    /// and the cleanup-destroy branch never runs. `MemoryBackend` mints
    /// uuid v4 ids (36 chars of `[0-9a-f-]` starting with a hex digit),
    /// so `"0"` is lex-smaller than every id it can assign.
    ///
    /// Fire-once.
    pub fn queue_chat_race_phantom(&self, account_id: &str, phantom_id: &str, contact_id: &str) {
        *self.chat_race_phantom.lock().unwrap() = Some(ChatRacePhantom {
            account_id: account_id.to_owned(),
            phantom_id: phantom_id.to_owned(),
            contact_id: contact_id.to_owned(),
        });
    }

    /// Fail the first `get_objects::<Chat>` call that follows the next
    /// successful `create_object::<Chat>` call. Fire-once.
    pub fn queue_chat_get_fault_after_create(&self) {
        self.chat_get_fault_after_create
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Remove and return a previously-injected fault (fire-once).
    /// Returns `true` if the fault was present (and is now consumed).
    fn take_fault(&self, type_name: &'static str, op: &'static str) -> bool {
        self.failures.lock().unwrap().remove(&(type_name, op))
    }

    /// Remove and return a previously-queued chat race phantom (fire-once).
    fn take_chat_race_phantom(&self) -> Option<ChatRacePhantom> {
        self.chat_race_phantom.lock().unwrap().take()
    }
}

impl Default for InjectableBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl JmapBackend for InjectableBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(caller, account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        if self.take_fault(O::TYPE_NAME, "get") {
            return Err(MemoryError::new(format!(
                "injected get error {INJECTABLE_BACKEND_CANARY}"
            )));
        }
        self.inner
            .get_objects::<O>(caller, account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(caller, account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .get_changes::<O>(caller, account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner
            .query_objects::<O>(
                caller,
                account_id,
                filter,
                sort,
                limit,
                position,
                anchor,
                anchor_offset,
            )
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .query_changes::<O>(
                caller,
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

impl ChatBackend for InjectableBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "create") {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected create error {INJECTABLE_BACKEND_CANARY}"
            ))));
        }
        let result = self
            .inner
            .create_object::<O>(caller, account_id, create_id, obj)
            .await;
        // Race-phantom seeding only applies to Chat creates and only
        // after a successful inner create. The phantom is inserted
        // via `insert_object_for_test` which bypasses the normal
        // dedupe / change-log machinery — exactly the harness shape
        // needed to simulate "another transaction won the race".
        if O::TYPE_NAME == "Chat" && result.is_ok() {
            if self
                .chat_get_fault_after_create
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                self.inject("Chat", "get");
            }
            if let Some(phantom) = self.take_chat_race_phantom() {
                let phantom_val = serde_json::json!({
                    "id": &phantom.phantom_id,
                    "kind": "direct",
                    "createdAt": "2024-01-01T00:00:00Z",
                    "unreadCount": 0,
                    "pinnedMessageIds": [],
                    "muted": false,
                    "receiveTypingIndicators": true,
                    "contactId": &phantom.contact_id,
                });
                self.inner.insert_object_for_test(
                    "Chat",
                    &phantom.account_id,
                    &phantom.phantom_id,
                    phantom_val,
                );
            }
        }
        result
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        self.inner
            .update_object::<O>(caller, account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "destroy") {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected destroy error {INJECTABLE_BACKEND_CANARY}"
            ))));
        }
        self.inner.destroy_object::<O>(caller, account_id, id).await
    }

    fn supports_type<O: JmapObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> bool {
        self.inner.supports_type::<O>(caller, account_id)
    }

    async fn join_space_with_invite(
        &self,
        caller: &(),
        account_id: &Id,
        invite_id: &Id,
        joined_at: &UTCDate,
    ) -> Result<Result<(), InviteJoinRefusal>, Self::Error> {
        self.inner
            .join_space_with_invite(caller, account_id, invite_id, joined_at)
            .await
    }

    fn generate_invite_code(&self) -> String {
        self.inner.generate_invite_code()
    }

    fn limits(&self, caller: &(), account_id: &Id) -> ChatLimits {
        self.inner.limits(caller, account_id)
    }

    async fn apply_space_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> Result<Vec<OpResult>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_patch(caller, account_id, space_id, ops)
            .await
    }

    async fn apply_space_metadata_patch(
        &self,
        caller: &(),
        account_id: &Id,
        space_id: &Id,
        patch: jmap_chat_types::SpaceMetadataPatch,
    ) -> Result<Option<jmap_chat_types::Space>, BackendSetError<Self::Error>> {
        self.inner
            .apply_space_metadata_patch(caller, account_id, space_id, patch)
            .await
    }

    async fn expire_message(
        &self,
        caller: &(),
        account_id: &Id,
        message_id: &Id,
    ) -> Result<(), Self::Error> {
        self.inner
            .expire_message(caller, account_id, message_id)
            .await
    }

    async fn read_and_burn_message(
        &self,
        caller: &(),
        account_id: &Id,
        message_id: &Id,
        patch: PatchObject,
    ) -> Result<(), BackendSetError<Self::Error>> {
        if self.take_fault("Message", "read_and_burn") {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected read_and_burn error {INJECTABLE_BACKEND_CANARY}"
            ))));
        }
        self.inner
            .read_and_burn_message(caller, account_id, message_id, patch)
            .await
    }
}

// ---------------------------------------------------------------------------
// Shared Space seeding helpers (bd:JMAP-x2gd.80)
//
// Used by the apply_space_patch integration tests
// (role_member_apply.rs, channel_category_apply.rs,
// space_metadata_apply.rs). The projection-test fixture in
// space_get_projection.rs has a structurally different shape (full
// categories/channels) and stays local to that file.
// ---------------------------------------------------------------------------

/// Default account id used by the apply_space_patch test suites and
/// by the bulk of [`integration.rs`](file://./integration.rs).
pub const ACCOUNT_ID: &str = "a1";

/// Secondary account id used by integration.rs cross-account
/// isolation tests. Sibling apply-test files do not use this.
pub const ACCOUNT_ID_B: &str = "a2";

/// Default Space id used by the apply_space_patch test suites.
pub const SPACE_ID: &str = "s1";

/// Seed a [`SPACE_ID`] in [`ACCOUNT_ID`] of the given backend with
/// the supplied `roles` and `members`. The Space is seeded with
/// `description: "original"` so the metadata-mutation tests can
/// assert the description was (or was not) mutated; the apply-tests
/// for Role/Member/Channel/Category do not inspect the description
/// and are unaffected.
///
/// Bypasses `handle_space_set` create flow (which in the reference
/// impl does NOT auto-add the creator as a member) by going through
/// [`MemoryBackend::insert_object_for_test`].
pub fn seed_space(
    backend: &IdentityBackend,
    roles: serde_json::Value,
    members: serde_json::Value,
) -> Id {
    let space_val = serde_json::json!({
        "id": SPACE_ID,
        "name": "Test Space",
        "description": "original",
        "createdAt": "2026-01-01T00:00:00Z",
        "memberCount": members.as_array().map(Vec::len).unwrap_or(0),
        "categories": [],
        "uncategorizedChannelIds": [],
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "roles": roles,
        "members": members,
    });
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend
        .inner()
        .insert_object_for_test("Space", ACCOUNT_ID, SPACE_ID, space_val);
    Id::from(SPACE_ID)
}

/// Convenience: seed a Space where `admin_id` holds full admin
/// permissions at position 100. Returns the seeded Space id.
pub fn seed_with_admin(backend: &IdentityBackend, admin_id: &str) -> Id {
    seed_space(
        backend,
        serde_json::json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": [
                "manage_space",
                "manage_roles",
                "manage_members",
                "manage_channels"
            ],
            "position": 100
        }]),
        serde_json::json!([{
            "id": admin_id,
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    )
}

/// Seed a Space where `caller_id` is a non-admin member holding only
/// the implicit `@everyone` floor (no explicit roles). One admin
/// (id `"admin-user"`) is also seeded so the Space is not empty of
/// admins (relevant when last-admin-protection is active).
pub fn seed_with_non_admin_caller(backend: &IdentityBackend, caller_id: &str) {
    seed_space(
        backend,
        serde_json::json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": [
                "manage_space",
                "manage_roles",
                "manage_members",
                "manage_channels"
            ],
            "position": 100
        }]),
        serde_json::json!([
            { "id": "admin-user", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": caller_id,    "roleIds": [],          "joinedAt": "2026-01-02T00:00:00Z" }
        ]),
    );
}

// ---------------------------------------------------------------------------
// Builder helpers: create-as-prerequisite for tests-of-something-else.
//
// These helpers exist to collapse the recurring 14-line boilerplate of
// "call handle_*_set in create-mode, extract the server-assigned id"
// at sites where the create is a fixture step, not the subject of the
// test. Tests that exercise the create path itself MUST call the
// underlying `handle_*_set` directly so the JSON shape under test
// remains visible at the call site (bd:JMAP-x2gd.82).
// ---------------------------------------------------------------------------

/// Create a Space with the given name in [`ACCOUNT_ID`] and return its
/// server-assigned id. Goes through the `handle_space_set` create flow.
///
/// The placeholder client id is `"s0"`. Only `name` is set on the
/// create object. For non-default props (e.g. `isPublic: true`), see
/// [`make_space_with_props`].
pub async fn make_space(backend: &MemoryBackend, name: &str) -> String {
    let HandlerOutput { primary: resp, .. } = jmap_chat_server::handle_space_set(
        backend,
        &(),
        serde_json::json!({
            "accountId": ACCOUNT_ID,
            "create": { "s0": { "name": name } }
        }),
    )
    .await
    .expect("make_space: handle_space_set");
    resp["created"]["s0"]["id"]
        .as_str()
        .expect("make_space: server-assigned id")
        .to_owned()
}

/// Create a Space with custom props in [`ACCOUNT_ID`] and return its
/// server-assigned id. `props` is the body of the create object — for
/// example `serde_json::json!({"name": "X", "isPublic": true})`.
///
/// Use this when [`make_space`]'s name-only shape is insufficient.
pub async fn make_space_with_props(backend: &MemoryBackend, props: serde_json::Value) -> String {
    let HandlerOutput { primary: resp, .. } = jmap_chat_server::handle_space_set(
        backend,
        &(),
        serde_json::json!({
            "accountId": ACCOUNT_ID,
            "create": { "s0": props }
        }),
    )
    .await
    .expect("make_space_with_props: handle_space_set");
    resp["created"]["s0"]["id"]
        .as_str()
        .expect("make_space_with_props: server-assigned id")
        .to_owned()
}

/// Create a Chat with `kind: "group"` and the given name in
/// [`ACCOUNT_ID`] and return its server-assigned id. Goes through the
/// `handle_chat_set` create flow.
///
/// The placeholder client id is `"c0"`. For Chats with other kinds
/// (`"direct"` / `"channel"`) or extra props (`contactId`, `spaceId`),
/// call `handle_chat_set` directly so the JSON shape under test
/// remains visible at the call site.
pub async fn make_chat_group(backend: &MemoryBackend, name: &str) -> String {
    let HandlerOutput { primary: resp, .. } = jmap_chat_server::handle_chat_set(
        backend,
        &(),
        serde_json::json!({
            "accountId": ACCOUNT_ID,
            "create": { "c0": { "kind": "group", "name": name } }
        }),
    )
    .await
    .expect("make_chat_group: handle_chat_set");
    resp["created"]["c0"]["id"]
        .as_str()
        .expect("make_chat_group: server-assigned id")
        .to_owned()
}

/// Store a group Chat with the given `id` in [`ACCOUNT_ID`], with the
/// account as its admin member, so a `Message/set` create naming that
/// `chatId` passes the draft-atwood-jmap-chat-00 §Input Validation
/// `chatId` check. Overwrites any Chat already stored under `id`.
pub fn seed_group_chat(backend: &MemoryBackend, id: &str) {
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        id,
        serde_json::json!({
            "id": id,
            "kind": "group",
            "name": "G",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
}

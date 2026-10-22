//! Task `chatId` write and read rules.
//!
//! Oracle: draft-atwood-jmap-chat-tasks-00.md at jmap-chat-spec be46208
//! ("tasks Lnnn") and RFC 8620:
//!
//! - tasks L106: "`true` if tasks may carry a `chatId` back-reference to a
//!   discussion Chat (see {{task-chatid}}), `false` otherwise."
//! - tasks L153: "When absent, this Task has no associated discussion
//!   thread."
//! - tasks L155: "The referenced Chat, once it exists, MUST be on the same
//!   account as the Task. The server MUST NOT validate `chatId` existence
//!   at write time, allowing the two-step creation pattern described
//!   below."
//! - tasks L240: "The two operations MAY be performed in either order; the
//!   server tolerates transient inconsistency".
//! - tasks L270: "Deployments MAY hide the `chatId` field from Task readers
//!   who lack access to the referenced Chat. The wire contract permits
//!   this (it is equivalent to the Task not exposing the field to that
//!   reader)".
//! - RFC 8620 §5.3 `invalidProperties`: "The record given is invalid in
//!   some way."
//!
//! Records are hand-written JSON from the drafts' field lists.

use std::sync::atomic::{AtomicUsize, Ordering};

use jmap_chat_server::memory::{MemoryBackend as ChatMemory, MemoryError};
use jmap_chat_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, ChatBackend,
    ChatLimits, GetObject, InviteJoinRefusal, JmapBackend, JmapObject, OpResult,
    QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult, SetObject,
    SpacePatchOp,
};
use jmap_chat_tasks_server::{check_task_chat_id, redact_task_chat_ids};
use jmap_chat_types::ChatTasksCapability;

use jmap_tasks_server::SetErrorType;
use jmap_tasks_types::Task;
use jmap_types::{Id, PatchObject, State, UTCDate};
use serde_json::{json, Value};

const ACCOUNT: &str = "A1";

fn capability(may_back_reference_chat: bool) -> ChatTasksCapability {
    serde_json::from_value(json!({
        "mayBindTaskList": true,
        "mayBackReferenceChat": may_back_reference_chat
    }))
    .expect("capability fixture")
}

/// A Task in TaskList `TL` with `extra` merged in.
fn task(id: &str, extra: Value) -> Task {
    let mut v = json!({
        "id": id, "@type": "Task", "uid": format!("{id}-uid"),
        "title": "Ship it", "taskListId": "TL"
    });
    v.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("object").clone());
    serde_json::from_value(v).expect("task fixture")
}

/// Asserts `result` is `invalidProperties` naming exactly `chatId`.
fn assert_invalid_chat_id(result: Result<(), jmap_tasks_server::SetError>) {
    let err = result.expect_err("must be refused");
    let v = serde_json::to_value(&err).expect("SetError serializes");
    assert_eq!(err.error_type, SetErrorType::InvalidProperties, "{v}");
    assert_eq!(v["properties"], json!(["chatId"]), "{v}");
}

/// L155: "The server MUST NOT validate `chatId` existence at write time".
/// No Chat named `C-later` exists anywhere: the check reads no backend.
#[test]
fn chat_id_naming_a_chat_that_does_not_exist_is_accepted() {
    assert_eq!(
        check_task_chat_id(&capability(true), &json!("C-later")),
        Ok(())
    );
}

/// L153: "When absent, this Task has no associated discussion thread";
/// RFC 8620 §5.3 patch `null` removes the property.
#[test]
fn null_chat_id_clears_the_back_reference() {
    assert_eq!(check_task_chat_id(&capability(true), &Value::Null), Ok(()));
}

/// L106: `mayBackReferenceChat` is "`true` if tasks may carry a `chatId`
/// back-reference ..., `false` otherwise".
#[test]
fn chat_id_is_refused_when_back_references_are_not_supported() {
    assert_invalid_chat_id(check_task_chat_id(&capability(false), &json!("C1")));
}

/// L152: "`chatId` (String, optional)": a JMAP Chat `Chat.id`.
#[test]
fn chat_id_that_is_not_an_id_string_is_invalid() {
    assert_invalid_chat_id(check_task_chat_id(&capability(true), &json!(42)));
    assert_invalid_chat_id(check_task_chat_id(&capability(true), &json!("")));
    assert_invalid_chat_id(check_task_chat_id(&capability(true), &json!(["C1"])));
}

/// L270 MAY hide; the default `may_view_chat` is `Ok(true)` (bd:JMAP-bghk
/// fail-open), so nothing is hidden, not even a Chat that does not exist
/// yet (L240 "transient inconsistency").
#[tokio::test]
async fn default_hook_hides_no_chat_id() {
    let chat = ChatMemory::new();
    chat.register_account(&Id::from(ACCOUNT));
    let mut tasks = vec![
        task("T1", json!({ "chatId": "C1" })),
        task("T2", json!({ "chatId": "C-later" })),
    ];
    redact_task_chat_ids(&chat, &(), &Id::from(ACCOUNT), &mut tasks)
        .await
        .expect("redact");
    assert_eq!(
        serde_json::to_value(&tasks).expect("serialize"),
        json!([
            { "id": "T1", "@type": "Task", "uid": "T1-uid", "title": "Ship it",
              "taskListId": "TL", "chatId": "C1" },
            { "id": "T2", "@type": "Task", "uid": "T2-uid", "title": "Ship it",
              "taskListId": "TL", "chatId": "C-later" },
        ])
    );
}

/// L270: "Deployments MAY hide the `chatId` field from Task readers who
/// lack access to the referenced Chat ... (it is equivalent to the Task
/// not exposing the field to that reader)". The reader may not view
/// `C2`: T2 and T3 lose `chatId`, everything else is untouched.
#[tokio::test]
async fn chat_id_of_a_chat_the_reader_cannot_view_is_removed() {
    let chat = ViewGate::new(&["C2"], false);
    let mut tasks = vec![
        task("T1", json!({ "chatId": "C1" })),
        task("T2", json!({ "chatId": "C2" })),
        task("T3", json!({ "chatId": "C2" })),
        task("T4", json!({})),
    ];
    redact_task_chat_ids(&chat, &(), &Id::from(ACCOUNT), &mut tasks)
        .await
        .expect("redact");
    assert_eq!(
        serde_json::to_value(&tasks).expect("serialize"),
        json!([
            { "id": "T1", "@type": "Task", "uid": "T1-uid", "title": "Ship it",
              "taskListId": "TL", "chatId": "C1" },
            { "id": "T2", "@type": "Task", "uid": "T2-uid", "title": "Ship it",
              "taskListId": "TL" },
            { "id": "T3", "@type": "Task", "uid": "T3-uid", "title": "Ship it",
              "taskListId": "TL" },
            { "id": "T4", "@type": "Task", "uid": "T4-uid", "title": "Ship it",
              "taskListId": "TL" },
        ])
    );
    assert_eq!(
        chat.calls.load(Ordering::SeqCst),
        2,
        "the hook is asked once per distinct chatId (C1, C2)"
    );
}

/// A failing hook is an error, not a decision: the Tasks are returned
/// unchanged rather than exposed or hidden by guess.
#[tokio::test]
async fn hook_error_leaves_tasks_unchanged() {
    let chat = ViewGate::new(&["C2"], true);
    let mut tasks = vec![
        task("T1", json!({ "chatId": "C2" })),
        task("T2", json!({ "chatId": "C1" })),
    ];
    let before = serde_json::to_value(&tasks).expect("serialize");
    redact_task_chat_ids(&chat, &(), &Id::from(ACCOUNT), &mut tasks)
        .await
        .expect_err("hook error propagates");
    assert_eq!(serde_json::to_value(&tasks).expect("serialize"), before);
}

/// The reference Chat backend with `may_view_chat` overridden: `false` for
/// the `denied` Chats, an error for every call when `fail` is set.
struct ViewGate {
    inner: ChatMemory,
    denied: Vec<Id>,
    fail: bool,
    calls: AtomicUsize,
}

impl ViewGate {
    fn new(denied: &[&str], fail: bool) -> Self {
        let inner = ChatMemory::new();
        inner.register_account(&Id::from(ACCOUNT));
        Self {
            inner,
            denied: denied.iter().map(|d| Id::from(*d)).collect(),
            fail,
            calls: AtomicUsize::new(0),
        }
    }
}

impl JmapBackend for ViewGate {
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
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        self.inner
            .query_objects::<O>(caller, account_id, params)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .query_changes::<O>(caller, account_id, params)
            .await
    }
}

impl ChatBackend for ViewGate {
    async fn attachment_blob(
        &self,
        caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<
        Result<Option<jmap_chat_server::AttachmentBlob>, jmap_chat_server::SetError>,
        MemoryError,
    > {
        self.inner
            .attachment_blob(caller, account_id, blob_id)
            .await
    }

    async fn may_view_chat(
        &self,
        _caller: &(),
        _account_id: &Id,
        chat_id: &Id,
    ) -> Result<bool, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(MemoryError::new("may_view_chat failed"));
        }
        Ok(!self.denied.contains(chat_id))
    }

    async fn create_object<O, F>(
        &self,
        caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        self.inner
            .create_object(caller, account_id, create_id, build)
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

    fn supports_type<O: JmapObject>(&self, caller: &(), account_id: &Id) -> bool {
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

    async fn everyone_permissions(
        &self,
        _caller: &(),
        account_id: &Id,
        space: &jmap_chat_types::Space,
    ) -> Result<std::collections::HashSet<jmap_chat_types::ChatPermission>, MemoryError> {
        self.inner
            .everyone_permissions(&(), account_id, space)
            .await
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
        self.inner
            .read_and_burn_message(caller, account_id, message_id, patch)
            .await
    }
}

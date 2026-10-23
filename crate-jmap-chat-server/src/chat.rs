//! Chat/* method handlers (draft-atwood-jmap-chat-00 §Chat).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §Chat. The returned
//! `Value` is the corresponding method-response object per the same
//! section refs. `Chat/typing` (draft §Chat) is a Chat-specific signal
//! method with its own request/response shape.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set` surface
//! in the `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use std::collections::{HashMap, HashSet};

use jmap_chat_types::ChatComparator;
use jmap_chat_types::SPEC_CHAT_MEMBER_ROLES;
use jmap_chat_types::{Chat, ChatKind, ChatMember, ChatMemberRole, Message, ReadPosition};
use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::helpers::{PropType, mistyped_create_props};
use crate::helpers::{
    SetAccumulators, default_sort, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, now_utc_string, reject_unsupported_sort, set_error_value,
};
use crate::outbound::{GroupUpdateAction, OutboundEvent};
use crate::{ChatBackend, SetError, SetErrorType};
use jmap_server::{
    HandlerOutput, backend_set_error_value, is_client_updatable, reject_server_set,
    server_fail_from_backend, server_fail_value_from_backend,
};

/// `Chat/set` update keys that change a group Chat's member list
/// (draft-atwood-jmap-chat-00 §Chat/set "Updating a Chat").
const MEMBER_PATCH_KEYS: &[&str] = &["addMembers", "removeMembers", "updateMemberRoles"];

/// `Chat/set` update keys, beyond [`MEMBER_PATCH_KEYS`], that a closed
/// group rejects (draft-atwood-jmap-chat-00 §Chat `closedAt`).
const SHARED_GROUP_KEYS: &[&str] = &["name", "description", "avatarBlobId", "pinnedMessageIds"];

/// `Chat/set` update keys only a group Chat's admins may change
/// (draft-atwood-jmap-chat-00 §Chat/set "Updating a Chat": "For group
/// chats, admins additionally may update: `name`, `description`,
/// `avatarBlobId`.").
const GROUP_ADMIN_KEYS: &[&str] = &["name", "description", "avatarBlobId"];

/// Chat properties draft-atwood-jmap-chat-00 §Chat Object Fields scopes
/// to one kind ("**Direct chats only.**", "**Group chats only.**",
/// "**Channel Chats only.**"), with that kind. `name` is "**Group and
/// channel Chats.**", and `Chat/set` never creates a channel, so a group
/// is the only created kind that carries it. A `Chat/set` create of
/// another kind that carries one is `invalidProperties` (RFC 8620 §5.3).
const KIND_SCOPED_CREATE_PROPS: &[(&str, ChatKind)] = &[
    ("contactId", ChatKind::Direct),
    ("name", ChatKind::Group),
    ("description", ChatKind::Group),
    ("avatarBlobId", ChatKind::Group),
    ("spaceId", ChatKind::Channel),
];

/// Whether `account_id` holds the `"admin"` role in `chat.members`.
fn is_chat_admin(chat: &Chat, account_id: &Id) -> bool {
    chat.members
        .iter()
        .flatten()
        .any(|m| m.id == *account_id && m.role == ChatMemberRole::Admin)
}

/// One `addMembers` or `updateMemberRoles` entry.
#[derive(Deserialize)]
struct MemberRoleEntry {
    id: Id,
    role: Option<ChatMemberRole>,
}

/// Apply the member patch keys in `patch` to `chat.members` on behalf of
/// `owner_id` and return the resulting member list, and whether the
/// patch closes the group.
///
/// Per draft-atwood-jmap-chat-00 §Chat/set "Updating a Chat":
/// - a malformed entry, or a `role` other than `"admin"` or `"member"`,
///   is `invalidProperties` naming the key; an `updateMemberRoles` entry
///   must carry a `role` and name a current member;
/// - "all require admin role, except leaving the group": a non-admin may
///   only send `removeMembers` listing just their own id, else
///   `forbidden`. `owner_id` is the accountId, which references the owner
///   (draft §Terminology), so a non-admin owner leaves with
///   `removeMembers: [accountId]`; the keys apply to group chats only;
/// - more members than `max_members` after `addMembers` is `overQuota`;
/// - "If the owner is the last admin and other members remain, leaving
///   closes the group": the caller was an admin, is removed, and members
///   but no admin remain.
///
/// Per the implementer guide's "Deployments SHOULD avoid admin-less open
/// groups by refusing to let the last admin demote themselves via
/// `updateMemberRoles`", an `updateMemberRoles` patch that leaves members
/// but no admin is `forbidden`.
fn apply_member_patch_keys(
    chat: &Chat,
    owner_id: &Id,
    patch: &serde_json::Map<String, Value>,
    max_members: u32,
) -> Result<(Vec<ChatMember>, bool), Box<SetError>> {
    fn parse<T: serde::de::DeserializeOwned>(
        patch: &serde_json::Map<String, Value>,
        key: &str,
    ) -> Result<Option<T>, Box<SetError>> {
        patch
            .get(key)
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|_| {
                Box::new(SetError::new(SetErrorType::InvalidProperties).with_properties([key]))
            })
    }
    let valid_role = |role: &ChatMemberRole| SPEC_CHAT_MEMBER_ROLES.contains(&role.as_str());
    let invalid =
        |key: &str| Box::new(SetError::new(SetErrorType::InvalidProperties).with_properties([key]));
    let forbidden =
        |desc: &str| Box::new(SetError::new(SetErrorType::Forbidden).with_description(desc));

    let add: Option<Vec<MemberRoleEntry>> = parse(patch, "addMembers")?;
    let remove: Option<Vec<Id>> = parse(patch, "removeMembers")?;
    let update: Option<Vec<MemberRoleEntry>> = parse(patch, "updateMemberRoles")?;
    if add
        .iter()
        .flatten()
        .any(|e| e.role.as_ref().is_some_and(|r| !valid_role(r)))
    {
        return Err(invalid("addMembers"));
    }
    if update
        .iter()
        .flatten()
        .any(|e| !e.role.as_ref().is_some_and(valid_role))
    {
        return Err(invalid("updateMemberRoles"));
    }

    if chat.kind != ChatKind::Group {
        return Err(forbidden("member patch keys apply to group chats only"));
    }
    let is_admin = is_chat_admin(chat, owner_id);
    let mut members = chat.members.clone().unwrap_or_default();
    let self_leave = add.is_none()
        && update.is_none()
        && remove.as_deref() == Some(std::slice::from_ref(owner_id));
    if !is_admin && !self_leave {
        return Err(forbidden("only group chat admins may change members"));
    }

    if let Some(add) = &add {
        let now = UTCDate::from(now_utc_string().as_ref());
        for entry in add {
            if members.iter().all(|m| m.id != entry.id) {
                let role = entry.role.clone().unwrap_or(ChatMemberRole::Member);
                let mut m = ChatMember::new(entry.id.clone(), role, now.clone());
                m.invited_by = Some(owner_id.clone());
                members.push(m);
            }
        }
        if members.len() > max_members as usize {
            return Err(Box::new(
                SetError::new(SetErrorType::OverQuota)
                    .with_description("group chat member limit exceeded"),
            ));
        }
    }
    if let Some(remove) = &remove {
        members.retain(|m| !remove.contains(&m.id));
    }
    if let Some(update) = &update {
        for entry in update {
            let member = members
                .iter_mut()
                .find(|m| m.id == entry.id)
                .ok_or_else(|| invalid("updateMemberRoles"))?;
            member.role = entry.role.clone().expect("role checked above");
        }
        if !members.is_empty() && members.iter().all(|m| m.role != ChatMemberRole::Admin) {
            return Err(forbidden("the last admin may not demote themselves"));
        }
    }
    let closes = is_admin
        && members.iter().all(|m| m.id != *owner_id)
        && !members.is_empty()
        && members.iter().all(|m| m.role != ChatMemberRole::Admin);
    Ok((members, closes))
}

/// Ids of `members` other than the owner, who is referenced by the
/// accountId (draft-atwood-jmap-chat-00 §Terminology).
fn other_member_ids<'a>(
    members: impl IntoIterator<Item = &'a ChatMember>,
    owner_id: &Id,
) -> Vec<Id> {
    members
        .into_iter()
        .filter(|m| m.id != *owner_id)
        .map(|m| m.id.clone())
        .collect()
}

/// The `Peer/groupUpdate` events for a successful `Chat/set` update of
/// the group `before`, in [`GroupUpdateAction`] order.
///
/// `after` is the member list the update stored (`None` when it did not
/// touch members) and `closes` whether it closed the group;
/// `metadata_changed` is whether it patched `name`, `description` or
/// `avatarBlobId`. The member actions are the difference between the two
/// lists, so a no-op entry (adding a current member, re-assigning a
/// member's role) reports nothing. The owner's own removal is `"leave"`
/// or `"close"`, not `"removeMembers"`
/// (draft-atwood-jmap-chat-federation-00 §Peer/groupUpdate: `"leave"` "is
/// equivalent to `"removeMembers"` with `removedMemberIds:
/// [senderUserId]`"). Recipients follow draft-atwood-jmap-chat-00
/// §Chat/set "Updating a Chat".
fn group_update_events(
    chat_id: &Id,
    owner_id: &Id,
    before: &[ChatMember],
    after: Option<&[ChatMember]>,
    closes: bool,
    metadata_changed: bool,
) -> Vec<OutboundEvent> {
    let after = after.unwrap_or(before);
    let find = |list: &[ChatMember], id: &Id| list.iter().find(|m| m.id == *id).cloned();
    let everyone = other_member_ids(after, owner_id);
    let mut actions = Vec::new();
    if metadata_changed {
        actions.push((GroupUpdateAction::UpdateMetadata, everyone.clone()));
    }
    let added: Vec<ChatMember> = after
        .iter()
        .filter(|m| find(before, &m.id).is_none())
        .cloned()
        .collect();
    if !added.is_empty() {
        actions.push((GroupUpdateAction::AddMembers { added }, everyone.clone()));
    }
    let removed_ids = other_member_ids(
        before.iter().filter(|m| find(after, &m.id).is_none()),
        owner_id,
    );
    if !removed_ids.is_empty() {
        let mut recipients = everyone.clone();
        recipients.extend(removed_ids.iter().cloned());
        actions.push((GroupUpdateAction::RemoveMembers { removed_ids }, recipients));
    }
    let updated: Vec<ChatMember> = after
        .iter()
        .filter(|m| find(before, &m.id).is_some_and(|old| old.role != m.role))
        .cloned()
        .collect();
    if !updated.is_empty() {
        actions.push((GroupUpdateAction::UpdateRoles { updated }, everyone.clone()));
    }
    if find(before, owner_id).is_some() && find(after, owner_id).is_none() {
        let action = if closes {
            GroupUpdateAction::Close
        } else {
            GroupUpdateAction::Leave
        };
        actions.push((action, everyone));
    }
    actions
        .into_iter()
        .map(|(action, recipients)| OutboundEvent::GroupUpdate {
            chat_id: chat_id.clone(),
            action,
            recipients,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Chat/get
// ---------------------------------------------------------------------------

/// Handle a `Chat/get` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// `unreadCount` is computed on every call (§Chat, jmap-chat-spec 33702a5
/// L629): "The number of Messages in this Chat whose `id` ULID is
/// lexicographically greater than `ReadPosition.lastReadMessageId`,
/// derived server-side from the owner's ReadPosition record. If
/// `lastReadMessageId` is absent, all messages are counted as unread." A
/// Chat with no ReadPosition counts every Message. A backend whose
/// [`ChatBackend::maintains_unread_count`] is `true` gets its stored
/// value back instead.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
// NOTE: properties forwarded via handle_get
pub async fn handle_chat_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let mut out = jmap_server::handle_get::<Chat, B>(backend, caller, args).await?;
    fill_unread_counts(backend, caller, &mut out.primary).await?;
    Ok(out)
}

/// Overwrite `unreadCount` on each Chat in a `Chat/get` response `list`
/// that carries the property (it is absent when `properties` leaves it
/// out), unless the backend maintains the count itself
/// ([`ChatBackend::maintains_unread_count`]).
// ponytail: scans every Message and ReadPosition in the account per call
// (O(messages) per Chat/get); a backend with many messages keeps its own
// count via ChatBackend::maintains_unread_count.
async fn fill_unread_counts<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    response: &mut Value,
) -> Result<(), JmapError> {
    if backend.maintains_unread_count() {
        return Ok(());
    }
    // Id::from: the accountId this handler already validated and echoed.
    let account_id = Id::from(response["accountId"].as_str().unwrap_or_default());
    let Some(list) = response["list"].as_array_mut() else {
        return Ok(());
    };
    if !list.iter().any(|c| c.get("unreadCount").is_some()) {
        return Ok(());
    }
    let (messages, positions) = messages_and_positions(backend, caller, &account_id).await?;
    for chat in list.iter_mut() {
        let Some(chat) = chat.as_object_mut() else {
            continue;
        };
        if !chat.contains_key("unreadCount") {
            continue;
        }
        let Some(chat_id) = chat.get("id").and_then(Value::as_str) else {
            continue;
        };
        let unread = unread_count(&messages, &positions, chat_id);
        chat.insert("unreadCount".to_owned(), Value::from(unread));
    }
    Ok(())
}

async fn messages_and_positions<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
) -> Result<(Vec<Message>, Vec<ReadPosition>), JmapError> {
    let (messages, _) = backend
        .get_objects::<Message>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let (positions, _) = backend
        .get_objects::<ReadPosition>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok((messages, positions))
}

/// §Chat `unreadCount` for `chat_id`: Messages in the Chat whose `id`
/// sorts after the owner's `lastReadMessageId`, or all of them when it
/// is absent.
fn unread_count(messages: &[Message], positions: &[ReadPosition], chat_id: &str) -> u64 {
    let last_read = positions
        .iter()
        .find(|p| p.chat_id.as_ref() == chat_id)
        .and_then(|p| p.last_read_message_id.as_ref());
    messages
        .iter()
        .filter(|m| m.chat_id.as_ref() == chat_id)
        .filter(|m| last_read.is_none_or(|l| m.id.as_ref() > l.as_ref()))
        .count() as u64
}

/// Write the recomputed `unreadCount` to each Chat in `chat_ids` so the
/// `Chat` state advances and `Chat/changes` reports the Chat as updated.
///
/// RFC 8620 §5.1: "If the data changes, this string MUST change", and
/// §ReadPosition/set (jmap-chat-spec 33702a5 L1434) "updates
/// `Chat.unreadCount` for the affected Chat, possibly later". `Chat/get`
/// derives the count on every call, so a ReadPosition update or a Message
/// create/destroy changes Chat data; this records that change through
/// [`ChatBackend::update_object`], whose contract bumps the type state
/// and logs the id as updated. Does nothing when the backend maintains
/// the count itself ([`ChatBackend::maintains_unread_count`]).
///
/// Best-effort, like [`crate::message::ensure_read_positions`]: the
/// caller's own write already succeeded and its response must report
/// it, so a failure here (including a Chat that no longer exists) is
/// not surfaced.
pub(crate) async fn record_unread_count_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    mut chat_ids: Vec<Id>,
) {
    if chat_ids.is_empty() || backend.maintains_unread_count() {
        return;
    }
    let Ok((messages, positions)) = messages_and_positions(backend, caller, account_id).await
    else {
        return;
    };
    chat_ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    chat_ids.dedup();
    for chat_id in chat_ids {
        let unread = unread_count(&messages, &positions, chat_id.as_ref());
        let mut patch = PatchObject::new();
        patch
            .as_map_mut()
            .insert("unreadCount".to_owned(), Value::from(unread));
        let _ = backend
            .update_object::<Chat>(caller, account_id, &chat_id, patch)
            .await;
    }
}

// ---------------------------------------------------------------------------
// Chat/changes
// ---------------------------------------------------------------------------

/// Handle a `Chat/changes` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/query
// ---------------------------------------------------------------------------

/// Handle a `Chat/query` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Unknown filter clauses and sort properties are refused with
/// `unsupportedFilter` / `unsupportedSort` (RFC 8620 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // §Chat/query: "Default sort: `lastMessageAt` descending; chats
    // without messages sort last."
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "lastMessageAt", "isAscending": false }]),
        );
    }
    reject_unsupported_sort::<ChatComparator>(args.get("sort"))?;
    jmap_server::handle_query::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Chat/queryChanges` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // RFC 8620 §5.6: the `filter` and `sort` are those of the original
    // query, so the §Chat/query default sort applies here too.
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "lastMessageAt", "isAscending": false }]),
        );
    }
    reject_unsupported_sort::<ChatComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/set
// ---------------------------------------------------------------------------

/// Handle a `Chat/set` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `kind` is required on create.
/// - `direct` chats require `contactId`.
/// - `channel` chats are `forbidden` on create and destroy (managed via
///   `Space/set` `addChannels` / `removeChannels`).
/// - a group create whose membership exceeds
///   [`ChatLimits::max_group_chat_members`](crate::ChatLimits::max_group_chat_members)
///   is `overQuota`.
/// - an update key outside §Chat/set "Updating a Chat" is
///   `invalidProperties`; `name`, `description` and `avatarBlobId` updates
///   are `forbidden` unless the Chat is a group and the caller is an admin.
/// - `pinnedMessageIds` updates are gated by
///   [`ChatBackend::may_set_pinned_messages`].
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Chat>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        // Only pay the cost of a full get_objects fetch when the batch contains
        // at least one Direct create (JMAP-63k.4).
        let has_direct_create = create_map.values().any(|v| {
            v.get("kind")
                .and_then(|k| k.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("direct"))
        });

        // Fetch all existing chats once before the loop. For a batch of N
        // creates against an account with K existing chats this is O(K + N)
        // reads (one full-account scan + a HashSet lookup per create) rather
        // than the naive O(N * K) of one fetch per create. The per-batch fetch
        // still scales linearly in the account's chat count K, so production
        // backends serving accounts with large K should push the
        // contact-id-uniqueness check into a typed query method on
        // ChatBackend rather than rely on this hoisted scan (tracked by
        // JMAP-63k.9). Skipped entirely for non-Direct batches.
        let (existing_chats, mut known_direct_contact_ids): (Vec<Chat>, HashSet<String>) =
            if has_direct_create {
                let (chats, _) = backend
                    .get_objects::<Chat>(caller, &account_id, None, None)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;
                let known = chats
                    .iter()
                    .filter(|c| c.kind == ChatKind::Direct)
                    .filter_map(|c| c.contact_id.as_ref().map(|id| id.as_ref().to_owned()))
                    .collect();
                (chats, known)
            } else {
                (Vec::new(), HashSet::new())
            };

        // Maps contactId -> assigned new_id for Direct chats successfully
        // created earlier in this batch.  Used to resolve intra-batch duplicates
        // without a re-fetch (JMAP-63k.12).
        let mut batch_direct_ids: HashMap<String, Id> = HashMap::new();

        for (create_id, obj_val) in create_map {
            // Optional properties: a wrongly-typed value is invalidProperties
            // (RFC 8620 §5.3), not a silent absence.
            if let Some(err) = mistyped_create_props(
                obj_val,
                &[
                    ("contactId", PropType::Str),
                    ("name", PropType::Str),
                    ("description", PropType::Str),
                    ("avatarBlobId", PropType::Str),
                    ("messageExpirySeconds", PropType::UnsignedInt),
                    ("spaceId", PropType::Str),
                    ("muted", PropType::Bool),
                    ("receiveTypingIndicators", PropType::Bool),
                ],
            ) {
                not_created.insert(create_id.clone(), err);
                continue;
            }
            // kind is required.
            let Some(kind_str) = obj_val
                .get("kind")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
            else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["kind"] }),
                );
                continue;
            };

            // ChatKind::Other(_) is the deserialize-from-wire forward-compat
            // catch-all: it preserves round-trip fidelity when reading data
            // produced by a future server that knows a new spec variant. It
            // is NOT a legitimate value on /set create — a client supplying
            // an unrecognised kind must be rejected with invalidProperties,
            // otherwise junk Chats end up in storage and break every
            // downstream kind-dispatched invariant (kind-specific required
            // field checks, /get response shape, Message/set chatId
            // resolution, etc.).
            let kind = match serde_json::from_value::<ChatKind>(Value::String(kind_str)) {
                Ok(k) if !matches!(k, ChatKind::Other(_)) => k,
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["kind"] }),
                    );
                    continue;
                }
            };

            // A channel create is `forbidden` below whatever it carries.
            // `null` is read as absent, as in `mistyped_create_props`.
            if kind != ChatKind::Channel {
                let foreign: Vec<&str> = KIND_SCOPED_CREATE_PROPS
                    .iter()
                    .filter(|(prop, scope)| {
                        *scope != kind && obj_val.get(*prop).is_some_and(|v| !v.is_null())
                    })
                    .map(|(prop, _)| *prop)
                    .collect();
                if !foreign.is_empty() {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": foreign }),
                    );
                    continue;
                }
                if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Chat>) {
                    not_created.insert(create_id.clone(), set_error_value(&err));
                    continue;
                }
            }

            // Validate kind-specific required fields and extract per-kind state.
            // `direct_contact_id_str` is Some(id) for Direct chats and None for
            // all other kinds — it simultaneously encodes the "is direct" flag and
            // the contact ID, avoiding a bool+Option pair whose invariant (Some iff
            // direct) would otherwise be implicit.
            let direct_contact_id_str: Option<String> = match &kind {
                ChatKind::Direct => {
                    let Some(contact_id_str) = obj_val
                        .get("contactId")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                    else {
                        not_created.insert(
                            create_id.clone(),
                            json!({ "type": "invalidProperties", "properties": ["contactId"] }),
                        );
                        continue;
                    };

                    // Pre-check: reject if a direct chat with this contactId is
                    // already known from the hoisted fetch.
                    if let Some(dup) = existing_chats.iter().find(|c| {
                        c.kind == ChatKind::Direct
                            && c.contact_id.as_ref().map(|id| id.as_ref())
                                == Some(contact_id_str.as_str())
                    }) {
                        not_created.insert(
                            create_id.clone(),
                            serde_json::to_value(
                                SetError::new(SetErrorType::AlreadyExists)
                                    .with_existing_id(dup.id.clone()),
                            )
                            .expect("derive(Serialize) on plain data is infallible"),
                        );
                        continue;
                    }
                    // Also check contactIds created earlier in this same batch.
                    // Resolve the canonical id from the hoisted pre-fetch data or
                    // from the batch map — no re-fetch required (JMAP-63k.12).
                    if known_direct_contact_ids.contains(&contact_id_str) {
                        // Try the pre-fetch snapshot first (pre-existing chat).
                        let canonical_id = if let Some(c) = existing_chats.iter().find(|c| {
                            c.kind == ChatKind::Direct
                                && c.contact_id.as_ref().map(|id| id.as_ref())
                                    == Some(contact_id_str.as_str())
                        }) {
                            c.id.clone()
                        } else if let Some(id) = batch_direct_ids.get(&contact_id_str) {
                            // Created earlier in this batch.
                            id.clone()
                        } else {
                            // Should not happen: known_direct_contact_ids is only
                            // populated from existing_chats and batch_direct_ids.
                            not_created.insert(
                                create_id.clone(),
                                json!({
                                    "type": "serverFail",
                                    "description": "direct chat for contact not found after concurrent operation; retry"
                                }),
                            );
                            continue;
                        };
                        not_created.insert(
                            create_id.clone(),
                            serde_json::to_value(
                                SetError::new(SetErrorType::AlreadyExists)
                                    .with_existing_id(canonical_id),
                            )
                            .expect("derive(Serialize) on plain data is infallible"),
                        );
                        continue;
                    }
                    Some(contact_id_str)
                }
                // §Chat/set: "Creating a Chat with `kind: "channel"` via
                // `Chat/set` is not supported; the server MUST return a
                // SetError of type `forbidden`."
                ChatKind::Channel => {
                    not_created.insert(create_id.clone(), json!({ "type": "forbidden" }));
                    continue;
                }
                _ => None,
            };

            // Group create: "`memberIds` (String[], required): ChatContact.ids
            // of additional initial members." Absent is tolerated for
            // backward compatibility; a non-String[] value is rejected.
            let member_ids: Option<Vec<Id>> = if kind == ChatKind::Group {
                match obj_val.get("memberIds") {
                    None => Some(Vec::new()),
                    Some(v) => match serde_json::from_value::<Vec<String>>(v.clone()) {
                        Ok(ids) => {
                            match ids
                                .iter()
                                .map(|s| jmap_server::parse_wire_id(s, "memberIds"))
                                .collect::<Result<Vec<Id>, _>>()
                            {
                                Ok(ids) => Some(ids),
                                Err(err) => {
                                    not_created.insert(create_id.clone(), set_error_value(&err));
                                    continue;
                                }
                            }
                        }
                        Err(_) => {
                            not_created.insert(
                                create_id.clone(),
                                json!({ "type": "invalidProperties", "properties": ["memberIds"] }),
                            );
                            continue;
                        }
                    },
                }
            } else {
                None
            };

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_ref());

            // "The server [...] adds the creator to `members` with at least
            // one role granting permissions sufficient to administer the
            // chat": the creator is `"admin"`, each distinct memberIds entry
            // is `"member"` invited by the creator. The creator is the owner,
            // referenced by the accountId (§Terminology).
            let members: Option<Vec<ChatMember>> = member_ids.map(|ids| {
                let creator: Id = account_id.clone();
                let mut members = vec![ChatMember::new(
                    creator.clone(),
                    ChatMemberRole::Admin,
                    now.clone(),
                )];
                for id in ids {
                    if members.iter().all(|m| m.id != id) {
                        let mut m = ChatMember::new(id, ChatMemberRole::Member, now.clone());
                        m.invited_by = Some(creator.clone());
                        members.push(m);
                    }
                }
                members
            });

            // Group create `memberIds`: "If the resulting membership would
            // exceed a server-defined limit on the number of members per
            // group Chat, the server MUST return an `overQuota` SetError".
            let max_members = backend.limits(caller, &account_id).max_group_chat_members;
            if members
                .as_ref()
                .is_some_and(|m| m.len() > max_members as usize)
            {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "overQuota",
                        "description": "group chat member limit exceeded",
                    }),
                );
                continue;
            }

            let contact_id: Option<Id> = match obj_val.get("contactId").and_then(|v| v.as_str()) {
                Some(s) => match jmap_server::parse_wire_id(s, "contactId") {
                    Ok(id) => Some(id),
                    Err(err) => {
                        not_created.insert(create_id.clone(), set_error_value(&err));
                        continue;
                    }
                },
                None => None,
            };
            let name: Option<String> = obj_val
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let description: Option<String> = obj_val
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let muted: bool = obj_val
                .get("muted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let receive_typing_indicators: bool = obj_val
                .get("receiveTypingIndicators")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            // "Optional at creation: `description` (String), `avatarBlobId`
            // (String), `messageExpirySeconds` (UnsignedInt)."
            let avatar_blob_id = match obj_val.get("avatarBlobId").and_then(Value::as_str) {
                Some(s) => match jmap_server::parse_wire_id(s, "avatarBlobId") {
                    Ok(id) => Some(id),
                    Err(err) => {
                        not_created.insert(create_id.clone(), set_error_value(&err));
                        continue;
                    }
                },
                None => None,
            };
            let message_expiry_seconds =
                obj_val.get("messageExpirySeconds").and_then(Value::as_u64);
            let build = |id: &Id| {
                let mut chat = Chat::new(
                    id.clone(),
                    kind,
                    now,
                    0,
                    vec![],
                    muted,
                    receive_typing_indicators,
                );
                chat.contact_id = contact_id;
                chat.name = name;
                chat.description = description;
                chat.avatar_blob_id = avatar_blob_id;
                chat.message_expiry_seconds = message_expiry_seconds;
                chat.members = members;
                chat
            };

            match backend
                .create_object::<Chat, _>(caller, &account_id, create_id, build)
                .await
            {
                Ok((new_id, created_obj)) => {
                    // The create is committed; every branch below (including
                    // rollback and cleanup failure) has moved backend state,
                    // so newState must be re-read (RFC 8620 §5.3).
                    mutated = true;
                    // For Direct chats: re-fetch to detect a concurrent duplicate
                    // (optimistic create-then-validate, required for JMAP-0c9).
                    // We fetch all chats because the backend does not currently
                    // expose a filter-by-kind query; a tighter fetch (Direct only)
                    // would be preferable but requires backend support (JMAP-63k.9).
                    if let Some(contact_id_str) = direct_contact_id_str.as_deref() {
                        let current_chats = match backend
                            .get_objects::<Chat>(caller, &account_id, None, None)
                            .await
                        {
                            Ok((chats, _)) => chats,
                            Err(read_err) => {
                                // The Chat (and earlier creates in this
                                // batch) are already committed, so a
                                // method-level serverFail would break
                                // RFC 8620 §3.6.2 ("The method call made
                                // no changes to the server's state").
                                // Without the re-check the draft's
                                // alreadyExists rule cannot be enforced,
                                // so roll this create back and fail it
                                // per id; the batch continues.
                                let value = match backend
                                    .destroy_object::<Chat>(caller, &account_id, &new_id)
                                    .await
                                {
                                    Ok(()) => server_fail_value_from_backend(&read_err),
                                    Err(e) => server_fail_value_from_backend(&e),
                                };
                                not_created.insert(create_id.clone(), value);
                                continue;
                            }
                        };
                        let duplicates: Vec<&Chat> = current_chats
                            .iter()
                            .filter(|c| {
                                c.kind == ChatKind::Direct
                                    && c.contact_id.as_ref().map(|id| id.as_ref())
                                        == Some(contact_id_str)
                            })
                            .collect();
                        if duplicates.len() > 1 {
                            // Race occurred: pick lexicographically smallest id
                            // as the canonical winner.
                            let canonical_id: Id = duplicates
                                .iter()
                                .map(|c| c.id.as_ref())
                                .min()
                                .map(Id::from)
                                .unwrap_or_else(|| new_id.clone());
                            if new_id != canonical_id {
                                // We lost the race: destroy our copy.
                                if let Err(e) = backend
                                    .destroy_object::<Chat>(caller, &account_id, &new_id)
                                    .await
                                {
                                    // Cleanup failed — the duplicate is still
                                    // live. Return a retryable server error
                                    // rather than alreadyExists with a
                                    // potentially inconsistent state.
                                    //
                                    // Route through server_fail_value_from_backend
                                    // to redact backend Display text from the
                                    // wire description per workspace redaction
                                    // discipline.
                                    not_created.insert(
                                        create_id.clone(),
                                        server_fail_value_from_backend(&e),
                                    );
                                    continue;
                                }
                                // Cleanup succeeded: report alreadyExists
                                // pointing to the canonical winner.
                                not_created.insert(
                                    create_id.clone(),
                                    serde_json::to_value(
                                        SetError::new(SetErrorType::AlreadyExists)
                                            .with_existing_id(canonical_id),
                                    )
                                    .unwrap_or_else(|e| server_fail_value_from_backend(&e)),
                                );
                                continue;
                            }
                            // We won the race (our id is canonical): fall through
                            // to success path below.
                        }
                        // Exactly one (or we won): record contactId as known so
                        // subsequent creates in this batch are pre-checked.
                        known_direct_contact_ids.insert(contact_id_str.to_owned());
                        batch_direct_ids.insert(contact_id_str.to_owned(), new_id.clone());
                    }
                    // §Creating a Chat: the server "MUST send
                    // `Peer/groupUpdate` to each initial member".
                    if created_obj.kind == ChatKind::Group {
                        let event = OutboundEvent::GroupUpdate {
                            chat_id: new_id.clone(),
                            action: GroupUpdateAction::Create,
                            recipients: other_member_ids(
                                created_obj.members.iter().flatten(),
                                &account_id,
                            ),
                        };
                        backend.on_outbound_event(caller, &account_id, &event).await;
                    }
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(e) => {
                    not_created.insert(create_id.clone(), backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

            // Only the keys §Chat/set "Updating a Chat" defines may be
            // patched: the client-settable Chat properties that are not
            // immutable, and [`MEMBER_PATCH_KEYS`]. Any other key names a
            // server-set, immutable or `Space/set`-managed property and is
            // `invalidProperties` (RFC 8620 §5.3). JSON Pointer paths count
            // by their first segment.
            let bad_props: Vec<&str> = patch_val
                .as_object()
                .map(|obj| {
                    obj.keys()
                        .map(String::as_str)
                        .filter(|k| {
                            let top = k.split('/').next().unwrap_or(k);
                            !(is_client_updatable::<Chat>(top) || MEMBER_PATCH_KEYS.contains(&top))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            }

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            // The stored Chat, read once for every check below. A Chat
            // that does not resolve is left for update_object to surface
            // `notFound`.
            let stored = match backend
                .get_objects::<Chat>(caller, &account_id, Some(std::slice::from_ref(&id)), None)
                .await
            {
                Ok((chats, _)) => chats.into_iter().next(),
                Err(e) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
                    continue;
                }
            };
            // Per-Chat restrictions of extension-bound Chats; see
            // `ChatBackend::may_update_chat`.
            if let Some(chat) = &stored {
                let patch_keys: Vec<&str> = patch.as_map().keys().map(String::as_str).collect();
                match backend
                    .may_update_chat(caller, &account_id, chat, &patch_keys)
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(set_err)) => {
                        not_updated.insert(id_str, set_error_value(&set_err));
                        continue;
                    }
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // draft-atwood-jmap-chat-00 §Chat `closedAt`: "After it is
            // set, the server MUST reject with a `forbidden` SetError ...
            // every `Chat/set` update that changes membership ... or
            // shared group settings". Pointer paths beneath them count.
            // The stored group and the member list the update stores, for
            // `group_update_events` once the update succeeds.
            let mut group_before: Option<Vec<ChatMember>> = None;
            let mut member_change: Option<(Vec<ChatMember>, bool)> = None;
            let touches_closed_keys = patch.as_map().keys().any(|k| {
                let top = k.split('/').next().unwrap_or(k);
                MEMBER_PATCH_KEYS.contains(&top) || SHARED_GROUP_KEYS.contains(&top)
            });
            if touches_closed_keys && let Some(chat) = &stored {
                if chat.closed_at.is_some() {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "forbidden", "description": "the group is closed" }),
                    );
                    continue;
                }
                // "For group chats, admins additionally may update:
                // `name`, `description`, `avatarBlobId`."
                let touches_admin_keys = patch
                    .as_map()
                    .keys()
                    .any(|k| GROUP_ADMIN_KEYS.contains(&k.split('/').next().unwrap_or(k)));
                if touches_admin_keys
                    && !(chat.kind == ChatKind::Group && is_chat_admin(chat, &account_id))
                {
                    not_updated.insert(
                            id_str,
                            json!({
                                "type": "forbidden",
                                "description": "only group chat admins may update name, description and avatarBlobId",
                            }),
                        );
                    continue;
                }
                if chat.kind == ChatKind::Group {
                    group_before = Some(chat.members.clone().unwrap_or_default());
                }
            }

            // draft-atwood-jmap-chat-00 §Chat/set "Updating a Chat": the
            // member patch keys are operations on `members`, not
            // properties. They are applied to the stored list here and
            // replaced by the resulting `members`, so the backend never
            // stores them.
            // ponytail: check-then-update is not atomic; two concurrent
            // member patches can lose one. A backend needing strictness
            // re-checks inside its update transaction.
            if MEMBER_PATCH_KEYS
                .iter()
                .any(|k| patch.as_map().contains_key(*k))
                && let Some(stored) = &stored
            {
                let max_members = backend.limits(caller, &account_id).max_group_chat_members;
                match apply_member_patch_keys(stored, &account_id, patch.as_map(), max_members) {
                    Ok((members, closes)) => {
                        member_change = Some((members.clone(), closes));
                        let map = patch.as_map_mut();
                        for key in MEMBER_PATCH_KEYS {
                            map.remove(*key);
                        }
                        map.insert(
                            "members".to_owned(),
                            serde_json::to_value(members)
                                .expect("derive(Serialize) on plain data is infallible"),
                        );
                        // "the server MUST set `closedAt` ({{chat}}) in
                        // the same operation".
                        if closes {
                            map.insert("closedAt".to_owned(), json!(now_utc_string()));
                        }
                    }
                    Err(set_err) => {
                        not_updated.insert(id_str, set_error_value(&set_err));
                        continue;
                    }
                }
            }

            // draft-atwood-jmap-chat-00 §Chat `pinnedMessageIds`: who may
            // modify the list depends on the chat kind; the backend decides.
            // Pointer paths beneath the property are gated too.
            let touches_pins = patch
                .as_map()
                .keys()
                .any(|k| k == "pinnedMessageIds" || k.starts_with("pinnedMessageIds/"));
            if touches_pins {
                match backend
                    .may_set_pinned_messages(caller, &account_id, &id)
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(set_err)) => {
                        not_updated.insert(id_str, set_error_value(&set_err));
                        continue;
                    }
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // draft-atwood-jmap-chat-00 §Chat `pinnedMessageIds`: an update
            // that would add an id not naming a Message in this Chat is
            // `invalidProperties`. RFC 8620 §5.3 forbids pointers into
            // arrays (update_object rejects them as invalidPatch), so only
            // the whole-property key can add ids, and its value is the
            // post-patch list. Only ids absent from the stored list count
            // as added; see ChatBackend::validate_pinned_message_ids.
            // ponytail: check-then-update is not atomic; a Message
            // hard-deleted between the two leaves a stale pin. A backend
            // needing strictness re-checks inside its update transaction.
            if let Some(Value::Array(new_pins)) = patch.as_map().get("pinnedMessageIds")
                && let Some(stored) = &stored
            {
                let pins = match new_pins
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|s| jmap_server::parse_wire_id(s, "pinnedMessageIds"))
                    .collect::<Result<Vec<Id>, _>>()
                {
                    Ok(pins) => pins,
                    Err(err) => {
                        not_updated.insert(id_str, set_error_value(&err));
                        continue;
                    }
                };
                let added: Vec<Id> = pins
                    .into_iter()
                    .filter(|pin| !stored.pinned_message_ids.contains(pin))
                    .collect();
                if !added.is_empty() {
                    match backend
                        .validate_pinned_message_ids(caller, &account_id, &id, &added)
                        .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(set_err)) => {
                            not_updated.insert(id_str, set_error_value(&set_err));
                            continue;
                        }
                        Err(e) => {
                            not_updated.insert(id_str, server_fail_value_from_backend(&e));
                            continue;
                        }
                    }
                }
            }

            let metadata_changed = patch
                .as_map()
                .keys()
                .any(|k| GROUP_ADMIN_KEYS.contains(&k.split('/').next().unwrap_or(k)));
            let outcome = backend
                .update_object::<Chat>(caller, &account_id, &id, patch)
                .await;
            if let (Ok(_), Some(before)) = (&outcome, &group_before) {
                let (after, closes) = match &member_change {
                    Some((members, closes)) => (Some(members.as_slice()), *closes),
                    None => (None, false),
                };
                for event in
                    group_update_events(&id, &account_id, before, after, closes, metadata_changed)
                {
                    backend.on_outbound_event(caller, &account_id, &event).await;
                }
            }
            match outcome {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        // §Destroying a Chat: "For channel Chats, destroy via `Chat/set` is
        // not supported; the server MUST return a SetError of type
        // `forbidden`." An id that does not resolve is left for
        // destroy_object to surface `notFound`.
        let mut ids: Vec<Id> = Vec::new();
        for raw in destroy_arr.iter().filter_map(Value::as_str) {
            match jmap_server::parse_wire_id(raw, "id") {
                Ok(id) => ids.push(id),
                Err(err) => {
                    not_destroyed.insert(raw.to_owned(), set_error_value(&err));
                }
            }
        }
        // A failed read is per-id `serverFail`, not a method error: the
        // creates and updates above are already committed, and a method
        // `serverFail` means "The method call made no changes to the
        // server's state" (RFC 8620 §3.6.2). Without the kinds no id can
        // be checked, so none is destroyed.
        let channel_ids: HashSet<Id> = if ids.is_empty() {
            HashSet::new()
        } else {
            match backend
                .get_objects::<Chat>(caller, &account_id, Some(&ids), None)
                .await
            {
                Ok((chats, _)) => chats
                    .into_iter()
                    .filter(|c| c.kind == ChatKind::Channel)
                    .map(|c| c.id)
                    .collect(),
                Err(e) => {
                    let err = server_fail_value_from_backend(&e);
                    for id in ids.drain(..) {
                        not_destroyed.insert(id.as_ref().to_owned(), err.clone());
                    }
                    HashSet::new()
                }
            }
        };
        for id in ids {
            let id_str = id.as_ref();
            if channel_ids.contains(&id) {
                not_destroyed.insert(
                    id_str.to_owned(),
                    json!({
                        "type": "forbidden",
                        "description": "channel Chats are removed via Space/set removeChannels",
                    }),
                );
                continue;
            }

            match backend
                .destroy_object::<Chat>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                    backend.on_chat_destroyed(caller, &account_id, &id).await;
                }
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }

    finalize_set_response::<B, Chat>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed: destroyed_list,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Chat/typing
// ---------------------------------------------------------------------------

/// Handle a `Chat/typing` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the draft §Chat `Chat/typing` request shape (`accountId`,
/// `chatId`); the returned `Value` is the §Chat response shape
/// (`accountId` echo). No persistent state is changed.
///
/// This method is ephemeral — it signals the user is typing in a chat.
/// No state is persisted. In a full implementation, the server would
/// fan out a typing event to chat participants; this handler validates
/// and returns. An unknown `chatId` and a Chat the account does not
/// participate in both yield the same `invalidArguments` error. The
/// sender identity is always derived server-side from `accountId` —
/// clients MUST NOT supply a `senderId` field.
///
/// # Blocked-sender suppression
///
/// Per draft-atwood-jmap-chat-00 commit `d68b4e3` ("close blocked-sender
/// suppression gaps for typing/presence"): when the requesting account
/// corresponds to a [`ChatContact`] whose `blocked` is `true` on the
/// recipient's contact list, the server MUST silently suppress the
/// typing event for that recipient. The sender is NOT informed.
///
/// [`ChatContact`]: jmap_chat_types::ChatContact
///
/// This handler does not call [`ChatBackend::is_contact_blocked`]: that
/// check reads the recipient's contact list, and the request names only
/// the sender's `accountId`. The transport that fans the event out knows
/// each recipient's account and MUST call [`crate::should_deliver_typing`]
/// per recipient, which applies this check and the
/// `receiveTypingIndicators` and membership checks.
/// The wire response (echo `accountId`) is the same either way.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_typing<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let chat_id: Id = match args.get("chatId").and_then(|v| v.as_str()) {
        Some(s) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("chatId: {e}")))?,
        None => return Err(JmapError::invalid_arguments("chatId is required")),
    };

    let typing: bool = match args.get("typing") {
        Some(Value::Bool(b)) => *b,
        None => return Err(JmapError::invalid_arguments("typing is required")),
        Some(_) => return Err(JmapError::invalid_arguments("typing must be a boolean")),
    };

    // draft-atwood-jmap-chat-00 §Chat/typing `chatId`: "The server MUST
    // return an `invalidArguments` method error if `chatId` does not
    // exist or the account is not a participant in that Chat, and MUST
    // NOT distinguish the two cases." A group chat's participants are its
    // `members`, where the owner is referenced by the accountId
    // (§Terminology).
    let (found, _not_found) = backend
        .get_objects::<Chat>(
            caller,
            &account_id,
            Some(std::slice::from_ref(&chat_id)),
            None,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    found
        .into_iter()
        .next()
        .filter(|c| {
            c.kind != ChatKind::Group || c.members.iter().flatten().any(|m| m.id == account_id)
        })
        .ok_or_else(|| {
            JmapError::invalid_arguments("chatId is not a Chat the account participates in")
        })?;

    // §Chat/typing Server Behavior: deliver a `typing` push event to the
    // other local participants and, "For federated Chats, the server SHOULD
    // invoke `Peer/typing`". The backend does both from this event.
    backend
        .on_outbound_event(
            caller,
            &account_id,
            &OutboundEvent::Typing { chat_id, typing },
        )
        .await;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
    })))
}

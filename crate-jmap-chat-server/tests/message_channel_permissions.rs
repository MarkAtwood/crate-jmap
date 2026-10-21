//! The reference `MemoryBackend` enforces sender membership and the
//! channel `"send"` / `"view"` permissions, resolved per §Space
//! Permission Resolution, on `Message/set` create, `Message/get` and
//! `Message/query` (bd:JMAP-y3f2.1), on `Message/queryChanges`
//! (bd:JMAP-y3f2.14), and the channel `"mention_broadcast"` permission
//! (bd:JMAP-559u).
//!
//! Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec 91672c9) §Message/set
//! L1126-1128, §Message/query L1209 and §Space Permission Resolution
//! L1619-1625, quoted on each test. Fixtures are hand-written JSON.

mod common;

use common::{MemoryBackend, TrackingBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{
    handle_message_get, handle_message_query, handle_message_query_changes, handle_message_set,
    ChatBackend, JmapBackend,
};
use jmap_chat_types::Message;
use jmap_server::HandlerOutput;
use jmap_types::{Id, State};
use serde_json::{json, Value};
use std::collections::HashSet;

const CHANNEL_ID: &str = "ch1";

/// A `MemoryBackend` whose `@everyone` role grants nothing, so a test
/// controls every permission through role `r1` and the overrides.
fn backend_without_everyone_floor() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.set_everyone_permissions(HashSet::new());
    backend
}

/// Seed `SPACE_ID` with role `r1` (`role_perms`), the owner's member
/// entry (id = accountId, draft §Terminology) holding `r1` when
/// `owner_is_member`, and channel `CHANNEL_ID` with `overrides` as its
/// `permissionOverrides`.
fn seed_channel(
    backend: &MemoryBackend,
    role_perms: Value,
    owner_is_member: bool,
    overrides: Value,
) {
    let members = if owner_is_member {
        json!([{ "id": ACCOUNT_ID, "roleIds": ["r1"], "joinedAt": "2026-01-01T00:00:00Z" }])
    } else {
        json!([{ "id": "someone-else", "roleIds": ["r1"], "joinedAt": "2026-01-01T00:00:00Z" }])
    };
    seed_channel_with_members(backend, role_perms, members, overrides);
}

/// Seed as [`seed_channel`], with `members` as the Space's `members`.
fn seed_channel_with_members(
    backend: &MemoryBackend,
    role_perms: Value,
    members: Value,
    overrides: Value,
) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "S",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [CHANNEL_ID],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{ "id": "r1", "name": "Member", "permissions": role_perms, "position": 1 }],
            "members": members
        }),
    );
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHANNEL_ID,
        json!({
            "id": CHANNEL_ID,
            "kind": "channel",
            "name": "general",
            "spaceId": SPACE_ID,
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "slowModeSeconds": 0,
            "permissionOverrides": overrides
        }),
    );
}

fn seed_message(backend: &MemoryBackend, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "senderMsgId": format!("smsg-{id}"),
            "senderId": "someone-else",
            "chatId": chat_id,
            "body": "hi @everyone",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [{ "scope": "everyone", "offset": 3, "length": 9 }],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }),
    );
}

async fn create_message(backend: &MemoryBackend, chat_id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": chat_id,
                "body": "hello",
                "bodyType": "text/plain",
                "sentAt": "2026-01-01T00:00:00Z"
            }}
        }),
    )
    .await
    .expect("Message/set");
    resp
}

fn assert_forbidden(resp: &Value) {
    assert_eq!(resp["created"], Value::Null, "{resp:?}");
    assert_eq!(resp["notCreated"]["m"]["type"], "forbidden", "{resp:?}");
}

fn assert_created(resp: &Value) {
    assert!(resp["created"]["m"].is_object(), "{resp:?}");
}

/// §Message/set L1126: "The server MUST verify the sender is a current
/// member of the target Chat. For group chats, this means a ChatMember
/// record exists; ... Non-members MUST receive a SetError of type
/// `forbidden`."
#[tokio::test]
async fn group_create_by_non_member_is_forbidden() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "g1",
        json!({
            "id": "g1",
            "kind": "group",
            "name": "G",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [{ "id": "c-peer", "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }]
        }),
    );
    assert_forbidden(&create_message(&backend, "g1").await);
}

/// §Message/set L1126: "for channel chats, the sender must be a member of
/// the containing Space. Non-members MUST receive a SetError of type
/// `forbidden`." The role grants `send`, but the owner holds no member
/// entry.
#[tokio::test]
async fn channel_create_by_space_non_member_is_forbidden() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view", "send"]), false, json!([]));
    assert_forbidden(&create_message(&backend, CHANNEL_ID).await);
}

/// §Message/set L1128: "For channel Chats, the server MUST verify the
/// sender holds the `\"send\"` permission (resolved per
/// {{space-permissions}}) before accepting the message; if absent, the
/// server MUST return a SetError of type `forbidden`."
#[tokio::test]
async fn channel_create_without_send_is_forbidden() {
    let backend = backend_without_everyone_floor();
    seed_channel(&backend, json!(["view"]), true, json!([]));
    assert_forbidden(&create_message(&backend, CHANNEL_ID).await);
}

/// §Message/set L1128, with `send` granted by the member's Space role
/// (§Space Permission Resolution step 1: "Compute the union of
/// `permissions` across all SpaceRoles held by the member").
#[tokio::test]
async fn channel_create_with_send_succeeds() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view", "send"]), true, json!([]));
    assert_created(&create_message(&backend, CHANNEL_ID).await);
}

/// §Space Permission Resolution step 2: "Apply `deny` entries from any
/// ChannelPermission records matching the member's roles
/// (`targetType: \"role\"`)". The Space role grants `send`; the channel
/// denies it for that role.
#[tokio::test]
async fn channel_role_deny_removes_send() {
    let backend = MemoryBackend::new();
    seed_channel(
        &backend,
        json!(["view", "send"]),
        true,
        json!([{ "targetId": "r1", "targetType": "role", "allow": [], "deny": ["send"] }]),
    );
    assert_forbidden(&create_message(&backend, CHANNEL_ID).await);
}

/// §Space Permission Resolution steps 2 and 5: a role-targeted deny is
/// applied before "`allow` entries from the same member-targeted
/// record", so a member allow restores `send`.
#[tokio::test]
async fn channel_member_allow_overrides_role_deny() {
    let backend = MemoryBackend::new();
    seed_channel(
        &backend,
        json!(["view", "send"]),
        true,
        json!([
            { "targetId": "r1", "targetType": "role", "allow": [], "deny": ["send"] },
            { "targetId": ACCOUNT_ID, "targetType": "member", "allow": ["send"], "deny": [] }
        ]),
    );
    assert_created(&create_message(&backend, CHANNEL_ID).await);
}

/// §SpaceMember L738: "An empty list means the member holds only the
/// `@everyone` role." Role `r1` would grant `send` but is not held, and
/// this backend's `@everyone` floor is set to nothing, so the owner starts without
/// `send`. §Space Permission Resolution step 5: "Apply `allow` entries
/// from the same member-targeted record" restores it.
#[tokio::test]
async fn channel_member_allow_overrides_everyone_floor() {
    let backend = backend_without_everyone_floor();
    let members = json!([{ "id": ACCOUNT_ID, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }]);
    seed_channel_with_members(
        &backend,
        json!(["view", "send"]),
        members.clone(),
        json!([]),
    );
    assert_forbidden(&create_message(&backend, CHANNEL_ID).await);

    let backend = backend_without_everyone_floor();
    seed_channel_with_members(
        &backend,
        json!(["view", "send"]),
        members,
        json!([{ "targetId": ACCOUNT_ID, "targetType": "member", "allow": ["send"], "deny": [] }]),
    );
    assert_created(&create_message(&backend, CHANNEL_ID).await);
}

/// §SpaceMember L822: "An empty list means the member holds only the
/// `@everyone` role." `MemoryBackend::new` gives that role `view` and
/// `send` (bd:JMAP-3rq7), so a member with no SpaceRole may send in and
/// read a channel with no overrides.
#[tokio::test]
async fn default_everyone_floor_grants_view_and_send() {
    let backend = MemoryBackend::new();
    let members = json!([{ "id": ACCOUNT_ID, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }]);
    seed_channel_with_members(&backend, json!([]), members, json!([]));
    assert_created(&create_message(&backend, CHANNEL_ID).await);
    assert!(backend
        .may_view_chat(&(), &Id::from(ACCOUNT_ID), &Id::from(CHANNEL_ID))
        .await
        .expect("may_view_chat"));
}

/// §ChannelPermission `targetId` (jmap-chat-spec 29e9a41, L853): "A
/// record with `targetType: \"role\"` whose `targetId` is the id of the
/// Space itself addresses the implicit `@everyone` role". Step 2 (L1706)
/// applies its `deny` to every member, here both a member with no
/// SpaceRole and one holding `r1`, which grants `send`.
#[tokio::test]
async fn space_id_override_denies_everyone() {
    let everyone_deny =
        json!([{ "targetId": SPACE_ID, "targetType": "role", "allow": [], "deny": ["send"] }]);
    for role_ids in [json!([]), json!(["r1"])] {
        let backend = MemoryBackend::new();
        let members =
            json!([{ "id": ACCOUNT_ID, "roleIds": role_ids, "joinedAt": "2026-01-01T00:00:00Z" }]);
        seed_channel_with_members(&backend, json!(["send"]), members, everyone_deny.clone());
        assert_forbidden(&create_message(&backend, CHANNEL_ID).await);
    }
}

/// §Message/query L1209: "For channel Chats, the server MUST verify the
/// caller holds the `\"view\"` permission before returning messages in
/// `Message/get` or `Message/query` responses; messages in channels the
/// caller cannot view MUST be omitted." A requested hidden id is
/// reported as `notFound` (RFC 8620 §5.1), and a full fetch drops it.
#[tokio::test]
async fn get_omits_messages_in_channel_without_view() {
    let backend = backend_without_everyone_floor();
    seed_channel(&backend, json!(["send"]), true, json!([]));
    seed_message(&backend, "m1", CHANNEL_ID);

    let HandlerOutput { primary: by_id, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["m1"] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(by_id["list"], json!([]), "{by_id:?}");
    assert_eq!(by_id["notFound"], json!(["m1"]), "{by_id:?}");

    let HandlerOutput { primary: all, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("Message/get");
    assert_eq!(all["list"], json!([]), "{all:?}");
}

/// §Message/query L1209, positive case: with `view` the message is
/// returned.
#[tokio::test]
async fn get_returns_messages_in_channel_with_view() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view"]), true, json!([]));
    seed_message(&backend, "m1", CHANNEL_ID);

    let HandlerOutput { primary: resp, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["m1"] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(resp["list"][0]["id"], "m1", "{resp:?}");
}

/// §Message/query L1209 with step 4: "Apply `deny` entries from any
/// ChannelPermission record matching the member directly" — the Space
/// role grants `view`; a member-targeted deny hides the channel from
/// `Message/query`, both scoped by `chatId` and across Chats via
/// `hasMention: true`.
#[tokio::test]
async fn query_omits_messages_in_channel_without_view() {
    let backend = MemoryBackend::new();
    seed_channel(
        &backend,
        json!(["view"]),
        true,
        json!([{ "targetId": ACCOUNT_ID, "targetType": "member", "allow": [], "deny": ["view"] }]),
    );
    seed_message(&backend, "m1", CHANNEL_ID);

    for filter in [
        json!({ "chatId": CHANNEL_ID }),
        json!({ "hasMention": true }),
    ] {
        let HandlerOutput { primary: resp, .. } = handle_message_query(
            &backend,
            &(),
            json!({ "accountId": ACCOUNT_ID, "filter": filter, "calculateTotal": true }),
        )
        .await
        .expect("Message/query");
        assert_eq!(resp["ids"], json!([]), "{filter}: {resp:?}");
        assert_eq!(resp["total"], 0, "{filter}: {resp:?}");
    }
}

/// §Message/query L1209, positive case for both query shapes.
#[tokio::test]
async fn query_returns_messages_in_channel_with_view() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view"]), true, json!([]));
    seed_message(&backend, "m1", CHANNEL_ID);

    for filter in [
        json!({ "chatId": CHANNEL_ID }),
        json!({ "hasMention": true }),
    ] {
        let HandlerOutput { primary: resp, .. } = handle_message_query(
            &backend,
            &(),
            json!({ "accountId": ACCOUNT_ID, "filter": filter }),
        )
        .await
        .expect("Message/query");
        assert_eq!(resp["ids"], json!(["m1"]), "{filter}: {resp:?}");
    }
}

/// The handlers consult `ChatBackend::may_view_chat` themselves for
/// `Message/get` and for a `chatId`-scoped `Message/query`, so a backend
/// that overrides only the hook still satisfies §Message/query L1209:
/// "messages in channels the caller cannot view MUST be omitted". The
/// wrapper's hook hides every Chat while its stored data stays visible.
#[tokio::test]
async fn handlers_consult_view_hook() {
    let backend = TrackingBackend::with_chat_view_denied();
    seed_channel(backend.inner(), json!(["view"]), true, json!([]));
    seed_message(backend.inner(), "m1", CHANNEL_ID);

    let HandlerOutput { primary: got, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["m1"] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(got["list"], json!([]), "{got:?}");
    assert_eq!(got["notFound"], json!(["m1"]), "{got:?}");

    let HandlerOutput {
        primary: queried, ..
    } = handle_message_query(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": { "chatId": CHANNEL_ID },
            "calculateTotal": true
        }),
    )
    .await
    .expect("Message/query");
    assert_eq!(queried["ids"], json!([]), "{queried:?}");
    assert_eq!(queried["total"], 0, "{queried:?}");
}

/// §Message/query L1209 with RFC 8620 §5.6: `/queryChanges` reports the
/// "id and index in the query results (in the new state) for every Foo
/// that has been added to the results since the old state", and a
/// Message in a channel the caller cannot view is not in the results, so
/// it is not announced. The role grants `send` without `view`.
#[tokio::test]
async fn query_changes_omits_messages_in_channel_without_view() {
    let backend = backend_without_everyone_floor();
    seed_channel(&backend, json!(["send"]), true, json!([]));
    assert_created(&create_message(&backend, CHANNEL_ID).await);

    let result = backend
        .query_changes::<Message>(
            &(),
            &Id::from(ACCOUNT_ID),
            &State::from("0"),
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .expect("query_changes");
    assert!(result.added.is_empty(), "{:?}", result.added);
    assert_eq!(result.total, Some(0));
}

/// §Message/query L1209 for a `chatId`-scoped `Message/queryChanges`
/// against a backend that overrides only `ChatBackend::may_view_chat`:
/// the changes cannot be reported, so the handler answers RFC 8620 §5.6
/// `cannotCalculateChanges` ("The server cannot calculate the changes
/// from the queryState string given by the client") and the client's
/// re-run `Message/query` returns nothing.
#[tokio::test]
async fn query_changes_in_unviewable_chat_cannot_calculate() {
    let backend = TrackingBackend::with_chat_view_denied();
    seed_channel(backend.inner(), json!(["view", "send"]), true, json!([]));

    let err = handle_message_query_changes(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": { "chatId": CHANNEL_ID },
            "sinceQueryState": "0"
        }),
    )
    .await
    .expect_err("Message/queryChanges");
    assert_eq!(err.error_type.to_string(), "cannotCalculateChanges");
}

async fn create_broadcast_message(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": CHANNEL_ID,
                "body": "hello all",
                "bodyType": "text/plain",
                "sentAt": "2026-01-01T00:00:00Z",
                "broadcastMentions": [{ "scope": "everyone" }]
            }}
        }),
    )
    .await
    .expect("Message/set");
    resp
}

/// §BroadcastMention L456: "Servers MUST reject `Message/set` create or
/// update requests that would write a broadcast mention from a sender
/// lacking this permission with `forbidden`", with the permission
/// resolved per §Space Permission Resolution L1703: "When determining
/// whether a member may perform an action in a channel Chat, servers
/// MUST evaluate permissions in this order", step 2 applying
/// role-targeted `deny` entries. The Space role grants
/// `mention_broadcast`; the channel denies it for that role
/// (bd:JMAP-559u).
#[tokio::test]
async fn channel_role_deny_removes_mention_broadcast() {
    let backend = MemoryBackend::new();
    seed_channel(
        &backend,
        json!(["view", "send", "mention_broadcast"]),
        true,
        json!([]),
    );
    assert_created(&create_broadcast_message(&backend).await);

    let backend = MemoryBackend::new();
    seed_channel(
        &backend,
        json!(["view", "send", "mention_broadcast"]),
        true,
        json!([{ "targetId": "r1", "targetType": "role", "allow": [], "deny": ["mention_broadcast"] }]),
    );
    assert_forbidden(&create_broadcast_message(&backend).await);
}

/// §Space Permission Resolution L1703 step 5: "Apply `allow` entries from
/// the same member-targeted record". The Space role lacks
/// `mention_broadcast`; a member-targeted allow grants it in this channel
/// (bd:JMAP-559u).
#[tokio::test]
async fn channel_member_allow_grants_mention_broadcast() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view", "send"]), true, json!([]));
    assert_forbidden(&create_broadcast_message(&backend).await);

    let backend = MemoryBackend::new();
    seed_channel(
        &backend,
        json!(["view", "send"]),
        true,
        json!([{ "targetId": ACCOUNT_ID, "targetType": "member", "allow": ["mention_broadcast"], "deny": [] }]),
    );
    assert_created(&create_broadcast_message(&backend).await);
}

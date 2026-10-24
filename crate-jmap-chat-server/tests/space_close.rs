//! Space close: when the last `manage_space` holder leaves, the server
//! sets `Space.closedAt`, per draft-atwood-jmap-chat-00 §Space
//! `closedAt` and §Space/set `removeMembers`. The owner's member entry is
//! the accountId (§Terminology), and the owner removes themselves "by
//! including only the accountId in `removeMembers`".

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{handle_invite_set, handle_space_get, handle_space_join, handle_space_set};
use jmap_chat_types::Space;
use jmap_server::HandlerOutput;
use jmap_server::JmapBackend;
use jmap_types::Id;
use serde_json::{Value, json};

/// Seed a public, publicly-previewable Space with `members`, where
/// members listed with `r-admin` hold the only `manage_space` role. The
/// role also grants `send`, which §Message/set requires of channel
/// senders: "the server MUST verify the sender holds the `"send"`
/// permission".
fn seed(backend: &IdentityBackend, members: Value, closed_at: Option<&str>) {
    let mut space_val = json!({
        "id": SPACE_ID,
        "name": "Test Space",
        "createdAt": "2026-01-01T00:00:00Z",
        "memberCount": members.as_array().map(Vec::len).unwrap_or(0),
        "categories": [],
        "uncategorizedChannelIds": [],
        "isPublic": true,
        "isPubliclyPreviewable": true,
        "roles": [{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space", "manage_members", "manage_channels", "send"],
            "position": 100
        }],
        "members": members,
    });
    if let Some(at) = closed_at {
        space_val["closedAt"] = json!(at);
    }
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend
        .inner()
        .insert_object_for_test("Space", ACCOUNT_ID, SPACE_ID, space_val);
}

fn admin_and_regular() -> Value {
    json!([
        { "id": ACCOUNT_ID, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
        { "id": "u-regular", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
    ])
}

async fn update_as(backend: &IdentityBackend, caller: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        backend,
        &Id::from(caller),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { SPACE_ID: patch } }),
        &[],
    )
    .await
    .expect("Space/set");
    resp
}

async fn space_as(backend: &IdentityBackend, viewer: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_space_get(
        backend,
        &Id::from(viewer),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    resp["list"][0].clone()
}

/// Close the Space by having its only `manage_space` holder, the owner,
/// leave.
async fn close(backend: &IdentityBackend) {
    let resp = update_as(
        backend,
        ACCOUNT_ID,
        json!({ "removeMembers": [ACCOUNT_ID] }),
    )
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");
}

async fn create_invite_as(backend: &IdentityBackend, caller: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        backend,
        &Id::from(caller),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": { "i": { "spaceId": SPACE_ID } } }),
    )
    .await
    .expect("SpaceInvite/set");
    resp
}

/// Oracle: draft §Space/set `removeMembers` — "If the member removing
/// themselves is the last member holding `"manage_space"` and other
/// members remain, leaving closes the Space: the server MUST set
/// `closedAt` ({{space}}) in the same operation."
#[tokio::test]
async fn last_manage_space_holder_leaving_sets_closed_at() {
    let backend = IdentityBackend::new();
    seed(&backend, admin_and_regular(), None);
    close(&backend).await;
    let space = space_as(&backend, "u-regular").await;
    assert!(space["closedAt"].is_string(), "{space}");
}

/// Oracle: draft §Space `closedAt` — "Absent while the Space is open. A
/// Space closes when its last administrator, the last member holding
/// `"manage_space"`, leaves while other members remain".
#[tokio::test]
async fn leaving_without_closing_conditions_keeps_space_open() {
    // A member without `manage_space` (the owner) leaves.
    let backend = IdentityBackend::new();
    seed(
        &backend,
        json!([
            { "id": "admin-user", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": ACCOUNT_ID, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
        None,
    );
    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        json!({ "removeMembers": [ACCOUNT_ID] }),
    )
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");
    let space = space_as(&backend, "admin-user").await;
    assert!(space.get("closedAt").is_none(), "{space}");

    // The last `manage_space` holder leaves with no other members left.
    let backend = IdentityBackend::new();
    seed(
        &backend,
        json!([{ "id": ACCOUNT_ID, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" }]),
        None,
    );
    close(&backend).await;
    let space = space_as(&backend, "outsider").await;
    assert!(space.get("closedAt").is_none(), "{space}");
}

/// Oracle: draft §Space `closedAt` — "After it is set, the Space's
/// channels are read-only: the server MUST reject every `Message/set`
/// create in them with a `forbidden` SetError."
#[tokio::test]
async fn closed_space_rejects_channel_message_create() {
    let backend = IdentityBackend::new();
    seed(&backend, admin_and_regular(), None);
    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        json!({ "addChannels": [{ "name": "general" }] }),
    )
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");
    let channel_id = space_as(&backend, ACCOUNT_ID).await["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();

    let send = || async {
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &Id::from("u-regular"),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "m": {
                    "chatId": channel_id,
                    "body": "hi",
                    "bodyType": "text/plain",
                    "sentAt": "2024-01-01T00:00:00Z",
                } },
            }),
            &[],
        )
        .await
        .expect("Message/set");
        resp
    };
    let resp = send().await;
    assert!(resp["created"].get("m").is_some(), "{resp}");

    close(&backend).await;
    let resp = send().await;
    assert_eq!(resp["notCreated"]["m"]["type"], "forbidden", "{resp}");
}

/// Oracle: draft §Space/join — "If the target Space has `closedAt` set
/// ({{space}}), the server MUST reject the request with a `forbidden`
/// method error, whether the caller supplied `inviteCode` or `spaceId`."
/// and "The server evaluates these checks in order: the argument check
/// (`invalidArguments`), then resolution of the target Space from
/// `inviteCode` (`invalidArguments`) or `spaceId` (`notPermitted`), then
/// the ban check and the closed-Space check (`forbidden`)."
#[tokio::test]
async fn closed_space_rejects_join_in_draft_check_order() {
    let backend = IdentityBackend::new();
    seed(&backend, admin_and_regular(), None);
    let resp = create_invite_as(&backend, ACCOUNT_ID).await;
    let code = resp["created"]["i"]["code"]
        .as_str()
        .expect("invite code")
        .to_owned();
    close(&backend).await;

    let join = |args: Value| {
        let backend = &backend;
        async move {
            let mut args = args;
            args["accountId"] = json!(ACCOUNT_ID);
            handle_space_join(backend, &Id::from("newcomer"), args)
                .await
                .unwrap_err()
                .error_type
                .to_string()
                .to_owned()
        }
    };
    assert_eq!(join(json!({ "spaceId": SPACE_ID })).await, "forbidden");
    assert_eq!(join(json!({ "inviteCode": code })).await, "forbidden");
    assert_eq!(
        join(json!({ "inviteCode": "nosuchcode" })).await,
        "invalidArguments"
    );
    assert_eq!(
        join(json!({ "spaceId": SPACE_ID, "inviteCode": code })).await,
        "invalidArguments"
    );
}

/// Oracle: draft §Space `closedAt` — "The server MUST also reject ...
/// `SpaceInvite/set` create for the Space with a `forbidden` SetError".
#[tokio::test]
async fn closed_space_rejects_invite_create() {
    let backend = IdentityBackend::new();
    seed(&backend, admin_and_regular(), None);
    let resp = create_invite_as(&backend, "u-regular").await;
    assert!(resp["created"].get("i").is_some(), "{resp}");

    close(&backend).await;
    let resp = create_invite_as(&backend, "u-regular").await;
    assert_eq!(resp["notCreated"]["i"]["type"], "forbidden", "{resp}");
}

/// Oracle: draft §Space `closedAt` — "The server MUST also reject ...
/// every `Space/set` update to the Space with a `forbidden` SetError,
/// except a member removing themselves via `removeMembers`." The seeded
/// closed Space keeps a `manage_space` holder so the refusal cannot come
/// from the permission gate.
#[tokio::test]
async fn closed_space_rejects_updates_except_self_removal() {
    let backend = IdentityBackend::new();
    seed(&backend, admin_and_regular(), Some("2026-02-01T00:00:00Z"));

    for patch in [
        json!({ "name": "Renamed" }),
        json!({ "addChannels": [{ "name": "general" }] }),
        json!({ "removeMembers": ["u-regular"] }),
    ] {
        let resp = update_as(&backend, ACCOUNT_ID, patch.clone()).await;
        assert_eq!(
            resp["notUpdated"][SPACE_ID]["type"], "forbidden",
            "{patch}: {resp}"
        );
    }
    let space = space_as(&backend, ACCOUNT_ID).await;
    assert_eq!(space["name"], "Test Space");
    assert_eq!(space["members"].as_array().map(Vec::len), Some(2));

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        json!({ "removeMembers": [ACCOUNT_ID] }),
    )
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");
    // The owner has left, and draft §Space/get gives a non-member only
    // the restricted view, so read the stored Space from the backend.
    let (stored, _) = backend
        .inner()
        .get_objects::<Space>(
            &(),
            &Id::from(ACCOUNT_ID),
            Some(&[Id::from(SPACE_ID)]),
            None,
        )
        .await
        .expect("get_objects");
    let space = serde_json::to_value(&stored[0]).expect("serialize Space");
    assert_eq!(space["members"].as_array().map(Vec::len), Some(1));
    assert_eq!(space["closedAt"], "2026-02-01T00:00:00Z");
}

/// Oracle: draft §Space/get — "the server MUST return a restricted view
/// of the Space containing only the fields `id`, `name`, `description`,
/// `iconBlobId`, `memberCount`, `createdAt`, `isPublic`,
/// `isPubliclyPreviewable`, and `closedAt` (when set)."
#[tokio::test]
async fn non_member_preview_includes_closed_at() {
    let backend = IdentityBackend::new();
    // The owner (member entry = accountId, §Terminology) is not a member.
    seed(
        &backend,
        json!([{ "id": "u-admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" }]),
        Some("2026-02-01T00:00:00Z"),
    );
    let space = space_as(&backend, "outsider").await;
    assert_eq!(space["closedAt"], "2026-02-01T00:00:00Z", "{space}");
    assert!(space.get("members").is_none(), "{space}");
}

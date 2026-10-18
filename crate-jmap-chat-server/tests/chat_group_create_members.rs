//! `Chat/set` group create honours `memberIds` and adds the creator to
//! `members` (bd:JMAP-yt1w.1).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Chat/set "Creating a
//! Group Chat": "`memberIds` (String[], required): ChatContact.ids of
//! additional initial members." and "The server assigns the chatId (a
//! ULID), adds the creator to `members` with at least one role granting
//! permissions sufficient to administer the chat". §ChatMember `role`:
//! "Either `"admin"` or `"member"`. Admins may add and remove members and
//! update group chat metadata."

mod common;

use common::{IdentityBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_get, handle_chat_set};
use jmap_types::Id;
use serde_json::{json, Value};

/// Create one group Chat as `caller` with `create` and return the
/// `Chat/set` response.
async fn create_as(backend: &IdentityBackend, caller: &str, create: Value) -> Value {
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let (resp, _) = handle_chat_set(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT_ID, "create": { "g": create } }),
    )
    .await
    .expect("Chat/set");
    resp
}

/// `(id, role)` of every entry in the stored Chat's `members`.
async fn stored_members(
    backend: &IdentityBackend,
    caller: &str,
    chat_id: &str,
) -> Vec<(String, String)> {
    let (resp, _) = handle_chat_get(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0]["members"]
        .as_array()
        .expect("group Chat must carry members")
        .iter()
        .map(|m| {
            (
                m["id"].as_str().expect("member id").to_owned(),
                m["role"].as_str().expect("member role").to_owned(),
            )
        })
        .collect()
}

/// "adds the creator to `members` with at least one role granting
/// permissions sufficient to administer the chat" plus "`memberIds`
/// (String[], required): ChatContact.ids of additional initial members."
#[tokio::test]
async fn group_create_adds_creator_as_admin_and_member_ids_as_members() {
    let backend = IdentityBackend::new();
    let resp = create_as(
        &backend,
        "alice",
        json!({ "kind": "group", "name": "Team", "memberIds": ["bob", "carol"] }),
    )
    .await;
    let chat_id = resp["created"]["g"]["id"].as_str().expect("created");

    let mut members = stored_members(&backend, "alice", chat_id).await;
    members.sort();
    assert_eq!(
        members,
        vec![
            ("alice".to_owned(), "admin".to_owned()),
            ("bob".to_owned(), "member".to_owned()),
            ("carol".to_owned(), "member".to_owned()),
        ]
    );
}

/// "`memberIds` (String[], required)": a value that is not a String[]
/// is an invalid property (RFC 8620 §5.3 `invalidProperties`).
#[tokio::test]
async fn group_create_rejects_non_string_array_member_ids() {
    let backend = IdentityBackend::new();
    let resp = create_as(
        &backend,
        "alice",
        json!({ "kind": "group", "name": "Team", "memberIds": [42] }),
    )
    .await;
    assert_eq!(resp["notCreated"]["g"]["type"], "invalidProperties");
    assert_eq!(resp["notCreated"]["g"]["properties"], json!(["memberIds"]));
}

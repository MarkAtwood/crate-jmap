//! Default `/query` order, `Space/query` `isPublic`, and CustomEmoji
//! `name` uniqueness (bd:JMAP-y3f2.7).
//!
//! Oracles: draft-atwood-jmap-chat-00 (jmap-chat-spec 91672c9):
//! - L837 §CustomEmoji `name`: "MUST be unique within its scope (Space or
//!   server-global)."
//! - L1056 §Chat/query: "Default sort: `lastMessageAt` descending; chats
//!   without messages sort last."
//! - L1211 §Message/query: "Default sort: `receivedAt` ascending when
//!   `chatId` is present; `receivedAt` descending when `chatId` is absent."
//! - L1306 §Space/query and L1399 §CustomEmoji/query: "Default sort: `name`
//!   ascending."
//! - RFC 8620 §5.6 `added`: "The id and index in the query results (in the
//!   new state) for every Foo that has been added to the results since the
//!   old state" (bd:JMAP-y3f2.14).
//! - L1310 §Space/query: "When the request includes an `isPublic: true`
//!   filter condition, the server MUST include in the result `ids` array
//!   the identifiers of Spaces for which the caller is not a member but
//!   which have both `isPublic: true` and `isPubliclyPreviewable: true`."
//!
//! Objects are seeded as hand-written JSON, inserted in an order that
//! neither id order nor insertion order would turn into the expected one.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::handle_message_query;
use jmap_chat_server::{
    handle_chat_query, handle_emoji_query, handle_emoji_query_changes, handle_emoji_set,
    handle_space_query,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

fn ids(resp: &Value) -> Vec<String> {
    resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("id").to_owned())
        .collect()
}

fn seeded(type_name: &'static str, objects: &[(&str, Value)]) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    for (id, obj) in objects {
        let mut obj = obj.clone();
        obj["id"] = json!(id);
        backend.insert_object_for_test(type_name, ACCOUNT_ID, id, obj);
    }
    backend
}

/// L1056: "`lastMessageAt` descending; chats without messages sort last."
#[tokio::test]
async fn chat_query_unsorted_is_last_message_at_descending() {
    let backend = seeded(
        "Chat",
        &[
            ("a", json!({ "kind": "group" })),
            (
                "b",
                json!({ "kind": "group", "lastMessageAt": "2026-01-01T00:00:00Z" }),
            ),
            (
                "c",
                json!({ "kind": "group", "lastMessageAt": "2026-03-01T00:00:00Z" }),
            ),
        ],
    );
    let HandlerOutput { primary: resp, .. } =
        handle_chat_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("Chat/query");
    assert_eq!(ids(&resp), ["c", "b", "a"]);
}

fn message(received_at: &str) -> Value {
    json!({
        "chatId": "chat1",
        "receivedAt": received_at,
        "mentions": [{ "id": ACCOUNT_ID }],
    })
}

fn message_backend() -> MemoryBackend {
    let backend = seeded(
        "Message",
        &[
            ("m1", message("2026-03-01T00:00:00Z")),
            ("m2", message("2026-01-01T00:00:00Z")),
            ("m3", message("2026-02-01T00:00:00Z")),
        ],
    );
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "chat1",
        json!({ "id": "chat1", "kind": "group" }),
    );
    backend
}

/// L1211: "`receivedAt` ascending when `chatId` is present".
#[tokio::test]
async fn message_query_unsorted_with_chat_id_is_received_at_ascending() {
    let backend = message_backend();
    let HandlerOutput { primary: resp, .. } = handle_message_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "chatId": "chat1" } }),
    )
    .await
    .expect("Message/query");
    assert_eq!(ids(&resp), ["m2", "m3", "m1"]);
}

/// L1211: "`receivedAt` descending when `chatId` is absent."
#[tokio::test]
async fn message_query_unsorted_without_chat_id_is_received_at_descending() {
    let backend = message_backend();
    let HandlerOutput { primary: resp, .. } = handle_message_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "hasMention": true } }),
    )
    .await
    .expect("Message/query");
    assert_eq!(ids(&resp), ["m1", "m3", "m2"]);
}

fn member_space(name: &str) -> Value {
    json!({
        "name": name,
        "members": [{ "id": ACCOUNT_ID }],
        "isPublic": false,
        "isPubliclyPreviewable": false,
    })
}

/// L1306: "Default sort: `name` ascending."
#[tokio::test]
async fn space_query_unsorted_is_name_ascending() {
    let backend = seeded(
        "Space",
        &[
            ("s1", member_space("Charlie")),
            ("s2", member_space("Alpha")),
            ("s3", member_space("Bravo")),
        ],
    );
    let HandlerOutput { primary: resp, .. } =
        handle_space_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("Space/query");
    assert_eq!(ids(&resp), ["s2", "s3", "s1"]);
}

/// L1399: "Default sort: `name` ascending."
#[tokio::test]
async fn emoji_query_unsorted_is_name_ascending() {
    let backend = seeded(
        "CustomEmoji",
        &[
            ("e1", json!({ "name": "zebra" })),
            ("e2", json!({ "name": "apple" })),
            ("e3", json!({ "name": "mango" })),
        ],
    );
    let HandlerOutput { primary: resp, .. } =
        handle_emoji_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("CustomEmoji/query");
    assert_eq!(ids(&resp), ["e2", "e3", "e1"]);
}

/// L1399 with RFC 8620 §5.6: `/queryChanges` reports "the id and index
/// in the query results (in the new state)", and an unsorted
/// `CustomEmoji/query` is ordered by `name` ascending, so the indices of
/// the created emoji follow their names, not their creation order.
#[tokio::test]
async fn emoji_query_changes_unsorted_indexes_by_name_ascending() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let created = emoji_set(
        &backend,
        json!({ "create": {
            "z": { "name": "zebra", "blobId": "b1" },
            "a": { "name": "apple", "blobId": "b2" },
            "m": { "name": "mango", "blobId": "b3" },
        } }),
    )
    .await;
    let id = |k: &str| created["created"][k]["id"].clone();

    let HandlerOutput { primary: resp, .. } = handle_emoji_query_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceQueryState": "0" }),
    )
    .await
    .expect("CustomEmoji/queryChanges");
    let mut added = resp["added"].as_array().expect("added").clone();
    added.sort_by_key(|a| a["index"].as_u64());
    assert_eq!(
        added,
        [
            json!({ "id": id("a"), "index": 0 }),
            json!({ "id": id("m"), "index": 1 }),
            json!({ "id": id("z"), "index": 2 }),
        ]
    );
}

/// L1310: an `isPublic: true` query includes a public, previewable Space
/// the caller is not in, alongside public Spaces the caller is in. A
/// non-member Space lacking `isPubliclyPreviewable: true` is not one the
/// sentence admits, and a private member Space fails the condition.
#[tokio::test]
async fn space_query_is_public_includes_non_member_previewable_space() {
    let non_member = |previewable: bool| {
        json!({
            "name": "x",
            "members": [{ "id": "someone-else" }],
            "isPublic": true,
            "isPubliclyPreviewable": previewable,
        })
    };
    let mut mine_public = member_space("x");
    mine_public["isPublic"] = json!(true);
    let backend = seeded(
        "Space",
        &[
            ("open", non_member(true)),
            ("closed", non_member(false)),
            ("mine-public", mine_public),
            ("mine-private", member_space("x")),
        ],
    );
    let HandlerOutput { primary: resp, .. } = handle_space_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "isPublic": true } }),
    )
    .await
    .expect("Space/query");
    let mut got = ids(&resp);
    got.sort();
    assert_eq!(got, ["mine-public", "open"]);
}

/// jmap-chat-spec 29e9a41 L1394: "When the request includes an `isPublic:
/// true` filter condition, the server MUST include ... Spaces for which
/// the caller is not a member but which have both `isPublic: true` and
/// `isPubliclyPreviewable: true`." A FilterCondition nested in an RFC 8620
/// §5.5 FilterOperator is still included in the request, and the rest of
/// the filter (here `name`) still applies.
#[tokio::test]
async fn space_query_nested_is_public_includes_non_member_space() {
    let non_member = |name: &str, previewable: bool| {
        json!({
            "name": name,
            "members": [{ "id": "someone-else" }],
            "isPublic": true,
            "isPubliclyPreviewable": previewable,
        })
    };
    let backend = seeded(
        "Space",
        &[
            ("open", non_member("x", true)),
            ("open-other-name", non_member("y", true)),
            ("closed", non_member("x", false)),
            ("mine-private", member_space("x")),
        ],
    );
    let HandlerOutput { primary: resp, .. } = handle_space_query(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "filter": {
                "operator": "AND",
                "conditions": [{ "isPublic": true }, { "name": "x" }],
            },
        }),
    )
    .await
    .expect("Space/query");
    assert_eq!(ids(&resp), ["open"]);
}

/// L815 `isPubliclyPreviewable`: "When the Space also has `isPublic:
/// true`, non-members may discover its existence via `Space/query`
/// requests that include an `isPublic: true` filter condition", and L1310
/// admits non-member Spaces only "When the request includes an `isPublic:
/// true` filter condition". An unfiltered query returns the caller's own
/// Spaces only.
#[tokio::test]
async fn space_query_unfiltered_hides_non_member_public_space() {
    let backend = seeded(
        "Space",
        &[
            (
                "open",
                json!({
                    "name": "x",
                    "members": [{ "id": "someone-else" }],
                    "isPublic": true,
                    "isPubliclyPreviewable": true,
                }),
            ),
            ("mine", member_space("x")),
        ],
    );
    let HandlerOutput { primary: resp, .. } =
        handle_space_query(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("Space/query");
    assert_eq!(ids(&resp), ["mine"]);
}

async fn emoji_set(backend: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_emoji_set(backend, &(), args)
        .await
        .expect("CustomEmoji/set")
        .primary
}

/// L837: a second server-global emoji named `catjam` is rejected; the
/// same name in a Space's scope is a different scope and is accepted.
/// The create error follows the draft's other uniqueness rules (L970,
/// L1006: "SetError of type `alreadyExists`, including the existing ...
/// id in the `existingId` field").
#[tokio::test]
async fn emoji_create_duplicate_name_in_scope_is_already_exists() {
    let backend = MemoryBackend::new();
    let first = emoji_set(
        &backend,
        json!({ "create": { "a": { "name": "catjam", "blobId": "b1" } } }),
    )
    .await;
    let existing = first["created"]["a"]["id"].as_str().expect("created id");

    let resp = emoji_set(
        &backend,
        json!({ "create": {
            "dup": { "name": "catjam", "blobId": "b2" },
            "inSpace": { "name": "catjam", "blobId": "b3", "spaceId": "space1" },
        } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["dup"]["type"], "alreadyExists");
    assert_eq!(resp["notCreated"]["dup"]["existingId"], existing);
    assert!(
        resp["created"]["inSpace"]["id"].is_string(),
        "other scope: {resp}"
    );
}

/// L837: renaming an emoji onto a name its scope already holds is
/// rejected. RFC 8620 §5.3 defines `alreadyExists` for create only, so an
/// update names the offending property: "invalidProperties: The record
/// given is invalid in some way."
#[tokio::test]
async fn emoji_update_onto_held_name_is_invalid_properties() {
    let backend = MemoryBackend::new();
    let resp = emoji_set(
        &backend,
        json!({ "create": {
            "a": { "name": "catjam", "blobId": "b1" },
            "b": { "name": "dogjam", "blobId": "b2" },
        } }),
    )
    .await;
    let b = resp["created"]["b"]["id"].as_str().expect("created id");

    let resp = emoji_set(&backend, json!({ "update": { b: { "name": "catjam" } } })).await;
    assert_eq!(resp["notUpdated"][b]["type"], "invalidProperties");
    assert_eq!(resp["notUpdated"][b]["properties"], json!(["name"]));
}

/// L837: a name is unique "within its scope", so renaming an emoji to a
/// name no emoji in its scope holds is accepted, as is an update that
/// keeps the emoji's own name: no other emoji holds it. RFC 8620 §5.3:
/// `updated` has an entry for each record successfully updated.
#[tokio::test]
async fn emoji_update_onto_free_or_own_name_is_updated() {
    let backend = MemoryBackend::new();
    let resp = emoji_set(
        &backend,
        json!({ "create": { "a": { "name": "catjam", "blobId": "b1" } } }),
    )
    .await;
    let a = resp["created"]["a"]["id"].as_str().expect("created id");

    let resp = emoji_set(&backend, json!({ "update": { a: { "name": "catjam" } } })).await;
    assert!(resp["updated"].get(a).is_some(), "own name: {resp}");

    let resp = emoji_set(&backend, json!({ "update": { a: { "name": "dogjam" } } })).await;
    assert!(resp["updated"].get(a).is_some(), "free name: {resp}");
    let HandlerOutput { primary: got, .. } = jmap_chat_server::handle_emoji_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [a] }),
    )
    .await
    .expect("CustomEmoji/get");
    assert_eq!(got["list"][0]["name"], "dogjam", "{got}");
}

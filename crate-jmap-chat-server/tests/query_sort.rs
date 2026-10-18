//! `/query` honours the request's `sort` (bd:JMAP-yt1w.1).
//!
//! Oracles: draft-atwood-jmap-chat-00 §Space/query "Standard JMAP
//! `/query`." and §ChatContact/query "Sort properties: `lastSeenAt`,
//! `login`, `lastActiveAt`. When sorting by `lastActiveAt`, ChatContacts
//! for which the field is absent sort last."; RFC 8620 §5.5 Comparator
//! `isAscending`: "If true, sort in ascending order. If false, reverse the
//! comparator's results to sort in descending order."

mod common;

use common::{make_space, MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_contact_query, handle_space_get, handle_space_query};
use jmap_types::Id;
use serde_json::{json, Value};

/// `ids` of a `/query` response.
fn ids(resp: &Value) -> Vec<String> {
    resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("id").to_owned())
        .collect()
}

/// Names of the Spaces in `space_ids`, in that order.
async fn space_names(backend: &MemoryBackend, space_ids: &[String]) -> Vec<String> {
    let (resp, _) = handle_space_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": space_ids }),
    )
    .await
    .expect("Space/get");
    space_ids
        .iter()
        .map(|id| {
            resp["list"]
                .as_array()
                .expect("list")
                .iter()
                .find(|s| s["id"] == id.as_str())
                .and_then(|s| s["name"].as_str())
                .expect("space name")
                .to_owned()
        })
        .collect()
}

/// `Space/query` sorted by `name` with the given direction.
async fn query_spaces_by_name(backend: &MemoryBackend, is_ascending: bool) -> Vec<String> {
    let (resp, _) = handle_space_query(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "sort": [{ "property": "name", "isAscending": is_ascending }],
        }),
    )
    .await
    .expect("Space/query");
    space_names(backend, &ids(&resp)).await
}

/// "If false, reverse the comparator's results to sort in descending
/// order."
#[tokio::test]
async fn space_query_sort_descending_reverses_order() {
    let backend = MemoryBackend::new();
    for name in ["Alpha", "Bravo", "Charlie"] {
        make_space(&backend, name).await;
    }
    assert_eq!(
        query_spaces_by_name(&backend, false).await,
        ["Charlie", "Bravo", "Alpha"]
    );
}

/// "If true, sort in ascending order." The Spaces are created out of
/// name order so creation order cannot satisfy the assertion.
#[tokio::test]
async fn space_query_sort_ascending_orders_by_property() {
    let backend = MemoryBackend::new();
    for name in ["Charlie", "Alpha", "Bravo"] {
        make_space(&backend, name).await;
    }
    assert_eq!(
        query_spaces_by_name(&backend, true).await,
        ["Alpha", "Bravo", "Charlie"]
    );
}

/// "When sorting by `lastActiveAt`, ChatContacts for which the field is
/// absent sort last." in both directions.
#[tokio::test]
async fn contact_query_sort_last_active_at_absent_sorts_last() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    for (id, last_active_at) in [
        ("c1", None),
        ("c2", Some("2026-01-01T00:00:00Z")),
        ("c3", Some("2026-03-01T00:00:00Z")),
    ] {
        let mut contact = json!({ "id": id, "login": id });
        if let Some(at) = last_active_at {
            contact["lastActiveAt"] = json!(at);
        }
        backend.insert_object_for_test("ChatContact", ACCOUNT_ID, id, contact);
    }

    for (is_ascending, expected) in [(true, ["c2", "c3", "c1"]), (false, ["c3", "c2", "c1"])] {
        let (resp, _) = handle_contact_query(
            &backend,
            &(),
            json!({
                "accountId": ACCOUNT_ID,
                "sort": [{ "property": "lastActiveAt", "isAscending": is_ascending }],
            }),
        )
        .await
        .expect("ChatContact/query");
        assert_eq!(ids(&resp), expected, "isAscending: {is_ascending}");
    }
}

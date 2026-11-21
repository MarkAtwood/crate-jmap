//! Email/query anchors (RFC 8620 §5.5): "If the anchor is not found, the
//! call is rejected with an "anchorNotFound" error", whatever the limit.

mod common;

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_query, handle_email_set};
use jmap_types::Id;
use serde_json::{Value, json};

async fn backend() -> (MemoryBackend<()>, Vec<String>) {
    let b = MemoryBackend::<()>::default();
    b.register_account(&Id::from("acc"));
    common::seed_mailboxes(&b, &Id::from("acc"), &["inbox"]);
    let create: serde_json::Map<String, Value> = (0..3)
        .map(|i| {
            (
                format!("c{i}"),
                json!({"mailboxIds": {"inbox": true}, "receivedAt": format!("2026-01-0{}T00:00:00Z", i + 1)}),
            )
        })
        .collect();
    let out = handle_email_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": "acc", "create": create}),
        &[],
    )
    .await
    .unwrap();
    let ids = (0..3)
        .map(|i| {
            out.primary["created"][format!("c{i}")]["id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    (b, ids)
}

async fn query(b: &MemoryBackend<()>, args: Value) -> Result<Value, String> {
    let mut args = args;
    args["accountId"] = json!("acc");
    args["sort"] = json!([{"property": "receivedAt", "isAscending": true}]);
    handle_email_query(b, &(), args, &[])
        .await
        .map(|o| o.primary)
        .map_err(|e| e.error_type.to_string())
}

#[tokio::test]
async fn missing_anchor_with_limit_zero_is_anchor_not_found() {
    let (b, _) = backend().await;
    for collapse in [false, true] {
        assert_eq!(
            query(
                &b,
                json!({"anchor": "nonexistent", "limit": 0, "collapseThreads": collapse})
            )
            .await,
            Err("anchorNotFound".to_owned()),
            "collapseThreads {collapse}"
        );
    }
}

#[tokio::test]
async fn anchor_pages_from_the_anchor() {
    let (b, ids) = backend().await;
    let resp = query(&b, json!({"anchor": ids[1], "anchorOffset": 0, "limit": 2}))
        .await
        .unwrap();
    assert_eq!(resp["ids"], json!([ids[1], ids[2]]), "{resp}");
    assert_eq!(resp["position"], 1, "{resp}");
}

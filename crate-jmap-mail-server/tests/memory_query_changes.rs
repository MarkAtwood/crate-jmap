//! Memory backend /changes and /queryChanges edge cases.
//!
//! Oracles: RFC 8620 §5.2 "cannotCalculateChanges: The server cannot
//! calculate the changes from the state string given by the client";
//! RFC 8620 §5.6 queryChanges "total ... only if requested".

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_email_changes, handle_email_query_changes, handle_email_set};
use jmap_types::Id;
use serde_json::json;

async fn backend_with_one_email() -> MemoryBackend {
    let b = MemoryBackend::new();
    b.register_account(&Id::from("acc"));
    handle_email_set(
        &b,
        &(),
        &BTreeMap::new(),
        json!({"accountId": "acc", "create": {"c": {"mailboxIds": {"inbox": true}}}}),
        &[],
    )
    .await
    .unwrap();
    b
}

/// A state ahead of the server's (as after a restart of this in-memory
/// store) is not answered with "no changes".
#[tokio::test]
async fn future_since_state_cannot_be_calculated() {
    let b = backend_with_one_email().await;
    let err = handle_email_changes(
        &b,
        &(),
        json!({"accountId": "acc", "sinceState": "50"}),
        &[],
    )
    .await
    .expect_err("future state");
    assert_eq!(err.error_type.to_string(), "cannotCalculateChanges");
    let err = handle_email_query_changes(
        &b,
        &(),
        json!({"accountId": "acc", "sinceQueryState": "50"}),
        &[],
    )
    .await
    .expect_err("future query state");
    assert_eq!(err.error_type.to_string(), "cannotCalculateChanges");
}

#[tokio::test]
async fn query_changes_returns_total_when_asked() {
    let b = backend_with_one_email().await;
    let resp = handle_email_query_changes(
        &b,
        &(),
        json!({"accountId": "acc", "sinceQueryState": "0", "calculateTotal": true}),
        &[],
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(resp["total"], 1, "{resp}");
}

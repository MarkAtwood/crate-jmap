//! SieveScript/set onSuccessActivateScript (RFC 9661 §2.4).
//!
//! Oracles: RFC 9661 §2 "A user may have at most one active script";
//! §2.4 onSuccessActivateScript: "The currently active SieveScript (if
//! any) will be deactivated before activating the specified SieveScript",
//! and "if the id is either invalid or nonexistent, it MUST be ignored,
//! and the currently active SieveScript (if any) will remain as such."

mod common;

use common::{MemoryBackend, VALID_SIEVE_SCRIPT};
use jmap_mail_server::{handle_sieve_get, handle_sieve_set};
use jmap_types::Id;
use serde_json::{Value, json};

/// A backend with scripts "a" (active) and "b"; returns their ids.
async fn two_scripts() -> (MemoryBackend, String, String) {
    let b = MemoryBackend::new();
    b.register_account(&Id::from("acc"));
    b.store_blob(&Id::from("blob1"), VALID_SIEVE_SCRIPT.to_vec());
    let out = handle_sieve_set(
        &b,
        &(),
        json!({"accountId": "acc",
               "create": {"A": {"name": "a", "blobId": "blob1"}, "B": {"name": "b", "blobId": "blob1"}},
               "onSuccessActivateScript": "#A"}),
    )
    .await
    .unwrap();
    let id = |k: &str| out.primary["created"][k]["id"].as_str().unwrap().to_owned();
    (b, id("A"), id("B"))
}

async fn set(b: &MemoryBackend, args: Value) -> Value {
    handle_sieve_set(b, &(), args).await.unwrap().primary
}

/// Ids of the active scripts.
async fn active(b: &MemoryBackend) -> Vec<String> {
    let got = handle_sieve_get(b, &(), json!({"accountId": "acc"}))
        .await
        .unwrap()
        .primary;
    got["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["isActive"] == true)
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn nonexistent_target_leaves_the_active_script() {
    let (b, a, _) = two_scripts().await;
    let resp = set(
        &b,
        json!({"accountId": "acc", "onSuccessActivateScript": "doesnotexist"}),
    )
    .await;
    assert!(
        resp["updated"].is_null() || resp["updated"] == json!({}),
        "{resp}"
    );
    assert_eq!(active(&b).await, [a]);
}

#[tokio::test]
async fn activating_while_renaming_the_active_script_leaves_one_active() {
    let (b, a, bid) = two_scripts().await;
    let resp = set(
        &b,
        json!({"accountId": "acc", "update": {a.clone(): {"name": "a2"}},
               "onSuccessActivateScript": bid.clone()}),
    )
    .await;
    assert_eq!(active(&b).await, std::slice::from_ref(&bid), "{resp}");
    assert_eq!(resp["updated"][&a]["isActive"], false, "{resp}");
    assert_eq!(resp["updated"][&bid]["isActive"], true, "{resp}");
}

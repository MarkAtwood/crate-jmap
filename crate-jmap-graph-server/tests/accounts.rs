//! The public account setup of the Graph `MemoryBackend`: `with_account`,
//! `register_account`, `register_edit`, `register_search` and `advertise`
//! (bd:JMAP-fhvk).
//!
//! Oracle: RFC 8620 §3.6.2 (an unknown accountId is `accountNotFound`, so
//! `account_exists` is the observable) and the documented contract of each
//! method. Capability values are hand-written JSON
//! (draft-atwood-jmap-graph-00 §Account-Level Capability Object,
//! -graph-edit-00 and -graph-search-00).

use jmap_graph_server::GraphBackend;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_types::{GraphCapability, GraphEditCapability, GraphSearchCapability};
use jmap_server::JmapAccounts;
use jmap_types::Id;
use serde_json::json;

fn capability(max: u64) -> GraphCapability {
    serde_json::from_value(json!({ "maxAnnotationSize": max })).expect("capability fixture")
}

/// `with_account` makes exactly the named account known, with the
/// capability it was given.
#[tokio::test]
async fn with_account_registers_the_account() {
    let alice = Id::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new().with_account(&account, &alice, capability(100));
    assert!(backend.account_exists(&alice, &account).await.unwrap());
    assert!(
        !backend
            .account_exists(&alice, &Id::from("A2"))
            .await
            .unwrap()
    );
    assert_eq!(
        backend.graph_capability(&alice, &account).await.unwrap(),
        capability(100)
    );
}

/// `register_edit` and `register_search` advertise their capabilities on
/// a registered account; registering the account again replaces its
/// capability and drops both.
#[tokio::test]
async fn re_registering_replaces_the_capability_and_drops_edit_and_search() {
    let alice = Id::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new().with_account(&account, &alice, capability(100));
    let edit: GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 4096,
        "maxCursorSize": 256,
        "opRetention": null
    }))
    .expect("edit fixture");
    let search: GraphSearchCapability =
        serde_json::from_value(json!({ "languages": [], "maxDepth": 4, "supportsText": false }))
            .expect("search fixture");
    backend.register_edit(&account, edit.clone());
    backend.register_search(&account, search.clone());
    assert_eq!(
        backend
            .graph_edit_capability(&alice, &account)
            .await
            .unwrap(),
        Some(edit)
    );
    assert_eq!(
        backend
            .graph_search_capability(&alice, &account)
            .await
            .unwrap(),
        Some(search)
    );

    backend.register_account(&account, &alice, capability(200));
    assert_eq!(
        backend.graph_capability(&alice, &account).await.unwrap(),
        capability(200)
    );
    assert_eq!(
        backend
            .graph_edit_capability(&alice, &account)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        backend
            .graph_search_capability(&alice, &account)
            .await
            .unwrap(),
        None
    );
}

/// `register_edit` does nothing for an account that is not registered.
#[tokio::test]
async fn register_edit_ignores_an_unknown_account() {
    let alice = Id::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new();
    let edit: GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 4096,
        "maxCursorSize": 256,
        "opRetention": null
    }))
    .expect("edit fixture");
    backend.register_edit(&account, edit);
    assert!(!backend.account_exists(&alice, &account).await.unwrap());
    assert_eq!(
        backend
            .graph_edit_capability(&alice, &account)
            .await
            .unwrap(),
        None
    );
}

/// `advertise` makes `has_capability` answer `true` for that URI on that
/// account only.
#[test]
fn advertise_answers_has_capability() {
    let alice = Id::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new().with_account(&account, &alice, capability(100));
    backend.advertise(&account, "urn:ietf:params:jmap:chat");
    assert!(backend.has_capability(&alice, &account, "urn:ietf:params:jmap:chat"));
    assert!(!backend.has_capability(&alice, &account, "urn:ietf:params:jmap:vtc"));
    assert!(!backend.has_capability(&alice, &Id::from("A2"), "urn:ietf:params:jmap:chat"));
}

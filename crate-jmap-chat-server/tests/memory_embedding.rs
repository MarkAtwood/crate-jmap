//! `MemoryBackend` for wrapping backends (bd JMAP-it7g.4, JMAP-it7g.5):
//! store an object built ahead of time, under a minted or caller-chosen
//! id, with the change log a create records.
//!
//! Oracles: RFC 8620 §5.1 state "If the data changes, this string MUST
//! change"; §5.2 `/changes` lists the ids "created" since the old state;
//! §5.3 `alreadyExists` "the server forbids a duplicate being created".

use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_types::ChatContact;
use jmap_server::{BackendSetError, ChangesParams, JmapReads, SetErrorType};
use jmap_types::{Id, UTCDate};

fn contact(id: &str, identity: &str) -> ChatContact {
    let t = UTCDate::from("2026-01-01T00:00:00Z");
    ChatContact::new(Id::from(id), identity, "login", t.clone(), t, false)
}

#[tokio::test]
async fn insert_object_under_a_chosen_id_is_a_logged_create() {
    let account = Id::from("a1");
    let backend = MemoryBackend::new().with_account(&account);
    let before = backend
        .get_state::<ChatContact>(&(), &account)
        .await
        .expect("state");

    // A hash-derived id the embedder chose.
    backend
        .insert_object(&account, contact("h-1f2e", "did:example:alice"))
        .await
        .expect("insert");

    let changes = backend
        .get_changes::<ChatContact>(&(), &account, ChangesParams::new(&before))
        .await
        .expect("changes");
    assert_eq!(changes.created, [Id::from("h-1f2e")], "{changes:?}");
    let (got, _) = backend
        .get_objects::<ChatContact>(&(), &account, Some(&[Id::from("h-1f2e")]), None)
        .await
        .expect("get");
    assert_eq!(got.len(), 1);
}

#[tokio::test]
async fn insert_object_rejects_a_taken_id() {
    let account = Id::from("a1");
    let backend = MemoryBackend::new().with_account(&account);
    backend
        .insert_object(&account, contact("h1", "did:example:alice"))
        .await
        .expect("first");
    let err = backend
        .insert_object(&account, contact("h1", "did:example:bob"))
        .await
        .expect_err("duplicate id");
    assert!(
        matches!(&err, BackendSetError::SetError(se) if se.error_type == SetErrorType::AlreadyExists),
        "{err:?}"
    );
    // The per-type uniqueness contract still applies.
    let err = backend
        .insert_object(&account, contact("h2", "did:example:alice"))
        .await
        .expect_err("duplicate identity");
    assert!(
        matches!(&err, BackendSetError::SetError(se) if se.error_type == SetErrorType::AlreadyExists),
        "{err:?}"
    );
}

#[test]
fn minted_ids_are_distinct() {
    let backend = MemoryBackend::new();
    assert_ne!(backend.mint_id(), backend.mint_id());
}

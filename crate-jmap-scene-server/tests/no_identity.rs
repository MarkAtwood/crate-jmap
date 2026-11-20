//! A `MemoryBackend<()>`: a deployment without identity
//! (`jmap_server::MemoryCaller`), whose every caller acts as the user
//! named by the account id; and a `MemoryBackend<Id>`, whose caller's id is
//! its userId.

mod common;

use common::{ACCOUNT, capability};
use jmap_scene_server::SceneBackend;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_types::SceneCapability;
use jmap_server::JmapAccounts;
use jmap_types::Id;

const ALICE: &str = "user:alice@example.com";
const EVE: &str = "user:eve@example.com";

fn cap() -> SceneCapability {
    serde_json::from_value(capability()).expect("capability fixture")
}

#[tokio::test]
async fn caller_without_identity_is_the_account_user() {
    let backend = MemoryBackend::<()>::default();
    backend.register_account(&Id::from(ACCOUNT), cap());
    assert_eq!(MemoryBackend::<()>::principal_id(&()), None);
    let user = backend
        .caller_user_id(&(), &Id::from(ACCOUNT))
        .await
        .expect("caller_user_id");
    assert_eq!(user, ACCOUNT);
    // A caller without identity has blocked no one.
    backend.block(ACCOUNT, EVE);
    let blocked = backend
        .is_user_blocked(&(), EVE)
        .await
        .expect("is_user_blocked");
    assert!(!blocked);
}

#[tokio::test]
async fn principal_caller_is_its_own_user() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT), cap());
    let alice = Id::from(ALICE);
    assert_eq!(MemoryBackend::<Id>::principal_id(&alice), Some(&alice));
    let user = backend
        .caller_user_id(&alice, &Id::from(ACCOUNT))
        .await
        .expect("caller_user_id");
    assert_eq!(user, ALICE);
    backend.block(ALICE, EVE);
    let blocked = backend
        .is_user_blocked(&alice, EVE)
        .await
        .expect("is_user_blocked");
    assert!(blocked);
}

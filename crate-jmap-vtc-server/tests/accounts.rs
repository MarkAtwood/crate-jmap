//! The public account setup of the VTC `MemoryBackend`: `with_account`,
//! `register_account` and `advertise` (bd:JMAP-fhvk).
//!
//! Oracle: RFC 8620 §3.6.2 (an unknown accountId is `accountNotFound`, so
//! `account_exists` is the observable) and the documented contract of each
//! method. Capability values are hand-written JSON from the draft's
//! Account-Level Capability Object.

use jmap_server::JmapAccounts;
use jmap_types::Id;
use jmap_vtc_server::VtcBackend;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_types::VtcCapability;
use serde_json::json;

/// A VTC capability whose `mayCreateCall` is `may_create`.
fn capability(may_create: bool) -> VtcCapability {
    serde_json::from_value(json!({
        "mayCreateCall": may_create,
        "supportsRingCalls": true,
        "supportsRoomCalls": true,
        "supportsScheduledCalls": false,
        "supportsRecording": false,
        "supportsLivestream": false,
        "livestreamModes": [],
        "supportsLobby": false,
        "supportsBreakoutRooms": false,
        "supportedMediaTypes": ["audio"]
    }))
    .expect("capability fixture")
}

/// `with_account` makes exactly the named account known, with the
/// capability it was given.
#[tokio::test]
async fn with_account_registers_the_account() {
    let caller = String::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new().with_account(&account, capability(true));
    assert!(backend.account_exists(&caller, &account).await.unwrap());
    assert!(
        !backend
            .account_exists(&caller, &Id::from("A2"))
            .await
            .unwrap()
    );
    assert_eq!(
        backend.vtc_capability(&caller, &account).await.unwrap(),
        capability(true)
    );
}

/// Registering an account again replaces its capability.
#[tokio::test]
async fn re_registering_replaces_the_capability() {
    let caller = String::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new().with_account(&account, capability(true));
    backend.register_account(&account, capability(false));
    assert_eq!(
        backend.vtc_capability(&caller, &account).await.unwrap(),
        capability(false)
    );
}

/// `advertise` makes `has_capability` answer `true` for that URI on that
/// account only.
#[test]
fn advertise_answers_has_capability() {
    let caller = String::from("alice");
    let account = Id::from("A1");
    let backend = MemoryBackend::new().with_account(&account, capability(true));
    backend.advertise(&account, "urn:ietf:params:jmap:chat");
    assert!(backend.has_capability(&caller, &account, "urn:ietf:params:jmap:chat"));
    assert!(!backend.has_capability(&caller, &account, "urn:ietf:params:jmap:graph"));
    assert!(!backend.has_capability(&caller, &Id::from("A2"), "urn:ietf:params:jmap:chat"));
}

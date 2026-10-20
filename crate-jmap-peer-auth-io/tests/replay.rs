//! `MemoryReplayStore` against draft-atwood-jmap-chat-federation-00
//! §Request Signing check 5: "The receiver has not accepted a request
//! carrying the same peer domain and `nonce` before. The receiver MUST
//! remember each accepted (peer domain, `nonce`) pair at least until 60
//! seconds after that signature's `expires`."

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jmap_peer_auth::PeerDomain;
use jmap_peer_auth_io::{MemoryReplayStore, ReplayStore};

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn domain(name: &str) -> PeerDomain {
    name.parse().unwrap()
}

/// `expires` + 60 for a signature with `expires` = 1300.
const FORGET_AFTER: u64 = 1360;

#[tokio::test]
async fn the_same_pair_is_refused_the_second_time() {
    let store = MemoryReplayStore::new();
    let alice = domain("alice.example");
    assert!(store
        .insert(&alice, "n1", at(FORGET_AFTER), at(1000))
        .await
        .unwrap());
    assert!(!store
        .insert(&alice, "n1", at(FORGET_AFTER), at(1001))
        .await
        .unwrap());
}

/// The pair is (peer domain, nonce): the same nonce from another peer
/// domain, or another nonce from the same domain, is a different pair.
#[tokio::test]
async fn pairs_differ_by_domain_and_by_nonce() {
    let store = MemoryReplayStore::new();
    let alice = domain("alice.example");
    let bob = domain("bob.example");
    assert!(store
        .insert(&alice, "n1", at(FORGET_AFTER), at(1000))
        .await
        .unwrap());
    assert!(store
        .insert(&bob, "n1", at(FORGET_AFTER), at(1000))
        .await
        .unwrap());
    assert!(store
        .insert(&alice, "n2", at(FORGET_AFTER), at(1000))
        .await
        .unwrap());
}

/// "at least until 60 seconds after that signature's `expires`": still
/// refused at exactly `expires` + 60.
#[tokio::test]
async fn the_pair_is_remembered_through_forget_after() {
    let store = MemoryReplayStore::new();
    let alice = domain("alice.example");
    assert!(store
        .insert(&alice, "n1", at(FORGET_AFTER), at(1000))
        .await
        .unwrap());
    assert!(!store
        .insert(&alice, "n1", at(FORGET_AFTER), at(FORGET_AFTER))
        .await
        .unwrap());
}

/// After `expires` + 60, check 4 rejects the signature as expired before
/// check 5 runs, so the entry may go and the slot is reusable.
#[tokio::test]
async fn the_pair_expires_after_forget_after() {
    let store = MemoryReplayStore::new();
    let alice = domain("alice.example");
    assert!(store
        .insert(&alice, "n1", at(FORGET_AFTER), at(1000))
        .await
        .unwrap());
    assert!(store
        .insert(&alice, "n1", at(5000), at(FORGET_AFTER + 1))
        .await
        .unwrap());
}

/// Sweeping expired entries must not drop a pair still inside its
/// window: insert enough expired pairs to trigger sweeps, then replay
/// the live one.
#[tokio::test]
async fn sweeps_keep_live_pairs() {
    let store = MemoryReplayStore::new();
    let alice = domain("alice.example");
    assert!(store
        .insert(&alice, "live", at(10_000), at(1000))
        .await
        .unwrap());
    for i in 0..5000 {
        let nonce = format!("old{i}");
        assert!(store
            .insert(&alice, &nonce, at(1001), at(1000))
            .await
            .unwrap());
    }
    for i in 0..5000 {
        let nonce = format!("new{i}");
        assert!(store
            .insert(&alice, &nonce, at(9000), at(2000))
            .await
            .unwrap());
    }
    assert!(!store
        .insert(&alice, "live", at(10_000), at(2000))
        .await
        .unwrap());
}

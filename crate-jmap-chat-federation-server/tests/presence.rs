//! `Peer/subscribePresence`, `Peer/unsubscribePresence` and `Peer/presence`
//! (draft-atwood-jmap-chat-federation-00 §Peer/subscribePresence,
//! §Peer/unsubscribePresence, §Peer/presence).
//!
//! "fed Lnnn" cites draft-atwood-jmap-chat-federation-00.md and "chat
//! Lnnn" draft-atwood-jmap-chat-00.md, both at jmap-chat-spec 85162c4.
//! Oracle: the quoted draft text and hand-written JSON records.

mod common;

use common::*;
use jmap_chat_federation_server::AuthenticatedPeer;
use jmap_chat_types::ChatContact;
use jmap_peer_auth::PeerDomain;
use serde_json::{json, Value};

const SEEN: &str = "2026-10-20T12:00:00Z";
const DAVE: &str = "dave@peer.example";

fn ack() -> Value {
    json!({"accountId": ACCOUNT})
}

/// The owner's PresenceStatus singleton, hand-written.
fn owner_status(presence: &str) -> Value {
    json!({
        "id": "ps1",
        "presence": presence,
        "receiptSharing": true,
        "updatedAt": PAST,
        "statusText": "In a meeting",
        "statusEmoji": "📅",
    })
}

async fn bob(fed: &Fed) -> ChatContact {
    fed.contacts()
        .await
        .into_iter()
        .find(|c| c.id.as_ref() == BOB_ID)
        .expect("bob")
}

/// Bob's ChatContact with a status already set.
fn bob_with_status() -> Value {
    let mut c = contact(BOB_ID, BOB, false);
    c["presence"] = json!("online");
    c["lastActiveAt"] = json!(PAST);
    c["statusText"] = json!("Lunch");
    c["statusEmoji"] = json!("🥪");
    c
}

// --- Peer/subscribePresence -------------------------------------------

/// fed L678: `subscriberId` "MUST match the peer identity; the remote
/// server MUST reject the request if they differ." Nothing is recorded.
#[tokio::test]
async fn subscribe_rejects_other_subscriber() {
    let fed = Fed::new();
    let r = fed
        .subscribe_presence(json!({"accountId": ACCOUNT, "subscriberId": CAROL}))
        .await;
    assert_error(r, "forbidden");
    assert!(fed.subscriptions().is_empty());
}

/// fed L688-701: the response carries the owner's current `presence`,
/// `statusText`, `statusEmoji` and the granted `ttl`. fed L681: "Servers
/// MAY cap or adjust this value"; the test deployment caps at `MAX_TTL`.
#[tokio::test]
async fn subscribe_returns_presence_and_granted_ttl() {
    let fed = Fed::new();
    fed.seed("PresenceStatus", owner_status("busy"));
    let r = fed
        .subscribe_presence(json!({"accountId": ACCOUNT, "subscriberId": BOB, "ttl": 999_999}))
        .await
        .expect("subscribed");
    assert_eq!(
        r,
        json!({
            "accountId": ACCOUNT,
            "presence": "busy",
            "statusText": "In a meeting",
            "statusEmoji": "📅",
            "ttl": MAX_TTL,
        })
    );
    assert_eq!(fed.subscriptions().get(BOB), Some(&MAX_TTL));
}

/// fed L681: "If absent, the server assigns an implementation-defined
/// default TTL." fed L707: "Calling `Peer/subscribePresence` again before
/// the TTL expires renews the subscription and SHOULD reset the TTL to the
/// newly granted value."
#[tokio::test]
async fn subscribe_default_ttl_and_renewal() {
    let fed = Fed::new();
    fed.seed("PresenceStatus", owner_status("online"));
    let first = fed
        .subscribe_presence(json!({"accountId": ACCOUNT, "subscriberId": BOB}))
        .await
        .expect("subscribed");
    assert_eq!(first["ttl"], DEFAULT_TTL);
    let renewed = fed
        .subscribe_presence(json!({"accountId": ACCOUNT, "subscriberId": BOB, "ttl": 30}))
        .await
        .expect("renewed");
    assert_eq!(renewed["ttl"], 30);
    assert_eq!(fed.subscriptions().get(BOB), Some(&30));
}

/// fed L743: "An owner whose PresenceStatus is `"invisible"` is reported
/// as `"offline"`; `"invisible"` is never sent to a peer."
#[tokio::test]
async fn subscribe_reports_invisible_as_offline() {
    let fed = Fed::new();
    fed.seed("PresenceStatus", owner_status("invisible"));
    let r = fed
        .subscribe_presence(json!({"accountId": ACCOUNT, "subscriberId": BOB}))
        .await
        .expect("subscribed");
    assert_eq!(r["presence"], "offline");
}

// --- Peer/unsubscribePresence -----------------------------------------

/// fed L721: `subscriberId` "MUST match the peer identity."
#[tokio::test]
async fn unsubscribe_rejects_other_subscriber() {
    let fed = Fed::new();
    fed.subscriptions
        .lock()
        .unwrap()
        .insert(CAROL.to_owned(), 60);
    let r = fed
        .unsubscribe_presence(json!({"accountId": ACCOUNT, "subscriberId": CAROL}))
        .await;
    assert_error(r, "forbidden");
    assert_eq!(fed.subscriptions().get(CAROL), Some(&60));
}

/// fed L711: "Cancels a presence subscription before its TTL expires. ...
/// Servers MUST succeed if no active subscription exists for the given
/// `subscriberId`." fed L725-726: the response is the `accountId`.
#[tokio::test]
async fn unsubscribe_cancels_and_succeeds_without_subscription() {
    let fed = Fed::new();
    fed.subscriptions.lock().unwrap().insert(BOB.to_owned(), 60);
    let args = json!({"accountId": ACCOUNT, "subscriberId": BOB});
    assert_eq!(fed.unsubscribe_presence(args.clone()).await, Ok(ack()));
    assert!(fed.subscriptions().is_empty());
    assert_eq!(fed.unsubscribe_presence(args).await, Ok(ack()));
}

// --- Peer/presence ----------------------------------------------------

/// fed L740: `identity` "MUST match the peer identity; the receiver MUST
/// reject the request if they differ." Carol's ChatContact is untouched.
#[tokio::test]
async fn presence_rejects_other_identity() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(CAROL_ID, CAROL, false));
    let r = fed
        .presence(json!({"accountId": ACCOUNT, "identity": CAROL, "presence": "away"}))
        .await;
    assert_error(r, "forbidden");
    let carol = fed
        .contacts()
        .await
        .into_iter()
        .find(|c| c.id.as_ref() == CAROL_ID)
        .expect("carol");
    assert_eq!(carol.presence, None);
    assert!(fed.presences().is_empty());
}

/// fed L743: `presence` is "One of `"online"`, `"away"`, `"busy"`, or
/// `"offline"`. ... `"invisible"` is never sent to a peer." chat L520:
/// ChatContact `presence` holds only those four values.
#[tokio::test]
async fn presence_rejects_values_outside_the_four() {
    let fed = Fed::new();
    for value in ["invisible", "dnd"] {
        let r = fed
            .presence(json!({"accountId": ACCOUNT, "identity": BOB, "presence": value}))
            .await;
        assert_error(r, "invalidArguments");
    }
    assert_eq!(bob(&fed).await.presence, None);
}

/// fed L763: "the receiving server MUST update the `presence`,
/// `lastActiveAt`, `statusText`, and `statusEmoji` fields on the
/// ChatContact record whose `identity` equals the `identity` argument.
/// ... The server MUST then fire a local presence push event". chat L1638
/// (spec 6bcd9d8): the event is `{"contactId","presence","lastActiveAt",
/// "statusText","statusEmoji"}`; wss L266-267: "`accountId` (String): The
/// id of the account that contains the ChatContact."
#[tokio::test]
async fn presence_updates_contact_and_fires_event() {
    let fed = Fed::new();
    let r = fed
        .presence(json!({
            "accountId": ACCOUNT,
            "identity": BOB,
            "presence": "away",
            "lastActiveAt": SEEN,
            "statusText": "Commuting",
            "statusEmoji": "🚆",
        }))
        .await;
    assert_eq!(r, Ok(ack()));
    let bob = serde_json::to_value(bob(&fed).await).unwrap();
    assert_eq!(bob["presence"], "away");
    assert_eq!(bob["lastActiveAt"], SEEN);
    assert_eq!(bob["statusText"], "Commuting");
    assert_eq!(bob["statusEmoji"], "🚆");
    let events: Vec<Value> = fed
        .presences()
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    assert_eq!(
        events,
        vec![json!({
            "accountId": ACCOUNT,
            "contactId": BOB_ID,
            "presence": "away",
            "lastActiveAt": SEEN,
            "statusText": "Commuting",
            "statusEmoji": "🚆",
        })]
    );
}

/// fed L749 and L752: "If absent, the receiver MUST treat the current
/// value as unchanged." chat L1638: the event's status fields "reflect the
/// contact's current PresenceStatus values".
#[tokio::test]
async fn presence_absent_status_fields_unchanged() {
    let fed = Fed::new();
    fed.seed("ChatContact", bob_with_status());
    let r = fed
        .presence(json!({"accountId": ACCOUNT, "identity": BOB, "presence": "busy"}))
        .await;
    assert_eq!(r, Ok(ack()));
    let bob = serde_json::to_value(bob(&fed).await).unwrap();
    assert_eq!(bob["presence"], "busy");
    assert_eq!(bob["lastActiveAt"], PAST);
    assert_eq!(bob["statusText"], "Lunch");
    assert_eq!(bob["statusEmoji"], "🥪");
    let event = serde_json::to_value(&fed.presences()[0]).unwrap();
    assert_eq!(event["statusText"], "Lunch");
    assert_eq!(event["statusEmoji"], "🥪");
}

/// fed L749 and L752: "To explicitly clear the field, the sender MUST send
/// `null`." fed L763: "a `null` value in the request MUST clear the
/// corresponding ChatContact field (setting it to absent)". chat L1638:
/// in the event "they are `null` when the contact has no active custom
/// status".
#[tokio::test]
async fn presence_null_clears_status_fields() {
    let fed = Fed::new();
    fed.seed("ChatContact", bob_with_status());
    let r = fed
        .presence(json!({
            "accountId": ACCOUNT,
            "identity": BOB,
            "presence": "online",
            "statusText": null,
            "statusEmoji": null,
        }))
        .await;
    assert_eq!(r, Ok(ack()));
    let bob = serde_json::to_value(bob(&fed).await).unwrap();
    let fields = bob.as_object().unwrap();
    assert!(!fields.contains_key("statusText"), "{bob}");
    assert!(!fields.contains_key("statusEmoji"), "{bob}");
    let event = serde_json::to_value(&fed.presences()[0]).unwrap();
    assert_eq!(event["statusText"], Value::Null);
    assert_eq!(event["statusEmoji"], Value::Null);
    assert!(event.as_object().unwrap().contains_key("statusText"));
    assert!(event.as_object().unwrap().contains_key("statusEmoji"));
}

/// fed L761: for a ChatContact with `blocked` set to `true` "the server
/// SHOULD silently drop the request: it SHOULD NOT update the ChatContact
/// record and SHOULD NOT fire any local presence push event. The server
/// SHOULD return a success response".
#[tokio::test]
async fn presence_from_blocked_contact_dropped_silently() {
    let fed = Fed::new();
    fed.seed("ChatContact", contact(BOB_ID, BOB, true));
    let r = fed
        .presence(json!({
            "accountId": ACCOUNT,
            "identity": BOB,
            "presence": "online",
            "statusText": "Hi",
        }))
        .await;
    assert_eq!(r, Ok(ack()));
    let bob = bob(&fed).await;
    assert_eq!(bob.presence, None);
    assert_eq!(bob.status_text, None);
    assert!(fed.presences().is_empty());
}

/// fed L763 updates "the ChatContact record whose `identity` equals the
/// `identity` argument"; with no such record there is nothing to update,
/// and the method creates none. Dave signs as `peer.example` but has no
/// ChatContact.
#[tokio::test]
async fn presence_without_contact_creates_nothing() {
    let mut fed = Fed::new();
    fed.peer = Some(AuthenticatedPeer::new(
        PeerDomain::parse(PEER_DOMAIN).unwrap(),
        DAVE,
    ));
    let r = fed
        .presence(json!({"accountId": ACCOUNT, "identity": DAVE, "presence": "online"}))
        .await;
    assert_eq!(r, Ok(ack()));
    let contacts = fed.contacts().await;
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].presence, None);
    assert!(fed.presences().is_empty());
}

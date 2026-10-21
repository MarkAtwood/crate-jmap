//! A ring from a blocked contact is dropped at delivery time.
//!
//! Oracle: vtc L1329: "Before delivering a `VTCCallPush`, the server MUST
//! check whether the initiator corresponds to a contact whose `blocked`
//! field is `true` on the target participant's contact list (when
//! {{JMAP-CHAT}} is present) or an equivalent deployment-defined block
//! list. If the initiator is blocked, the server MUST silently drop the
//! ring notification." vtc L1350: "the server MUST NOT deliver a
//! `VTCRingEvent` if the initiator is blocked."

mod common;

use common::*;
use jmap_types::Id;
use jmap_vtc_server::{should_deliver, VtcEphemeralSubscription};
use jmap_vtc_types::{VtcEvent, VtcStreamEnable};
use serde_json::json;

fn ring_from(initiator: &str) -> VtcEvent {
    serde_json::from_value(json!({
        "@type": "VTCRingEvent",
        "accountId": ACCOUNT,
        "callId": "C1",
        "initiatorId": initiator,
        "mediaTypes": ["audio"],
        "joinUri": "https://meet.example.com/room/xkz"
    }))
    .expect("ring event fixture")
}

async fn delivers(backend: &Bound, event: &VtcEvent) -> bool {
    let enable: VtcStreamEnable =
        serde_json::from_value(json!({ "callIds": null, "eventTypes": null }))
            .expect("subscription fixture");
    let mut sub = VtcEphemeralSubscription::default();
    sub.replace_with(&enable).expect("subscription");
    let account = Id::from(ACCOUNT);
    should_deliver(backend, &OWNER.to_owned(), &account, &account, &sub, event)
        .await
        .expect("should_deliver")
}

#[tokio::test]
async fn ring_from_blocked_contact_is_dropped() {
    let backend = Bound::new();
    backend.seed_chat_object("ChatContact", contact("c-bob", BOB, true));
    backend.seed_chat_object("ChatContact", contact("c-alice", ALICE, false));

    assert!(
        !delivers(&backend, &ring_from(BOB)).await,
        "blocked contact"
    );
    assert!(
        delivers(&backend, &ring_from(ALICE)).await,
        "unblocked contact"
    );
    assert!(
        delivers(&backend, &ring_from("user:stranger@example.com")).await,
        "no contact record"
    );
}

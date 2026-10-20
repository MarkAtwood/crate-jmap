//! Chat-side events applied to VTC records: Space and channel destroy, and
//! the identity a SpaceBan's `userId` names.

mod common;

use common::*;
use jmap_chat_vtc_server::{clear_channel_references, clear_space_references, contact_identity};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use jmap_vtc_server::handle_vtc_call_changes;
use serde_json::{json, Value};

const SPACE: &str = "S1";
const CHANNEL: &str = "CH1";

/// A backend whose owner may start calls in [`SPACE`] and [`CHANNEL`].
fn bound_with_space() -> Bound {
    let mut backend = Bound::new();
    backend.everyone.insert("start_call".to_owned());
    backend.seed_chat_object("Space", space(SPACE, &[], &[(ACCOUNT, &[])]));
    backend.seed_chat_object("Space", space("S2", &[], &[(ACCOUNT, &[])]));
    backend.seed_chat_object("Chat", channel_chat(CHANNEL, SPACE, json!([])));
    backend
}

async fn room_call(backend: &Bound, space_id: &str) -> String {
    let call = create_call(
        backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "spaceId": space_id, "channelId": CHANNEL }),
    )
    .await;
    call["id"].as_str().expect("call id").to_owned()
}

async fn vtc_state(backend: &Bound) -> String {
    let HandlerOutput { primary, .. } = jmap_vtc_server::handle_vtc_call_get(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "ids": [] }),
    )
    .await
    .expect("VTCCall/get");
    primary["state"].as_str().expect("state").to_owned()
}

async fn vtc_updated_since(backend: &Bound, since: &str) -> Value {
    let HandlerOutput { primary, .. } = handle_vtc_call_changes(
        backend,
        &OWNER.to_owned(),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
    )
    .await
    .expect("VTCCall/changes");
    primary["updated"].clone()
}

/// vtc L421: "When a Space or channel Chat referenced by `spaceId` or
/// `channelId` is destroyed, the server MUST clear the corresponding field
/// to `null` on all VTCCalls that reference it. This is a side effect of
/// the Space or Chat destruction and MUST emit a `StateChange` for the
/// VTCCall type."
#[tokio::test]
async fn space_destroy_clears_space_id_with_state_change() {
    let backend = bound_with_space();
    let bound_call = room_call(&backend, SPACE).await;
    let other_call = room_call(&backend, "S2").await;
    let before = vtc_state(&backend).await;

    clear_space_references(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(SPACE),
    )
    .await
    .expect("clear spaceId");

    assert_eq!(
        get_call(&backend, &bound_call).await["spaceId"],
        Value::Null
    );
    assert_eq!(get_call(&backend, &other_call).await["spaceId"], "S2");
    assert_eq!(
        vtc_updated_since(&backend, &before).await,
        json!([bound_call])
    );
}

/// vtc L421, for `channelId`.
#[tokio::test]
async fn channel_destroy_clears_channel_id_with_state_change() {
    let backend = bound_with_space();
    let call = room_call(&backend, SPACE).await;
    let before = vtc_state(&backend).await;

    clear_channel_references(
        &backend.vtc,
        &OWNER.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(CHANNEL),
    )
    .await
    .expect("clear channelId");

    let stored = get_call(&backend, &call).await;
    assert_eq!(stored["channelId"], Value::Null);
    assert_eq!(
        stored["spaceId"], SPACE,
        "only the destroyed binding clears"
    );
    assert_eq!(vtc_updated_since(&backend, &before).await, json!([call]));
}

/// chat L217: a user's identity is "recorded in the `identity` property
/// of the user's ChatContact"; "The owner has no ChatContact: a
/// client-facing reference to the owner carries the accountId of the
/// owner's account"; "Between servers, the owner's identity is the
/// `ownerUserId` Session field". chat L893: SpaceBan `userId` is "The
/// ChatContact.id of the banned user".
#[tokio::test]
async fn contact_identity_maps_contacts_and_the_owner() {
    let backend = Bound::new();
    backend.seed_chat_object("ChatContact", contact("c-bob", BOB, false));
    let account = Id::from(ACCOUNT);

    let bob = contact_identity(&backend.chat, &(), &account, &Id::from("c-bob"))
        .await
        .expect("contact read");
    assert_eq!(bob.as_deref(), Some(BOB));
    let owner = contact_identity(&backend.chat, &(), &account, &account)
        .await
        .expect("owner read");
    assert_eq!(owner.as_deref(), Some(OWNER));
    let missing = contact_identity(&backend.chat, &(), &account, &Id::from("c-nobody"))
        .await
        .expect("contact read");
    assert_eq!(missing, None);
}

//! Scene ephemeral delivery check and interaction relay validation.
//!
//! Oracle: draft-atwood-jmap-scene-wss-00 at jmap-chat-spec 0cd3af0,
//! §Event Summary L626-629, §Visibility Filtering L716-726,
//! §Blocked-Sender Suppression L728-807; §Client-to-Server Submission at
//! jmap-chat-spec 6bcd9d8 L408-419. Events are hand-written JSON, taken from the draft's
//! examples where it has one; each assertion quotes the rule it checks.

mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{SceneEphemeralSubscription, should_deliver, validate_interaction};
use jmap_scene_types::{SceneEvent, SceneInteractionEvent, SceneStreamEnable};
use jmap_types::Id;
use serde_json::{Value, json};

/// The region of every draft example.
const R1: &str = "01J5ABC0000000000000000001";
/// An `"invite"` region owned by [`ALICE`].
const VAULT: &str = "01J5ABC0000000000000000002";
/// The object of the L230-252 examples.
const OBJ42: &str = "01J5OBJ0000000000000000042";
/// The sculpture of the L288-297 example; interactable.
const SCULPTURE: &str = "01J5OBJ0000000000000000001";
/// The puzzle piece of the L301-310 example; not interactable.
const PUZZLE: &str = "01J5OBJ0000000000000000007";
/// An interactable object in [`VAULT`].
const VAULT_OBJ: &str = "01J5OBJ0000000000000000008";
/// The blocked user of the L794-803 example.
const MALLORY: &str = "user:mallory@example.com";
const CAROL: &str = "user:carol@example.com";
const CHAT: &str = "urn:ietf:params:jmap:chat";

fn avatar(id: &str, region: &str, user: &str, left_at: Value) -> Value {
    json!({
        "id": id,
        "regionId": region,
        "userId": user,
        "displayName": user,
        "position": [0, 0, 5],
        "orientation": [0, 0, 0, 1],
        "visualRef": null,
        "visualType": null,
        "joinedAt": "2026-06-06T10:00:00Z",
        "leftAt": left_at,
        "worldState": null
    })
}

fn object(id: &str, region: &str, interactable: bool) -> Value {
    let mut o = stored_object(id, region, json!([0, 0, 0]), "2026-06-02T10:00:00Z");
    o["interactable"] = json!(interactable);
    o
}

/// [`R1`] (public) and [`VAULT`] (invite), the objects above, and active
/// avatars in [`R1`] for [`ALICE`], [`BOB`], [`CAROL`] and [`MALLORY`].
/// [`ADMIN`] has an avatar in [`R1`] that has left. [`BOB`] also has an
/// active avatar in [`VAULT`] but no invitation, as after revoked access.
fn scene() -> MemoryBackend {
    let backend = backend();
    seed_as(
        &backend,
        "SceneRegion",
        stored_region(R1, "Plaza", "2026-06-01T10:00:00Z"),
    );
    let mut vault = stored_region(VAULT, "Vault", "2026-06-01T11:00:00Z");
    vault["accessPolicy"] = json!("invite");
    seed_as(&backend, "SceneRegion", vault);
    for (id, region, interactable) in [
        (OBJ42, R1, false),
        (SCULPTURE, R1, true),
        (PUZZLE, R1, false),
        (VAULT_OBJ, VAULT, true),
    ] {
        seed_as(&backend, "SceneObject", object(id, region, interactable));
    }
    for (id, region, user, left_at) in [
        ("01J5AVT0000000000000000001", R1, ALICE, json!(null)),
        ("01J5AVT0000000000000000002", R1, BOB, json!(null)),
        ("01J5AVT0000000000000000003", R1, CAROL, json!(null)),
        ("01J5AVT0000000000000000004", R1, MALLORY, json!(null)),
        (
            "01J5AVT0000000000000000005",
            R1,
            ADMIN,
            json!("2026-06-06T11:00:00Z"),
        ),
        ("01J5AVT0000000000000000006", VAULT, BOB, json!(null)),
    ] {
        seed_as(&backend, "SceneAvatar", avatar(id, region, user, left_at));
    }
    backend
}

fn subscription(raw: Value) -> SceneEphemeralSubscription {
    let enable: SceneStreamEnable = serde_json::from_value(raw).unwrap();
    let mut sub = SceneEphemeralSubscription::default();
    sub.replace_with(&enable).unwrap();
    sub
}

/// `SceneStreamEnable` with `regionIds` and `eventTypes` both `null`.
fn all() -> SceneEphemeralSubscription {
    subscription(json!({ "@type": "SceneStreamEnable" }))
}

fn event(raw: Value) -> SceneEvent {
    serde_json::from_value(raw).unwrap()
}

/// The jmap-chat-spec 6bcd9d8 L214-222 example, for `user` and `kind`,
/// with [`ACCOUNT`] as its `accountId`.
fn avatar_event(user: &str, kind: &str) -> SceneEvent {
    event(json!({
        "@type": "SceneAvatarEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "avatarId": "01J5AVT0000000000000000001",
        "event": kind,
        "userId": user,
        "displayName": "Alice Chen"
    }))
}

/// The jmap-chat-spec 6bcd9d8 L256-264 example with `accountId`,
/// `objectId` and `updatedBy` replaced.
fn object_event(object_id: &str, kind: &str, updated_by: Value) -> SceneEvent {
    event(json!({
        "@type": "SceneObjectEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "objectId": object_id,
        "event": kind,
        "updatedBy": updated_by,
        "position": [15.0, 0, -5.0]
    }))
}

/// The jmap-chat-spec 6bcd9d8 L316-324 example with `accountId`,
/// `regionId`, `objectId` and `userId` replaced.
fn interaction(region: &str, object_id: &str, user: &str) -> SceneInteractionEvent {
    serde_json::from_value(json!({
        "@type": "SceneInteractionEvent",
        "accountId": ACCOUNT,
        "regionId": region,
        "objectId": object_id,
        "userId": user,
        "action": "click",
        "data": null
    }))
    .unwrap()
}

async fn delivers(
    backend: &MemoryBackend,
    recipient: &str,
    sub: &SceneEphemeralSubscription,
    e: &SceneEvent,
) -> bool {
    let account = Id::from(ACCOUNT);
    should_deliver(backend, &recipient.to_owned(), &account, &account, sub, e)
        .await
        .unwrap()
}

async fn relayed(
    backend: &MemoryBackend,
    sender: &str,
    e: &SceneInteractionEvent,
) -> Option<SceneInteractionEvent> {
    validate_interaction(backend, &sender.to_owned(), &Id::from(ACCOUNT), e)
        .await
        .unwrap()
}

async fn relayable(backend: &MemoryBackend, sender: &str, e: &SceneInteractionEvent) -> bool {
    relayed(backend, sender, e).await.is_some()
}

/// L604-607: events "are delivered as server-to-client text frames over
/// the WebSocket connection when an ephemeral Scene subscription is
/// active."
#[tokio::test]
async fn nothing_without_an_active_subscription() {
    let backend = scene();
    let e = avatar_event(ALICE, "entered");
    assert!(!delivers(&backend, BOB, &SceneEphemeralSubscription::default(), &e).await);
    assert!(delivers(&backend, BOB, &all(), &e).await);
}

/// L398-399: `eventTypes` is "An explicit list of event type names the
/// client wishes to receive".
#[tokio::test]
async fn only_requested_event_types() {
    let backend = scene();
    let sub = subscription(json!({
        "@type": "SceneStreamEnable",
        "eventTypes": ["SceneObjectEvent"]
    }));
    assert!(!delivers(&backend, BOB, &sub, &avatar_event(ALICE, "entered")).await);
    assert!(
        delivers(
            &backend,
            BOB,
            &sub,
            &object_event(OBJ42, "updated", json!(ALICE))
        )
        .await
    );
}

/// L156: "This specification defines three ephemeral event types"; an
/// event of any other `@type` is none of them.
#[tokio::test]
async fn unrecognized_event_type_is_not_delivered() {
    let backend = scene();
    let e = event(json!({ "@type": "SceneWeatherEvent", "regionId": R1 }));
    assert!(!delivers(&backend, BOB, &all(), &e).await);
}

/// L626-629: "All Scene ephemeral events are per-region: they are
/// delivered only for regions that match the active subscription's
/// `regionIds` filter (or all regions if `regionIds` is `null`) and for
/// which the user has an active avatar."
#[tokio::test]
async fn per_region_scope_and_active_avatar() {
    let backend = scene();
    let e = avatar_event(ALICE, "entered");
    let other = subscription(json!({
        "@type": "SceneStreamEnable",
        "regionIds": ["01J5ABC0000000000000000099"]
    }));
    let listed = subscription(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1]
    }));
    assert!(!delivers(&backend, BOB, &other, &e).await);
    assert!(delivers(&backend, BOB, &listed, &e).await);
    // ADMIN's avatar in R1 has `leftAt` set, so it is not active.
    assert!(!delivers(&backend, ADMIN, &listed, &e).await);
    assert!(!delivers(&backend, ADMIN, &all(), &e).await);
}

/// L718-719: "The server MUST apply the same visibility contract defined
/// in {{JMAP-SCENE}}, Section 7.3 to ephemeral events", which starts
/// (draft-atwood-jmap-scene-00 L2340): "The server MUST NOT include a
/// SceneObject in a `SceneObject/get` response ... if the authenticated
/// user does not have access to the object's containing SceneRegion."
/// CAROL has neither an invitation to nor an avatar in the invite-only
/// VAULT. BOB's active avatar there keeps his access: "A user has access
/// to a region when ... the user has an active SceneAvatar in it", and "A
/// change to `accessPolicy` or `invitedUserIds` does not set `leftAt` on
/// avatars already present" (draft-atwood-jmap-scene-00 at 6bcd9d8,
/// L2595, L2608).
#[tokio::test]
async fn no_events_for_a_region_without_access() {
    let backend = scene();
    let e = event(json!({
        "@type": "SceneObjectEvent",
        "accountId": ACCOUNT,
        "regionId": VAULT,
        "objectId": VAULT_OBJ,
        "event": "updated",
        "updatedBy": ALICE,
        "position": [1, 0, 0]
    }));
    assert!(!delivers(&backend, CAROL, &all(), &e).await);
    assert!(delivers(&backend, BOB, &all(), &e).await);
}

/// L720-721: "The server MUST NOT deliver a `SceneObjectEvent` for an
/// object the client would not receive in a `SceneObject/get` response."
/// L725-726: "a `SceneObjectEvent` for an object outside the client's
/// visibility scope SHOULD be suppressed."
#[tokio::test]
async fn object_event_needs_a_visible_object() {
    let backend = scene();
    let e = object_event(OBJ42, "updated", json!(ALICE));
    backend.hide_object(BOB, OBJ42);
    assert!(!delivers(&backend, BOB, &all(), &e).await);
    assert!(delivers(&backend, CAROL, &all(), &e).await);
    // A created or updated object that no longer exists is not in a
    // `SceneObject/get` response either.
    let gone = object_event("01J5OBJ0000000000000000099", "created", json!(ALICE));
    assert!(!delivers(&backend, CAROL, &all(), &gone).await);
}

/// L202: a SceneObjectEvent is "Delivered when an object is created,
/// updated, or destroyed within a subscribed region"; the L241-252
/// example is a destroyed event, sent after the object is gone.
#[tokio::test]
async fn destroyed_event_for_a_gone_object_is_delivered() {
    let backend = scene();
    let e = event(json!({
        "@type": "SceneObjectEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "objectId": "01J5OBJ0000000000000000099",
        "event": "destroyed",
        "updatedBy": "user:bob@example.com",
        "position": null
    }));
    assert!(delivers(&backend, CAROL, &all(), &e).await);
}

/// L745-750: "The server MUST NOT deliver a `SceneAvatarEvent` to a
/// recipient who has blocked the user identified by the event's `userId`
/// field. This applies to all `event` values: `\"entered\"`, `\"left\"`,
/// and `\"ejected\"`." L736-737: "The event is delivered normally to all
/// other recipients".
#[tokio::test]
async fn avatar_event_from_a_blocked_user_is_dropped() {
    let backend = scene();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    backend.block(BOB, MALLORY);
    for kind in ["entered", "left", "ejected"] {
        let e = avatar_event(MALLORY, kind);
        assert!(!delivers(&backend, BOB, &all(), &e).await, "{kind}");
        assert!(delivers(&backend, CAROL, &all(), &e).await, "{kind}");
    }
}

/// draft-atwood-jmap-scene-00 at jmap-chat-spec 6bcd9d8, L2654: for a
/// region bound to an E2EE VTCCall the server SHOULD "suppress
/// `SceneAvatarEvent` ... delivery (`entered`, `left`, and `ejected`)" to
/// users who are not participants in the call. Only `SceneAvatarEvent` is
/// named, so a `SceneObjectEvent` in the region still goes out.
/// `hide_region_avatars` stands in for the binding's "not a participant".
#[tokio::test]
async fn avatar_event_in_a_region_hiding_its_avatars_is_dropped() {
    let backend = scene();
    backend.hide_region_avatars(BOB, R1);
    for kind in ["entered", "left", "ejected"] {
        let e = avatar_event(ALICE, kind);
        assert!(!delivers(&backend, BOB, &all(), &e).await, "{kind}");
        assert!(delivers(&backend, CAROL, &all(), &e).await, "{kind}");
    }
    let moved = object_event(OBJ42, "updated", json!(null));
    assert!(delivers(&backend, BOB, &all(), &moved).await);
}

/// L752-756: "The server MUST NOT deliver a `SceneObjectEvent` to a
/// recipient who has blocked the user identified by the event's
/// `updatedBy` field. Events where `updatedBy` is `null`
/// (server-initiated changes) are not subject to blocked-sender
/// suppression and are delivered normally."
#[tokio::test]
async fn object_event_checks_updated_by() {
    let backend = scene();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    backend.block(BOB, MALLORY);
    let by_mallory = object_event(OBJ42, "updated", json!(MALLORY));
    let by_server = object_event(OBJ42, "updated", json!(null));
    assert!(!delivers(&backend, BOB, &all(), &by_mallory).await);
    assert!(delivers(&backend, CAROL, &all(), &by_mallory).await);
    assert!(delivers(&backend, BOB, &all(), &by_server).await);
}

/// L760-762: "The server MUST NOT deliver a `SceneInteractionEvent` to a
/// recipient who has blocked the user identified by the event's `userId`
/// field."
#[tokio::test]
async fn interaction_from_a_blocked_user_is_dropped() {
    let backend = scene();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    backend.block(BOB, MALLORY);
    let e = SceneEvent::Interaction(interaction(R1, SCULPTURE, MALLORY));
    assert!(!delivers(&backend, BOB, &all(), &e).await);
    assert!(delivers(&backend, CAROL, &all(), &e).await);
}

/// L766-770: "If the JMAP Chat capability (`urn:ietf:params:jmap:chat`)
/// is not present in the account's capabilities, no blocked-sender
/// filtering is applied to Scene ephemeral events."
#[tokio::test]
async fn no_blocked_sender_filtering_without_chat() {
    let backend = scene();
    backend.block(BOB, MALLORY);
    for e in [
        avatar_event(MALLORY, "entered"),
        object_event(OBJ42, "updated", json!(MALLORY)),
        SceneEvent::Interaction(interaction(R1, SCULPTURE, MALLORY)),
    ] {
        assert!(delivers(&backend, BOB, &all(), &e).await, "{e:?}");
    }
}

/// jmap-chat-spec 33702a5, wss L178-181: an event's `accountId` is "the
/// id of the region's hosting account ... Other users reach that account
/// as a shared account." L791-795: "When the JMAP Chat capability
/// (`urn:ietf:params:jmap:chat`) is available for the receiving account,
/// the server SHOULD consult the recipient's ChatContact records". So
/// the Chat capability that gates suppression is the recipient's own
/// account's, not the hosting account's.
#[tokio::test]
async fn blocked_sender_check_uses_the_receiving_account() {
    let hosting = Id::from(ACCOUNT);
    let bob_personal = Id::from("acct-bob");
    let e = avatar_event(MALLORY, "entered");
    for (chat_account, expect_delivered) in [(&hosting, true), (&bob_personal, false)] {
        let backend = scene();
        backend.advertise_capability(chat_account, CHAT);
        backend.block(BOB, MALLORY);
        let delivered = should_deliver(
            &backend,
            &BOB.to_owned(),
            &hosting,
            &bob_personal,
            &all(),
            &e,
        )
        .await
        .unwrap();
        assert_eq!(delivered, expect_delivered, "Chat on {chat_account:?}");
    }
}

/// L774-778: "Blocked-sender suppression is applied independently of and
/// in addition to the visibility filtering ... An event may be
/// suppressed by either mechanism." The interaction names an object, so
/// the same visibility contract (L718-719) applies to it.
#[tokio::test]
async fn interaction_on_a_hidden_object_is_dropped() {
    let backend = scene();
    backend.hide_object(BOB, SCULPTURE);
    let e = SceneEvent::Interaction(interaction(R1, SCULPTURE, ALICE));
    assert!(!delivers(&backend, BOB, &all(), &e).await);
    assert!(delivers(&backend, CAROL, &all(), &e).await);
}

/// jmap-chat-spec 6bcd9d8 wss L413-416: the server verifies that "1. The
/// user has an active avatar ... in the region identified by `accountId`
/// and `regionId`. 2. The `objectId` references a SceneObject in that
/// region with `interactable: true`."
#[tokio::test]
async fn interaction_passing_both_checks_is_relayed() {
    let backend = scene();
    assert!(relayable(&backend, ALICE, &interaction(R1, SCULPTURE, ALICE)).await);
}

/// jmap-chat-spec 6bcd9d8 wss L408-410: "The server sets `userId` on the
/// relayed event to the authenticated user's userId; a `userId` supplied
/// in the frame is ignored." BOB, who has an active avatar in R1, sends a
/// frame naming ALICE; the relayed event names BOB.
#[tokio::test]
async fn interaction_user_id_is_set_to_the_sender() {
    let backend = scene();
    let e = relayed(&backend, BOB, &interaction(R1, SCULPTURE, ALICE))
        .await
        .expect("relayed");
    assert_eq!(e.user_id, BOB);
}

/// jmap-chat-spec 6bcd9d8 wss L408-410: since "a `userId` supplied in the
/// frame is ignored", a frame without one is accepted and relayed with
/// the sender's.
#[tokio::test]
async fn interaction_frame_without_user_id_is_relayed() {
    let backend = scene();
    let frame: SceneInteractionEvent = serde_json::from_value(json!({
        "@type": "SceneInteractionEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "objectId": SCULPTURE,
        "action": "click",
        "data": null
    }))
    .unwrap();
    let e = relayed(&backend, ALICE, &frame).await.expect("relayed");
    assert_eq!(e.user_id, ALICE);
}

/// jmap-chat-spec 6bcd9d8 wss L413-414, check 1: "The user has an active
/// avatar ... in the region identified by `accountId` and `regionId`."
/// L418-419: "If any check fails, the server discards the frame". ADMIN's
/// avatar has left; CAROL has none in VAULT.
#[tokio::test]
async fn interaction_without_an_active_avatar_is_discarded() {
    let backend = scene();
    assert!(!relayable(&backend, ADMIN, &interaction(R1, SCULPTURE, ADMIN)).await);
    assert!(!relayable(&backend, CAROL, &interaction(VAULT, VAULT_OBJ, CAROL)).await);
}

/// jmap-chat-spec 6bcd9d8 wss L415-416, check 2: "The `objectId`
/// references a SceneObject in that region with `interactable: true`."
#[tokio::test]
async fn interaction_needs_an_interactable_object_in_the_region() {
    let backend = scene();
    // Not interactable.
    assert!(!relayable(&backend, ALICE, &interaction(R1, PUZZLE, ALICE)).await);
    // Interactable, but in VAULT, not in R1.
    assert!(!relayable(&backend, ALICE, &interaction(R1, VAULT_OBJ, ALICE)).await);
    // No such object.
    assert!(
        !relayable(
            &backend,
            ALICE,
            &interaction(R1, "01J5OBJ0000000000000000099", ALICE)
        )
        .await
    );
}

/// jmap-chat-spec 6bcd9d8 wss L178-179: "Every event carries an
/// `accountId` (String) property: the id of the region's hosting
/// account"; L184-185: "SceneRegion and SceneObject ids are unique only
/// within an account, so `regionId` alone does not identify a region."
/// An event naming another account is not about [`ACCOUNT`]'s R1.
#[tokio::test]
async fn event_for_another_hosting_account_is_not_delivered() {
    let backend = scene();
    let mut e = interaction(R1, SCULPTURE, ALICE);
    assert!(delivers(&backend, CAROL, &all(), &SceneEvent::Interaction(e.clone())).await);
    e.account_id = Id::from("acct-002");
    assert!(!delivers(&backend, CAROL, &all(), &SceneEvent::Interaction(e)).await);
}

/// jmap-chat-spec 6bcd9d8 wss L413-414: the server verifies that "The
/// user has an active avatar ... in the region identified by `accountId`
/// and `regionId`."
#[tokio::test]
async fn interaction_for_another_hosting_account_is_discarded() {
    let backend = scene();
    let mut e = interaction(R1, SCULPTURE, ALICE);
    e.account_id = Id::from("acct-002");
    assert!(!relayable(&backend, ALICE, &e).await);
}

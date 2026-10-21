//! The union of the `ChatStreamEnable` `"scene"` path and
//! `SceneStreamEnable` (`should_deliver_scene`).
//!
//! Oracle: draft-atwood-jmap-scene-wss-00.md at jmap-chat-spec 0cd3af0;
//! "wss Lnnn" cites it, "scene Lnnn" cites draft-atwood-jmap-scene-00.md.
//! Records and events are hand-written JSON.

use jmap_chat_scene_server::should_deliver_scene;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::SceneEphemeralSubscription;
use jmap_scene_types::{SceneCapability, SceneEvent, SceneStreamEnable};
use jmap_types::Id;
use serde_json::{json, Value};

const ACCOUNT: &str = "acct-001";
const ALICE: &str = "user:alice@example.com";
const BOB: &str = "user:bob@example.com";
/// [`ALICE`] and [`BOB`] both have an active avatar here.
const R1: &str = "01J5ABC0000000000000000001";
/// [`ALICE`] and [`BOB`] both have an active avatar here.
const R2: &str = "01J5ABC0000000000000000002";
/// Only [`BOB`] has an active avatar here; [`ALICE`]'s has left.
const R3: &str = "01J5ABC0000000000000000003";

/// A public region, hand-written from the field list at scene L263-402.
fn region(id: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "ownerId": ALICE,
        "name": id,
        "description": null,
        "bounds": { "min": [-20, 0, -20], "max": [20, 8, 20] },
        "environment": null,
        "simulationUri": null,
        "spawnPosition": [0, 0, 5],
        "spawnOrientation": [0, 0, 0, 1],
        "activeAvatarCount": 0,
        "viewHint": "3d",
        "accessPolicy": "public",
        "createdAt": "2026-06-01T10:00:00Z",
        "updatedAt": "2026-06-01T10:00:00Z"
    })
}

/// A SceneAvatar, hand-written from its field list at scene L548 (§SceneAvatar Object Fields).
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

fn fixture() -> MemoryBackend {
    let backend = MemoryBackend::new();
    let cap: SceneCapability = serde_json::from_value(json!({
        "mayCreateRegion": true,
        "mayCreateObject": true,
        "supportedVisualTypes": ["model/gltf-binary"],
        "maxRegionsPerAccount": null,
        "maxObjectsPerRegion": null,
        "maxAvatarsPerRegion": null,
        "maxAssetSizeBytes": null
    }))
    .expect("capability fixture");
    let account = Id::from(ACCOUNT);
    backend.register_account(&account, cap);
    for id in [R1, R2, R3] {
        backend.insert_object_for_test("SceneRegion", &account, id, region(id));
    }
    for (id, region, user, left_at) in [
        ("AV-ALICE-1", R1, ALICE, json!(null)),
        ("AV-ALICE-2", R2, ALICE, json!(null)),
        ("AV-ALICE-3", R3, ALICE, json!("2026-06-06T11:00:00Z")),
        ("AV-BOB-1", R1, BOB, json!(null)),
        ("AV-BOB-2", R2, BOB, json!(null)),
        ("AV-BOB-3", R3, BOB, json!(null)),
    ] {
        backend.insert_object_for_test(
            "SceneAvatar",
            &account,
            id,
            avatar(id, region, user, left_at),
        );
    }
    backend
}

fn subscribed(enable: Value) -> SceneEphemeralSubscription {
    let enable: SceneStreamEnable = serde_json::from_value(enable).expect("SceneStreamEnable");
    let mut sub = SceneEphemeralSubscription::default();
    sub.replace_with(&enable).expect("valid SceneStreamEnable");
    sub
}

/// The wss L214-222 `SceneAvatarEvent` example at jmap-chat-spec 6bcd9d8,
/// for [`BOB`] in `region` of [`ACCOUNT`].
fn bob_entered(region: &str) -> SceneEvent {
    serde_json::from_value(json!({
        "@type": "SceneAvatarEvent",
        "accountId": ACCOUNT,
        "regionId": region,
        "avatarId": "AV-BOB",
        "event": "entered",
        "userId": BOB,
        "displayName": "Bob"
    }))
    .expect("SceneAvatarEvent")
}

/// Whether [`ALICE`]'s connection gets `ev`.
async fn delivers(chat_scene: bool, sub: &SceneEphemeralSubscription, ev: &SceneEvent) -> bool {
    should_deliver_scene(
        &fixture(),
        &ALICE.to_owned(),
        &Id::from(ACCOUNT),
        &Id::from(ACCOUNT),
        chat_scene,
        sub,
        ev,
    )
    .await
    .expect("should_deliver_scene")
}

/// wss L837-840: "`ChatStreamEnable` with `"scene"` in `dataTypes`:
/// Subscribes to all Scene event types for all regions in which the user
/// has an active avatar. This is the coarse-grained path and requires no
/// additional control messages."
#[tokio::test]
async fn chat_path_alone_covers_every_region_with_an_active_avatar() {
    let none = SceneEphemeralSubscription::default();
    assert!(delivers(true, &none, &bob_entered(R1)).await);
    assert!(delivers(true, &none, &bob_entered(R2)).await);
    assert!(
        !delivers(true, &none, &bob_entered(R3)).await,
        "Alice's avatar in R3 has left, so it is not a region in which she has an active avatar"
    );
}

/// wss L847-851: "If the client has an active `ChatStreamEnable`
/// subscription that includes `"scene"` AND an active `SceneStreamEnable`
/// subscription, the server MUST deliver the union of both subscriptions.
/// The server MUST NOT deliver duplicate events; if an event matches both
/// subscriptions, it is delivered exactly once."
#[tokio::test]
async fn union_covers_what_either_path_covers() {
    let r1_interactions = subscribed(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1],
        "eventTypes": ["SceneInteractionEvent"]
    }));
    let r2 = bob_entered(R2);
    assert!(
        !delivers(false, &r1_interactions, &r2).await,
        "SceneStreamEnable alone does not cover avatar events in R2"
    );
    assert!(
        delivers(true, &r1_interactions, &r2).await,
        "the chat path covers it, so the union does"
    );

    let r1 = subscribed(json!({ "@type": "SceneStreamEnable", "regionIds": [R1] }));
    assert!(
        delivers(true, &r1, &bob_entered(R1)).await,
        "an event matching both paths gets one delivery decision: send"
    );
}

/// wss L855-857: "`SceneStreamDisable` cancels only the Scene-specific
/// subscription established by `SceneStreamEnable`. It does not affect
/// Scene events delivered through `ChatStreamEnable`."
#[tokio::test]
async fn scene_stream_disable_leaves_chat_path() {
    let disabled = SceneEphemeralSubscription::default();
    assert!(delivers(true, &disabled, &bob_entered(R1)).await);
}

/// wss L859-862: "`ChatStreamDisable` cancels only the Chat ephemeral
/// subscription. If `"scene"` was included in the `ChatStreamEnable`
/// `dataTypes`, Scene events delivered through that path stop; events
/// delivered through an active `SceneStreamEnable` subscription are
/// unaffected."
#[tokio::test]
async fn chat_stream_disable_leaves_scene_path() {
    let r1 = subscribed(json!({ "@type": "SceneStreamEnable", "regionIds": [R1] }));
    assert!(delivers(false, &r1, &bob_entered(R1)).await);
    assert!(
        !delivers(false, &r1, &bob_entered(R2)).await,
        "R2 came only through the chat path, which stopped"
    );
    let none = SceneEphemeralSubscription::default();
    assert!(!delivers(false, &none, &bob_entered(R1)).await);
}

/// wss L874-876: "Errors in one mechanism (e.g., invalid `regionIds` in
/// SceneStreamEnable) MUST NOT affect the other's active subscriptions."
/// wss L502-506 gives the rejected frame used here: "If `eventTypes` is an
/// empty array ... the server MUST respond with a `RequestError` ... and
/// MUST NOT update the current ephemeral subscription state."
#[tokio::test]
async fn rejected_scene_stream_enable_leaves_chat_path() {
    let mut sub = SceneEphemeralSubscription::default();
    let bad: SceneStreamEnable =
        serde_json::from_value(json!({ "@type": "SceneStreamEnable", "eventTypes": [] }))
            .expect("SceneStreamEnable");
    assert!(sub.replace_with(&bad).is_err());
    assert!(delivers(true, &sub, &bob_entered(R2)).await);
}

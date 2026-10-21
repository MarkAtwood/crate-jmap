//! `SceneObject/interact`.
//!
//! Oracle: draft-atwood-jmap-scene-00 at jmap-chat-spec 6bcd9d8,
//! §SceneObject/interact L2015-2110: the example request and response
//! (L2063-2110) verbatim, and the SetError table (L2054-2059), quoted at
//! each assertion. Fixtures are hand-written JSON.

#[macro_use]
mod common;

use std::sync::Arc;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_object_interact, register_scene_handlers};
use jmap_scene_types::SceneObject;
use jmap_server::{Dispatcher, JmapBackend};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{json, Value};

/// The region of the example request.
const R1: &str = "01J5ABC0000000000000000001";
/// The first object of the example request; interactable.
const OBJ2: &str = "01J5QBJ0000000000000000002";
/// The second object of the example request: "the second object has
/// `interactable: false`".
const OBJ3: &str = "01J5QBJ0000000000000000003";
/// An `"invite"` region [`BOB`] is not invited to.
const VAULT: &str = "01J5ABC0000000000000000002";
/// An interactable object in [`VAULT`].
const VAULT_OBJ: &str = "01J5QBJ0000000000000000004";

fn object(id: &str, region: &str, interactable: bool) -> Value {
    let mut o = stored_object(id, region, json!([0, 0, 0]), "2026-06-02T10:00:00Z");
    o["interactable"] = json!(interactable);
    o
}

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

/// [`R1`] (public) holding [`OBJ2`] and [`OBJ3`], [`VAULT`] (invite,
/// owned by [`ALICE`]) holding [`VAULT_OBJ`]. [`ALICE`] has an active
/// avatar in both regions, [`BOB`] in both (with no invitation to
/// [`VAULT`]), and [`ADMIN`] an avatar in [`R1`] that has left.
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
        (OBJ2, R1, true),
        (OBJ3, R1, false),
        (VAULT_OBJ, VAULT, true),
    ] {
        seed_as(&backend, "SceneObject", object(id, region, interactable));
    }
    for (id, region, user, left_at) in [
        ("01J5AVT0000000000000000001", R1, ALICE, json!(null)),
        ("01J5AVT0000000000000000002", VAULT, ALICE, json!(null)),
        ("01J5AVT0000000000000000003", R1, BOB, json!(null)),
        ("01J5AVT0000000000000000004", VAULT, BOB, json!(null)),
        (
            "01J5AVT0000000000000000005",
            R1,
            ADMIN,
            json!("2026-06-06T11:00:00Z"),
        ),
    ] {
        seed_as(&backend, "SceneAvatar", avatar(id, region, user, left_at));
    }
    backend
}

/// `SceneObject/interact` as `caller` with one interaction `i0`.
async fn interact_one(backend: &MemoryBackend, caller: &str, interaction: Value) -> Value {
    call!(
        handle_scene_object_interact,
        backend,
        caller,
        json!({ "interactions": { "i0": interaction } }),
    )
}

fn click(region: &str, object_id: &str) -> Value {
    json!({ "regionId": region, "objectId": object_id, "action": "click", "data": null })
}

/// The SetError of the single refused interaction `i0`.
async fn refusal(backend: &MemoryBackend, caller: &str, interaction: Value) -> Value {
    let resp = interact_one(backend, caller, interaction).await;
    assert_eq!(resp["accepted"], Value::Null, "{resp}");
    resp["notAccepted"]["i0"].clone()
}

/// The example request and response of L2063-2110, through the
/// dispatcher: "It works over HTTP and, as a `Request`, over the JMAP
/// WebSocket".
#[tokio::test]
async fn draft_example_through_the_dispatcher() {
    let backend = Arc::new(scene());
    let mut dispatcher: Dispatcher<String> = Dispatcher::new();
    register_scene_handlers(&mut dispatcher, Arc::clone(&backend));

    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:scene"
        ],
        "methodCalls": [
            ["SceneObject/interact", {
                "accountId": "acct-001",
                "interactions": {
                    "i0": {
                        "regionId": "01J5ABC0000000000000000001",
                        "objectId": "01J5QBJ0000000000000000002",
                        "action": "click",
                        "data": null
                    },
                    "i1": {
                        "regionId": "01J5ABC0000000000000000001",
                        "objectId": "01J5QBJ0000000000000000003",
                        "action": "grab",
                        "data": {"hand": "left"}
                    }
                }
            }, "x0"]
        ]
    }))
    .unwrap();
    let response = dispatcher
        .dispatch(request, ALICE.to_owned(), State::from("s0"))
        .await;
    assert_eq!(
        serde_json::to_value(&response).unwrap()["methodResponses"],
        json!([
            ["SceneObject/interact", {
                "accountId": "acct-001",
                "accepted": ["i0"],
                "notAccepted": {
                    "i1": {
                        "type": "forbidden",
                        "description": "The object is not interactable."
                    }
                }
            }, "x0"]
        ])
    );
}

/// L2061: "For each accepted interaction ... the server delivers a
/// `SceneInteractionEvent` carrying the interaction and the caller's
/// userId"; draft-atwood-jmap-scene-wss-00 L178-179: "Every event carries
/// an `accountId` (String) property: the id of the region's hosting
/// account".
#[tokio::test]
async fn accepted_interaction_reaches_the_backend_as_an_event() {
    let backend = scene();
    let resp = interact_one(
        &backend,
        BOB,
        json!({
            "regionId": R1,
            "objectId": OBJ2,
            "action": "grab",
            "data": {"hand": "left"}
        }),
    )
    .await;
    assert_eq!(
        resp,
        json!({ "accountId": ACCOUNT, "accepted": ["i0"], "notAccepted": null })
    );
    let events = backend.accepted_interactions();
    assert_eq!(events.len(), 1);
    assert_eq!(
        serde_json::to_value(&events[0]).unwrap(),
        json!({
            "accountId": ACCOUNT,
            "regionId": R1,
            "objectId": OBJ2,
            "userId": BOB,
            "action": "grab",
            "data": {"hand": "left"}
        })
    );
}

/// L2017: "the method does not change any record or state string."
#[tokio::test]
async fn interact_changes_no_state() {
    let backend = scene();
    let caller = ALICE.to_owned();
    let account = Id::from(ACCOUNT);
    let before = backend
        .get_state::<SceneObject>(&caller, &account)
        .await
        .unwrap();
    interact_one(&backend, ALICE, click(R1, OBJ2)).await;
    let after = backend
        .get_state::<SceneObject>(&caller, &account)
        .await
        .unwrap();
    assert_eq!(before, after);
}

/// L2056 `notFound`: "The region or object does not exist, the caller has
/// no access to it ({{access-control}}), or the object is outside the
/// caller's visibility set ({{visibility-contract}})."
#[tokio::test]
async fn not_found_cases() {
    let backend = scene();
    backend.hide_object(BOB, OBJ2);
    for (caller, interaction) in [
        // The object does not exist.
        (ALICE, click(R1, "01J5QBJ0000000000000000099")),
        // The region does not exist.
        (ALICE, click("01J5ABC0000000000000000099", OBJ2)),
        // The object is in another region than `regionId`.
        (ALICE, click(R1, VAULT_OBJ)),
        // No access to the region (L2606: "`notFound` ... for
        // `SceneObject/interact`"). BOB's active avatar in VAULT would
        // give him access: "A user has access to a region when ... the
        // user has an active SceneAvatar in it" (6bcd9d8 L2595).
        (CAROL, click(VAULT, VAULT_OBJ)),
        // Outside the visibility set (L2589: "The server SHOULD NOT return
        // `forbidden`, as that would confirm the object still exists").
        (BOB, click(R1, OBJ2)),
    ] {
        let err = refusal(&backend, caller, interaction.clone()).await;
        assert_eq!(err["type"], "notFound", "{caller} {interaction}: {err}");
    }
    assert!(backend.accepted_interactions().is_empty());
}

/// L2057 `forbidden`: "The caller has no active SceneAvatar in the
/// region, or the object has `interactable: false`."
#[tokio::test]
async fn forbidden_cases() {
    let backend = scene();
    backend.grant_region_access(VAULT, CAROL);
    for (caller, interaction) in [
        // An avatar that has left (`leftAt` not `null`).
        (ADMIN, click(R1, OBJ2)),
        // No avatar at all.
        (CAROL, click(VAULT, VAULT_OBJ)),
        // `interactable: false`.
        (ALICE, click(R1, OBJ3)),
    ] {
        let err = refusal(&backend, caller, interaction.clone()).await;
        assert_eq!(err["type"], "forbidden", "{caller} {interaction}: {err}");
    }
    assert!(backend.accepted_interactions().is_empty());
}

const CAROL: &str = "user:carol@example.com";

/// L2058 `rateLimit`: "The caller has exceeded a deployment-defined
/// interaction rate."
#[tokio::test]
async fn rate_limit() {
    let backend = scene();
    backend.rate_limit_interactions(ALICE);
    let err = refusal(&backend, ALICE, click(R1, OBJ2)).await;
    assert_eq!(err["type"], "rateLimit", "{err}");
    assert!(backend.accepted_interactions().is_empty());
}

/// L2059 `invalidProperties`: "A property is missing or has the wrong
/// type; `properties` names it."
#[tokio::test]
async fn invalid_properties_name_the_property() {
    let backend = scene();
    for (interaction, properties) in [
        (
            json!({ "regionId": R1, "objectId": OBJ2 }),
            json!(["action"]),
        ),
        (
            json!({ "regionId": 7, "objectId": OBJ2, "action": "click" }),
            json!(["regionId"]),
        ),
        (
            json!({ "regionId": R1, "action": "click", "data": "left" }),
            json!(["objectId", "data"]),
        ),
    ] {
        let err = refusal(&backend, ALICE, interaction.clone()).await;
        assert_eq!(err["type"], "invalidProperties", "{interaction}: {err}");
        assert_eq!(err["properties"], properties, "{interaction}: {err}");
    }
    let err = refusal(&backend, ALICE, json!("click")).await;
    assert_eq!(err["type"], "invalidProperties", "{err}");
}

/// Each interaction is decided on its own: one refusal does not refuse
/// the others (L2046-2050 `accepted` / `notAccepted`).
#[tokio::test]
async fn mixed_batch_splits_accepted_and_not_accepted() {
    let backend = scene();
    let resp = call!(
        handle_scene_object_interact,
        &backend,
        ALICE,
        json!({ "interactions": {
            "a": click(R1, OBJ2),
            "b": click(R1, "01J5QBJ0000000000000000099"),
            "c": click(VAULT, VAULT_OBJ)
        } }),
    );
    let mut accepted: Vec<&str> = resp["accepted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    accepted.sort_unstable();
    assert_eq!(accepted, ["a", "c"]);
    assert_eq!(resp["notAccepted"]["b"]["type"], "notFound");
    assert_eq!(backend.accepted_interactions().len(), 2);
}

/// L2024-2025: `interactions` (Id[SceneInteraction]) is a required
/// argument; RFC 8620 §3.6.2 `invalidArguments` and `accountNotFound`.
#[tokio::test]
async fn method_level_errors() {
    let backend = scene();
    let caller = ALICE.to_owned();
    let err = handle_scene_object_interact(&backend, &caller, json!({ "accountId": ACCOUNT }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type, "invalidArguments");
    let err = handle_scene_object_interact(
        &backend,
        &caller,
        json!({ "accountId": "acct-404", "interactions": {} }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type, "accountNotFound");
}

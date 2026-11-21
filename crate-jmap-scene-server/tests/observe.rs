//! Observer subscriptions without the WebSocket: the `observe` member of
//! `SceneStreamEnable`, delivery to observers, the SceneRegion
//! `allowObservers` / `observerCount` properties and the MemoryBackend
//! count behind `SceneBackend::set_observing`.
//!
//! Oracle: jmap-chat-spec 93f2258, draft-atwood-jmap-scene-wss-00
//! §Capability and §SceneStreamEnable, draft-atwood-jmap-scene-00
//! §SceneRegion and §Creating a Region; each test quotes the sentence it
//! checks. Fixtures are hand-written JSON.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    SceneBackend, SceneEphemeralSubscription, handle_scene_avatar_set, handle_scene_region_changes,
    handle_scene_region_get, handle_scene_region_set, should_deliver,
};
use jmap_scene_types::{SceneEvent, SceneStreamEnable};
use jmap_types::Id;
use serde_json::{Value, json};

/// A public region owned by [`ALICE`], with the observer properties.
const R1: &str = "01J5ABC0000000000000000001";
/// An `"invite"` region owned by [`ALICE`].
const VAULT: &str = "01J5ABC0000000000000000002";
/// A user with access to [`R1`] and no avatar anywhere.
const CAROL: &str = "user:carol@example.com";

fn region(id: &str, policy: &str) -> Value {
    let mut r = stored_region(id, id, "2026-06-01T10:00:00Z");
    r["accessPolicy"] = json!(policy);
    r["allowObservers"] = json!(true);
    r["observerCount"] = json!(0);
    r
}

/// [`R1`] and [`VAULT`]; [`BOB`] has an active avatar in [`R1`];
/// `supportsObserve` on.
fn scene() -> MemoryBackend {
    let backend = backend();
    backend.set_supports_observe(true);
    seed_as(&backend, "SceneRegion", region(R1, "public"));
    seed_as(&backend, "SceneRegion", region(VAULT, "invite"));
    seed_as(
        &backend,
        "SceneAvatar",
        json!({
            "id": "01J5AVT0000000000000000002",
            "regionId": R1,
            "userId": BOB,
            "displayName": "Bob",
            "position": [0, 0, 5],
            "orientation": [0, 0, 0, 1],
            "visualRef": null,
            "visualType": null,
            "joinedAt": "2026-06-06T10:00:00Z",
            "leftAt": null,
            "worldState": null
        }),
    );
    backend
}

fn enable(raw: Value) -> SceneStreamEnable {
    serde_json::from_value(raw).unwrap()
}

fn observer(regions: &[&str]) -> SceneEphemeralSubscription {
    let mut sub = SceneEphemeralSubscription::default();
    sub.replace_with_observe(&enable(json!({"regionIds": regions, "observe": true})))
        .unwrap();
    sub
}

fn region_event(region: &str) -> SceneEvent {
    serde_json::from_value(json!({
        "@type": "SceneRegionEvent",
        "accountId": ACCOUNT,
        "regionId": region,
        "event": "com.example.game.round-start",
        "data": null
    }))
    .unwrap()
}

async fn delivers(backend: &MemoryBackend, to: &str, sub: &SceneEphemeralSubscription) -> bool {
    let account = Id::from(ACCOUNT);
    should_deliver(
        backend,
        &Id::from(to),
        &account,
        &account,
        sub,
        &region_event(R1),
    )
    .await
    .unwrap()
}

async fn observer_count(backend: &MemoryBackend, region: &str) -> Value {
    let resp = call!(
        handle_scene_region_get,
        backend,
        ALICE,
        json!({ "ids": [region] })
    );
    resp["list"][0]["observerCount"].clone()
}

/// "If `observe` is `true` and `regionIds` is `null`, the server MUST
/// respond with a `RequestError` of type ... `invalidArguments` and MUST
/// NOT update the current ephemeral subscription state". Without
/// `supportsObserve` the member is "Recognized only when the server
/// advertises `supportsObserve`": `replace_with` ignores it.
#[test]
fn observe_needs_region_ids_and_is_ignored_without_support() {
    let mut sub = observer(&[R1]);
    let before = sub.clone();
    let err = sub
        .replace_with_observe(&enable(json!({"regionIds": null, "observe": true})))
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
    assert_eq!(sub, before);
    assert!(sub.filter().unwrap().observes());

    let mut plain = SceneEphemeralSubscription::default();
    plain
        .replace_with(&enable(json!({"regionIds": null, "observe": true})))
        .unwrap();
    assert!(!plain.filter().unwrap().observes());
    plain
        .replace_with_observe(&enable(json!({"regionIds": [R1], "observe": false})))
        .unwrap();
    assert!(!plain.filter().unwrap().observes());
}

/// "When `true`, the subscription is an observer subscription: events for
/// each region in `regionIds` are delivered whether or not the user has
/// an active avatar in it"; without `observe` they need one.
#[tokio::test]
async fn observers_receive_events_without_an_avatar() {
    let backend = scene();
    let mut listed = SceneEphemeralSubscription::default();
    listed
        .replace_with(&enable(json!({"regionIds": [R1]})))
        .unwrap();
    assert!(!delivers(&backend, CAROL, &listed).await);
    assert!(delivers(&backend, CAROL, &observer(&[R1])).await);
    // Only the listed regions.
    assert!(!delivers(&backend, CAROL, &observer(&[VAULT])).await);
    // An avatar still works under an observer subscription.
    assert!(delivers(&backend, BOB, &observer(&[R1])).await);
}

/// "If the region owner later sets `allowObservers` to `false`, or the
/// user loses access, events for that region stop"; "A server MAY also
/// decline observers of a region for deployment-defined policy reasons".
#[tokio::test]
async fn delivery_stops_when_observers_are_refused() {
    let backend = scene();
    let sub = observer(&[R1]);
    let resp = call!(
        ids handle_scene_region_set,
        &backend,
        ALICE,
        json!({"update": {(R1): {"allowObservers": false}}})
    );
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(!delivers(&backend, CAROL, &sub).await);
    // Bob is present, so he still receives them.
    assert!(delivers(&backend, BOB, &sub).await);

    let backend = scene();
    backend.decline_observers(R1);
    assert!(!delivers(&backend, CAROL, &sub).await);

    // VAULT is invite-only: no access, no events.
    let backend = scene();
    let account = Id::from(ACCOUNT);
    let e = region_event(VAULT);
    assert!(
        !should_deliver(
            &backend,
            &Id::from(CAROL),
            &account,
            &account,
            &observer(&[VAULT]),
            &e
        )
        .await
        .unwrap()
    );
}

/// "Region create sets it to `0`"; `allowObservers` "Default: `true`";
/// both are "Present only when the server advertises `supportsObserve`".
/// `observerCount` is server-set.
#[tokio::test]
async fn region_create_sets_the_observer_properties() {
    let backend = scene();
    let create = json!({"name": "Lobby", "bounds": {"min": [0, 0, 0], "max": [1, 1, 1]}});
    let resp = call!(
        ids handle_scene_region_set,
        &backend,
        ALICE,
        json!({"create": {"r": create.clone()}})
    );
    assert_eq!(resp["created"]["r"]["observerCount"], 0);
    assert_eq!(resp["created"]["r"]["allowObservers"], true);
    let mut closed = create.clone();
    closed["allowObservers"] = json!(false);
    let resp = call!(
        ids handle_scene_region_set,
        &backend,
        ALICE,
        json!({"create": {"r": closed}})
    );
    assert_eq!(resp["created"]["r"]["observerCount"], 0);
    let id = resp["created"]["r"]["id"].as_str().unwrap();
    let got = call!(
        handle_scene_region_get,
        &backend,
        ALICE,
        json!({ "ids": [id] })
    );
    assert_eq!(got["list"][0]["allowObservers"], false);

    for (bad, property) in [
        (json!({"observerCount": 3}), "observerCount"),
        (json!({"allowObservers": "yes"}), "allowObservers"),
    ] {
        let mut c = create.clone();
        for (k, v) in bad.as_object().unwrap() {
            c[k] = v.clone();
        }
        let resp = call!(
            ids handle_scene_region_set,
            &backend,
            ALICE,
            json!({"create": {"r": c}})
        );
        assert_eq!(
            resp["notCreated"]["r"]["properties"],
            json!([property]),
            "{resp}"
        );
    }

    let plain = backend_with(capability());
    let resp = call!(
        ids handle_scene_region_set,
        &plain,
        ALICE,
        json!({"create": {"r": create.clone()}})
    );
    assert!(
        resp["created"]["r"].get("observerCount").is_none(),
        "{resp}"
    );
    let mut c = create;
    c["allowObservers"] = json!(true);
    let resp = call!(
        ids handle_scene_region_set,
        &plain,
        ALICE,
        json!({"create": {"r": c}})
    );
    assert_eq!(
        resp["notCreated"]["r"]["properties"],
        json!(["allowObservers"])
    );
}

/// `allowObservers`: "The region owner and users with deployment-defined
/// administrative privileges may update it, as for any other region
/// property."
#[tokio::test]
async fn only_the_owner_or_an_admin_changes_allow_observers() {
    let backend = scene();
    let resp = call!(
        ids handle_scene_region_set,
        &backend,
        CAROL,
        json!({"update": {(R1): {"allowObservers": false}}})
    );
    assert_eq!(resp["notUpdated"][R1]["type"], "forbidden");
    let resp = call!(
        ids handle_scene_region_set,
        &backend,
        ADMIN,
        json!({"update": {(R1): {"allowObservers": false}}})
    );
    assert!(resp["notUpdated"].is_null(), "{resp}");
}

/// `observerCount`: "the number of users currently observing the region
/// through an observer subscription ... who do not have an active avatar
/// in it. A user with several observing connections counts once." "a
/// change to it is a change to the SceneRegion: the server reports the
/// region in `updated` of `SceneRegion/changes` and advances the
/// SceneRegion state string". "The count drops when the subscription
/// stops covering the region, through ... the user entering the region
/// with an avatar."
#[tokio::test]
async fn observer_count_counts_distinct_users_without_an_avatar() {
    let backend = scene();
    let account = Id::from(ACCOUNT);
    let r1 = Id::from(R1);
    let state = |b: &MemoryBackend| {
        let b = b.clone();
        async move {
            call!(handle_scene_region_get, &b, ALICE, json!({"ids": []}))["state"]
                .as_str()
                .unwrap()
                .to_owned()
        }
    };
    let s0 = state(&backend).await;
    let carol = Id::from(CAROL);
    backend
        .set_observing(&carol, &account, &r1, true)
        .await
        .unwrap();
    assert_eq!(observer_count(&backend, R1).await, 1);
    let changes = call!(
        handle_scene_region_changes,
        &backend,
        ALICE,
        json!({"sinceState": s0})
    );
    assert_eq!(changes["updated"], json!([R1]));

    // Carol's second connection: still one user.
    backend
        .set_observing(&carol, &account, &r1, true)
        .await
        .unwrap();
    assert_eq!(observer_count(&backend, R1).await, 1);
    // Bob observes too but is present with an avatar.
    backend
        .set_observing(&Id::from(BOB), &account, &r1, true)
        .await
        .unwrap();
    assert_eq!(observer_count(&backend, R1).await, 1);

    // Carol enters with an avatar: she no longer counts.
    let s1 = state(&backend).await;
    let resp = call!(
        ids handle_scene_avatar_set,
        &backend,
        CAROL,
        json!({"create": {"a": {"regionId": R1}}})
    );
    assert!(resp["notCreated"].is_null(), "{resp}");
    assert_eq!(observer_count(&backend, R1).await, 0);
    assert_ne!(state(&backend).await, s1);
    // She leaves again: counted again.
    let avatar = resp["created"]["a"]["id"].as_str().unwrap();
    call!(
        ids handle_scene_avatar_set,
        &backend,
        CAROL,
        json!({"update": {(avatar): {"leftAt": "2026-06-06T12:00:00Z"}}})
    );
    assert_eq!(observer_count(&backend, R1).await, 1);

    // One connection closes: one remains.
    backend
        .set_observing(&carol, &account, &r1, false)
        .await
        .unwrap();
    assert_eq!(observer_count(&backend, R1).await, 1);
    backend
        .set_observing(&carol, &account, &r1, false)
        .await
        .unwrap();
    assert_eq!(observer_count(&backend, R1).await, 0);
}

/// "Setting it to `false` stops event delivery to the region's current
/// observers", so they no longer count; the update echoes the changed
/// `observerCount` (RFC 8620 §5.3: properties "changed in a way not
/// explicitly requested").
#[tokio::test]
async fn closing_the_region_to_observers_resets_the_count() {
    let backend = scene();
    let account = Id::from(ACCOUNT);
    backend
        .set_observing(&Id::from(CAROL), &account, &Id::from(R1), true)
        .await
        .unwrap();
    assert_eq!(observer_count(&backend, R1).await, 1);
    let resp = call!(
        ids handle_scene_region_set,
        &backend,
        ALICE,
        json!({"update": {(R1): {"allowObservers": false}}})
    );
    assert_eq!(resp["updated"][R1]["observerCount"], 0, "{resp}");
    assert_eq!(observer_count(&backend, R1).await, 0);
    call!(
        ids handle_scene_region_set,
        &backend,
        ALICE,
        json!({"update": {(R1): {"allowObservers": true}}})
    );
    assert_eq!(observer_count(&backend, R1).await, 1);
}

/// "The server recomputes the count when ... the region is updated, for
/// example its `allowObservers` or access policy. While the region does
/// not admit observers, the count is `0`" (jmap-chat-spec 42aed85). A
/// decline by policy and an access change made outside the region record
/// change whom delivery reaches just as much, so the MemoryBackend
/// recounts on them too, advancing the SceneRegion state (bd:JMAP-1mep).
#[tokio::test]
async fn policy_and_access_changes_outside_the_region_recount() {
    let backend = scene();
    let account = Id::from(ACCOUNT);
    let carol = Id::from(CAROL);
    for region in [R1, VAULT] {
        backend
            .set_observing(&carol, &account, &Id::from(region), true)
            .await
            .unwrap();
    }
    assert_eq!(observer_count(&backend, R1).await, 1);
    assert_eq!(observer_count(&backend, VAULT).await, 0);

    // The observer policy.
    let s0 = call!(handle_scene_region_get, &backend, ALICE, json!({"ids": []}))["state"].clone();
    backend.decline_observers(R1);
    assert_eq!(observer_count(&backend, R1).await, 0);
    let changes = call!(
        handle_scene_region_changes,
        &backend,
        ALICE,
        json!({"sinceState": s0})
    );
    assert_eq!(changes["updated"], json!([R1]));
    backend.admit_observers(R1);
    assert_eq!(observer_count(&backend, R1).await, 1);

    // Access to the invite region, granted and withdrawn.
    backend.grant_region_access(VAULT, CAROL);
    assert_eq!(observer_count(&backend, VAULT).await, 1);
    backend.revoke_region_access(VAULT, CAROL);
    assert_eq!(observer_count(&backend, VAULT).await, 0);
    // Admin privileges give access to every region.
    backend.add_admin(CAROL);
    assert_eq!(observer_count(&backend, VAULT).await, 1);
}

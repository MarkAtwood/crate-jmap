//! Visibility, enumeration-safe `notFound` and blocked-contact filtering
//! (draft-atwood-jmap-scene-00 §Visibility Contract L2338-2350, §Region
//! Access L2354-2367, §Blocked-Contact Spatial Presence L2423-2435, at
//! jmap-chat-spec fda85cb). Each assertion quotes the draft text it
//! checks; the expected ids are worked out by hand from the seeded
//! records.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    handle_scene_avatar_get, handle_scene_avatar_query, handle_scene_avatar_set,
    handle_scene_object_get, handle_scene_object_query, handle_scene_object_set,
    handle_scene_region_get, handle_scene_region_query, handle_scene_region_set,
};
use serde_json::{Value, json};

/// A `"public"` region owned by [`ALICE`].
const PUBLIC: &str = "01J5ABC0000000000000000001";
/// An `"invite"` region owned by [`ALICE`]; [`BOB`] is not invited.
const INVITE: &str = "01J5ABC0000000000000000002";
/// Objects owned by [`ALICE`] in [`PUBLIC`].
const O1: &str = "01J5OBJ0000000000000000001";
const O2: &str = "01J5OBJ0000000000000000002";
/// An object owned by [`ALICE`] in [`INVITE`].
const SECRET: &str = "01J5OBJ0000000000000000003";
/// No record has this id.
const MISSING: &str = "01J5ZZZ0000000000000000009";
const ALICE_AV: &str = "01J5AVT0000000000000000001";
const BOB_AV: &str = "01J5AVT0000000000000000002";
/// [`ALICE`]'s avatar in [`INVITE`].
const INVITE_AV: &str = "01J5AVT0000000000000000003";

/// A stored active avatar, hand-written from the field list at L550-581.
fn avatar(id: &str, region: &str, user: &str) -> Value {
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
        "leftAt": null,
        "worldState": null
    })
}

fn scene() -> MemoryBackend {
    let backend = backend();
    let mut public = stored_region(PUBLIC, "Plaza", "2026-06-01T10:00:00Z");
    public["activeAvatarCount"] = json!(2);
    seed_as(&backend, "SceneRegion", public);
    let mut invite = stored_region(INVITE, "Vault", "2026-06-01T11:00:00Z");
    invite["accessPolicy"] = json!("invite");
    invite["activeAvatarCount"] = json!(1);
    seed_as(&backend, "SceneRegion", invite);
    for (id, region, at) in [
        (O1, PUBLIC, "2026-06-02T10:00:00Z"),
        (O2, PUBLIC, "2026-06-02T11:00:00Z"),
        (SECRET, INVITE, "2026-06-02T12:00:00Z"),
    ] {
        seed_as(
            &backend,
            "SceneObject",
            stored_object(id, region, json!([0, 0, 0]), at),
        );
    }
    for (id, region, user) in [
        (ALICE_AV, PUBLIC, ALICE),
        (BOB_AV, PUBLIC, BOB),
        (INVITE_AV, INVITE, ALICE),
    ] {
        seed_as(&backend, "SceneAvatar", avatar(id, region, user));
    }
    backend
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .expect("array")
        .iter()
        .map(|v| v.get("id").unwrap_or(v).as_str().expect("id"))
        .collect()
}

fn not_found(r: &Value) -> Vec<&str> {
    ids(&r["notFound"])
}

/// L2367: "Regardless of `accessPolicy`, the server MUST return `notFound`
/// (not `forbidden`) for `SceneRegion/get`, `SceneObject/get`, and
/// `SceneAvatar/get` requests from users who do not have access. This
/// prevents access-policy enumeration." L2362: under `"invite"` "Only
/// users explicitly granted access may enter."
#[tokio::test]
async fn get_without_region_access_is_not_found() {
    let backend = scene();
    let region = call!(
        handle_scene_region_get,
        &backend,
        BOB,
        json!({ "ids": [INVITE, MISSING] })
    );
    let object = call!(
        handle_scene_object_get,
        &backend,
        BOB,
        json!({ "ids": [SECRET, MISSING] })
    );
    let avatar = call!(
        handle_scene_avatar_get,
        &backend,
        BOB,
        json!({ "ids": [INVITE_AV, MISSING] })
    );
    // The hidden id is reported exactly like the id that never existed,
    // in request order.
    assert_eq!(region["list"], json!([]), "{region}");
    assert_eq!(not_found(&region), [INVITE, MISSING], "{region}");
    assert_eq!(object["list"], json!([]), "{object}");
    assert_eq!(not_found(&object), [SECRET, MISSING], "{object}");
    assert_eq!(avatar["list"], json!([]), "{avatar}");
    assert_eq!(not_found(&avatar), [INVITE_AV, MISSING], "{avatar}");
}

/// L2367 applies to users "who do not have access"; the region owner and
/// an invited user have it.
#[tokio::test]
async fn get_with_region_access_returns_the_records() {
    let backend = scene();
    backend.grant_region_access(INVITE, BOB);
    for user in [ALICE, BOB] {
        let region = call!(
            handle_scene_region_get,
            &backend,
            user,
            json!({ "ids": [INVITE] })
        );
        let object = call!(
            handle_scene_object_get,
            &backend,
            user,
            json!({ "ids": [SECRET] })
        );
        let avatar = call!(
            handle_scene_avatar_get,
            &backend,
            user,
            json!({ "ids": [INVITE_AV] })
        );
        assert_eq!(ids(&region["list"]), [INVITE], "{region}");
        assert_eq!(ids(&object["list"]), [SECRET], "{object}");
        assert_eq!(ids(&avatar["list"]), [INVITE_AV], "{avatar}");
    }
}

/// RFC 8620 §5.1: with `ids` `null` the server returns all records, and
/// `notFound` lists only requested ids; a hidden record is simply absent.
#[tokio::test]
async fn full_get_leaves_out_records_without_region_access() {
    let backend = scene();
    let region = call!(
        handle_scene_region_get,
        &backend,
        BOB,
        json!({ "ids": null })
    );
    assert_eq!(ids(&region["list"]), [PUBLIC], "{region}");
    assert_eq!(region["notFound"], json!([]), "{region}");
    let mut objects = ids(&call!(handle_scene_object_get, &backend, BOB, json!({}))["list"])
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    objects.sort();
    assert_eq!(objects, [O1, O2]);
}

/// draft-atwood-jmap-scene-00 at 29e9a41, L2577: "The server SHOULD NOT
/// include a SceneObject in a `SceneObject/get` response or
/// `SceneObject/query` result if the authenticated user does not have
/// access to the object's containing SceneRegion." The same holds for the
/// regions and avatars `/get` hides: "no results from the region in
/// `/query`" (L2606).
#[tokio::test]
async fn query_omits_records_without_region_access() {
    let backend = scene();
    let objects = call!(
        handle_scene_object_query,
        &backend,
        BOB,
        json!({ "filter": { "regionId": INVITE }, "calculateTotal": true })
    );
    assert_eq!(objects["ids"], json!([]), "{objects}");
    assert_eq!(objects["total"], json!(0), "{objects}");
    let avatars = call!(
        handle_scene_avatar_query,
        &backend,
        BOB,
        json!({ "filter": { "regionId": INVITE }, "calculateTotal": true })
    );
    assert_eq!(avatars["ids"], json!([]), "{avatars}");
    let regions = call!(handle_scene_region_query, &backend, BOB, json!({}));
    assert_eq!(regions["ids"], json!([PUBLIC]), "{regions}");
    // The owner sees both regions, newest first ("Default sort:
    // `createdAt` descending").
    let regions = call!(handle_scene_region_query, &backend, ALICE, json!({}));
    assert_eq!(regions["ids"], json!([INVITE, PUBLIC]), "{regions}");
}

/// L2342: "The server SHOULD apply visibility filtering to limit the
/// objects returned based on the authenticated user's avatar position
/// within the region. The specific visibility algorithm is
/// deployment-defined". L2345: "A security-conscious deployment MAY return
/// only objects within a radius of the avatar's last known position."
#[tokio::test]
async fn objects_outside_the_visibility_set_are_hidden() {
    let backend = scene();
    backend.hide_object(BOB, O1);
    let got = call!(
        handle_scene_object_get,
        &backend,
        BOB,
        json!({ "ids": [O1, O2] })
    );
    assert_eq!(ids(&got["list"]), [O2], "{got}");
    assert_eq!(not_found(&got), [O1], "{got}");
    let query = call!(
        handle_scene_object_query,
        &backend,
        BOB,
        json!({ "filter": { "regionId": PUBLIC }, "calculateTotal": true })
    );
    assert_eq!(query["ids"], json!([O2]), "{query}");
    // The visibility set is per user.
    let query = call!(
        handle_scene_object_query,
        &backend,
        ALICE,
        json!({ "filter": { "regionId": PUBLIC }, "calculateTotal": true })
    );
    assert_eq!(query["ids"], json!([O1, O2]), "{query}");
}

/// L2350: "When a client attempts to interact with an object that is no
/// longer within the client's visibility set ... the server MAY return
/// `notFound` for JMAP method calls (`SceneObject/set`, `SceneObject/get`)
/// targeting that object ... The server MUST NOT return `forbidden`, as
/// that would confirm the object still exists." L2377: "All other users
/// MUST receive `forbidden` for `SceneObject/set` updates or destroys
/// targeting objects they do not own" applies to the visible [`O2`].
#[tokio::test]
async fn set_on_a_hidden_object_is_not_found_never_forbidden() {
    let backend = scene();
    backend.hide_object(BOB, O1);
    let update = call!(ids handle_scene_object_set,
        &backend,
        BOB,
        json!({ "update": {
            O1: { "name": "Moved" },
            O2: { "name": "Moved" },
            SECRET: { "name": "Moved" }
        } })
    );
    assert_eq!(update["notUpdated"][O1]["type"], "notFound", "{update}");
    assert_eq!(update["notUpdated"][SECRET]["type"], "notFound", "{update}");
    assert_eq!(update["notUpdated"][O2]["type"], "forbidden", "{update}");
    let destroy = call!(ids handle_scene_object_set,
        &backend,
        BOB,
        json!({ "destroy": [O1, SECRET] })
    );
    assert_eq!(destroy["notDestroyed"][O1]["type"], "notFound", "{destroy}");
    assert_eq!(
        destroy["notDestroyed"][SECRET]["type"], "notFound",
        "{destroy}"
    );
}

/// SceneRegion/set Update Errors L1329: "The region exists but the caller
/// lacks access | Per the access-control enumeration rule, the server
/// returns `notFound` rather than `forbidden` to prevent probing." Destroy
/// Errors L1343: "Same enumeration-safe rule as update." L1322 "Only the
/// owner or an admin may modify region properties" and L1337 "Only the
/// region owner or an admin may destroy a region" still give `forbidden` on the
/// [`PUBLIC`] region [`BOB`] may access but does not own.
#[tokio::test]
async fn region_set_without_access_is_not_found_never_forbidden() {
    let backend = scene();
    let update = call!(ids handle_scene_region_set,
        &backend,
        BOB,
        json!({ "update": {
            INVITE: { "name": "Probe" },
            PUBLIC: { "name": "Probe" },
            MISSING: { "name": "Probe" }
        } })
    );
    assert_eq!(update["notUpdated"][INVITE]["type"], "notFound", "{update}");
    assert_eq!(
        update["notUpdated"][MISSING]["type"], "notFound",
        "{update}"
    );
    assert_eq!(
        update["notUpdated"][PUBLIC]["type"], "forbidden",
        "{update}"
    );
    let destroy = call!(ids handle_scene_region_set,
        &backend,
        BOB,
        json!({ "destroy": [INVITE, PUBLIC, MISSING] })
    );
    assert_eq!(
        destroy["notDestroyed"][INVITE]["type"], "notFound",
        "{destroy}"
    );
    assert_eq!(
        destroy["notDestroyed"][MISSING]["type"], "notFound",
        "{destroy}"
    );
    assert_eq!(
        destroy["notDestroyed"][PUBLIC]["type"], "forbidden",
        "{destroy}"
    );
}

/// SceneObject/set L1576: "The server MUST return `invalidProperties`
/// naming `regionId` when `regionId` does not exist, and SHOULD return the
/// same error (not `forbidden`) when the caller does not have access to the
/// region"; L1767 "The server returns the same error in both cases (not
/// `forbidden`)"; L2606 "`invalidProperties` naming `regionId` for a `/set`
/// create that names the region". [`BOB`] is not invited to [`INVITE`], so
/// a create there gets the same error as one naming [`MISSING`]; the same
/// create in [`PUBLIC`] succeeds.
#[tokio::test]
async fn object_create_without_region_access_is_invalid_properties() {
    let backend = scene();
    let resp = call!(ids handle_scene_object_set,
        &backend,
        BOB,
        json!({ "create": {
            "hidden": { "regionId": INVITE, "position": [0, 0, 0] },
            "missing": { "regionId": MISSING, "position": [0, 0, 0] },
            "open": { "regionId": PUBLIC, "position": [0, 0, 0] }
        } })
    );
    for key in ["hidden", "missing"] {
        let err = &resp["notCreated"][key];
        assert_eq!(err["type"], "invalidProperties", "{resp}");
        assert_eq!(err["properties"], json!(["regionId"]), "{resp}");
    }
    assert!(resp["created"]["open"]["id"].is_string(), "{resp}");
    assert!(resp["created"].get("hidden").is_none(), "{resp}");
}

/// SceneAvatar/set Update Errors L2222: "The avatar id does not exist or
/// caller lacks access to the containing region | Same enumeration-safe
/// rule." L2216 "Users MUST NOT update other users' avatars" still gives
/// `forbidden` for [`ALICE_AV`] in the [`PUBLIC`] region.
#[tokio::test]
async fn avatar_update_without_region_access_is_not_found() {
    let backend = scene();
    let update = call!(ids handle_scene_avatar_set,
        &backend,
        BOB,
        json!({ "update": {
            INVITE_AV: { "displayName": "Probe" },
            ALICE_AV: { "displayName": "Probe" },
            MISSING: { "displayName": "Probe" }
        } })
    );
    assert_eq!(
        update["notUpdated"][INVITE_AV]["type"], "notFound",
        "{update}"
    );
    assert_eq!(
        update["notUpdated"][MISSING]["type"], "notFound",
        "{update}"
    );
    assert_eq!(
        update["notUpdated"][ALICE_AV]["type"], "forbidden",
        "{update}"
    );
}

/// L2427: "SceneAvatar records belonging to a blocked ChatContact SHOULD
/// be excluded from `SceneAvatar/get` responses, `SceneAvatar/query`
/// results ... delivered to the blocking user. The blocked user's avatar
/// still exists in the region; it is simply not visible to the blocking
/// user's client." L2429: "The server MUST NOT return an error, a
/// filtered-result indicator, or any other signal that distinguishes
/// \"filtered because blocked\" from \"not present.\""
#[tokio::test]
async fn blocked_users_avatars_are_absent_for_the_blocker() {
    let backend = scene();
    backend.block(ALICE, BOB);
    let got = call!(
        handle_scene_avatar_get,
        &backend,
        ALICE,
        json!({ "ids": [ALICE_AV, BOB_AV, MISSING] })
    );
    assert_eq!(ids(&got["list"]), [ALICE_AV], "{got}");
    assert_eq!(not_found(&got), [BOB_AV, MISSING], "{got}");
    let query = call!(
        handle_scene_avatar_query,
        &backend,
        ALICE,
        json!({ "filter": { "regionId": PUBLIC }, "calculateTotal": true })
    );
    assert_eq!(query["ids"], json!([ALICE_AV]), "{query}");
    assert_eq!(query["total"], json!(1), "{query}");
}

/// L2429: "From the blocked user's perspective, the blocking user's query
/// behavior is indistinguishable from normal operation." The block hides
/// the blocked user from the blocker, not the reverse.
#[tokio::test]
async fn blocking_does_not_hide_the_blocker_from_the_blocked_user() {
    let backend = scene();
    backend.block(ALICE, BOB);
    let query = call!(
        handle_scene_avatar_query,
        &backend,
        BOB,
        json!({ "filter": { "regionId": PUBLIC }, "calculateTotal": true })
    );
    let mut seen = ids(&query["ids"]);
    seen.sort_unstable();
    assert_eq!(seen, [ALICE_AV, BOB_AV], "{query}");
}

/// draft-atwood-jmap-scene-00 at jmap-chat-spec 33702a5, L2676: the server
/// "removes any SceneAvatar records whose `userId` corresponds to a
/// ChatContact with `blocked: true` on the requesting user's contact
/// list." [`ACCOUNT`] hosts the regions and is owned by [`ALICE`]; [`BOB`]
/// reaches it as a shared account. Bob's block of Alice hides her avatar
/// from him, while Alice, the host, still sees her own.
#[tokio::test]
async fn shared_region_block_uses_the_requesters_contact_list() {
    let backend = scene();
    backend.block(BOB, ALICE);
    let got = call!(
        handle_scene_avatar_get,
        &backend,
        BOB,
        json!({ "ids": [ALICE_AV, BOB_AV] })
    );
    assert_eq!(ids(&got["list"]), [BOB_AV], "{got}");
    assert_eq!(not_found(&got), [ALICE_AV], "{got}");
    let query = call!(
        handle_scene_avatar_query,
        &backend,
        BOB,
        json!({ "filter": { "regionId": PUBLIC } })
    );
    assert_eq!(query["ids"], json!([BOB_AV]), "{query}");
    let host = call!(
        handle_scene_avatar_get,
        &backend,
        ALICE,
        json!({ "ids": [ALICE_AV, BOB_AV] })
    );
    assert_eq!(not_found(&host), Vec::<&str>::new(), "{host}");
}

/// draft-atwood-jmap-scene-00 at jmap-chat-spec 6bcd9d8, L2654: for a
/// region bound to an E2EE VTCCall, toward users who are not participants
/// "it SHOULD return `notFound` for `SceneAvatar/get`, exclude the records
/// from `SceneAvatar/query`". `hide_region_avatars` stands in for the
/// binding's "not a participant"; [`ALICE`]'s avatar in [`INVITE`] stays
/// visible, and [`BOB`], who is not hidden, still sees both.
#[tokio::test]
async fn avatars_of_a_region_hiding_them_are_absent() {
    let backend = scene();
    backend.hide_region_avatars(ALICE, PUBLIC);
    let got = call!(
        handle_scene_avatar_get,
        &backend,
        ALICE,
        json!({ "ids": [ALICE_AV, BOB_AV, INVITE_AV] })
    );
    assert_eq!(ids(&got["list"]), [INVITE_AV], "{got}");
    assert_eq!(not_found(&got), [ALICE_AV, BOB_AV], "{got}");
    let query = call!(
        handle_scene_avatar_query,
        &backend,
        ALICE,
        json!({ "filter": { "regionId": PUBLIC }, "calculateTotal": true })
    );
    assert_eq!(query["ids"], json!([]), "{query}");
    assert_eq!(query["total"], json!(0), "{query}");
    let other = call!(
        handle_scene_avatar_query,
        &backend,
        BOB,
        json!({ "filter": { "regionId": PUBLIC }, "calculateTotal": true })
    );
    assert_eq!(other["total"], json!(2), "{other}");
}

/// L2433: "Deployments SHOULD document which behavior they implement and
/// SHOULD prefer the true count, since it is consistent for all observers
/// and does not leak per-user blocking decisions." The seeded count of 2
/// includes the blocked user.
#[tokio::test]
async fn active_avatar_count_stays_the_true_count() {
    let backend = scene();
    backend.block(ALICE, BOB);
    let region = call!(
        handle_scene_region_get,
        &backend,
        ALICE,
        json!({ "ids": [PUBLIC] })
    );
    assert_eq!(region["list"][0]["activeAvatarCount"], json!(2), "{region}");
}

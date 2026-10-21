//! `SceneStreamEnable` validation, replace semantics and region scope.
//!
//! Oracle: draft-atwood-jmap-scene-wss-00 §SceneStreamEnable L472-572.
//! Frames are hand-written JSON, most taken from the draft's examples.

use jmap_scene_server::{RegionScope, SceneEphemeralSubscription, SceneStreamFilter};
use jmap_scene_types::SceneStreamEnable;
use jmap_types::Id;
use serde_json::json;

const R1: &str = "01J5ABC0000000000000000001";
const R2: &str = "01J5ABC0000000000000000002";
const R99: &str = "01J5ABC0000000000000000099";

fn enable(raw: serde_json::Value) -> SceneStreamEnable {
    serde_json::from_value(raw).unwrap()
}

fn enabled(raw: serde_json::Value) -> SceneEphemeralSubscription {
    let mut sub = SceneEphemeralSubscription::default();
    sub.replace_with(&enable(raw)).unwrap();
    sub
}

fn filter(sub: &SceneEphemeralSubscription) -> &SceneStreamFilter {
    sub.filter().expect("subscription is active")
}

fn id(s: &str) -> Id {
    Id::from(s)
}

/// L472-477: "This message REPLACES any previous `SceneStreamEnable`
/// subscription in its entirety. ... If the client had previously
/// subscribed to region `01J5ABC0000000000000000099` and now sends the
/// message above, events for region `...0099` stop immediately."
/// L488-490: "There is no partial-update mechanism".
#[test]
fn subsequent_enable_replaces_in_entirety() {
    let mut sub = enabled(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R99],
        "eventTypes": ["SceneInteractionEvent"]
    }));
    sub.replace_with(&enable(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1, R2],
        "eventTypes": null
    })))
    .unwrap();
    let f = filter(&sub);
    assert_eq!(f.region_ids(), &RegionScope::Only(vec![id(R1), id(R2)]));
    assert!(!f.region_ids().covers(&id(R99)));
    // L468-470: "Setting `eventTypes` to `null` means all three event
    // types ... are delivered", so the earlier narrowing is gone.
    for t in [
        "SceneAvatarEvent",
        "SceneObjectEvent",
        "SceneInteractionEvent",
    ] {
        assert!(f.includes(t));
    }
}

/// L492-494: "If a `regionIds` entry refers to a region in which the
/// authenticated user does not have an active avatar, that entry produces
/// no events but is retained in the subscription record." L513-516:
/// inaccessible regions "are silently ignored, not treated as errors".
#[test]
fn region_without_avatar_is_retained_not_an_error() {
    let sub = enabled(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1, R2]
    }));
    // Validation has no view of avatars; both entries stay so R2 can
    // start producing events when the user enters it (L483-484, L495-496).
    assert_eq!(
        filter(&sub).region_ids(),
        &RegionScope::Only(vec![id(R1), id(R2)])
    );
}

/// L496-499: "If all entries refer to inactive regions (or `regionIds` is
/// an empty array), the subscription is effectively empty" — accepted,
/// covering no region.
#[test]
fn empty_region_ids_is_accepted_and_covers_no_region() {
    let sub = enabled(json!({ "@type": "SceneStreamEnable", "regionIds": [] }));
    let scope = filter(&sub).region_ids();
    assert_eq!(scope, &RegionScope::Only(vec![]));
    assert!(!scope.covers(&id(R1)));
}

/// L564-566: "When `regionIds` is `null`, the subscription is dynamic: as
/// the user enters new regions or leaves existing regions, the set of
/// regions producing events adjusts automatically." L396 "Default:
/// `null`."
#[test]
fn null_or_absent_region_ids_is_dynamic() {
    for raw in [
        json!({ "@type": "SceneStreamEnable", "regionIds": null }),
        json!({ "@type": "SceneStreamEnable" }),
    ] {
        let sub = enabled(raw);
        let scope = filter(&sub).region_ids();
        assert_eq!(scope, &RegionScope::All);
        assert!(scope.covers(&id(R99)));
    }
}

/// L568-570: "When `regionIds` is an explicit list, the subscription is
/// static: only events from the listed regions are delivered."
#[test]
fn explicit_region_ids_is_static() {
    let sub = enabled(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1],
        "eventTypes": ["SceneAvatarEvent", "SceneObjectEvent"]
    }));
    let scope = filter(&sub).region_ids();
    assert!(scope.covers(&id(R1)));
    assert!(!scope.covers(&id(R2)));
}

/// L556-559: with `"eventTypes": ["SceneFooBarEvent", "SceneAvatarEvent"]`
/// "the unrecognized value would be silently ignored and the subscription
/// would proceed with `SceneAvatarEvent` only." L501-503.
#[test]
fn unrecognized_event_type_beside_recognized_is_ignored() {
    let sub = enabled(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1],
        "eventTypes": ["SceneFooBarEvent", "SceneAvatarEvent"]
    }));
    let f = filter(&sub);
    assert!(f.includes("SceneAvatarEvent"));
    assert!(!f.includes("SceneObjectEvent"));
    assert!(!f.includes("SceneInteractionEvent"));
    assert!(!f.includes("SceneFooBarEvent"));
}

/// L442-446: after `"eventTypes": ["SceneInteractionEvent"]` "the server
/// delivers only `SceneInteractionEvent` frames ... A `SceneAvatarEvent`
/// for the same region would NOT be delivered".
#[test]
fn single_event_type_filters_out_the_others() {
    let sub = enabled(json!({
        "@type": "SceneStreamEnable",
        "regionIds": [R1],
        "eventTypes": ["SceneInteractionEvent"]
    }));
    let f = filter(&sub);
    assert!(f.includes("SceneInteractionEvent"));
    assert!(!f.includes("SceneAvatarEvent"));
    assert!(!f.includes("SceneObjectEvent"));
}

/// L503-507: "If `eventTypes` is an empty array or contains only
/// unrecognized values, the server MUST respond with a `RequestError` ...
/// `\"urn:ietf:params:jmap:error:invalidArguments\"` and MUST NOT update
/// the current ephemeral subscription state." L552: "The client's previous
/// subscription (if any) remains unchanged."
#[test]
fn empty_or_only_unrecognized_event_types_is_invalid_and_keeps_state() {
    for event_types in [json!([]), json!(["SceneFooBarEvent"])] {
        let mut sub = enabled(json!({
            "@type": "SceneStreamEnable",
            "regionIds": [R99],
            "eventTypes": ["SceneObjectEvent"]
        }));
        let before = sub.clone();
        let err = sub
            .replace_with(&enable(json!({
                "@type": "SceneStreamEnable",
                "regionIds": [R1],
                "eventTypes": event_types
            })))
            .unwrap_err();
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        assert_eq!(sub, before);
    }
}

/// L503-507 applied with no prior subscription: "(if any)" (L552) — the
/// connection stays inactive.
#[test]
fn invalid_first_enable_leaves_subscription_inactive() {
    let mut sub = SceneEphemeralSubscription::default();
    assert!(sub
        .replace_with(&enable(json!({
            "@type": "SceneStreamEnable",
            "eventTypes": []
        })))
        .is_err());
    assert!(sub.filter().is_none());
}

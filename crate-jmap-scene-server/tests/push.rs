//! draft-atwood-jmap-scene-00 §Push Notifications. Every expected value
//! is quoted from the draft (jmap-chat-spec/draft-atwood-jmap-scene-00.md,
//! line numbers cited).

use std::collections::BTreeMap;

use jmap_scene_server::{push_urgency, SceneEvent, Urgency, SCENE_DATA_TYPES};
use jmap_server::state_change_for;
use jmap_types::{Id, PushSubscription, State};

/// L2473: "All four Scene data types -- SceneRegion, SceneObject,
/// SceneAvatar, and SceneAsset -- participate in the RFC 8620 state-change
/// mechanism".
#[test]
fn the_four_types_participate_in_state_change() {
    let mut types = SCENE_DATA_TYPES.to_vec();
    types.sort_unstable();
    assert_eq!(
        types,
        ["SceneAsset", "SceneAvatar", "SceneObject", "SceneRegion"]
    );
}

/// All four Scene types changed in one account, with the state strings
/// of the L2481 example.
fn all_four_changed() -> BTreeMap<Id, BTreeMap<String, State>> {
    serde_json::from_str(
        r#"{"account-xyz":{"SceneRegion":"r41a","SceneObject":"o88f2","SceneAvatar":"av03c","SceneAsset":"as7e1"}}"#,
    )
    .unwrap()
}

/// L2560 subscription `"types": ["SceneRegion", "SceneAvatar"]`; L2570:
/// "the subscription receives `StateChange` notifications only when
/// SceneRegion or SceneAvatar records change. SceneObject and SceneAsset
/// changes are excluded."
#[test]
fn subscription_types_select_scene_types() {
    let sub: PushSubscription = serde_json::from_str(
        r#"{"deviceClientId":"d1","url":"https://push.example.com/endpoint/scene-abc123","types":["SceneRegion","SceneAvatar"]}"#,
    )
    .unwrap();
    let change = state_change_for(&sub, &all_four_changed()).unwrap();
    let expected: BTreeMap<Id, BTreeMap<String, State>> =
        serde_json::from_str(r#"{"account-xyz":{"SceneRegion":"r41a","SceneAvatar":"av03c"}}"#)
            .unwrap();
    assert_eq!(change.changed, expected);

    // Only SceneObject and SceneAsset changed: nothing is sent.
    let excluded: BTreeMap<Id, BTreeMap<String, State>> =
        serde_json::from_str(r#"{"account-xyz":{"SceneObject":"o88f3","SceneAsset":"as7e2"}}"#)
            .unwrap();
    assert!(state_change_for(&sub, &excluded).is_none());
}

/// L2534 `high`: "Region destruction (`SceneRegion/set destroy`) when the
/// receiving user has SceneObject records in the destroyed region."
/// L2537 `normal`: "Avatar enter and leave events. ... Region creation and
/// configuration changes also warrant `normal` urgency when the user is an
/// active participant in the region." L2540 `low`: "Object destruction by
/// an administrator". L2543 `very-low`: "SceneAsset changes. ... SceneObject
/// position and property updates that result from periodic
/// simulation-layer reconciliation also fall into this category".
#[test]
fn recommended_urgencies() {
    let cases = [
        (SceneEvent::RegionDestroyedWithOwnObjects, "high"),
        (
            SceneEvent::AvatarEnterOrLeave {
                bound_chat_notified: false,
            },
            "normal",
        ),
        (SceneEvent::RegionChangedForParticipant, "normal"),
        (SceneEvent::ObjectDestroyedByAdmin, "low"),
        (SceneEvent::AssetChanged, "very-low"),
        (SceneEvent::PositionReconciliation, "very-low"),
    ];
    for (event, header) in cases {
        assert_eq!(event.urgency().as_str(), header, "{event:?}");
    }
}

/// L2545: "Servers MUST NOT assign `high` urgency to routine position
/// reconciliation updates."
#[test]
fn reconciliation_is_never_high() {
    assert_ne!(
        push_urgency([SceneEvent::PositionReconciliation; 3]),
        Some(Urgency::High)
    );
}

/// L2547: "When a single `StateChange` event covers multiple data types
/// with different recommended urgencies, the server SHOULD use the highest
/// applicable urgency for the combined payload."
#[test]
fn combined_payload_takes_the_highest_urgency() {
    let urgency = push_urgency([
        SceneEvent::AssetChanged,
        SceneEvent::RegionDestroyedWithOwnObjects,
        SceneEvent::ObjectDestroyedByAdmin,
    ]);
    assert_eq!(urgency.map(Urgency::as_str), Some("high"));

    let urgency = push_urgency([
        SceneEvent::PositionReconciliation,
        SceneEvent::ObjectDestroyedByAdmin,
    ]);
    assert_eq!(urgency.map(Urgency::as_str), Some("low"));

    assert_eq!(push_urgency([]), None);
}

/// L2604: "the server SHOULD NOT set `Urgency: normal` or higher on the
/// SceneAvatar `StateChange` when a Chat push for the same event has
/// already been delivered or is being delivered concurrently." L2606: "If
/// the Chat capability does not generate a notification for the join
/// event ... the Scene `StateChange` for SceneAvatar proceeds with its
/// normal urgency." L2610: "The same deduplication logic applies to avatar
/// departure."
#[test]
fn avatar_push_deduplicated_against_bound_chat_push() {
    let deduped = SceneEvent::AvatarEnterOrLeave {
        bound_chat_notified: true,
    };
    assert!(
        !["normal", "high"].contains(&deduped.urgency().as_str()),
        "{:?}",
        deduped.urgency()
    );
    let alone = SceneEvent::AvatarEnterOrLeave {
        bound_chat_notified: false,
    };
    assert_eq!(alone.urgency().as_str(), "normal");

    // L2614-2619 "MUST generate push notifications independently": an
    // independent event in the same payload is not lowered by the dedup.
    let urgency = push_urgency([deduped, SceneEvent::RegionDestroyedWithOwnObjects]);
    assert_eq!(urgency.map(Urgency::as_str), Some("high"));
}

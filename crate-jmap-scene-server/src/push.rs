//! Push support for the Scene data types (draft-atwood-jmap-scene-00
//! §Push Notifications).
//!
//! [`SCENE_DATA_TYPES`] names the four types that take part in the RFC
//! 8620 §7.1 state-change mechanism. A consumer filters each `StateChange`
//! for a subscription with [`jmap_server::state_change_for`], which applies
//! the subscription's `types`, and picks the Web Push `Urgency` header
//! (RFC 8030 §5.3) for it with [`push_urgency`].
//!
//! Coalescing rapid SceneAvatar and SceneObject mutations (§State-Change
//! Frequency, RECOMMENDED 1-5 seconds) needs a clock and a delivery queue,
//! so it is the consumer's job.

use jmap_scene_types::{SceneAsset, SceneAvatar, SceneObject, SceneRegion};

use crate::JmapObject;

/// The data types that participate in the state-change mechanism.
///
/// §State-Change Events: "All four Scene data types -- SceneRegion,
/// SceneObject, SceneAvatar, and SceneAsset -- participate in the RFC 8620
/// state-change mechanism".
pub const SCENE_DATA_TYPES: [&str; 4] = [
    SceneRegion::TYPE_NAME,
    SceneObject::TYPE_NAME,
    SceneAvatar::TYPE_NAME,
    SceneAsset::TYPE_NAME,
];

/// A Web Push `Urgency` header value (RFC 8030 §5.3), ordered from lowest
/// to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Urgency {
    /// `very-low`
    VeryLow,
    /// `low`
    Low,
    /// `normal`
    Normal,
    /// `high`
    High,
}

impl Urgency {
    /// The header value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VeryLow => "very-low",
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }
}

/// A change, as seen by the user who receives the push, for which
/// §Push Urgency recommends an urgency.
///
/// The consumer classifies each change it is about to push; only it knows
/// who receives the push and why the record changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SceneEvent {
    /// `SceneRegion/set destroy` of a region in which the receiving user
    /// had SceneObject records: `high`.
    RegionDestroyedWithOwnObjects,
    /// A user entered or left a region the receiving user is present in:
    /// `normal`, or `low` when `bound_chat_notified`.
    AvatarEnterOrLeave {
        /// A Chat push for the same join or leave in the region's bound
        /// Chat has been or is being delivered (§Interaction with Chat
        /// Push).
        bound_chat_notified: bool,
    },
    /// Region creation or a configuration change of a region the receiving
    /// user is an active participant in: `normal`.
    RegionChangedForParticipant,
    /// A region owner or deployment admin destroyed the receiving user's
    /// SceneObject: `low`.
    ObjectDestroyedByAdmin,
    /// A SceneAsset change: `very-low`.
    AssetChanged,
    /// A SceneObject or SceneAvatar position or property update from
    /// periodic simulation-layer reconciliation: `very-low`.
    PositionReconciliation,
}

impl SceneEvent {
    /// The §Push Urgency recommendation for this event.
    ///
    /// For an avatar enter or leave already announced by a Chat push,
    /// §Avatar Enter and Chat Member Join says the server "SHOULD NOT set
    /// `Urgency: normal` or higher on the SceneAvatar `StateChange`"; this
    /// returns `low`, the highest urgency that rule leaves, so the state
    /// change itself is not held back further.
    pub const fn urgency(self) -> Urgency {
        match self {
            Self::RegionDestroyedWithOwnObjects => Urgency::High,
            Self::AvatarEnterOrLeave {
                bound_chat_notified: true,
            } => Urgency::Low,
            Self::AvatarEnterOrLeave {
                bound_chat_notified: false,
            }
            | Self::RegionChangedForParticipant => Urgency::Normal,
            Self::ObjectDestroyedByAdmin => Urgency::Low,
            Self::AssetChanged | Self::PositionReconciliation => Urgency::VeryLow,
        }
    }
}

/// The urgency for one `StateChange` covering `events`: the highest of
/// their urgencies, or `None` when `events` is empty.
///
/// §Push Urgency: "When a single `StateChange` event covers multiple data
/// types with different recommended urgencies, the server SHOULD use the
/// highest applicable urgency for the combined payload."
///
/// `None` means the draft recommends nothing for the payload. A consumer
/// then omits the header, which RFC 8030 §5.3 treats as `normal`.
pub fn push_urgency(events: impl IntoIterator<Item = SceneEvent>) -> Option<Urgency> {
    events.into_iter().map(SceneEvent::urgency).max()
}

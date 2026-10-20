//! Ephemeral Scene event subscription state (draft-atwood-jmap-scene-wss-00
//! §SceneStreamEnable).
//!
//! A WebSocket transport keeps one [`SceneEphemeralSubscription`] per
//! connection, starting from [`SceneEphemeralSubscription::default`]
//! (inactive). Each `SceneStreamEnable` frame goes through
//! [`SceneEphemeralSubscription::replace_with`], which validates the frame,
//! normalizes it, and replaces the whole subscription. A
//! `SceneStreamDisable` frame resets it to the default, which "MUST
//! silently succeed if no ephemeral Scene subscription is currently
//! active" (§SceneStreamDisable).
//!
//! # Active avatars are checked at delivery
//!
//! `regionIds` is kept as sent. An entry naming a region in which the user
//! has no active avatar "produces no events but is retained in the
//! subscription record" (§SceneStreamEnable, Semantics), so whether a
//! region produces events is decided per event at delivery time, not here.
//! That lets events "begin (or resume) without a new `SceneStreamEnable`"
//! when the user enters or re-enters a listed region.

use jmap_scene_types::{SceneStreamEnable, SCENE_EVENT_TYPES};
use jmap_types::{Id, JmapError};

/// Which regions a subscription covers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionScope {
    /// The frame sent `null` or left `regionIds` out. The subscription is
    /// dynamic: every region in which the user has an active avatar,
    /// adjusting as the user enters and leaves regions (§Subscription
    /// Scope).
    All,
    /// The frame sent an explicit list, in the order sent. The
    /// subscription is static: only these regions, and of them only those
    /// with an active avatar of the user at delivery time. An empty list
    /// covers no region.
    Only(Vec<Id>),
}

impl RegionScope {
    /// Whether `region_id` is in scope. Always `true` for
    /// [`RegionScope::All`]; the active-avatar check is separate.
    pub fn covers(&self, region_id: &Id) -> bool {
        match self {
            RegionScope::All => true,
            RegionScope::Only(ids) => ids.contains(region_id),
        }
    }
}

/// The active filters of an enabled subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneStreamFilter {
    region_ids: RegionScope,
    event_types: Option<Vec<&'static str>>,
}

impl SceneStreamFilter {
    /// The regions covered.
    pub fn region_ids(&self) -> &RegionScope {
        &self.region_ids
    }

    /// Whether `event_type` (an `@type` such as `"SceneAvatarEvent"`) was
    /// requested. Always `true` when the frame's `eventTypes` was `null`.
    pub fn includes(&self, event_type: &str) -> bool {
        self.event_types
            .as_ref()
            .is_none_or(|types| types.contains(&event_type))
    }
}

/// The ephemeral Scene subscription of one WebSocket connection.
///
/// The default value is inactive, which is the state before the first
/// `SceneStreamEnable` and after a `SceneStreamDisable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneEphemeralSubscription {
    active: Option<SceneStreamFilter>,
}

impl SceneEphemeralSubscription {
    /// Replaces this subscription with the one in `enable`.
    ///
    /// The new frame "REPLACES any previous `SceneStreamEnable`
    /// subscription in its entirety"; nothing is merged from it.
    /// Unrecognized `eventTypes` values next to recognized ones are dropped
    /// (draft-atwood-jmap-scene-wss-00 §SceneStreamEnable, Semantics).
    ///
    /// # Errors
    ///
    /// Returns an `invalidArguments` [`JmapError`] when `eventTypes` is an
    /// empty array or holds only values outside [`SCENE_EVENT_TYPES`]. On
    /// error `self` is unchanged, as the draft requires. The transport
    /// sends the error as an RFC 8887 §4.3.4 `RequestError` frame whose
    /// `type` is the full URN
    /// `"urn:ietf:params:jmap:error:invalidArguments"`; the returned
    /// [`JmapError`] carries the short method-level name. The connection
    /// stays open.
    pub fn replace_with(&mut self, enable: &SceneStreamEnable) -> Result<(), JmapError> {
        let event_types = match &enable.event_types {
            None => None,
            Some(requested) => {
                let recognized: Vec<&'static str> = SCENE_EVENT_TYPES
                    .iter()
                    .copied()
                    .filter(|t| requested.iter().any(|r| r == t))
                    .collect();
                if recognized.is_empty() {
                    return Err(JmapError::invalid_arguments(
                        "eventTypes contains no recognized values",
                    ));
                }
                Some(recognized)
            }
        };
        self.active = Some(SceneStreamFilter {
            region_ids: enable
                .region_ids
                .clone()
                .map_or(RegionScope::All, RegionScope::Only),
            event_types,
        });
        Ok(())
    }

    /// The active filters, or `None` when no subscription is enabled.
    pub fn filter(&self) -> Option<&SceneStreamFilter> {
        self.active.as_ref()
    }
}

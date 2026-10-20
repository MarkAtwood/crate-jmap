//! Ephemeral VTC event subscription state (draft-atwood-jmap-vtc-wss-00
//! §VTCStreamEnable).
//!
//! A WebSocket transport keeps one [`VtcEphemeralSubscription`] per
//! connection, starting from [`VtcEphemeralSubscription::default`]
//! (inactive). Each `VTCStreamEnable` frame goes through
//! [`VtcEphemeralSubscription::replace_with`], which validates the frame,
//! normalizes it, and replaces the whole subscription. A `VTCStreamDisable`
//! frame resets it to the default.
//!
//! # Participation is checked at delivery
//!
//! `callIds` is kept as sent. An entry naming a call the user is not a
//! current participant in is "silently ignored" because delivery checks
//! participation: "Servers MUST verify authorization at the time of each
//! ephemeral event delivery, not only at the time `VTCStreamEnable` is
//! received" (draft-atwood-jmap-vtc-wss-00 §Event Authorization). The same
//! check lets events "resume without a new `VTCStreamEnable`" when the user
//! rejoins a listed call (§Subscription Scope).

use jmap_types::{Id, JmapError};
use jmap_vtc_types::{VtcStreamEnable, VTC_EVENT_TYPES};

/// Which calls a subscription covers for per-call events.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallScope {
    /// The frame sent `null` or left `callIds` out: every call in which the
    /// user is a current participant, adjusting as the user joins and
    /// leaves calls.
    All,
    /// Only the listed calls, in the order the frame sent them. An empty
    /// list covers no call.
    Only(Vec<Id>),
}

/// The active filters of an enabled subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtcStreamFilter {
    call_ids: CallScope,
    event_types: Option<Vec<&'static str>>,
}

impl VtcStreamFilter {
    /// The calls covered for per-call events.
    pub fn call_ids(&self) -> &CallScope {
        &self.call_ids
    }

    /// Whether `event_type` (an `@type` such as `"VTCRingEvent"`) was
    /// requested. Always `true` when the frame's `eventTypes` was `null`.
    pub fn includes(&self, event_type: &str) -> bool {
        self.event_types
            .as_ref()
            .is_none_or(|types| types.contains(&event_type))
    }
}

/// The ephemeral VTC subscription of one WebSocket connection.
///
/// The default value is inactive, which is the state before the first
/// `VTCStreamEnable` and after a `VTCStreamDisable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VtcEphemeralSubscription {
    active: Option<VtcStreamFilter>,
}

impl VtcEphemeralSubscription {
    /// Replaces this subscription with the one in `enable`.
    ///
    /// The new frame replaces the old subscription "in its entirety";
    /// nothing is merged from it. Unrecognized `eventTypes` values next to
    /// recognized ones are dropped (draft-atwood-jmap-vtc-wss-00
    /// §VTCStreamEnable).
    ///
    /// # Errors
    ///
    /// Returns an `invalidArguments` [`JmapError`] when `eventTypes` is an
    /// empty array or holds only values outside [`VTC_EVENT_TYPES`]. On
    /// error `self` is unchanged, as the draft requires. The transport
    /// sends the error as an RFC 8887 §4.3.4 `RequestError` frame whose
    /// `type` is the full URN
    /// `"urn:ietf:params:jmap:error:invalidArguments"`; the returned
    /// [`JmapError`] carries the short method-level name.
    pub fn replace_with(&mut self, enable: &VtcStreamEnable) -> Result<(), JmapError> {
        let event_types = match &enable.event_types {
            None => None,
            Some(requested) => {
                let recognized: Vec<&'static str> = VTC_EVENT_TYPES
                    .iter()
                    .copied()
                    .filter(|t| requested.iter().any(|r| r == t))
                    .collect();
                if recognized.is_empty() {
                    return Err(JmapError::invalid_arguments(
                        "eventTypes must contain a recognized VTC event type",
                    ));
                }
                Some(recognized)
            }
        };
        self.active = Some(VtcStreamFilter {
            call_ids: enable
                .call_ids
                .clone()
                .map_or(CallScope::All, CallScope::Only),
            event_types,
        });
        Ok(())
    }

    /// The active filters, or `None` when no subscription is enabled.
    pub fn filter(&self) -> Option<&VtcStreamFilter> {
        self.active.as_ref()
    }
}

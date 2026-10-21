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
//! Before sending an event to a connection, the transport asks
//! [`should_deliver`] whether that connection may receive it.
//!
//! # Participation is checked at delivery
//!
//! `callIds` is kept as sent. An entry naming a call the user is not a
//! current participant in "produces no events but is retained in the
//! subscription record" (draft-atwood-jmap-vtc-wss-00 §VTCStreamEnable)
//! because delivery checks participation: "Servers SHOULD verify
//! authorization at the time of each ephemeral event delivery, not only at
//! the time `VTCStreamEnable` is received" (§Event Authorization). The same
//! check lets "per-call events begin without a new `VTCStreamEnable`" when
//! the user joins a listed call, and "resume" when the user rejoins one
//! (§Subscription Scope).

use jmap_types::{Id, JmapError};
use jmap_vtc_types::{
    RingEndReason, VtcCall, VtcEvent, VtcParticipant, VtcStreamEnable, VTC_EVENT_TYPES,
};

use crate::VtcBackend;

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
    /// `"urn:ietf:params:jmap:error:invalidArguments"` and whose
    /// `requestId` is [`VtcStreamEnable::id`] when the client sent one;
    /// the returned [`JmapError`] carries the short method-level name.
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

/// Whom a per-call event's blocked-sender check names.
enum Origin<'a> {
    /// No check: `VTCRecordingStateEvent` and `VTCGatewaySignal`.
    Exempt,
    /// The user behind this VTCParticipant id in the event's call.
    Participant(&'a Id),
    /// This userId.
    User(&'a str),
}

/// Whether `event` may be delivered on a connection whose subscription is
/// `subscription`.
///
/// `caller` is the connection's one authenticated identity, the
/// recipient; every check below is scoped to it
/// (draft-atwood-jmap-vtc-wss-00 §Event Data Exposure). The transport
/// calls this at delivery time, for each event and connection, because
/// "participation, permissions, and block status can change while the
/// connection is open" (§Event Authorization). The sender is never told
/// of a suppression.
///
/// Delivery needs an active subscription whose `eventTypes` includes the
/// event's `@type`, except for `VTCRecordingStateEvent`: it "is the
/// recording consent signal ({{JMAP-VTC}}), so the server delivers it for
/// every call the subscription covers whether or not `eventTypes` lists
/// it" (§VTCStreamEnable). An event with an unrecognized `@type` is never
/// delivered. Then:
///
/// - `VTCRingEvent` and `VTCCallEndEvent` are per-user and ignore
///   `callIds` (§Subscription Scope). Both are dropped when the call's
///   initiator is blocked per [`VtcBackend::is_user_blocked`], a
///   `VTCCallEndEvent` only when its `endReason` is `"cancelled"` or
///   `"timeout"` (§Blocked-Sender Suppression). A `VTCCallEndEvent`
///   whose call the backend no longer returns has no initiator to check
///   and is delivered.
/// - Every other event is per-call. It needs a call in the
///   subscription's [`CallScope`] that the backend returns, and the
///   recipient must be a current participant in it: a VTCParticipant
///   record with the recipient's userId, `joinedAt` set and `leftAt`
///   `null` (draft-atwood-jmap-vtc-00 §VTCParticipant/query `isActive`).
/// - `VTCGatewaySignal` goes only to moderators, per
///   [`VtcBackend::is_moderator`], whose deployment policy is also the
///   place to permit broader delivery (§Event Authorization).
/// - `VTCActiveSpeakerEvent` is dropped for a call with `e2eeEnabled`
///   (§End-to-End Encrypted Calls: the server "SHOULD NOT deliver" it).
/// - `VTCParticipantEvent`, `VTCMediaStateEvent` and
///   `VTCActiveSpeakerEvent` are dropped when the named participant's
///   userId is blocked, and `VTCUnmuteRequestEvent` when its
///   `requestedBy` is (§In-Call Event Suppression). A participant without
///   a userId, or without a record in the call, is not blocked.
/// - `VTCRecordingStateEvent` is never block-suppressed: the rule "does
///   not apply to `VTCRecordingStateEvent`, the consent signal that
///   {{JMAP-VTC}} delivers to all participants regardless of block status".
///
/// `hosting_account_id` is the event's `accountId`: "the id of the call's
/// hosting account, the account that contains the VTCCall and its
/// VTCParticipant records ... This is usually not the recipient's personal
/// account" (draft-atwood-jmap-vtc-wss-00 L296-301). The call, its
/// participants, the recipient's userId and moderator status are read
/// there. `recipient_account_id` is the recipient's own account, whose
/// block list "the recipient's contact list" (L347, L360) names; every
/// [`VtcBackend::is_user_blocked`] check uses it.
///
/// This function does not rate-limit; the §Rate Limiting SHOULD rules are
/// the transport's or deployment's to apply.
///
/// # Errors
///
/// Returns the backend's error if reading the call, its participants, the
/// recipient's userId, moderator status or block status fails.
pub async fn should_deliver<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    hosting_account_id: &Id,
    recipient_account_id: &Id,
    subscription: &VtcEphemeralSubscription,
    event: &VtcEvent,
) -> Result<bool, B::Error> {
    let Some(filter) = subscription.filter() else {
        return Ok(false);
    };
    if !matches!(event, VtcEvent::RecordingState(_)) && !filter.includes(event.type_name()) {
        return Ok(false);
    }
    let (call_id, origin) = match event {
        VtcEvent::Ring(e) => {
            return Ok(!backend
                .is_user_blocked(caller, recipient_account_id, &e.initiator_id)
                .await?)
        }
        VtcEvent::CallEnd(e) => {
            if !matches!(
                e.end_reason,
                RingEndReason::Cancelled | RingEndReason::Timeout
            ) {
                return Ok(true);
            }
            return Ok(
                match fetch_call(backend, caller, hosting_account_id, &e.call_id).await? {
                    Some(call) => {
                        !backend
                            .is_user_blocked(caller, recipient_account_id, &call.initiator_id)
                            .await?
                    }
                    None => true,
                },
            );
        }
        VtcEvent::GatewaySignal(e) => (&e.call_id, Origin::Exempt),
        VtcEvent::RecordingState(e) => (&e.call_id, Origin::Exempt),
        VtcEvent::Participant(e) => (&e.call_id, Origin::Participant(&e.participant_id)),
        VtcEvent::MediaState(e) => (&e.call_id, Origin::Participant(&e.participant_id)),
        VtcEvent::ActiveSpeaker(e) => (&e.call_id, Origin::Participant(&e.participant_id)),
        VtcEvent::UnmuteRequest(e) => (&e.call_id, Origin::User(&e.requested_by)),
        _ => return Ok(false),
    };
    if let CallScope::Only(ids) = filter.call_ids() {
        if !ids.contains(call_id) {
            return Ok(false);
        }
    }
    let Some(call) = fetch_call(backend, caller, hosting_account_id, call_id).await? else {
        return Ok(false);
    };
    // ponytail: reads every VTCParticipant in the account and keeps this
    // call's; O(participants) per delivery. Upgrade: a VTCParticipant/query
    // on callId once the participant methods land (bd:JMAP-8mem.3).
    let (all, _) = backend
        .get_objects::<VtcParticipant>(caller, hosting_account_id, None, None)
        .await?;
    let in_call: Vec<VtcParticipant> = all.into_iter().filter(|p| p.call_id == *call_id).collect();
    let me = backend.caller_user_id(caller, hosting_account_id).await?;
    let participating = in_call.iter().any(|p| {
        p.user_id.as_deref() == Some(me.as_str()) && p.joined_at.is_some() && p.left_at.is_none()
    });
    if !participating {
        return Ok(false);
    }
    if matches!(event, VtcEvent::GatewaySignal(_)) {
        return backend
            .is_moderator(caller, hosting_account_id, &call)
            .await;
    }
    if matches!(event, VtcEvent::ActiveSpeaker(_)) && call.e2ee_enabled {
        return Ok(false);
    }
    let origin_user = match origin {
        Origin::Exempt => None,
        Origin::Participant(id) => in_call
            .iter()
            .find(|p| p.id == *id)
            .and_then(|p| p.user_id.as_deref()),
        Origin::User(user) => Some(user),
    };
    Ok(match origin_user {
        Some(user) => {
            !backend
                .is_user_blocked(caller, recipient_account_id, user)
                .await?
        }
        None => true,
    })
}

async fn fetch_call<B: VtcBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    call_id: &Id,
) -> Result<Option<VtcCall>, B::Error> {
    let (calls, _) = backend
        .get_objects::<VtcCall>(
            caller,
            account_id,
            Some(std::slice::from_ref(call_id)),
            None,
        )
        .await?;
    Ok(calls.into_iter().next())
}

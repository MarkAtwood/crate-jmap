//! BusyPeriod and related types for `Principal/getAvailability`.
//!
//! Normative reference: draft-ietf-jmap-calendars-34 §2.2.

use jmap_types::UTCDate;
use serde::{Deserialize, Serialize};

use crate::{CalendarEvent, IncludeInAvailability};

/// The `busyStatus` of a [`BusyPeriod`] (draft-ietf-jmap-calendars-34
/// §2.2): `String (default: "unavailable")` that "MUST be one of" the
/// three values below, so any other string fails to deserialize.
///
/// [`BusyStatus::for_event`] computes the value for a relevant event.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BusyStatus {
    /// `"confirmed"` — "The event status is "confirmed", and either the
    /// Principal is not a participant of the event, or the Principal's
    /// "participationStatus" is "accepted", or the Principal is an owner of
    /// the event and their "participationStatus" is "needs-action"."
    Confirmed,
    /// `"tentative"` — "The event status is "tentative" or the
    /// Principal's "participationStatus" is "tentative"."
    Tentative,
    /// `"unavailable"` — "The Principal is not available for scheduling
    /// at this time for any other reason." The default.
    #[default]
    Unavailable,
}

impl BusyStatus {
    /// Returns the `busyStatus` of `event` for the Principal whose calendar
    /// addresses are `principal_calendar_addresses`
    /// (draft-ietf-jmap-calendars-34 §2.2).
    ///
    /// The Principal is the first participant, in `participants` key order,
    /// whose `calendarAddress` equals one of `principal_calendar_addresses`
    /// exactly; normalise the addresses before calling if the deployment
    /// treats variants of one URI as equal. A missing event `status` is
    /// `"confirmed"` and a missing `participationStatus` is `"needs-action"`
    /// (the RFC 8984 defaults). For a recurring event, pass the expanded
    /// instance with its recurrence override applied, since an override can
    /// change both values.
    ///
    /// The result is:
    ///
    /// - [`Confirmed`](Self::Confirmed) when the event status is
    ///   `"confirmed"` and the Principal is not a participant, has accepted,
    ///   or is an owner (`roles/owner` true) still at `"needs-action"`;
    /// - otherwise [`Tentative`](Self::Tentative) when the event status or
    ///   the Principal's `participationStatus` is `"tentative"`;
    /// - otherwise [`Unavailable`](Self::Unavailable).
    ///
    /// This does not decide whether the event is relevant at all; see
    /// [`IncludeInAvailability::counts_event`].
    #[must_use]
    pub fn for_event(event: &CalendarEvent, principal_calendar_addresses: &[&str]) -> Self {
        let participant = principal_participant(event, principal_calendar_addresses);
        let event_status = event.status.as_deref().unwrap_or("confirmed");
        let confirmed_participation = participant.as_ref().is_none_or(|p| {
            p.participation_status == "accepted"
                || (p.is_owner && p.participation_status == "needs-action")
        });
        if event_status == "confirmed" && confirmed_participation {
            Self::Confirmed
        } else if event_status == "tentative"
            || participant
                .as_ref()
                .is_some_and(|p| p.participation_status == "tentative")
        {
            Self::Tentative
        } else {
            Self::Unavailable
        }
    }
}

impl IncludeInAvailability {
    /// Returns whether a calendar with this `includeInAvailability` value
    /// lets `event` count toward the availability of the Principal whose
    /// calendar addresses are `principal_calendar_addresses`, judged on the
    /// Principal's participation (draft-ietf-jmap-calendars-34 §2.2).
    ///
    /// `self` must be the Principal's own per-user value for the calendar
    /// (or the default, if the Principal never set one), not the requesting
    /// user's. The result is `false` for [`None`](Self::None); `false` when
    /// the Principal is a participant whose `participationStatus` is
    /// `"declined"` or `"delegated"`; for [`Attending`](Self::Attending),
    /// `true` only when the Principal is a participant whose status is
    /// `"accepted"` or `"tentative"`; and `true` otherwise. The Principal's
    /// participant is found as in [`BusyStatus::for_event`].
    ///
    /// The other §2.2 relevance rules (subscription, `mayReadFreeBusy`, the
    /// time window, `privacy`, `freeBusyStatus`, a `"cancelled"` status) are
    /// not checked here. An event in several calendars is relevant if every
    /// rule holds for any one of them.
    #[must_use]
    pub fn counts_event(
        &self,
        event: &CalendarEvent,
        principal_calendar_addresses: &[&str],
    ) -> bool {
        let participant = principal_participant(event, principal_calendar_addresses);
        let status = participant.as_ref().map(|p| p.participation_status);
        if matches!(status, Some("declined" | "delegated")) {
            return false;
        }
        match self {
            Self::All => true,
            Self::Attending => matches!(status, Some("accepted" | "tentative")),
            Self::None => false,
        }
    }
}

/// The Principal's entry in an event's `participants`, reduced to what the
/// §2.2 availability rules read.
struct PrincipalParticipant<'a> {
    participation_status: &'a str,
    is_owner: bool,
}

/// Finds the first participant of `event` whose `calendarAddress` is one of
/// `addresses`. `participants` is untyped JSON, so a participant that is not
/// an object or lacks a string `calendarAddress` never matches.
fn principal_participant<'a>(
    event: &'a CalendarEvent,
    addresses: &[&str],
) -> Option<PrincipalParticipant<'a>> {
    let participants = event.participants.as_ref()?.as_object()?;
    participants.values().find_map(|participant| {
        let address = participant.get("calendarAddress")?.as_str()?;
        if !addresses.contains(&address) {
            return None;
        }
        Some(PrincipalParticipant {
            participation_status: participant
                .get("participationStatus")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("needs-action"),
            is_owner: participant
                .get("roles")
                .and_then(|roles| roles.get("owner"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    })
}

/// A single busy period returned by `Principal/getAvailability`
/// (draft-ietf-jmap-calendars-34 §2.2).
///
/// The server returns an array of `BusyPeriod` objects representing time ranges
/// when the principal is occupied.  If `showDetails` was `false` in the request
/// (or the calendar has `privacy:"secret"`, or the requesting user lacks
/// `mayReadItems` rights), the `event` and `account_id` fields are `null`.
///
/// ## Field semantics
///
/// - `utc_start` / `utc_end` — inclusive/exclusive UTC date-time bounds of the
///   busy period; both are always present.
/// - `busy_status` — the kind of busy time; [`BusyStatus::Unavailable`]
///   when absent on the wire.
/// - `event` — the underlying [`crate::CalendarEvent`] if the requesting user
///   has the right to see it, otherwise `null`.
/// - `account_id` — the JMAP account id of the calendar containing this event;
///   `null` when `event` is `null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusyPeriod {
    /// UTC start of the busy period (inclusive).
    ///
    /// Uses the [`UTCDate`] newtype (RFC 8620 §1.4) rather than a bare
    /// `String` to make the wire-format constraint
    /// (20-character `YYYY-MM-DDTHH:MM:SSZ`) explicit at the type level.
    /// JSON serialization is unchanged because `UTCDate` is a transparent
    /// newtype.
    pub utc_start: UTCDate,

    /// UTC end of the busy period (exclusive).
    ///
    /// See [`utc_start`](Self::utc_start) for the typing rationale.
    pub utc_end: UTCDate,

    /// Characterisation of the busy time; an absent `busyStatus`
    /// deserializes as [`BusyStatus::Unavailable`], the draft's default.
    #[serde(default)]
    pub busy_status: BusyStatus,

    /// The underlying event, if the requester has read access.
    ///
    /// Represented as a raw JSON value because the server emits a
    /// server-side projection of the [`crate::CalendarEvent`] rather
    /// than the full object:
    ///
    /// - draft-ietf-jmap-calendars-34 §2.2 (the `Principal/getAvailability`
    ///   `eventProperties` argument) restricts the returned representation
    ///   to a caller-requested property subset.
    /// - The server also enforces privacy and `mayReadItems` permission
    ///   filtering on a per-event basis (the spec lists the conditions
    ///   under which `event` is `null`).
    ///
    /// Modelling that projection at the type level would require either
    /// a typed partial-`CalendarEvent` shape (large, tracks the spec) or
    /// `Option<crate::CalendarEvent>` where every field is `Option` by
    /// workspace convention. Using `serde_json::Value` keeps the
    /// projection nature explicit on the wire, avoids forcing
    /// availability consumers to walk the full `CalendarEvent` deserialize
    /// path, and lets callers that need typed access deserialize the
    /// value into [`crate::CalendarEvent`] on demand.
    ///
    /// (The previous version of this docstring cited a "circular type
    /// dependency" between `BusyPeriod` and `CalendarEvent`. That was
    /// inaccurate — both live in the same crate as sibling modules and
    /// neither depends on the other.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,

    /// JMAP account id of the calendar containing this event.
    ///
    /// `null` (i.e., `None`) when `event` is `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<jmap_types::Id>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ROOM: &str = "mailto:room@example.com";

    /// Builds an event with the given `status` (omitted when `None`) and two
    /// participants: `participant`, given `ROOM` as its calendar address,
    /// and an unrelated attendee who declined.
    fn event(status: Option<&str>, participant: serde_json::Value) -> CalendarEvent {
        let mut participant = participant;
        participant["calendarAddress"] = json!(ROOM);
        let mut value = json!({
            "participants": {
                "other": {
                    "calendarAddress": "mailto:other@example.com",
                    "participationStatus": "declined",
                    "roles": { "attendee": true }
                },
                "p1": participant
            }
        });
        if let Some(status) = status {
            value["status"] = json!(status);
        }
        serde_json::from_value(value).expect("test event deserializes")
    }

    /// An event with no participants and the default status is confirmed
    /// busy time, because the Principal is not a participant (-34 §2.2).
    #[test]
    fn busy_status_confirmed_when_principal_not_participant() {
        let event: CalendarEvent =
            serde_json::from_value(json!({})).expect("empty event deserializes");
        assert_eq!(
            BusyStatus::for_event(&event, &[ROOM]),
            BusyStatus::Confirmed
        );
    }

    /// A participant whose address matches none of the Principal's addresses
    /// does not stand for the Principal, even though one of them declined.
    #[test]
    fn busy_status_ignores_other_participants() {
        let event = event(
            Some("confirmed"),
            json!({ "participationStatus": "accepted" }),
        );
        assert_eq!(
            BusyStatus::for_event(&event, &["mailto:nobody@example.com"]),
            BusyStatus::Confirmed
        );
    }

    /// Each row is one branch of the -34 §2.2 busyStatus rule.
    #[test]
    fn busy_status_follows_participation() {
        let cases = [
            // accepted on a confirmed event
            (
                Some("confirmed"),
                json!({ "participationStatus": "accepted" }),
                BusyStatus::Confirmed,
            ),
            // owner who never replied, default status and participationStatus
            (
                None,
                json!({ "roles": { "owner": true } }),
                BusyStatus::Confirmed,
            ),
            // non-owner who never replied
            (
                Some("confirmed"),
                json!({ "roles": { "attendee": true } }),
                BusyStatus::Unavailable,
            ),
            // tentative reply on a confirmed event
            (
                Some("confirmed"),
                json!({ "participationStatus": "tentative" }),
                BusyStatus::Tentative,
            ),
            // accepted, but the event itself is tentative
            (
                Some("tentative"),
                json!({ "participationStatus": "accepted" }),
                BusyStatus::Tentative,
            ),
            // owner at needs-action on a tentative event
            (
                Some("tentative"),
                json!({ "roles": { "owner": true } }),
                BusyStatus::Tentative,
            ),
        ];
        for (status, participant, expected) in cases {
            let event = event(status, participant.clone());
            assert_eq!(
                BusyStatus::for_event(&event, &[ROOM]),
                expected,
                "status {status:?}, participant {participant}"
            );
        }
    }

    /// A Principal that declined or delegated the event does not count it
    /// toward availability, even with includeInAvailability "all" (-34 §2.2).
    #[test]
    fn counts_event_excludes_declined_and_delegated() {
        for status in ["declined", "delegated"] {
            let event = event(None, json!({ "participationStatus": status }));
            assert!(
                !IncludeInAvailability::All.counts_event(&event, &[ROOM]),
                "{status} must not count"
            );
        }
    }

    /// "attending" counts only events the Principal accepted or tentatively
    /// accepted; "all" also counts events without the Principal or not yet
    /// answered; "none" counts nothing.
    #[test]
    fn counts_event_by_include_in_availability() {
        let not_participant: CalendarEvent =
            serde_json::from_value(json!({})).expect("empty event deserializes");
        let accepted = event(None, json!({ "participationStatus": "accepted" }));
        let tentative = event(None, json!({ "participationStatus": "tentative" }));
        let needs_action = event(None, json!({}));

        assert!(IncludeInAvailability::All.counts_event(&not_participant, &[ROOM]));
        assert!(IncludeInAvailability::All.counts_event(&needs_action, &[ROOM]));
        assert!(IncludeInAvailability::Attending.counts_event(&accepted, &[ROOM]));
        assert!(IncludeInAvailability::Attending.counts_event(&tentative, &[ROOM]));
        assert!(!IncludeInAvailability::Attending.counts_event(&needs_action, &[ROOM]));
        assert!(!IncludeInAvailability::Attending.counts_event(&not_participant, &[ROOM]));
        assert!(!IncludeInAvailability::None.counts_event(&accepted, &[ROOM]));
    }
}

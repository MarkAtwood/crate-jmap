//! Chat-object shapes for the JMAP Chat Calendars binding
//! (draft-atwood-jmap-chat-calendars-00).
//!
//! - [`Space::calendar_id`] reads the optional `calendarId` Space
//!   extension (§calendarId). The value lives in [`Space::extra`], where
//!   the server's Space extension hook stores it.
//! - [`CalendarEventMetadata`] and [`AvailabilityMetadata`] are the typed
//!   `metadata` shapes of a [`MessageAction`](crate::MessageAction) whose
//!   `type` is [`CHAT_CAP_CALENDAR_EVENT`](crate::vocabulary::CHAT_CAP_CALENDAR_EVENT)
//!   or [`CHAT_CAP_AVAILABILITY`](crate::vocabulary::CHAT_CAP_AVAILABILITY).
//!   Cast with `serde_json::from_value` at the consumer boundary. The
//!   draft treats both `uri` and `metadata` as "peer-supplied and
//!   untrusted".
//!
//! The capability object is [`ChatCalendarsCapability`](crate::ChatCalendarsCapability).

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

use crate::Space;

/// Wire name of the Space extension property binding a Calendar
/// (draft-atwood-jmap-chat-calendars-00 §calendarId).
pub const SPACE_CALENDAR_ID_PROPERTY: &str = "calendarId";

impl Space {
    /// The `calendarId` of the Calendar bound to this Space
    /// (draft-atwood-jmap-chat-calendars-00 §calendarId), read from
    /// [`extra`](Self::extra).
    ///
    /// Returns `Ok(None)` when the property is absent or `null` ("When
    /// absent, this Space has no bound Calendar").
    ///
    /// # Errors
    ///
    /// Returns the deserialization error when the value is not a String.
    pub fn calendar_id(&self) -> Result<Option<Id>, serde_json::Error> {
        self.extra
            .get(SPACE_CALENDAR_ID_PROPERTY)
            .map_or(Ok(None), Option::<Id>::deserialize)
    }
}

/// `metadata` of a `urn:jmap:chat:cap:calendar-event` MessageAction
/// (draft-atwood-jmap-chat-calendars-00 §MessageAction format).
///
/// Every key is optional. Other keys land in [`extra`](Self::extra):
/// "Servers MAY include other type-specific metadata keys; clients MUST
/// ignore unknown keys."
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventMetadata {
    /// `startsAt` (UTCDate): event start time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<UTCDate>,
    /// `endsAt` (UTCDate): event end time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<UTCDate>,
    /// `location` (String): event location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// `participantCount` (UnsignedInt): count of participants on the
    /// underlying CalendarEvent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant_count: Option<u64>,
    /// Catch-all for other type-specific metadata keys. Preserves unknown
    /// fields across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// `metadata` of a `urn:jmap:chat:cap:availability` MessageAction
/// (draft-atwood-jmap-chat-calendars-00 §Availability Lookup).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityMetadata {
    /// `lookupWindow`: "hinting at the time range the sender expects the
    /// recipient to query".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookup_window: Option<LookupWindow>,
    /// Catch-all for other metadata keys. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The `lookupWindow` object of [`AvailabilityMetadata`]: "(Object, with
/// `startsAt` and `endsAt` fields)".
///
/// The draft does not type the two fields. They are UTCDates, as are the
/// same-named calendar-event metadata keys.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupWindow {
    /// `startsAt`: start of the hinted range.
    pub starts_at: UTCDate,
    /// `endsAt`: end of the hinted range.
    pub ends_at: UTCDate,
    /// Catch-all for vendor fields. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl LookupWindow {
    /// Builds a window from `startsAt` to `endsAt`.
    pub fn new(starts_at: UTCDate, ends_at: UTCDate) -> Self {
        Self {
            starts_at,
            ends_at,
            extra: serde_json::Map::new(),
        }
    }
}

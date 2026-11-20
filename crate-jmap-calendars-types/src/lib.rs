//! JMAP Calendars extension data types.
//!
//! Implements the data model from draft-ietf-jmap-calendars-34 and the
//! JSCalendar Event format of RFC 8984.  Types only — no method handlers,
//! no async, no network I/O.
//!
//! ## Crate family position
//!
//! ```text
//! jmap-types (RFC 8620 wire primitives)
//!     └── jmap-calendars-types  ← this crate
//!             ├── jmap-calendars-server (method handlers)
//!             └── jmap-calendars-client (client extension trait)
//! ```
//!
//! ## Modules
//!
//! | Module | Contents |
//! |---|---|
//! | `calendar` | [`Calendar`], [`CalendarRights`], [`IncludeInAvailability`] |
//! | `event` | [`CalendarEvent`], [`CalendarEventFilterCondition`], [`CalendarEventComparator`], [`CalendarEventComparatorProperty`] |
//! | `jmap-jscalendar-types` | Re-exported: [`RecurrenceRule`], [`NDay`], [`Location`], [`VirtualLocation`], [`Link`], [`Participant`], [`Alert`], [`AlertTrigger`], [`OffsetTrigger`], [`AbsoluteTrigger`], [`Relation`], [`LocalDateTime`], [`Duration`], [`SignedDuration`] |
//! | `notification` | [`CalendarEventNotification`], [`Person`], [`NotificationType`], [`NotificationFilterCondition`], [`CalendarEventNotificationComparator`], [`CalendarEventNotificationComparatorProperty`] |
//! | `participant_identity` | [`ParticipantIdentity`] |
//! | Crate root | [`CalendarsCapability`], [`CalendarsAccountCapability`], URI constants |
//!
//! The JSCalendar sub-object types (RFC 8984) live in the dedicated
//! `jmap-jscalendar-types` crate so `jmap-tasks-types` can also consume
//! them without depending on this crate. They are re-exported here at
//! the crate root (top-level names like `Location`, `Participant`,
//! `Alert`).
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`CalendarEvent`], [`Participant`], [`Location`], [`VirtualLocation`],
//! [`CalendarEventNotification`], [`Person`], [`ParticipantIdentity`], and
//! [`BusyPeriod`]). `tracing` or `log` macros that interpolate one of these
//! values with `?` (debug) or `%` (display) emit its full contents to the
//! configured sink: event titles and descriptions, participant names and
//! calendar addresses, physical locations, meeting URIs, and the owner's
//! schedule. That data is typically regulated as personal data under GDPR,
//! CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks title, attendees, locations, and meeting links.
//! tracing::debug!(event = ?event, "fetched event");
//!
//! // GOOD — emits only the opaque id.
//! tracing::debug!(event_id = ?event.id, "fetched event");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod availability;
mod backend;
mod calendar;
mod capability;
mod event;
mod notification;
mod participant_identity;

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use availability::{BusyPeriod, BusyStatus};
pub use calendar::{Calendar, CalendarRights, IncludeInAvailability};
pub use capability::{
    CalendarsAccountCapability, CalendarsCapability, CalendarsParseCapability,
    JMAP_CALENDARS_PARSE_URI, JMAP_CALENDARS_URI, JMAP_PRINCIPALS_AVAILABILITY_URI,
    PrincipalCalendarsCapability, PrincipalsAvailabilityAccountCapability,
    PrincipalsAvailabilityCapability,
};
pub use event::{
    CalendarEvent, CalendarEventComparator, CalendarEventComparatorProperty, CalendarEventFilter,
    CalendarEventFilterCondition,
};
pub use jmap_jscalendar_types::{
    AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
    OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, TimeZone, TimeZoneRule,
    VirtualLocation,
};
pub use notification::{
    CalendarAlert, CalendarEventNotification, CalendarEventNotificationComparator,
    CalendarEventNotificationComparatorProperty, NotificationFilter, NotificationFilterCondition,
    NotificationType, Person,
};
pub use participant_identity::ParticipantIdentity;

/// Generic filter algebra from `jmap-types` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-calendars-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the same root
/// re-exports in `jmap-mail-types`, the workspace canonical
/// extension-types template.
pub use jmap_types::{Filter, FilterOperator, Operator};

// ── Backend re-exports ────────────────────────────────────────────────────────

pub use backend::{
    CalendarEventNotificationProperty, CalendarEventProperty, CalendarProperty,
    PER_USER_CALENDAR_EVENT_PROPERTIES, ParticipantIdentityProperty,
    is_per_user_calendar_event_property,
};

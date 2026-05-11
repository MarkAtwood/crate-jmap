//! JMAP Calendars extension data types.
//!
//! Implements the data model from draft-ietf-jmap-calendars-26 and the
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
//! | [`calendar`] | [`Calendar`], [`CalendarRights`], [`IncludeInAvailability`], [`CalendarFilterCondition`] |
//! | [`event`] | [`CalendarEvent`], [`CalendarEventFilterCondition`], [`CalendarEventComparator`] |
//! | [`jscalendar`] | Module alias re-exporting `jmap-jscalendar-types`: [`RecurrenceRule`], [`NDay`], [`Location`], [`VirtualLocation`], [`Link`], [`Participant`], [`Alert`], [`AlertTrigger`], [`OffsetTrigger`], [`AbsoluteTrigger`], [`Relation`], [`LocalDateTime`], [`Duration`], [`SignedDuration`] |
//! | [`notification`] | [`CalendarEventNotification`], [`Person`], [`NotificationType`], [`NotificationFilterCondition`] |
//! | [`participant_identity`] | [`ParticipantIdentity`] |
//! | [`capability`] | [`CalendarsCapability`], [`CalendarsAccountCapability`], URI constants |
//!
//! The JSCalendar sub-object types (RFC 8984) live in the dedicated
//! `jmap-jscalendar-types` crate so `jmap-tasks-types` can also consume
//! them without depending on this crate. They are re-exported here both
//! at the crate root (top-level names like `Location`, `Participant`,
//! `Alert`) and via the `jscalendar` module alias for backwards
//! compatibility with any consumer that imported via the nested path.

#![forbid(unsafe_code)]

pub mod availability;
pub mod backend;
pub mod calendar;
pub mod capability;
pub mod event;
pub mod notification;
pub mod participant_identity;

/// Module alias re-exporting [`jmap_jscalendar_types`].
///
/// Preserved for backwards compatibility with consumers that imported
/// JSCalendar types via the nested path `jmap_calendars_types::jscalendar::*`
/// before the types were moved into their own crate. New code should prefer
/// the top-level re-exports (`jmap_calendars_types::Location`, etc.) or the
/// direct path `jmap_jscalendar_types::Location`.
pub use jmap_jscalendar_types as jscalendar;

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use availability::BusyPeriod;
pub use calendar::{Calendar, CalendarFilterCondition, CalendarRights, IncludeInAvailability};
pub use capability::{
    CalendarsAccountCapability, CalendarsCapability, CalendarsParseCapability,
    PrincipalCalendarsCapability, PrincipalsAvailabilityAccountCapability,
    PrincipalsAvailabilityCapability, JMAP_CALENDARS_PARSE_URI, JMAP_CALENDARS_URI,
    JMAP_PRINCIPALS_AVAILABILITY_URI,
};
pub use event::{CalendarEvent, CalendarEventComparator, CalendarEventFilterCondition};
pub use jmap_jscalendar_types::{
    AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
    OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, VirtualLocation,
};
pub use notification::{
    CalendarAlert, CalendarEventNotification, NotificationFilterCondition, NotificationType, Person,
};
pub use participant_identity::ParticipantIdentity;

// ── Backend re-exports ────────────────────────────────────────────────────────

pub use backend::{
    is_per_user_calendar_event_property, CalendarEventNotificationProperty, CalendarEventProperty,
    CalendarProperty, ParticipantIdentityProperty, PER_USER_CALENDAR_EVENT_PROPERTIES,
};

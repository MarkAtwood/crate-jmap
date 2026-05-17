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

/// Re-export module for JSCalendar sub-object types under the legacy
/// nested path `jmap_calendars_types::jscalendar::*`.
///
/// Preserved for backwards compatibility with consumers that imported
/// JSCalendar types via this path before the types were moved into the
/// shared `jmap-jscalendar-types` crate. New code should prefer the
/// top-level re-exports (`jmap_calendars_types::Location`, etc.) or the
/// direct path `jmap_jscalendar_types::Location`.
///
/// **Bounded surface.** This module re-exports exactly the same set of
/// names as the top-level re-exports below — it does NOT alias the
/// entire `jmap_jscalendar_types` crate. If a future addition to
/// `jmap_jscalendar_types` (e.g. a task-specific sub-type) should also
/// appear here, add it explicitly to both this module and the top-level
/// re-export list, with an intentional choice. See bd:JMAP-1rwf.2 for
/// the rationale.
pub mod jscalendar {
    pub use jmap_jscalendar_types::{
        AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
        OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, TimeZone,
        TimeZoneRule, VirtualLocation,
    };
}

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use availability::BusyPeriod;
pub use calendar::{
    Calendar, CalendarFilter, CalendarFilterCondition, CalendarRights, IncludeInAvailability,
};
pub use capability::{
    CalendarsAccountCapability, CalendarsCapability, CalendarsParseCapability,
    PrincipalCalendarsCapability, PrincipalsAvailabilityAccountCapability,
    PrincipalsAvailabilityCapability, JMAP_CALENDARS_PARSE_URI, JMAP_CALENDARS_URI,
    JMAP_PRINCIPALS_AVAILABILITY_URI,
};
pub use event::{
    CalendarEvent, CalendarEventComparator, CalendarEventFilter, CalendarEventFilterCondition,
};
pub use jmap_jscalendar_types::{
    AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
    OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, TimeZone, TimeZoneRule,
    VirtualLocation,
};
pub use notification::{
    CalendarAlert, CalendarEventNotification, NotificationFilter, NotificationFilterCondition,
    NotificationType, Person,
};
pub use participant_identity::ParticipantIdentity;

/// Generic filter algebra from `jmap-types::query` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-calendars-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the canonical
/// [`jmap_mail_types::query`] re-exports from the workspace canonical
/// extension-types template.
///
/// [`jmap_mail_types::query`]: https://docs.rs/jmap-mail-types/latest/jmap_mail_types/query/index.html
pub use jmap_types::query::{Filter, FilterOperator, Operator};

// ── Backend re-exports ────────────────────────────────────────────────────────

pub use backend::{
    is_per_user_calendar_event_property, CalendarEventNotificationProperty, CalendarEventProperty,
    CalendarProperty, ParticipantIdentityProperty, PER_USER_CALENDAR_EVENT_PROPERTIES,
};

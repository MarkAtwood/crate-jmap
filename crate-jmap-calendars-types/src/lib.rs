//! JMAP Calendars extension data types.
//!
//! Implements the data model from draft-ietf-jmap-calendars-33 and the
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
//! | `calendar` | [`Calendar`], [`CalendarRights`], [`IncludeInAvailability`], [`CalendarFilterCondition`] |
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

/// Generic filter algebra from `jmap-types::query` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-calendars-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the same root
/// re-exports in `jmap-mail-types`, the workspace canonical
/// extension-types template.
pub use jmap_types::query::{Filter, FilterOperator, Operator};

// ── Backend re-exports ────────────────────────────────────────────────────────

pub use backend::{
    is_per_user_calendar_event_property, CalendarEventNotificationProperty, CalendarEventProperty,
    CalendarProperty, ParticipantIdentityProperty, PER_USER_CALENDAR_EVENT_PROPERTIES,
};

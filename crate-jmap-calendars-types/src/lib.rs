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
//! | [`jscalendar`] | JSCalendar sub-object types: [`RecurrenceRule`], [`NDay`], [`Location`], [`VirtualLocation`], [`Link`], [`Participant`], [`Alert`], [`AlertTrigger`], [`OffsetTrigger`], [`AbsoluteTrigger`], [`Relation`], [`LocalDateTime`], [`Duration`], [`SignedDuration`] |
//! | [`notification`] | [`CalendarEventNotification`], [`Person`], [`NotificationType`], [`NotificationFilterCondition`] |
//! | [`participant_identity`] | [`ParticipantIdentity`] |
//! | [`capability`] | [`CalendarsCapability`], [`CalendarsAccountCapability`], URI constants |

#![forbid(unsafe_code)]

#[macro_use]
mod string_enum;

pub mod backend;
pub mod calendar;
pub mod capability;
pub mod event;
pub mod jscalendar;
pub mod notification;
pub mod participant_identity;

// ── Top-level re-exports ──────────────────────────────────────────────────────

pub use calendar::{Calendar, CalendarFilterCondition, CalendarRights, IncludeInAvailability};
pub use capability::{
    CalendarsAccountCapability, CalendarsCapability, CalendarsParseCapability,
    PrincipalsAvailabilityAccountCapability, PrincipalsAvailabilityCapability,
    JMAP_CALENDARS_PARSE_URI, JMAP_CALENDARS_URI, JMAP_PRINCIPALS_AVAILABILITY_URI,
};
pub use event::{CalendarEvent, CalendarEventComparator, CalendarEventFilterCondition};
pub use jscalendar::{
    AbsoluteTrigger, Alert, AlertTrigger, Duration, Link, LocalDateTime, Location, NDay,
    OffsetTrigger, Participant, RecurrenceRule, Relation, SignedDuration, VirtualLocation,
};
pub use notification::{
    CalendarEventNotification, NotificationFilterCondition, NotificationType, Person,
};
pub use participant_identity::ParticipantIdentity;

// ── Backend re-exports ────────────────────────────────────────────────────────

pub use backend::{
    CalendarEventNotificationProperty, CalendarEventProperty, CalendarProperty,
    ParticipantIdentityProperty,
};

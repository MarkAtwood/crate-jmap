// jmap-calendars-client — JMAP Calendars method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

/// Extension trait adding JMAP Calendars methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_calendars_client::JmapCalendarsExt;`
pub trait JmapCalendarsExt {
    // Methods will be added in implementation beads.
}

impl JmapCalendarsExt for jmap_base_client::JmapClient {}

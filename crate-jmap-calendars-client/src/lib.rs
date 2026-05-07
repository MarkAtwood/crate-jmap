// jmap-calendars-client — JMAP Calendars method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// Implements the 18-method surface described in draft-ietf-jmap-calendars-26.

#![forbid(unsafe_code)]

pub mod methods;

pub use methods::{
    CalendarEventGetParams, CalendarEventParseResponse, ChangesResponse, GetResponse,
    PrincipalGetAvailabilityResponse, QueryChangesResponse, QueryResponse, SetResponse,
};

/// Extension trait adding JMAP Calendars methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_calendars_client::JmapCalendarsExt;`
///
/// All methods require first obtaining a [`SessionClient`](methods::SessionClient) via
/// [`JmapCalendarsExt::with_calendars_session`].  The `SessionClient` binds the
/// HTTP client to a fetched JMAP session, resolving the API URL and primary
/// account id on every call.
pub trait JmapCalendarsExt {
    /// Bind this client to the given `session`, returning a [`SessionClient`](methods::SessionClient)
    /// on which all 18 JMAP Calendars methods are available.
    ///
    /// Re-create the `SessionClient` after each `fetch_session` call; a stale
    /// session will produce `unknownAccount` or similar errors.
    fn with_calendars_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapCalendarsExt for jmap_base_client::JmapClient {
    fn with_calendars_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}

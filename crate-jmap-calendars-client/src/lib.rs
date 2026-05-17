//! jmap-calendars-client — JMAP Calendars method implementations.
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! Implements the 19-method surface described in draft-ietf-jmap-calendars-26.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_calendars_client::JmapCalendarsExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_calendars_session(session);
//! // List all calendars in the primary account.
//! let calendars = sc.calendar_get(None, None).await?;
//! # let _ = calendars;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, CalendarEventGetParams, CalendarEventParseResponse, ChangesResponse, GetResponse,
    PrincipalGetAvailabilityResponse, QueryChangesResponse, QueryResponse, SessionClient, SetError,
    SetResponse,
};

/// Extension trait adding JMAP Calendars methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_calendars_client::JmapCalendarsExt;`
///
/// All methods require first obtaining a [`SessionClient`] via
/// [`JmapCalendarsExt::with_calendars_session`].  The `SessionClient` binds the
/// HTTP client to a fetched JMAP session, resolving the API URL and primary
/// account id on every call.
pub trait JmapCalendarsExt {
    /// Bind this client to the given `session`, returning a [`SessionClient`]
    /// on which all 19 JMAP Calendars methods are available.
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

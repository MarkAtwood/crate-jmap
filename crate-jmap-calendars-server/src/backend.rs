//! CalendarsBackend trait and supporting types for JMAP Calendars method handlers.
//!
//! Consumers implement [`CalendarsBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations and calendar-specific introspection are here.

pub use jmap_calendars_types::backend::{
    CalendarEventNotificationProperty, CalendarEventProperty, CalendarProperty,
    ParticipantIdentityProperty,
};
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

// ---------------------------------------------------------------------------
// CalendarsBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for JMAP Calendars method handlers (draft-ietf-jmap-calendars-26).
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are defined on the [`JmapBackend`]
/// supertrait. Only write operations and type introspection are here.
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl CalendarsBackend>` when sharing across tasks.
///
/// # Recurrence expansion contracts
///
/// ## `expandRecurrences` (draft-ietf-jmap-calendars-26 §5.11)
///
/// When a `CalendarEvent/query` request includes `"expandRecurrences": true`,
/// the backend **MUST** expand recurring events into individual instances
/// within the filter's time range (`after` / `before`).  Each expanded
/// instance is returned as a virtual object with a **synthetic id** formed by
/// appending the recurrence-id to the master event id (e.g.
/// `"<masterId>_<recurrenceId>"`).  The exact separator and encoding are
/// implementation-defined but must be stable across requests so that clients
/// can use those ids in subsequent `CalendarEvent/get` calls.
///
/// If the expansion would produce more instances than the implementation can
/// safely enumerate (e.g. an infinitely recurring event with no end date and
/// an unbounded query window), the backend SHOULD return the
/// `cannotCalculateOccurrences` error rather than truncating silently.
///
/// When `expandRecurrences` is absent or `false`, recurring events are
/// returned as a single master object and the backend MUST NOT synthesise
/// per-instance ids.
///
/// ## `recurrenceOverrides` patch passthrough (draft-ietf-jmap-calendars-26 §5.9.1)
///
/// `recurrenceOverrides` is a two-level JSON Pointer patch path:
/// the outer key is `"recurrenceOverrides"` and the inner key is a
/// `LocalDateTime` recurrence-id string (e.g. `"2025-03-05T09:00:00"`).
/// A `CalendarEvent/set` update patch may therefore contain keys of the form:
///
/// ```text
/// "recurrenceOverrides/2025-03-05T09:00:00/status"
/// ```
///
/// The handler passes these patch keys to the backend verbatim via
/// `update_object` / `update_per_user_properties`.  The backend is
/// responsible for interpreting the two-level path and merging it into the
/// stored `recurrenceOverrides` map; the handler does **not** pre-parse or
/// restructure the path.  Backends must preserve unaffected override entries
/// unchanged when applying a partial patch.
pub trait CalendarsBackend: JmapBackend {
    /// Create a new object of type `O`.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing object of type `O`.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object of type `O` by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this account supports the given JMAP object type.
    ///
    /// Called by the server consumer (e.g. session capability builder) — NOT
    /// called internally by the handler library. Backends that support all
    /// types unconditionally can return `true` always.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Returns `true` if `prop` is a per-user [`CalendarEvent`](jmap_calendars_types::CalendarEvent) property
    /// (draft-ietf-jmap-calendars-26 §5.4).
    ///
    /// Per-user properties — `keywords`, `color`, `freeBusyStatus`,
    /// `useDefaultAlerts`, and `alerts` — belong to the authenticated user and
    /// MUST NOT change the shared `updated` timestamp when patched.
    fn is_per_user_property(prop: &str) -> bool {
        matches!(
            prop,
            "keywords" | "color" | "freeBusyStatus" | "useDefaultAlerts" | "alerts"
        )
    }

    /// Apply a patch that contains only per-user [`CalendarEvent`](jmap_calendars_types::CalendarEvent) properties
    /// (draft-ietf-jmap-calendars-26 §5.4).
    ///
    /// Default implementation delegates to `update_object`. Backends serving
    /// multiple users SHOULD override this to store per-user properties
    /// separately so that shared `updated` timestamps are not affected.
    fn update_per_user_properties(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: serde_json::Value,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>>,
    > + Send {
        self.update_object::<jmap_calendars_types::CalendarEvent>(account_id, id, patch)
    }

    /// Returns `true` if the given Calendar has any events.
    ///
    /// Called by `Calendar/set` handler when `onDestroyRemoveEvents` is
    /// `false` (the default). If this returns `true`, the handler rejects the
    /// destroy with a `calendarHasEvents` error rather than forwarding to
    /// `destroy_object`.
    fn calendar_has_events(
        &self,
        account_id: &jmap_types::Id,
        calendar_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = bool> + Send;

    /// Compute `utcStart` and `utcEnd` for a [`CalendarEvent`](jmap_calendars_types::CalendarEvent) by converting the
    /// event's `start`/`duration` fields and time zone into UTC
    /// (draft-ietf-jmap-calendars-26 §5.2).
    ///
    /// Returns `(utc_start, utc_end)` as RFC 3339 strings, or `None` for each
    /// if the corresponding data is absent or the time zone is unknown.
    ///
    /// The default implementation returns `(None, None)` — backends that do not
    /// support time-zone conversion accept this behaviour and callers will omit
    /// both fields.  Backends with full tz support should override this.
    ///
    /// # Parameters
    /// - `account_id` — the account owning the event.
    /// - `event` — the event whose `start` and `duration` are to be converted.
    /// - `tz_hint` — an optional IANA time-zone override; if `None`, the event's
    ///   own `time_zone` field (if any) is used.
    fn compute_utc_times(
        &self,
        _account_id: &jmap_types::Id,
        _event: &jmap_calendars_types::CalendarEvent,
        _tz_hint: Option<&str>,
    ) -> impl std::future::Future<Output = (Option<String>, Option<String>)> + Send {
        async { (None, None) }
    }

    /// Parse calendar event blobs (draft-ietf-jmap-calendars-26 §5.13).
    ///
    /// Returns parsed events for each blob, or classifies blobs as `notFound`
    /// or `notParsable`.
    ///
    /// The default implementation puts all blobs in `not_parsable`.
    fn parse_calendar_event_blobs(
        &self,
        _account_id: &jmap_types::Id,
        blob_ids: &[jmap_types::Id],
        _properties: Option<&[String]>,
    ) -> impl std::future::Future<Output = Result<ParseResult, Self::Error>> + Send {
        let not_parsable = blob_ids.to_vec();
        async move {
            Ok(ParseResult {
                parsed: std::collections::HashMap::new(),
                not_found: vec![],
                not_parsable,
            })
        }
    }

    /// Fetch availability data for a principal (draft-ietf-jmap-calendars-26 §2.2).
    ///
    /// The default implementation returns an empty list.
    fn get_availability(
        &self,
        _account_id: &jmap_types::Id,
        _principal_id: &jmap_types::Id,
        _utc_start: &str,
        _utc_end: &str,
        _show_details: bool,
        _event_properties: Option<&[String]>,
    ) -> impl std::future::Future<
        Output = Result<Vec<jmap_calendars_types::BusyPeriod>, AvailabilityError<Self::Error>>,
    > + Send {
        async { Ok(vec![]) }
    }
}

// ---------------------------------------------------------------------------
// Supporting types for new backend methods
// ---------------------------------------------------------------------------

/// Result of a `CalendarEvent/parse` operation (draft-ietf-jmap-calendars-26 §5.13).
pub struct ParseResult {
    /// Successfully parsed: blobId → list of parsed [`CalendarEvent`](jmap_calendars_types::CalendarEvent)s.
    pub parsed: std::collections::HashMap<jmap_types::Id, Vec<jmap_calendars_types::CalendarEvent>>,
    /// Blob IDs that were not found in the blob store.
    pub not_found: Vec<jmap_types::Id>,
    /// Blob IDs that could not be parsed as iCalendar data.
    pub not_parsable: Vec<jmap_types::Id>,
}

/// Error type for [`CalendarsBackend::get_availability`] backend calls.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AvailabilityError<E: std::error::Error + 'static> {
    /// The requested principal was not found.
    #[error("principal not found")]
    NotFound,
    /// The caller is not permitted to query this principal's availability.
    #[error("forbidden")]
    Forbidden,
    /// The requested time range is too large.
    #[error("requested time range is too large")]
    TooLarge,
    /// Rate limit exceeded.
    #[error("rate limit exceeded")]
    RateLimit,
    /// An unexpected backend error.
    #[error("backend error: {0}")]
    Other(#[source] E),
}

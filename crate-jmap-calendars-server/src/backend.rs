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

    /// Compute `utcStart` and `utcEnd` for a [`CalendarEvent`] by converting the
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
}

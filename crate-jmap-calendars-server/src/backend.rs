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

// bd:JMAP-ic0j.30 — `jmap_types::PatchObject` is referenced as a fully
// qualified path in trait signatures below, NOT re-exported from this
// crate's public API. Callers needing `PatchObject` import it directly
// from `jmap_types` so a `jmap_types` major-version bump does not break
// consumers who never reference `PatchObject` through this crate's API
// surface. Matches the canonical `jmap-mail-server` pattern at
// `crate-jmap-mail-server/src/backend.rs:14-17` (no `PatchObject`
// re-export).
use jmap_types::PatchObject;

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
///
/// ## `sendSchedulingMessages` (draft-ietf-jmap-calendars-26 §5.9, §5.9.2)
///
/// `CalendarEvent/set` accepts a `sendSchedulingMessages` Boolean argument
/// (default `false`). When `true`, the backend MUST send appropriate iTIP
/// scheduling messages on success per §5.9.2. The handler parses this flag
/// and routes create/update/destroy through the dedicated
/// [`create_calendar_event`](Self::create_calendar_event),
/// [`update_calendar_event`](Self::update_calendar_event), and
/// [`destroy_calendar_event`](Self::destroy_calendar_event) methods, which
/// receive the parsed [`CalendarEventSetArgs`] alongside the object/patch.
///
/// If the backend cannot deliver to at least one recipient (no usable
/// `calendarAddress` URI), it MUST return a
/// `BackendSetError::SetError` whose type is
/// `SetErrorType::custom("noSupportedScheduleMethods")` for that operation
/// per §10.7.2; the handler surfaces this verbatim in the corresponding
/// `notCreated`/`notUpdated`/`notDestroyed` map entry.
///
/// Per-user-only updates (every patch key matches
/// [`is_per_user_calendar_event_property`](jmap_calendars_types::is_per_user_calendar_event_property))
/// bypass these methods and go through
/// [`update_per_user_properties`](Self::update_per_user_properties) instead,
/// since per-user changes do not generate iTIP REQUEST or CANCEL messages
/// (§5.9.2.1).
pub trait CalendarsBackend: JmapBackend {
    /// Create a new object of type `O`.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
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
    ///
    /// **Callers must handle the `Some` case.** When the return value is
    /// `Some(O)`, the handler should serialize the updated object and include
    /// the server-modified fields in the `updated` map of the `/set` response
    /// (RFC 8620 §5.3). Discarding the return value causes server-modified
    /// fields to be silently omitted from the response. Per-request auth
    /// context is available via the `caller` parameter, which the
    /// `register_calendars_handlers` closures forward unchanged from
    /// [`jmap_server::Dispatcher::dispatch`].
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object of type `O` by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend implementation supports the given
    /// JMAP object type.
    ///
    /// # Contract
    ///
    /// This is a **global, stateless backend-capability check** — it asks
    /// "did this implementation wire up the methods needed for type
    /// `O`?", not "does this user's account have this type enabled?".
    /// That is why the method is synchronous and takes no `caller` or
    /// `account_id` arguments. Per-account capability variation belongs
    /// in the consumer's session-capability builder (workspace AGENTS.md
    /// library-kit posture), NOT here.
    ///
    /// # Types in scope
    ///
    /// Implementors should answer for the JMAP object types this
    /// trait covers (draft-ietf-jmap-calendars-26): `Calendar`,
    /// `CalendarEvent`, `CalendarEventNotification`, and
    /// `ParticipantIdentity`. Backends MAY answer for object types
    /// from sibling extensions (`Email`, `Chat`, etc.) if they also
    /// implement those traits, but the typical pattern is one
    /// backend impl per extension family. A backend that does not
    /// recognise `O` SHOULD return `false`.
    ///
    /// # Callers
    ///
    /// As of draft-ietf-jmap-calendars-26 the calendars handler library
    /// itself does NOT consult `supports_type` — every method in
    /// [`register_calendars_handlers`](crate::register_calendars_handlers)
    /// is registered unconditionally. The intended caller is the server
    /// consumer's session-capability builder, which uses
    /// `supports_type::<Calendar>()` /
    /// `supports_type::<CalendarEventNotification>()` to decide which
    /// capability URIs to advertise. This is a deliberate divergence
    /// from canonical `jmap-mail-server`, where the handler library DOES
    /// short-circuit optional methods (notably
    /// `handle_search_snippet_get` via `supports_type::<SearchSnippet>`).
    /// A backend impl shared across both extension families should
    /// answer truthfully per-type, not paper over the difference.
    ///
    /// # Default
    ///
    /// Backends that support every type in this trait unconditionally
    /// can return `true` always.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Apply a patch that contains only per-user [`CalendarEvent`](jmap_calendars_types::CalendarEvent) properties
    /// (draft-ietf-jmap-calendars-26 §5.4).
    ///
    /// Default implementation delegates to `update_object`. Backends serving
    /// multiple users SHOULD override this to store per-user properties
    /// separately so that shared `updated` timestamps are not affected.
    ///
    /// **Callers must handle the `Some` case.** Same contract as
    /// [`update_object`](Self::update_object): when the return value is
    /// `Some(CalendarEvent)`, the handler MUST serialize the updated event
    /// and include the server-modified fields in the `updated` map of the
    /// `/set` response (RFC 8620 §5.3). Discarding the return value causes
    /// server-modified fields to be silently omitted. This is especially
    /// relevant on the per-user path because the backend often DOES modify
    /// fields the client did not patch (e.g. a per-user `alerts` map that
    /// the server normalises or attaches metadata to).
    fn update_per_user_properties(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: PatchObject,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>>,
    > + Send {
        self.update_object::<jmap_calendars_types::CalendarEvent>(caller, account_id, id, patch)
    }

    /// Create a [`CalendarEvent`](jmap_calendars_types::CalendarEvent) honouring
    /// `CalendarEvent/set` semantics (draft-ietf-jmap-calendars-26 §5.9).
    ///
    /// Receives the per-call [`CalendarEventSetArgs`] in addition to the event
    /// being created. When `args.send_scheduling_messages` is `true`, the
    /// backend MUST send appropriate iTIP REQUEST/ADD messages on success
    /// (§5.9.2.1), or return
    /// `BackendSetError::SetError(SetError::new(SetErrorType::custom("noSupportedScheduleMethods")))`
    /// when at least one recipient has no `calendarAddress` URI the server
    /// can deliver to (§10.7.2).
    ///
    /// The default implementation ignores `args` and delegates to
    /// [`create_object`](Self::create_object). Backends with iTIP delivery
    /// support MUST override this method.
    fn create_calendar_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        event: jmap_calendars_types::CalendarEvent,
        args: &CalendarEventSetArgs,
    ) -> impl std::future::Future<
        Output = Result<
            (jmap_types::Id, jmap_calendars_types::CalendarEvent),
            BackendSetError<Self::Error>,
        >,
    > + Send {
        // Default ignores scheduling args; backends with iTIP delivery override.
        let _ = args;
        self.create_object::<jmap_calendars_types::CalendarEvent>(
            caller, account_id, create_id, event,
        )
    }

    /// Apply a partial update to a
    /// [`CalendarEvent`](jmap_calendars_types::CalendarEvent) honouring
    /// `CalendarEvent/set` semantics (draft-ietf-jmap-calendars-26 §5.9).
    ///
    /// Receives the per-call [`CalendarEventSetArgs`]. When
    /// `args.send_scheduling_messages` is `true`, the backend MUST send
    /// appropriate iTIP messages on success (REQUEST for non-per-user property
    /// changes per §5.9.2.1), or return
    /// `BackendSetError::SetError(SetError::new(SetErrorType::custom("noSupportedScheduleMethods")))`
    /// when at least one recipient has no `calendarAddress` URI the server
    /// can deliver to.
    ///
    /// Per-user-only updates (every patch key matches
    /// [`is_per_user_calendar_event_property`](jmap_calendars_types::is_per_user_calendar_event_property))
    /// are routed through
    /// [`update_per_user_properties`](Self::update_per_user_properties)
    /// by the handler and never reach this method.
    ///
    /// The default implementation ignores `args` and delegates to
    /// [`update_object`](Self::update_object). Backends with iTIP delivery
    /// support MUST override this method.
    fn update_calendar_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: PatchObject,
        args: &CalendarEventSetArgs,
    ) -> impl std::future::Future<
        Output = Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>>,
    > + Send {
        let _ = args;
        self.update_object::<jmap_calendars_types::CalendarEvent>(caller, account_id, id, patch)
    }

    /// Destroy a [`CalendarEvent`](jmap_calendars_types::CalendarEvent) honouring
    /// `CalendarEvent/set` semantics (draft-ietf-jmap-calendars-26 §5.9).
    ///
    /// Receives the per-call [`CalendarEventSetArgs`]. When
    /// `args.send_scheduling_messages` is `true`, the backend MUST send
    /// appropriate iTIP CANCEL or REPLY messages on success (§5.9.2.2 /
    /// §5.9.2.4), or return
    /// `BackendSetError::SetError(SetError::new(SetErrorType::custom("noSupportedScheduleMethods")))`
    /// when at least one recipient has no `calendarAddress` URI the server
    /// can deliver to.
    ///
    /// The default implementation ignores `args` and delegates to
    /// [`destroy_object`](Self::destroy_object). Backends with iTIP delivery
    /// support MUST override this method.
    fn destroy_calendar_event(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        args: &CalendarEventSetArgs,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send {
        let _ = args;
        self.destroy_object::<jmap_calendars_types::CalendarEvent>(caller, account_id, id)
    }

    /// Returns `true` if the given Calendar has any events.
    ///
    /// Called by `Calendar/set` handler when `onDestroyRemoveEvents` is
    /// `false` (the default). If this returns `Ok(true)`, the handler
    /// rejects the destroy with a `calendarHasEvent` error rather than
    /// forwarding to `destroy_object`.
    ///
    /// # Three-way result
    ///
    /// The return type is `Result<bool, Self::Error>` to distinguish
    /// three states that callers actually need to tell apart
    /// (mirrors the canonical [`MailBackend::blob_exists`] shape):
    ///
    /// - `Ok(true)` — the calendar is definitely non-empty. The
    ///   handler rejects the destroy with `calendarHasEvent`.
    /// - `Ok(false)` — the calendar is definitely empty. The handler
    ///   forwards to `destroy_object`.
    /// - `Err(_)` — connectivity/transient failure. The handler maps
    ///   this to `serverFail` so the client knows to retry. Returning
    ///   `Ok(false)` for a transient backend failure is a bug: it
    ///   surfaces as a deterministic-looking 'no events present'
    ///   answer, the destroy proceeds, and any events that DID exist
    ///   become silently orphaned. Returning `Ok(true)` for a
    ///   transient failure is equally wrong: the client gets a
    ///   misleading `calendarHasEvent` error that hides the real
    ///   transient issue.
    ///
    /// [`MailBackend::blob_exists`]: https://docs.rs/jmap-mail-server/latest/jmap_mail_server/trait.MailBackend.html#tymethod.blob_exists
    fn calendar_has_events(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        calendar_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Compute `utcStart` and `utcEnd` for a [`CalendarEvent`](jmap_calendars_types::CalendarEvent) by converting the
    /// event's `start`/`duration` fields and time zone into UTC
    /// (draft-ietf-jmap-calendars-26 §5.2).
    ///
    /// Returns `(utc_start, utc_end)` as [`UTCDate`](jmap_types::UTCDate) values,
    /// or `None` for each if the corresponding data is absent or the time zone
    /// is unknown.
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
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _event: &jmap_calendars_types::CalendarEvent,
        _tz_hint: Option<&str>,
    ) -> impl std::future::Future<Output = (Option<jmap_types::UTCDate>, Option<jmap_types::UTCDate>)>
           + Send {
        async { (None, None) }
    }

    /// Fetch [`CalendarEvent`](jmap_calendars_types::CalendarEvent)s honouring
    /// the §5.7 extra arguments (draft-ietf-jmap-calendars-26).
    ///
    /// Receives the standard get parameters (ids, properties) alongside the
    /// parsed [`CalendarEventGetArgs`] carrying:
    ///
    /// - `recurrence_overrides_before` — when set, the backend MUST omit any
    ///   recurrence override whose recurrence id (translated into UTC) is on
    ///   or after this UTCDateTime.
    /// - `recurrence_overrides_after` — when set, the backend MUST omit any
    ///   recurrence override whose recurrence id (translated into UTC) is
    ///   before this UTCDateTime.
    /// - `reduce_participants` — when `true`, the backend MUST return only
    ///   participants with the `"owner"` role or matching the user's
    ///   ParticipantIdentities, in both the base event's `participants` and
    ///   in any recurrence override.
    /// - `time_zone` — the time zone (default `"Etc/UTC"` when `None`) used
    ///   when computing `utcStart` / `utcEnd` for floating events. The
    ///   handler also forwards this value to
    ///   [`compute_utc_times`](Self::compute_utc_times) for consistency.
    ///
    /// Returns `(found, not_found)` like
    /// [`get_objects`](JmapBackend::get_objects).
    ///
    /// The default implementation ignores `args` and delegates to
    /// `get_objects::<CalendarEvent>`. Backends that filter overrides or
    /// reduce participants MUST override this method.
    fn get_calendar_events(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        ids: Option<&[jmap_types::Id]>,
        properties: Option<&[String]>,
        args: &CalendarEventGetArgs,
    ) -> impl std::future::Future<
        Output = Result<
            (
                Vec<jmap_calendars_types::CalendarEvent>,
                Vec<jmap_types::Id>,
            ),
            Self::Error,
        >,
    > + Send {
        // Default ignores §5.7 extras; backends that filter overrides or
        // reduce participants override this method.
        let _ = args;
        self.get_objects::<jmap_calendars_types::CalendarEvent>(caller, account_id, ids, properties)
    }

    /// Run a `CalendarEvent/query` request honouring the §5.11 extra
    /// arguments (draft-ietf-jmap-calendars-26).
    ///
    /// Receives the standard query parameters (filter, sort, limit, position)
    /// alongside the parsed [`CalendarEventQueryArgs`] carrying
    /// `expandRecurrences` and `timeZone`. When
    /// `args.expand_recurrences` is `true`, the backend MUST expand recurring
    /// events into per-instance synthetic ids within the filter's
    /// `[after, before]` window (§5.11) and SHOULD use `args.time_zone` (or
    /// `Etc/UTC` when `None`) when evaluating the time-range conditions.
    ///
    /// Returns [`QueryCalendarEventsError::ExpandDurationTooLarge`] when
    /// expansion is requested and the duration between `before` and `after`
    /// exceeds the account's `maxExpandedQueryDuration` capability. Returns
    /// [`QueryCalendarEventsError::CannotCalculateOccurrences`] when a
    /// required recurrence cannot be expanded (e.g. unbounded recurrence
    /// with no end). The handler maps these to method-level
    /// `expandDurationTooLarge` / `cannotCalculateOccurrences` errors per
    /// §10.7.3 / §10.7.4.
    ///
    /// The default implementation ignores `args` and delegates to
    /// [`query_objects`](JmapBackend::query_objects). Backends that support
    /// recurrence expansion MUST override this method.
    #[allow(clippy::too_many_arguments)]
    fn query_calendar_events(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        filter: Option<&jmap_calendars_types::CalendarEventFilterCondition>,
        sort: Option<&[jmap_calendars_types::CalendarEventComparator]>,
        limit: Option<u64>,
        position: i64,
        args: &CalendarEventQueryArgs,
    ) -> impl std::future::Future<Output = Result<QueryResult, QueryCalendarEventsError<Self::Error>>>
           + Send {
        // Default ignores expandRecurrences/timeZone; backends with
        // recurrence expansion override this method.
        let _ = args;
        async move {
            self.query_objects::<jmap_calendars_types::CalendarEvent>(
                caller, account_id, filter, sort, limit, position,
            )
            .await
            .map_err(QueryCalendarEventsError::Other)
        }
    }

    /// Set the default `Calendar` for the account
    /// (draft-ietf-jmap-calendars-26 §4.3 `onSuccessSetIsDefault`).
    ///
    /// Called by `Calendar/set` after all create/update/destroy operations
    /// succeed without error. Returns a [`SetDefaultResult`] describing what
    /// the change actually accomplished:
    /// - `new_default = Some(id)` — the default is now this id (which may be
    ///   the same as `requested`, or different if the backend rewrote it).
    /// - `new_default = None` — the request was silently ignored because
    ///   the id was not found or the change was forbidden by policy
    ///   (§4.3 says no error is returned to the client in that case).
    /// - `previous_default` — the id of the calendar that WAS the default
    ///   before this call, if any. Required so the handler can mark it
    ///   `isDefault: false` in the response.
    ///
    /// `Err` is reserved for genuine backend storage failures. The handler
    /// silently swallows storage errors per §4.3 ("No error is returned to
    /// the client in this case") rather than escalating to `serverFail`.
    ///
    /// # Storage-during-lookup ambiguity (bd:JMAP-ic0j.64)
    ///
    /// A backend that experiences a transient storage failure while
    /// checking whether `calendar_id` is found / authorized cannot
    /// distinguish the spec-defined silent-ignore cases from a real
    /// outage. The trait contract is:
    ///
    /// **Return `Err` for any branch where the backend cannot complete
    /// the found / forbidden / storage-failure trichotomy.** The
    /// handler will silently swallow the `Err` per §4.3 — the user's
    /// `onSuccessSetIsDefault` intent is dropped, but no error is
    /// surfaced to the client. This is the §4.3-faithful behavior:
    /// "No error is returned to the client" is honored absolutely;
    /// the user's intent loss is the cost.
    ///
    /// Do NOT classify a transient storage failure as `Ok(default())`.
    /// That would conflate "id not found" (deterministic) with "we
    /// don't know because storage flaked" (transient), and a retry
    /// after the storage recovers would not be observable from the
    /// client side. Returning `Err` lets the backend's own
    /// observability (logs, metrics) surface the transient outage
    /// even though the wire stays §4.3-clean.
    ///
    /// The default implementation returns an empty
    /// [`SetDefaultResult`](SetDefaultResult::default), modelling a backend
    /// that does not maintain a per-account default. Backends that DO
    /// support defaults MUST override this method.
    fn set_default_calendar(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _calendar_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<SetDefaultResult, Self::Error>> + Send {
        async { Ok(SetDefaultResult::default()) }
    }

    /// Set the default `ParticipantIdentity` for the account
    /// (draft-ietf-jmap-calendars-26 §3.3 `onSuccessSetIsDefault`).
    ///
    /// Same contract as [`set_default_calendar`](Self::set_default_calendar)
    /// but for `ParticipantIdentity`. Silently ignores not-found / forbidden
    /// per §3.3.
    ///
    /// The default implementation returns an empty
    /// [`SetDefaultResult`](SetDefaultResult::default).
    fn set_default_participant_identity(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _identity_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<SetDefaultResult, Self::Error>> + Send {
        async { Ok(SetDefaultResult::default()) }
    }

    /// Parse calendar event blobs (draft-ietf-jmap-calendars-26 §5.13).
    ///
    /// Returns parsed events for each blob, or classifies blobs as `notFound`
    /// or `notParsable`.
    ///
    /// The default implementation puts all blobs in `not_parsable`.
    ///
    /// # Properties (bd:JMAP-ic0j.66)
    ///
    /// The `properties` argument selects which `CalendarEvent`
    /// properties are populated in each entry of `parsed` (RFC 8620
    /// §5.1 property projection). Backends implementing non-default
    /// behavior MUST honor this:
    ///
    /// - `None` — return all server-defined "default" properties.
    ///   The default set is the same as `CalendarEvent/get` with
    ///   `properties = null` (draft-ietf-jmap-calendars-26 §5.7).
    /// - `Some(&[])` — return only the spec-required keys (notably
    ///   `@type`, plus any property the server cannot omit while
    ///   keeping the result self-describing). Treat as "confirm
    ///   parseability without sending the body".
    /// - `Some(&[..])` — return only the named properties (plus any
    ///   the server cannot omit). Unknown property names MUST be
    ///   silently ignored per RFC 8620 §5.1; do NOT classify the
    ///   whole blob as `notParsable` because of an unknown property.
    ///
    /// Two backend impls given the same `(blob_ids, properties)`
    /// arguments SHOULD produce identical `CalendarEvent` shapes for
    /// successfully-parsed blobs; clients depend on this for
    /// deployment-portable parsing logic.
    ///
    /// # Errors (bd:JMAP-ic0j.61)
    ///
    /// Returns `Err` for genuine backend storage / runtime failures
    /// (connectivity loss, serialisation corruption, unrecoverable
    /// I/O). The handler maps `Err` to a method-level `serverFail`
    /// via [`jmap_server::server_fail_from_backend`], so the backend
    /// `Self::Error` `Display` text is NOT echoed onto the wire
    /// (see bd:JMAP-jfia.20 / .70).
    ///
    /// Malformed-but-readable blobs MUST be classified as
    /// `not_parsable`, NOT returned as `Err`. Unknown blob ids MUST
    /// be classified as `not_found`. A backend that returns `Err`
    /// for either case will surface `serverFail` to the client
    /// instead of the spec-defined parse-result buckets, and the
    /// client cannot distinguish the legitimate cases from a real
    /// outage.
    fn parse_calendar_event_blobs(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        blob_ids: &[jmap_types::Id],
        _properties: Option<&[String]>,
    ) -> impl std::future::Future<Output = Result<CalendarEventParseResult, Self::Error>> + Send
    {
        let not_parsable = blob_ids.to_vec();
        async move {
            Ok(CalendarEventParseResult {
                parsed: std::collections::HashMap::new(),
                not_found: vec![],
                not_parsable,
            })
        }
    }

    /// Fetch availability data for a principal (draft-ietf-jmap-calendars-26 §2.2).
    ///
    /// `utc_start` and `utc_end` are [`UTCDate`](jmap_types::UTCDate) values
    /// (RFC 8620 §1.4 wire form) bounding the half-open interval queried.
    ///
    /// The default implementation returns an empty list.
    #[allow(clippy::too_many_arguments)]
    fn get_availability(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
        _principal_id: &jmap_types::Id,
        _utc_start: &jmap_types::UTCDate,
        _utc_end: &jmap_types::UTCDate,
        _show_details: bool,
        _event_properties: Option<&[String]>,
    ) -> impl std::future::Future<
        Output = Result<Vec<jmap_calendars_types::BusyPeriod>, AvailabilityError<Self::Error>>,
    > + Send {
        async { Ok(vec![]) }
    }

    /// Implementation-defined per-account calendars caps for this caller and
    /// account (bd:JMAP-ic0j.31).
    ///
    /// Backends that enforce caps such as
    /// [`max_expanded_query_duration_seconds`](CalendarsLimits::max_expanded_query_duration_seconds)
    /// (draft-ietf-jmap-calendars-26 §1.5 `maxExpandedQueryDuration`)
    /// return them through this method. The default implementation returns
    /// [`CalendarsLimits::default`], which carries conservative
    /// reference-impl-grade values suitable for tests and single-tenant
    /// dev servers; production deployments are expected to override.
    ///
    /// Workspace cross-extension pattern: see `AGENTS.md` "Backend caps
    /// and limits" — every extension's backend trait exposes a
    /// `limits(caller, account_id) -> XxxLimits` sync default method
    /// returning a struct of related caps as a group. The `caller` and
    /// `account_id` arguments are plumbed even though the default impl
    /// ignores them so production backends can vary caps per-account
    /// (Free vs. Pro tier, multi-tenant SaaS, etc.) without a future API
    /// break.
    ///
    /// Consumers that wish to surface caps to clients via JMAP session
    /// capability advertisement (draft-ietf-jmap-calendars-26 §1.5
    /// `urn:ietf:params:jmap:calendars` capability fields, e.g.
    /// `maxExpandedQueryDuration`) read the value here and format it
    /// for the session response — this kit does not advertise caps on
    /// any session capability, per workspace policy that production
    /// cap-advertisement uses the JMAP Quotas extension.
    ///
    /// Enforcement of per-call rejections (such as the
    /// [`QueryCalendarEventsError::ExpandDurationTooLarge`] variant) is
    /// canonically the backend's responsibility per workspace
    /// AGENTS.md "Caller identity (foundation seam)". Handler-side
    /// pre-checks against [`Self::limits`] are defense-in-depth and
    /// optional; see bd:JMAP-ic0j.71 for the
    /// `maxExpandedQueryDuration` defense-in-depth follow-up.
    fn limits(&self, _caller: &Self::CallerCtx, _account_id: &jmap_types::Id) -> CalendarsLimits {
        CalendarsLimits::default()
    }
}

// ---------------------------------------------------------------------------
// Supporting types for new backend methods
// ---------------------------------------------------------------------------

/// Implementation-defined per-account caps that gate selected
/// `Calendar*` operations (bd:JMAP-ic0j.31).
///
/// Returned by [`CalendarsBackend::limits`]. Mirrors the workspace
/// cross-extension pattern documented in `AGENTS.md` "Backend caps and
/// limits": every extension defines its own `XxxLimits` struct
/// (`ChatLimits`, `CalendarsLimits`, …) returned via a sync default
/// method on the extension's backend trait.
///
/// The struct is `#[non_exhaustive]` so adding fields for additional
/// caps (e.g. `max_calendars_per_event` and `max_participants_per_event`,
/// both defined as capability fields in draft-ietf-jmap-calendars-26
/// §1.5 but not yet enforced) is an additive non-breaking change.
/// External callers (test backends, production backends overriding
/// [`CalendarsBackend::limits`]) construct instances via
/// [`CalendarsLimits::new`] rather than field-init syntax.
///
/// # Spec contract
///
/// `max_expanded_query_duration_seconds` corresponds to the
/// `maxExpandedQueryDuration` capability field
/// (draft-ietf-jmap-calendars-26 §1.5). The spec's wire shape is an
/// ISO 8601 [`Duration`](jmap_calendars_types::jscalendar::Duration);
/// consumers formatting the session capability convert from this
/// integer-seconds representation. The integer representation is the
/// implementation shape because backend enforcement requires arithmetic
/// (`before - after` in seconds), and the workspace's sealed dep set
/// excludes `chrono`.
///
/// # Default values
///
/// The defaults are conservative reference-impl values not tied to any
/// spec-mandated number. Production backends override the whole method,
/// not individual fields.
///
/// | Field | Default | Rationale |
/// |---|---|---|
/// | `max_expanded_query_duration_seconds` | `31_536_000` (365 days) | One calendar year is the common "what most clients reasonably ask for" range; longer windows tend to fan out recurrence expansion into prohibitive enumeration costs. |
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarsLimits {
    /// Maximum query window (in seconds) the server will honour when
    /// `CalendarEvent/query` is invoked with `expandRecurrences = true`
    /// (draft-ietf-jmap-calendars-26 §5.11, §10.7.3). Enforced against
    /// the difference between the filter's `before` and `after` values.
    /// When exceeded, the backend returns
    /// [`QueryCalendarEventsError::ExpandDurationTooLarge`] and the
    /// handler maps it to the method-level `expandDurationTooLarge`
    /// error per §10.7.3.
    pub max_expanded_query_duration_seconds: u64,
}

impl Default for CalendarsLimits {
    fn default() -> Self {
        Self {
            max_expanded_query_duration_seconds: 31_536_000,
        }
    }
}

impl CalendarsLimits {
    /// Construct a [`CalendarsLimits`] with the single cap field
    /// specified.
    ///
    /// The struct is `#[non_exhaustive]` so external callers (tests,
    /// production backends overriding [`CalendarsBackend::limits`])
    /// need a constructor to build one. Future cap-field additions to
    /// [`CalendarsLimits`] are an additive non-breaking change because
    /// the constructor stays stable; callers wanting to override a new
    /// field combine `CalendarsLimits::new(..)` with
    /// [`Self::with_max_expanded_query_duration_seconds`] and a future
    /// analogous setter.
    #[must_use]
    pub fn new(max_expanded_query_duration_seconds: u64) -> Self {
        Self {
            max_expanded_query_duration_seconds,
        }
    }

    /// Builder-style setter for [`Self::max_expanded_query_duration_seconds`].
    #[must_use]
    pub fn with_max_expanded_query_duration_seconds(mut self, max: u64) -> Self {
        self.max_expanded_query_duration_seconds = max;
        self
    }
}

/// Result of a `CalendarEvent/parse` operation (draft-ietf-jmap-calendars-26 §5.13).
///
/// Marked `#[non_exhaustive]` so future calendars-draft revisions (e.g. an
/// iCalendar parse-warnings vector or a per-blob error map) can be added
/// without a SemVer break for backends that construct the struct directly.
/// External crates must use [`CalendarEventParseResult::new`] rather than struct-literal
/// syntax. The object-prefix naming mirrors the `MdnParseResult` precedent in
/// the canonical sister crate `jmap-mail-server` (see
/// `crate-jmap-mail-server/src/mdn.rs`).
#[non_exhaustive]
#[derive(Debug)]
pub struct CalendarEventParseResult {
    /// Successfully parsed: blobId → list of parsed [`CalendarEvent`](jmap_calendars_types::CalendarEvent)s.
    pub parsed: std::collections::HashMap<jmap_types::Id, Vec<jmap_calendars_types::CalendarEvent>>,
    /// Blob IDs that were not found in the blob store.
    pub not_found: Vec<jmap_types::Id>,
    /// Blob IDs that could not be parsed as iCalendar data.
    pub not_parsable: Vec<jmap_types::Id>,
}

impl CalendarEventParseResult {
    /// Construct a `CalendarEventParseResult`.
    ///
    /// Required because the struct is `#[non_exhaustive]` — external crates
    /// cannot use struct-literal syntax.
    pub fn new(
        parsed: std::collections::HashMap<jmap_types::Id, Vec<jmap_calendars_types::CalendarEvent>>,
        not_found: Vec<jmap_types::Id>,
        not_parsable: Vec<jmap_types::Id>,
    ) -> Self {
        Self {
            parsed,
            not_found,
            not_parsable,
        }
    }
}

/// Result of [`set_default_calendar`](CalendarsBackend::set_default_calendar)
/// or [`set_default_participant_identity`](CalendarsBackend::set_default_participant_identity)
/// (draft-ietf-jmap-calendars-26 §3.3, §4.3 `onSuccessSetIsDefault`).
///
/// Both `new_default` and `previous_default` are `None` by default, modelling
/// a backend that does not maintain a notion of "the default".
///
/// Marked `#[non_exhaustive]` so future calendars-draft revisions can add
/// fields without a SemVer break for backends that construct the struct
/// directly. Use [`SetDefaultResult::default()`] and assign individual fields.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct SetDefaultResult {
    /// `Some(id)` if the default was successfully changed (or already this id).
    /// `None` if the request was silently ignored (id not found or forbidden
    /// by policy per §3.3 / §4.3) — the handler treats this as a no-op and
    /// makes no response-state changes.
    pub new_default: Option<jmap_types::Id>,
    /// The id of the previous default, if any. The handler emits an
    /// `updated.<previous_default>` entry with `isDefault: false` whenever
    /// this differs from `new_default`, so clients see the swap atomically.
    pub previous_default: Option<jmap_types::Id>,
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

/// Per-call arguments for `CalendarEvent/get` operations
/// (draft-ietf-jmap-calendars-26 §5.7).
///
/// Carries the §5.7 extras parsed from the JMAP request that need to be
/// threaded to the backend.
///
/// Marked `#[non_exhaustive]` so future calendars-draft revisions can add
/// fields without a SemVer break.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct CalendarEventGetArgs {
    /// `recurrenceOverridesBefore`: only overrides whose recurrence id
    /// (translated to UTC) is strictly before this [`UTCDate`](jmap_types::UTCDate)
    /// are returned. `None` means no upper bound on the override id. The
    /// handler validates the wire value via
    /// [`UTCDate::new_validated`](jmap_types::UTCDate::new_validated) and
    /// rejects malformed input as `invalidArguments` before the backend is
    /// called.
    pub recurrence_overrides_before: Option<jmap_types::UTCDate>,
    /// `recurrenceOverridesAfter`: only overrides whose recurrence id
    /// (translated to UTC) is on or after this [`UTCDate`](jmap_types::UTCDate)
    /// are returned. `None` means no lower bound on the override id. The
    /// handler validates the wire value via
    /// [`UTCDate::new_validated`](jmap_types::UTCDate::new_validated) and
    /// rejects malformed input as `invalidArguments` before the backend is
    /// called.
    pub recurrence_overrides_after: Option<jmap_types::UTCDate>,
    /// `reduceParticipants` (default `false`). When `true`, the backend
    /// returns only participants with the `"owner"` role or those matching
    /// the user's ParticipantIdentities.
    pub reduce_participants: bool,
    /// `timeZone` (default `"Etc/UTC"` when `None`). Used for computing
    /// `utcStart` / `utcEnd` of floating events when those properties are
    /// requested. Has no effect if the response does not include them.
    pub time_zone: Option<String>,
}

/// Per-call arguments for `CalendarEvent/query` operations
/// (draft-ietf-jmap-calendars-26 §5.11).
///
/// Carries the §5.11 extra args parsed from the JMAP request that need to be
/// threaded to the backend.
///
/// Marked `#[non_exhaustive]` so future calendars-draft revisions can add
/// fields without a SemVer break.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct CalendarEventQueryArgs {
    /// `expandRecurrences` (default `false`). When `true`, the backend
    /// expands recurring events into one synthetic id per instance within
    /// the filter's `[after, before]` window. The handler validates that
    /// `filter` is a single FilterCondition with both `before` and `after`
    /// before invoking the backend.
    pub expand_recurrences: bool,
    /// `timeZone` (default `Etc/UTC` when `None`). Time zone used by the
    /// backend when evaluating `before` / `after` conditions against
    /// floating events.
    pub time_zone: Option<String>,
}

/// Error type for [`CalendarsBackend::query_calendar_events`] backend calls
/// (draft-ietf-jmap-calendars-26 §10.7.3, §10.7.4).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum QueryCalendarEventsError<E: std::error::Error + 'static> {
    /// `expandRecurrences` was requested but `before - after` exceeds the
    /// account's `maxExpandedQueryDuration` capability (§5.11, §10.7.3).
    /// The handler maps this to a method-level `expandDurationTooLarge`
    /// error.
    #[error("query duration exceeds maxExpandedQueryDuration")]
    ExpandDurationTooLarge,
    /// The backend cannot expand a recurrence required to return results
    /// (e.g. unbounded recurrence with no end-date constraint, or a
    /// reference to a recurrence rule the backend does not understand).
    /// The handler maps this to a method-level `cannotCalculateOccurrences`
    /// error (§5.11, §10.7.4).
    #[error("server cannot expand a recurrence required by the query")]
    CannotCalculateOccurrences,
    /// An unexpected backend error.
    #[error("backend error: {0}")]
    Other(#[source] E),
}

/// Per-call arguments for `CalendarEvent/set` operations
/// (draft-ietf-jmap-calendars-26 §5.9).
///
/// Carries args parsed from the JMAP request that need to be threaded to the
/// backend on a per-create / per-update / per-destroy basis.
///
/// Marked `#[non_exhaustive]` so future calendars-draft revisions can add
/// fields without a SemVer break for backends that construct the struct
/// directly. Backends should use [`CalendarEventSetArgs::default()`] or
/// pattern-match on individual fields rather than exhaustive struct patterns.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct CalendarEventSetArgs {
    /// `sendSchedulingMessages` (draft-ietf-jmap-calendars-26 §5.9, default
    /// `false`). When `true`, the backend MUST send appropriate iTIP
    /// scheduling messages on success per §5.9.2, or return a
    /// `noSupportedScheduleMethods` SetError when at least one recipient has
    /// no `calendarAddress` URI the server can deliver to (§10.7.2).
    pub send_scheduling_messages: bool,
}

// ---------------------------------------------------------------------------
// Tests (bd:JMAP-ic0j.31)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: [`CalendarsLimits::default`] sets the reference-impl one-year
    /// `max_expanded_query_duration_seconds` (31_536_000) documented in the
    /// struct's rustdoc. Hand-computed: 365 * 24 * 60 * 60 = 31_536_000.
    /// Independent of the code under test (the formula here is the oracle,
    /// not the constant in the struct's Default impl).
    #[test]
    fn calendars_limits_default_is_one_year_in_seconds() {
        const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
        const DAYS_PER_YEAR: u64 = 365;
        let expected = DAYS_PER_YEAR * SECONDS_PER_DAY;
        assert_eq!(expected, 31_536_000, "oracle sanity");
        let limits = CalendarsLimits::default();
        assert_eq!(
            limits.max_expanded_query_duration_seconds, expected,
            "CalendarsLimits::default must advertise one year (in seconds)"
        );
    }

    /// Oracle: [`CalendarsLimits::new`] threads the provided value through to
    /// the field. A small enough custom value exercises the
    /// production-backend pattern of overriding per-account.
    #[test]
    fn calendars_limits_new_sets_max_expanded_query_duration_seconds() {
        let limits = CalendarsLimits::new(60);
        assert_eq!(limits.max_expanded_query_duration_seconds, 60);
    }

    /// Oracle: [`CalendarsLimits::with_max_expanded_query_duration_seconds`]
    /// returns a new struct with the field replaced.
    #[test]
    fn calendars_limits_builder_overrides_field() {
        let limits =
            CalendarsLimits::default().with_max_expanded_query_duration_seconds(7 * 24 * 60 * 60);
        assert_eq!(
            limits.max_expanded_query_duration_seconds,
            7 * 24 * 60 * 60,
            "builder must overwrite the default with the supplied value"
        );
    }

    /// Oracle: [`CalendarsLimits`] is `#[non_exhaustive]`, so the only
    /// stable way to construct one externally is via
    /// [`CalendarsLimits::new`] / [`CalendarsLimits::default`].
    /// This test merely compile-checks the trait derives (`Debug`,
    /// `Clone`, `Copy`, `PartialEq`, `Eq`) the rustdoc claims.
    #[test]
    fn calendars_limits_derives_compile() {
        fn assert_traits<T: std::fmt::Debug + Clone + Copy + PartialEq + Eq>() {}
        assert_traits::<CalendarsLimits>();
    }
}

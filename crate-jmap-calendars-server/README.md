# jmap-calendars-server

JMAP Calendars ([draft-ietf-jmap-calendars-26]) method handlers for Rust.
Backend-agnostic — plugs into `jmap-server::Dispatcher`. Implements all
20 Calendars method names.

Storage-agnostic — consumers implement the `CalendarsBackend` trait for their
own data layer.

## Usage

```rust
use std::sync::Arc;
use jmap_calendars_server::{CalendarsBackend, register_calendars_handlers};
use jmap_server::Dispatcher;

// 1. Implement CalendarsBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, etc. */ }
impl CalendarsBackend for MyBackend { /* ... */ }

// 2. Wire all 20 Calendars methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_calendars_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

After `register_calendars_handlers` returns, the dispatcher handles every
method name listed below. The same `Arc<MyBackend>` can be shared with other
parts of your application.

## Registered methods

All 20 method names are registered:

| Object | Methods |
|---|---|
| `Calendar` | `get`, `changes`, `set` |
| `CalendarEvent` | `get`, `changes`, `set`, `copy`, `query`, `queryChanges`, `parse` |
| `CalendarEventNotification` | `get`, `changes`, `set`, `query`, `queryChanges` |
| `ParticipantIdentity` | `get`, `changes`, `set` |
| `Principal` | `getAvailability` |

## CalendarsBackend trait

Implement this trait to connect the handlers to your storage system. The
read-side methods (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait (from
`jmap-server`). `CalendarsBackend` adds write operations and
calendar-specific operations.

```rust
pub trait CalendarsBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new object. Returns (assigned_id, created_object).
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing object.
    /// Returns Some(updated_object) if the backend modified any server-set
    /// fields beyond what the client requested (RFC 8620 §5.3 echo); None
    /// if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    /// Not called internally — used by the session capability builder.
    fn supports_type<O: JmapObject>(&self) -> bool;

    // --- Calendar-specific introspection ---

    /// Returns true if prop is a per-user CalendarEvent property (draft §5.4).
    ///
    /// Per-user properties — keywords, color, freeBusyStatus,
    /// useDefaultAlerts, and alerts — MUST NOT affect the shared `updated`
    /// timestamp when patched.
    ///
    /// Default: matches exactly those five property names.
    fn is_per_user_property(prop: &str) -> bool {
        matches!(
            prop,
            "keywords" | "color" | "freeBusyStatus" | "useDefaultAlerts" | "alerts"
        )
    }

    /// Apply a patch consisting only of per-user CalendarEvent properties
    /// (draft §5.4).
    ///
    /// Default implementation delegates to update_object. Backends serving
    /// multiple users SHOULD override this to store per-user properties
    /// separately so that the shared updated timestamp is not bumped.
    fn update_per_user_properties(
        &self,
        account_id: &Id,
        id: &Id,
        patch: serde_json::Value,
    ) -> impl Future<Output = Result<Option<CalendarEvent>, BackendSetError<Self::Error>>> + Send
    { /* delegates to update_object */ }

    /// Returns true if the given Calendar has any events.
    ///
    /// Called by Calendar/set before destroying a calendar when
    /// onDestroyRemoveEvents is false (the default). If true, the handler
    /// rejects the destroy with a calendarHasEvents SetError.
    fn calendar_has_events(
        &self,
        account_id: &Id,
        calendar_id: &Id,
    ) -> impl Future<Output = bool> + Send;

    /// Compute utcStart and utcEnd for a CalendarEvent by converting
    /// start/duration and the event's time zone into UTC (draft §5.2).
    ///
    /// Returns (utc_start, utc_end) as RFC 3339 strings, or None for each
    /// if data is absent or the time zone is unknown.
    ///
    /// Default: returns (None, None). Backends with time-zone support
    /// MUST override this.
    fn compute_utc_times(
        &self,
        account_id: &Id,
        event: &CalendarEvent,
        tz_hint: Option<&str>,
    ) -> impl Future<Output = (Option<String>, Option<String>)> + Send
    { async { (None, None) } }

    /// Parse calendar event blobs (draft §5.13 — CalendarEvent/parse).
    ///
    /// Returns successfully parsed events keyed by blobId, plus lists of
    /// not-found and not-parsable blob ids.
    ///
    /// Default: puts all blobs in not_parsable; returns no parsed events.
    fn parse_calendar_event_blobs(
        &self,
        account_id: &Id,
        blob_ids: &[Id],
        properties: Option<&[String]>,
    ) -> impl Future<Output = Result<ParseResult, Self::Error>> + Send
    { /* all blobs → not_parsable */ }

    /// Fetch availability data for a principal (draft §2.2 —
    /// Principal/getAvailability).
    ///
    /// Returns the list of BusyPeriod values within the requested UTC range.
    ///
    /// Default: returns an empty list.
    fn get_availability(
        &self,
        account_id: &Id,
        principal_id: &Id,
        utc_start: &str,
        utc_end: &str,
        show_details: bool,
        event_properties: Option<&[String]>,
    ) -> impl Future<Output = Result<Vec<BusyPeriod>, AvailabilityError<Self::Error>>> + Send
    { async { Ok(vec![]) } }
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `calendarHasEvents`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

`AvailabilityError<E>` covers `NotFound`, `Forbidden`, `TooLarge`,
`RateLimit`, and `Other(E)` — the handler maps these to the corresponding
JMAP method-level errors defined in draft §2.2.

## How it works

### Registration

`register_calendars_handlers` uses a `ClosureHandler` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<C>` and registers it with the dispatcher. One `Arc::clone` per
method name; no heap allocation per request.

### Per-user property routing

`CalendarEvent/set` inspects each update patch key. If the patch contains
exclusively per-user properties (`keywords`, `color`, `freeBusyStatus`,
`useDefaultAlerts`, `alerts`), the handler routes the patch to
`update_per_user_properties` instead of `update_object`. This preserves the
shared `updated` timestamp, which MUST NOT change when only the authenticated
user's private properties are modified (draft §5.4).

Patches that mix per-user and shared properties go through `update_object`
as a single patch.

### `CalendarEvent/set` §5.9 conflict — `utcStart`/`start` and `utcEnd`/`duration`

draft §5.9 requires that `utcStart` and `start` MUST NOT both be set in a
create or update, and similarly `utcEnd` and `duration` MUST NOT both be set.
The handler checks for these conflicts before forwarding to the backend and
returns `invalidProperties` citing both field names if detected.

### `CalendarEvent/copy` — cross-account event duplication

`CalendarEvent/copy` (draft §5.7) fetches the source event from
`fromAccountId` using `get_objects`, merges any per-entry patch overrides
from the create map, then calls `create_object` in the target account.
The `call_id` is forwarded to the handler for idempotency tracking.
If `onSuccessDestroyOriginal` is set, the handler issues a destroy of the
source event after all creates succeed.

### `CalendarEvent/get` — `utcStart`/`utcEnd` augmentation

When a client requests the `utcStart` or `utcEnd` properties,
`CalendarEvent/get` calls `compute_utc_times` for each returned event and
merges the result into the response object. If the backend returns
`(None, None)` (the default), both fields are absent from the response —
clients that request these properties will receive objects without them.

### `CalendarEvent/parse` — blob parsing delegation

`CalendarEvent/parse` (draft §5.13) passes the blob id list directly to
`parse_calendar_event_blobs`. The handler maps the `ParseResult` into the
wire response (`parsed`, `notFound`, `notParsable`). No blob-level logic
is performed by the handler itself.

### `Principal/getAvailability` — free/busy query delegation

`Principal/getAvailability` (draft §2.2) extracts `id`, `utcStart`,
`utcEnd`, `showDetails`, and `eventProperties` from the request and
delegates to `get_availability`. The handler maps `AvailabilityError`
variants to the appropriate JMAP method-level errors.

## Known Limitations

- **`compute_utc_times` default returns `(None, None)`** — `utcStart` and
  `utcEnd` will be absent from all `CalendarEvent/get` responses unless the
  backend overrides this method with a real timezone conversion implementation.
  There is no fallback computation in the handler.

- **`parse_calendar_event_blobs` default puts all blobs in `notParsable`** —
  `CalendarEvent/parse` will always report parse failure unless the backend
  overrides this method with an iCalendar parser.

- **`get_availability` default returns an empty list** —
  `Principal/getAvailability` always returns no busy periods unless the
  backend overrides this method with real free/busy data.

- **`CallerCtx` is not forwarded.** `register_calendars_handlers` discards
  the `CallerCtx` value from each dispatch. Handler closures receive only
  `(Arc<B>, call_id, args)`; the `caller: C` value is not forwarded. If
  per-request context (auth identity, tenant id, rate-limit token) is needed,
  implement `JmapHandler<C>` directly and register with
  `dispatcher.register(method_name, Arc::new(your_handler))`.

- **`expandRecurrences` query argument** — the handler passes this flag to
  `query_objects` as part of the filter, but recurring event expansion is
  entirely the backend's responsibility. The handler performs no expansion
  itself. Backends that do not support expansion SHOULD return
  `cannotCalculateOccurrences` rather than silently truncating.

## Capability URIs

Include these in your Session object's `capabilities` map:

```rust
pub const CAPABILITY_CALENDARS: &str = "urn:ietf:params:jmap:calendars";
// Also from jmap-calendars-types:
pub const JMAP_CALENDARS_URI: &str         = "urn:ietf:params:jmap:calendars";
pub const JMAP_CALENDARS_PARSE_URI: &str   = "urn:ietf:params:jmap:calendars:parse";
pub const JMAP_PRINCIPALS_AVAILABILITY_URI: &str
                                           = "urn:ietf:params:jmap:principals:availability";
```

Which URIs to advertise depends on which capabilities your backend supports.
Use `CalendarsBackend::supports_type::<O>()` to check at session-build time.

## Crate family

```
jmap-types
    ├── jmap-server              Dispatcher this plugs into
    └── jmap-calendars-types     domain types (Calendar, CalendarEvent, etc.)
            └── jmap-calendars-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[draft-ietf-jmap-calendars-26]** — JMAP Calendars binding (normative for
  all method semantics, error codes, and capability definitions)
- **[RFC 8984]** — JSCalendar Event format (normative for CalendarEvent
  content: JSCalendar properties, recurrence, alerts, participants)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference,
  `/set` response shape, `/copy` semantics)

[draft-ietf-jmap-calendars-26]: https://www.ietf.org/archive/id/draft-ietf-jmap-calendars-26.txt
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0

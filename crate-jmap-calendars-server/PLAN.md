# jmap-calendars-server — Implementation Plan

draft-ietf-jmap-calendars-26 (JMAP Calendars) method handlers. Plugs into
`jmap-server`'s `Dispatcher`. Backend-agnostic: defines a `CalendarsBackend`
trait; consumers provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server               dispatcher
    └── jmap-calendars-types      data types
            └── jmap-calendars-server  ← this crate
```

## What This Crate Is

Method handler implementations for every JMAP Calendars method defined in
draft-ietf-jmap-calendars-26: Calendar, CalendarEvent, CalendarEventNotification,
ParticipantIdentity.

Defines a `CalendarsBackend` trait (supertrait of `JmapBackend`) that the
application implements. The crate handles all JMAP protocol semantics (ordering,
partial success, resultReference threading, error type mapping, per-user
property isolation, recurrence override patch semantics). The backend handles
storage.

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage (SQLite, PostgreSQL, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not handling iCalendar parsing or recurrence expansion — backend's
  responsibility where those are needed
- Not handling iTIP scheduling messages — backend's responsibility
- Not axum-specific — any `http`-based framework works

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt` —
read the relevant section before implementing each handler. Wire field names
and method semantics come from the spec, not from memory.

`~/PROJECT/jmap-chat-spec/references/rfc8984.txt` — JSCalendar format. Read
before implementing CalendarEvent/set patch semantics and recurrenceOverrides
handling.

`~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol. §5.3 for
set semantics and PatchObject application. §5.4 for copy semantics. §5.5 for
query. §5.6 for queryChanges.

### Backend trait pattern — copy this

`~/PROJECT/JMAP/crate-jmap-mail-server/src/backend.rs` —
`MailBackend` is the exact structural pattern to follow for `CalendarsBackend`.
Copy the supertrait structure, error types, and AFIT method signatures. The
`JmapBackend` supertrait supplies `get_objects`, `get_state`, `get_changes`,
`query_objects`, `query_changes`. Only write operations and calendar-specific
operations go in `CalendarsBackend`.

### Handler logic reference — read, do not copy

No single production Rust reference implementation is available for JMAP
Calendars comparable to the Stalwart server for mail. Use the spec as the
sole authoritative source. The Stalwart mail handler code at
`~/GIT/stalwart-jmap-server/` is useful for understanding general JMAP handler
patterns (error wiring, partial success, ResultReference), but contains no
calendars code.

## Capability URI

`urn:ietf:params:jmap:calendars` — registered in IANA per draft §10.1.

Two additional capability URIs defined by the draft but handled at the server
consumer level (not registered here):
- `urn:ietf:params:jmap:principals:availability` — for Principal/getAvailability
- `urn:ietf:params:jmap:calendars:parse` — for CalendarEvent/parse

## Method Coverage

Total: 18 method registrations (plus 2 optional).

| Object | Methods | Draft §§ | Backend path |
|---|---|---|---|
| Calendar | get, changes, set | §4.1, §4.2, §4.3 | standard CRUD |
| Calendar | query, queryChanges | (implied §4, RFC 8620 §5.5–5.6) | standard query |
| CalendarEvent | get | §5.7 | standard get + 4 extra args |
| CalendarEvent | changes | §5.8 | standard changes |
| CalendarEvent | set | §5.9 | standard set + sendSchedulingMessages |
| CalendarEvent | copy | §5.10 | `copy_event` |
| CalendarEvent | query | §5.11 | standard query + expandRecurrences, timeZone |
| CalendarEvent | queryChanges | §5.12 | standard queryChanges |
| CalendarEvent | parse | §5.13 | `parse_event` (optional, `parse` feature) |
| CalendarEventNotification | get | §7.1 | standard get |
| CalendarEventNotification | changes | §7.2 | standard changes |
| CalendarEventNotification | set | §7.3 | destroy-only; handler enforces |
| CalendarEventNotification | query | §7.4 | standard query |
| CalendarEventNotification | queryChanges | §7.5 | standard queryChanges |
| ParticipantIdentity | get | §3.1 | standard get |
| ParticipantIdentity | changes | §3.2 | standard changes |
| ParticipantIdentity | set | §3.3 | standard set + onSuccessSetIsDefault |

## Key Design Decisions

### 1. CalendarsBackend follows MailBackend exactly for generic CRUD

Same AFIT pattern (`async fn` in trait, stable since Rust 1.75), same
`BackendChangesError`/`BackendSetError` error types, same
`ChangesResult`/`QueryResult`/`QueryChangesResult` structs from
`jmap-server`. Importers of `jmap-calendars-server` who have already
implemented `MailBackend` will find the contract structurally identical.

`CalendarsBackend` is NOT object-safe (generic methods). The dispatcher
and all handlers are generic over `B: CalendarsBackend`, monomorphized at
compile time. No `#[async_trait]` macro needed.

### 2. Per-user calendar properties require special backend handling

Draft §4.3 specifies that the following Calendar properties are always stored
per-user, even for shared calendars: `name`, `color`, `sortOrder`, `isVisible`,
`timeZone`, `includeInAvailability`, `defaultAlertsWithTime`,
`defaultAlertsWithoutTime`.

This means `get_objects<Calendar>` and `update_object<Calendar>` must receive
the calling `account_id` and behave differently for the owner vs. a subscriber.
The handler passes account_id to every backend call (per the standard CRUD
interface). Backends are responsible for maintaining separate per-user copies
of these properties. The handler does not implement this logic.

### 3. CalendarEvent/set patch semantics for recurrenceOverrides

Draft §5.9.1 specifies a two-level patch structure:

1. The `/set` `update` argument uses standard JMAP PatchObject semantics
   (RFC 8620 §5.3): paths like `recurrenceOverrides/2025-03-05T09:00:00/start`
   are applied to the stored CalendarEvent object.

2. Within `recurrenceOverrides`, the values are themselves PatchObjects applied
   to the base event to generate the override. These inner PatchObjects are
   stored as-is; they are NOT expanded by the handler.

The handler applies the outer JMAP patch (RFC 8620 §5.3 path semantics with
`~0`/`~1` escaping) to the stored CalendarEvent JSON before passing the updated
object to the backend. The backend stores the patched result. The inner
recurrenceOverrides PatchObject values are opaque to the handler.

**Important edge case**: A path like
`recurrenceOverrides/2025-03-05T09:00:00/participants~1{id}~1participationStatus`
modifies a key within the PatchObject stored at that recurrenceOverride, not
a path within a resolved occurrence. This distinction must be implemented
exactly per draft §5.9.1 examples (Figures 1–6).

### 4. CalendarEvent/set server-set fields on create

Per draft §5.9, on create the server MUST set:
- `@type = "Event"` if omitted
- `uid` = new UUID if omitted
- `created` = current UTC datetime if omitted
- `updated` = current UTC datetime (if isOrigin; overrides any client value)

The handler sets these before calling `create_object<CalendarEvent>`.

### 5. Calendar/set onDestroyRemoveEvents handled in the handler layer

Draft §4.3: when `onDestroyRemoveEvents` is true in the destroy arguments, the
handler must remove all CalendarEvents from the calendar before destroying it.
Events that are in no other calendar after removal must be destroyed outright.
The handler calls `query_objects<CalendarEvent>` (filter by calendarId), then
for each event:
- if it is in multiple calendars: `update_object<CalendarEvent>` removing this
  calendarId
- if it is the event's only calendar: `destroy_object<CalendarEvent>`
- then `destroy_object<Calendar>`

When `onDestroyRemoveEvents` is false (default): if the calendar contains any
events, the destroy MUST fail with `calendarHasEvent` SetError. The handler
calls `query_objects<CalendarEvent>` to check.

This is all handler logic. The backend has no `onDestroyRemoveEvents` concept.

### 6. Calendar/set onSuccessSetIsDefault

After a successful set (all creates/updates/destroys succeeded), if
`onSuccessSetIsDefault` is a valid Calendar id, the handler calls
`set_default_calendar` on the backend. If the id is not found or the
operation fails for policy reasons, it is silently ignored. The changed
calendar object (with updated `isDefault: true`) must appear in the response.

ParticipantIdentity/set has the same pattern via `onSuccessSetIsDefault`
(draft §3.3). The backend method for this is `set_default_participant_identity`.

### 7. CalendarEvent/get extra arguments

Draft §5.7 defines four extra arguments beyond standard `/get`:
- `recurrenceOverridesBefore`: UTCDateTime|null — filter overrides by date
- `recurrenceOverridesAfter`: UTCDateTime|null — filter overrides by date
- `reduceParticipants`: Boolean — omit non-owner, non-user participants
- `timeZone`: TimeZoneId — for utcStart/utcEnd calculation of floating events

The handler passes these as a `CalendarEventGetArgs` struct to the backend
method `get_calendar_events`. The backend is responsible for applying these
filters and computing utcStart/utcEnd when requested.

### 8. CalendarEvent/query expandRecurrences

Draft §5.11: when `expandRecurrences` is true, the server expands recurring
events and returns synthetic ids for each instance within the filter's
before/after window. If the window exceeds `maxExpandedQueryDuration`, the
handler returns `expandDurationTooLarge`. If the backend cannot expand a
recurrence, it returns `cannotCalculateOccurrences`.

Both error codes are new JMAP error codes registered by the draft (§10.7).
The handler maps backend-returned errors to these.

The synthetic ids (representing base-event-id + recurrence-id pairs) are
generated by the backend. The handler does not know their structure.

### 9. CalendarEventNotification is server-generated, destroy-only

Draft §7: CalendarEventNotification objects are created only by the server.
The handler enforces this by rejecting any create or update attempt in
`CalendarEventNotification/set` with a `forbidden` SetError. Only destroy
is allowed.

CalendarEventNotification is not created via the standard `create_object`
backend method from client calls. Backends create notifications internally
when processing CalendarEvent mutations. The handler has no path for
client-initiated notification creation.

### 10. CalendarEvent/copy is a cross-account operation

Draft §5.10 references RFC 8620 §5.4 copy semantics. The handler calls
a dedicated `copy_event` backend method that takes `from_account_id`,
`event_id`, `to_account_id`, and `calendar_ids`. This is analogous to
`Email/copy` in `jmap-mail-server`.

### 11. CalendarEvent/parse is feature-gated

`CalendarEvent/parse` (draft §5.13) requires iCalendar parsing. It is gated
behind a `parse` Cargo feature flag. When the feature is enabled, the handler
is registered and calls a `parse_event` backend method. When disabled, the
method is not registered.

Capability `urn:ietf:params:jmap:calendars:parse` must only be advertised in
the session when the `parse` feature is enabled and the backend supports it.

### 12. Floating events and UTC time handling

JSCalendar "floating" events have no `timeZone` property. Their `utcStart` and
`utcEnd` are computed using the `timeZone` argument to `CalendarEvent/get`
(default "Etc/UTC") or the Calendar's `timeZone` property.

The handler does NOT compute utcStart/utcEnd. That computation requires
timezone database access, which belongs in the backend. The handler passes
the requested `timeZone` argument to the backend, which performs the conversion
and returns the computed values when `utcStart`/`utcEnd` are in the requested
properties list.

## Planned Public API

```rust
/// Storage backend for JMAP Calendars method handlers.
///
/// Read-side operations (get_objects, get_state, get_changes,
/// query_objects, query_changes) are inherited from JmapBackend.
///
/// Uses AFIT (async fn in trait, stable since Rust 1.75). Not object-safe;
/// always monomorphized at compile time.
///
/// Implementor invariants:
/// 1. State monotonicity: state token changes after every mutation.
/// 2. Initial state: "0" is the valid initial state sentinel.
/// 3. Per-user calendar properties: get/update for Calendar types must
///    be scoped to account_id for per-user fields (§4.3).
/// 4. Partial set success: per-object failures do not roll back other
///    objects in the same /set call (RFC 8620 §5.3).
/// 5. Notifications: backends create CalendarEventNotifications internally
///    when other users modify shared calendar events. The handler does not.
#[allow(async_fn_in_trait)]
pub trait CalendarsBackend: JmapBackend {
    // ── Write operations (mirrors MailBackend) ──────────────────────────────

    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // ── Calendars-specific ──────────────────────────────────────────────────

    /// CalendarEvent/get with extra draft §5.7 arguments.
    /// Returns (found, not_found). Backend applies recurrenceOverrides
    /// window filters and computes utcStart/utcEnd for floating events
    /// using timeZone when those properties are requested.
    fn get_calendar_events(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        args: &CalendarEventGetArgs,
    ) -> impl Future<Output = Result<(Vec<CalendarEvent>, Vec<Id>), Self::Error>> + Send;

    /// CalendarEvent/copy (RFC 8620 §5.4): copy an event into another account.
    /// Returns the new id and the created CalendarEvent in to_account_id.
    fn copy_event(
        &self,
        from_account_id: &Id,
        event_id: &Id,
        to_account_id: &Id,
        calendar_ids: &[Id],
    ) -> impl Future<Output = Result<(Id, CalendarEvent), BackendSetError<Self::Error>>> + Send;

    /// CalendarEvent/parse (draft §5.13): parse iCalendar blob(s).
    /// Called only when the `parse` feature is enabled.
    /// Returns (parsed, not_found, not_parsable).
    #[cfg(feature = "parse")]
    fn parse_event(
        &self,
        account_id: &Id,
        blob_ids: &[Id],
        properties: Option<&[&str]>,
    ) -> impl Future<Output = Result<ParseEventResult, Self::Error>> + Send;

    /// Set the default calendar for an account (Calendar/set onSuccessSetIsDefault).
    /// If id is not found or the operation is not permitted, silently ignore.
    fn set_default_calendar(
        &self,
        account_id: &Id,
        calendar_id: &Id,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Set the default participant identity (ParticipantIdentity/set onSuccessSetIsDefault).
    fn set_default_participant_identity(
        &self,
        account_id: &Id,
        identity_id: &Id,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Returns true if this backend supports CalendarEvent/parse.
    fn supports_parse(&self) -> bool { false }
}

/// Extra arguments for CalendarEvent/get (draft §5.7).
pub struct CalendarEventGetArgs {
    pub recurrence_overrides_before: Option<String>, // UTCDateTime
    pub recurrence_overrides_after: Option<String>,  // UTCDateTime
    pub reduce_participants: bool,
    pub time_zone: Option<String>,                   // TimeZoneId
}

/// Result of CalendarEvent/parse (draft §5.13).
pub struct ParseEventResult {
    pub parsed: HashMap<Id, Vec<CalendarEvent>>,
    pub not_found: Vec<Id>,
    pub not_parsable: Vec<Id>,
}

/// Register all JMAP Calendars handlers with a jmap-server Dispatcher.
///
/// After calling this, the dispatcher handles all 18 JMAP Calendars method
/// names (plus CalendarEvent/parse if the `parse` feature is enabled and
/// backend.supports_parse() returns true).
pub fn register_calendars_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: CalendarsBackend + 'static,
    C: Clone + Send + 'static;

pub use backend::{
    BackendChangesError, BackendSetError,
    ChangesResult, QueryResult, QueryChangesResult, AddedItem,
    CalendarEventGetArgs, ParseEventResult,
};
```

## Module Layout

```
src/
  lib.rs              re-exports; register_calendars_handlers
  backend.rs          CalendarsBackend trait; CalendarEventGetArgs;
                      ParseEventResult; BackendChangesError, BackendSetError,
                      ChangesResult, QueryResult, QueryChangesResult
  calendar.rs         Calendar/get, /changes, /set (onDestroyRemoveEvents +
                      onSuccessSetIsDefault), /query, /queryChanges
  event.rs            CalendarEvent/get (extra args), /changes, /set
                      (recurrenceOverrides patch + sendSchedulingMessages),
                      /copy, /query (expandRecurrences), /queryChanges
  event_parse.rs      CalendarEvent/parse (feature-gated)
  notification.rs     CalendarEventNotification/get, /changes, /set
                      (destroy-only enforcement), /query, /queryChanges
  participant.rs      ParticipantIdentity/get, /changes, /set
                      (onSuccessSetIsDefault)
  error.rs            CalendarSetError (calendarHasEvent, noSupportedScheduleMethods,
                      expandDurationTooLarge, cannotCalculateOccurrences)
```

## Test Strategy

A `MemoryBackend` in `tests/common/mod.rs` provides an in-memory `HashMap`
implementation of `CalendarsBackend`. This serves as both the test harness and
the canonical example for implementors.

Test files per object group:

```
tests/
  common/
    mod.rs               MemoryBackend implementation
  calendar_tests.rs
  event_tests.rs
  notification_tests.rs
  participant_tests.rs
```

Test oracles come from draft-ietf-jmap-calendars-26 §8 example JSON (the spec
includes full request/response pairs). Extract them verbatim from the spec and
hardcode as `serde_json::json!({...})` literals. Never derive expected values
from the implementation under test.

Each test calls `register_calendars_handlers` with the `MemoryBackend`,
constructs a `JmapRequest` matching the spec example, calls
`Dispatcher::dispatch`, and asserts the response matches the spec example
response.

### Non-trivial test cases to include

**Calendar:**
- `Calendar/set`: `onDestroyRemoveEvents: true` removes events; single-calendar
  events are destroyed; multi-calendar events have calendarId removed
- `Calendar/set`: destroy with events and `onDestroyRemoveEvents: false` →
  `calendarHasEvent`
- `Calendar/set`: `onSuccessSetIsDefault` sets isDefault and returns updated
  calendar in response
- `Calendar/get`: calendar with only `mayReadFreeBusy` is not returned
- `Calendar/set`: per-user property update (isVisible, sortOrder) by a
  subscriber does not affect owner's view

**CalendarEvent:**
- `CalendarEvent/get`: `reduceParticipants: true` filters non-owner participants
- `CalendarEvent/get`: `recurrenceOverridesBefore`/`After` trims the overrides
  map
- `CalendarEvent/set`: create without uid → server assigns UUID
- `CalendarEvent/set`: server sets `updated` on create/update when isOrigin
- `CalendarEvent/set`: outer patch path into recurrenceOverrides applies
  correctly (draft Fig. 2 example)
- `CalendarEvent/set`: null value in outer patch removes key from
  recurrenceOverrides PatchObject (draft Fig. 4 example)
- `CalendarEvent/set`: privacy "private" on shared calendar → `invalidProperties`
- `CalendarEvent/query`: `expandRecurrences: true` with before/after returns
  synthetic ids for each occurrence
- `CalendarEvent/query`: `expandDurationTooLarge` when window exceeds capability
- `CalendarEvent/copy`: event appears in destination account; source unchanged

**CalendarEventNotification:**
- `CalendarEventNotification/set`: create attempt → `forbidden`
- `CalendarEventNotification/set`: update attempt → `forbidden`
- `CalendarEventNotification/set`: destroy succeeds
- `CalendarEventNotification/query`: filter by `after`/`before`/`type`

**ParticipantIdentity:**
- `ParticipantIdentity/set`: `onSuccessSetIsDefault` sets isDefault and returns
  updated identity
- `ParticipantIdentity/set`: unknown calendarAddress URI → `forbidden`

**Error codes:**
- `calendarHasEvent` returns correct SetError type
- `expandDurationTooLarge` returns correct method error
- `cannotCalculateOccurrences` returns correct method error

## Spec References

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt` —
  normative for all method handlers
- `~/PROJECT/jmap-chat-spec/references/rfc8984.txt` — normative for
  CalendarEvent content, PatchObject semantics, recurrenceOverrides
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol
  (set semantics §5.3, copy §5.4, query §5.5, queryChanges §5.6)

## Dependencies

```toml
jmap-types            = { path = "../crate-jmap-types" }
jmap-calendars-types  = { path = "../crate-jmap-calendars-types" }
jmap-server           = { path = "../crate-jmap-server" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
tokio      = { version = "1", features = ["rt"] }

[features]
parse = []   # enables CalendarEvent/parse handler and backend method
```

No iCalendar parsing libraries. No HTTP client. No database drivers.

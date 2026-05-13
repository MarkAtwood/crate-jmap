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

`~/PROJECT/crate-jmap/crate-jmap-mail-server/src/backend.rs` —
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
Re-exported from `jmap-calendars-types` as `JMAP_CALENDARS_URI`. Consumers
advertise this in the session capabilities.

Two additional capability URIs defined by the draft are NOT advertised by
this crate today; advertising them is the consumer's responsibility based on
backend-specific support:
- `urn:ietf:params:jmap:principals:availability` — for `Principal/getAvailability`.
  The handler is registered (see `principal.rs`) and a default trivial backend
  impl returns an empty list. Consumers that have a real availability source
  should advertise this URI.
- `urn:ietf:params:jmap:calendars:parse` — for `CalendarEvent/parse`. The
  handler is registered unconditionally; the default `CalendarsBackend`
  implementation classifies all blobs as `notParsable`. Consumers with real
  iCalendar parsing should advertise this URI.

(Resolved bd:JMAP-r3pg.21 — capability URI re-export reviewed; current
shape retained for cross-crate consistency.)

## Method Coverage

Total: 19 method registrations (Calendar/query and Calendar/queryChanges
are NOT registered today — see TODO below).

| Object | Methods | Draft §§ | Backend path |
|---|---|---|---|
| Calendar | get, changes, set | §4.1, §4.2, §4.3 | standard CRUD |
| CalendarEvent | get | §5.7 | standard get + 4 extra args via `CalendarEventGetArgs` |
| CalendarEvent | changes | §5.8 | standard changes |
| CalendarEvent | set | §5.9 | standard set + `CalendarEventSetArgs` |
| CalendarEvent | copy | §5.10 | `copy_event` |
| CalendarEvent | query | §5.11 | standard query + `CalendarEventQueryArgs` (expandRecurrences, timeZone) |
| CalendarEvent | queryChanges | §5.12 | standard queryChanges |
| CalendarEvent | parse | §5.13 | `parse_calendar_event_blobs` (registered unconditionally; default impl returns notParsable) |
| CalendarEventNotification | get | §7.1 | standard get |
| CalendarEventNotification | changes | §7.2 | standard changes |
| CalendarEventNotification | set | §7.3 | destroy-only; handler enforces |
| CalendarEventNotification | query | §7.4 | standard query |
| CalendarEventNotification | queryChanges | §7.5 | standard queryChanges |
| ParticipantIdentity | get | §3.1 | standard get |
| ParticipantIdentity | changes | §3.2 | standard changes |
| ParticipantIdentity | set | §3.3 | standard set + `onSuccessSetIsDefault` |
| Principal | getAvailability | §2.2 | `get_availability` (default impl returns empty list) |

Note: `Calendar/query` / `Calendar/queryChanges` are mentioned in the table
header for completeness with the §4 / RFC 8620 §5.5–5.6 lineage but are NOT
currently registered. If they are needed, they would be registered alongside
the existing `Calendar/get` / `Calendar/changes` / `Calendar/set` handlers.

The `register_calendars_handlers` doc comment in `lib.rs` cites "all 20"
methods; the actual registration count is 19 (one off-by-one between the
doc and the registrations) — minor doc bug, low priority.

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

### 7. CalendarEvent/get / /query / /set extra arguments

Three Args structs thread per-call extras into the backend:

- `CalendarEventGetArgs` (draft §5.7): `recurrenceOverridesBefore`,
  `recurrenceOverridesAfter`, `reduceParticipants`, `timeZone`.
- `CalendarEventQueryArgs` (draft §5.11): `expandRecurrences`, `timeZone`,
  plus the §5.11 windowing inputs.
- `CalendarEventSetArgs` (draft §5.9): `sendSchedulingMessages` and any
  related scheduling-side controls.

The handlers parse the incoming arguments into these structs and forward
them to `get_calendar_events`, `query_calendar_events`, and the
create/update/destroy methods respectively. Backends apply filters,
compute `utcStart`/`utcEnd` when requested, and emit the relevant errors
(`expandDurationTooLarge`, `cannotCalculateOccurrences`,
`noSupportedScheduleMethods`).

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

### 11. CalendarEvent/parse — registered unconditionally with trivial default

`CalendarEvent/parse` (draft §5.13) requires iCalendar parsing.

**Shipped reality (drift from earlier plan):** the `parse` Cargo feature
flag was NOT implemented. `CalendarEvent/parse` is registered
unconditionally in `register_calendars_handlers`. The
`CalendarsBackend::parse_calendar_event_blobs` trait method has a default
implementation that classifies all blobs as `notParsable`, so backends
without iCalendar parsing degrade gracefully without the consumer needing
to gate registration. Backends that want real parsing override the method.

The capability URI `urn:ietf:params:jmap:calendars:parse` is therefore
advertised by the consumer based on backend capability, not on a Cargo
feature in this crate. There is no `[features]` section in
`Cargo.toml` today.

### 12. Floating events and UTC time handling

JSCalendar "floating" events have no `timeZone` property. Their `utcStart` and
`utcEnd` are computed using the `timeZone` argument to `CalendarEvent/get`
(default "Etc/UTC") or the Calendar's `timeZone` property.

The handler does NOT compute utcStart/utcEnd. That computation requires
timezone database access, which belongs in the backend. The handler passes
the requested `timeZone` argument to the backend, which performs the conversion
and returns the computed values when `utcStart`/`utcEnd` are in the requested
properties list.

## Public API (shipped sketch)

The full reference is `cargo doc -p jmap-calendars-server --no-deps`. This
section sketches the trait and the supporting types as they exist today.
Names that drifted from the original plan are marked **(drift)**.

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
    //
    // create_object / update_object / destroy_object generic over O: SetObject.
    // Signatures follow the MailBackend pattern; see `backend.rs` for details.

    // ── Calendar-specific ────────────────────────────────────────────────────

    /// `CalendarEvent/get` with extra draft §5.7 arguments. The handler parses
    /// the wire-format args into [`CalendarEventGetArgs`] before calling.
    fn get_calendar_events(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
        args: &CalendarEventGetArgs,
    ) -> impl Future<Output = Result<(Vec<CalendarEvent>, Vec<Id>), Self::Error>> + Send;

    /// `CalendarEvent/query` with extra draft §5.11 arguments. The handler
    /// parses wire-format args into [`CalendarEventQueryArgs`] before calling.
    fn query_calendar_events(
        &self,
        // ... see backend.rs for full signature
    ) -> impl Future<Output = Result<QueryResult, QueryCalendarEventsError<Self::Error>>> + Send;

    /// `CalendarEvent/copy` (RFC 8620 §5.4).
    fn copy_event(
        // ... see backend.rs for full signature
    ) -> impl Future<Output = Result<(Id, CalendarEvent), BackendSetError<Self::Error>>> + Send;

    /// `CalendarEvent/parse` (draft §5.13).
    ///
    /// **(drift)** Originally planned as `parse_event` behind a `parse`
    /// Cargo feature flag. Shipped as `parse_calendar_event_blobs` registered
    /// unconditionally with a trivial default impl that classifies all blobs
    /// as `notParsable`. See §11 above.
    fn parse_calendar_event_blobs(
        &self,
        account_id: &Id,
        blob_ids: &[Id],
        properties: Option<&[String]>,
    ) -> impl Future<Output = Result<ParseResult, Self::Error>> + Send {
        /* default: all blobs notParsable */
    }

    /// `Principal/getAvailability` (draft §2.2). Default impl returns empty.
    fn get_availability(/* ... */) -> impl Future<...> + Send { /* default */ }

    /// `onSuccessSetIsDefault` for both Calendar/set and ParticipantIdentity/set.
    fn set_default_calendar(/* ... */) -> impl Future<Output = Result<SetDefaultResult, ...>> + Send;
    fn set_default_participant_identity(/* ... */) -> impl Future<Output = Result<SetDefaultResult, ...>> + Send;
}

/// Extra arguments for `CalendarEvent/get` (draft §5.7).
pub struct CalendarEventGetArgs {
    pub recurrence_overrides_before: Option<String>, // UTCDateTime
    pub recurrence_overrides_after: Option<String>,  // UTCDateTime
    pub reduce_participants: bool,
    pub time_zone: Option<String>,                   // TimeZoneId
}

/// Extra arguments for `CalendarEvent/query` (draft §5.11). Carries
/// `expandRecurrences`, `timeZone`, and the windowing inputs.
pub struct CalendarEventQueryArgs { /* see backend.rs */ }

/// Extra arguments for `CalendarEvent/set` (draft §5.9). Carries
/// `sendSchedulingMessages` and related scheduling controls.
pub struct CalendarEventSetArgs { /* see backend.rs */ }

/// Result of `CalendarEvent/parse` (draft §5.13).
///
/// **(drift)** Originally planned as `ParseEventResult`.
pub struct ParseResult {
    pub parsed: HashMap<Id, Vec<CalendarEvent>>,
    pub not_found: Vec<Id>,
    pub not_parsable: Vec<Id>,
}

/// Outcome of `set_default_calendar` / `set_default_participant_identity`.
/// Carries the new default and (if changed) the previous default so the
/// handler can emit the §3.3 / §4.3 response-mutation pair atomically.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct SetDefaultResult {
    pub new_default: Option<Id>,
    pub previous_default: Option<Id>,
}

/// Error type for `get_availability`.
#[non_exhaustive]
pub enum AvailabilityError<E: std::error::Error> {
    NotFound, Forbidden, RangeTooLarge, /* ... */
}

/// Register all JMAP Calendars handlers with a jmap-server Dispatcher.
///
/// After calling this, the dispatcher handles 19 method names (see the
/// Method Coverage table above).
pub fn register_calendars_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: CalendarsBackend + 'static,
    C: Clone + Send + 'static;
```

## Module Layout (shipped)

```
src/
  lib.rs                   re-exports; register_calendars_handlers macro
  backend.rs               CalendarsBackend trait; CalendarEventGetArgs,
                           CalendarEventQueryArgs, CalendarEventSetArgs;
                           ParseResult, SetDefaultResult; AvailabilityError,
                           QueryCalendarEventsError; re-exports of
                           BackendChangesError/BackendSetError/etc. from jmap-server
  calendar.rs              Calendar/get, /changes, /set (onDestroyRemoveEvents +
                           onSuccessSetIsDefault); inline tests
  event.rs                 CalendarEvent/get (extra args), /changes, /set
                           (recurrenceOverrides patch + sendSchedulingMessages),
                           /copy, /query (expandRecurrences), /queryChanges,
                           /parse — all in one module
  event_notification.rs    CalendarEventNotification/get, /changes, /set
                           (destroy-only enforcement), /query, /queryChanges
  participant_identity.rs  ParticipantIdentity/get, /changes, /set
                           (onSuccessSetIsDefault)
  principal.rs             Principal/getAvailability
  helpers.rs (private)     set_error_value, resolve_on_success_set_is_default,
                           apply_default_change_to_response,
                           extract_account_id (re-export from jmap-server)
```

**Drift from earlier plan:**
- `notification.rs` → `event_notification.rs` (clearer scoping).
- `participant.rs` → `participant_identity.rs` (matches type name).
- `event_parse.rs` was never separated; parse logic lives in `event.rs`
  alongside the rest of CalendarEvent handling. Reasonable given the small
  size of the parse handler.
- `error.rs` was never created. Error names like `calendarHasEvent`,
  `noSupportedScheduleMethods`, `expandDurationTooLarge`,
  `cannotCalculateOccurrences` are produced inline via
  `SetError::new(SetErrorType::custom("..."))` at the call sites. There is
  no consolidated `CalendarSetError` type.
- `principal.rs` is shipped but was not in the original plan.

## Test Strategy (shipped)

**Drift from earlier plan:** there is no `tests/` directory. Tests are
inline `#[cfg(test)] mod tests` blocks within each handler file plus a
`test_support` module providing `MockBackend`. This is the layout the
crate actually ships:

```
src/
  test_support module       MockBackend (in-memory CalendarsBackend impl,
                            visible only via cfg(test))
  calendar.rs               7 inline tests (Calendar/get/changes/set)
  event.rs                  18 inline tests (CalendarEvent/* methods)
  event_notification.rs     4 inline tests
  participant_identity.rs   3 inline tests
  principal.rs              3 inline tests
  lib.rs                    31 inline tests (registration shape, end-to-end
                            dispatch wiring across all 19 methods)
  Total: 66 inline tests passing as of 2026-05-08.
```

Test oracles come from draft-ietf-jmap-calendars-26 §8 example JSON (the spec
includes full request/response pairs). Hardcoded as `serde_json::json!({...})`
literals in the inline test modules. Never derive expected values from the
implementation under test.

Each test constructs a `MockBackend`, registers the relevant handler(s),
sends a `JmapRequest`-shaped argument map, and asserts the response.

(Resolved bd:JMAP-r3pg.10 — `copy_successful_with_overrides` test setup
tightened.)

(Resolved bd:JMAP-r3pg.20 — `test_support::MockBackend` import path cleaned
up.)

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

## Dependencies (shipped)

The shipped Cargo.toml uses workspace inheritance for all deps:

```toml
[dependencies]
jmap-types           = { workspace = true }
jmap-calendars-types = { workspace = true }
jmap-server          = { workspace = true }
serde                = { workspace = true }
serde_json           = { workspace = true }
thiserror            = { workspace = true }
tokio                = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

**Drift from earlier plan:** there is NO `[features]` section. The `parse`
feature flag was never implemented; `CalendarEvent/parse` is registered
unconditionally with a default trivial impl (see §11). If iCalendar
parsing becomes a feature this crate optionally enables, it would need to
be re-introduced.

No iCalendar parsing libraries. No HTTP client. No database drivers.

(Resolved bd:JMAP-r3pg.22 — runtime surface narrowed; tokio is now a
dev-only dep for tests.)

## Open Review Findings (JMAP-r3pg children)

The /review-rusty pass on this crate filed 25 findings under JMAP-r3pg.
All findings have been resolved except for `bd:JMAP-r3pg.14` (active —
coordinates with `bd:JMAP-g7wu.1` and `bd:JMAP-g7wu.3` for the next
cross-crate API-break window):

- **bd:JMAP-r3pg.14** (P3, open) — `extract_account_id` second pattern-match
  destructures already-extracted args in /copy and /set.

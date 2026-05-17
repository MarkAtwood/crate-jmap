# jmap-calendars-client

## What it is

Typed JMAP client methods for JMAP Calendars
([draft-ietf-jmap-calendars-26]).

Implements 19 typed `async fn` methods on a session-bound client. Depends on
`jmap-base-client` for transport, authentication, and session management.

## What it's for

Implements draft-ietf-jmap-calendars-26 method bindings on top of
`jmap-base-client`: `Calendar/*`, `CalendarEvent/*`,
`CalendarEventNotification/*`, `ParticipantIdentity/*`, and
`Principal/getAvailability`. Sibling of `jmap-mail-client` in the
extension-client family — mirrors that crate's shape. Depends on
`jmap-base-client` for transport and session, and on `jmap-calendars-types`
for the wire types (including the RFC 8984 JSCalendar sub-types re-exported
under the `jscalendar` module alias).

## How to use

```rust,no_run
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_calendars_client::JmapCalendarsExt;
use jmap_types::Id;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
// 1. Build the underlying HTTP client.
let auth = BearerAuth::new("my-token")?;
let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

// 2. Fetch a JMAP session (discovers API URL and account IDs).
let session = client.fetch_session().await?;

// 3. Bind the client to the session — gives access to all Calendars methods.
let sc = client.with_calendars_session(session);

// 4. Fetch all calendars.
let calendars = sc.calendar_get(None, None).await?;
println!("{} calendars", calendars.list.len());

// 5. Parse a calendar blob into CalendarEvent objects.
let blob_ids = [Id::new_validated("blob-abc")?];
let parsed = sc.calendar_event_parse(&blob_ids, None).await?;
if let Some(map) = parsed.parsed {
    for (id, events) in &map {
        println!("blob {id}: {} events", events.len());
    }
}
# Ok(())
# }
```

Id parameters are typed `&jmap_types::Id` (or `&[jmap_types::Id]` for slices)
to make invalid Ids unrepresentable. Construct Ids with
`Id::new_validated(s)` to enforce RFC 8620 §1.2 syntax (1..=255 SAFE-CHARs)
at the boundary, or with `Id::from(s)` when the value is known-valid (e.g.
already came back from a server response).

Re-create the `SessionClient` after each `fetch_session` call; a stale
session will produce `unknownAccount` or similar errors from the server.

## Methods

All methods are `async fn` on `SessionClient`. They require no extra
parameters beyond those shown — the account ID and API URL are resolved
from the bound session.

### Calendar

| Method | Signature | Returns |
|---|---|---|
| `calendar_get` | `(ids: Option<&[Id]>, properties: Option<&[&str]>)` | `GetResponse<Calendar>` |
| `calendar_changes` | `(since_state: &State, max_changes: Option<u64>)` | `ChangesResponse` |
| `calendar_set` | `(create, update, destroy: Option<&[Id]>, if_in_state: Option<&State>, params: Option<CalendarSetParams>)` | `SetResponse<Calendar>` |

### CalendarEvent

| Method | Signature | Returns |
|---|---|---|
| `calendar_event_get` | `(ids: Option<&[Id]>, properties: Option<&[&str]>, params: Option<CalendarEventGetParams>)` | `GetResponse<CalendarEvent>` |
| `calendar_event_changes` | `(since_state: &State, max_changes: Option<u64>)` | `ChangesResponse` |
| `calendar_event_set` | `(create, update, destroy: Option<&[Id]>)` | `SetResponse<CalendarEvent>` |
| `calendar_event_copy` | `(from_account_id: &Id, create: HashMap<String, CalendarEvent>)` | `SetResponse<CalendarEvent>` |
| `calendar_event_query` | `(filter, sort, position: Option<u64>, limit: Option<u64>, expand_recurrences: Option<bool>)` | `QueryResponse` |
| `calendar_event_query_changes` | `(since_query_state: &State, max_changes: Option<u64>)` | `QueryChangesResponse` |
| `calendar_event_parse` | `(blob_ids: &[Id], properties: Option<&[&str]>)` | `CalendarEventParseResponse` |

### CalendarEventNotification

| Method | Signature | Returns |
|---|---|---|
| `calendar_event_notification_get` | `(ids: Option<&[Id]>, properties: Option<&[&str]>)` | `GetResponse<CalendarEventNotification>` |
| `calendar_event_notification_changes` | `(since_state: &State, max_changes: Option<u64>)` | `ChangesResponse` |
| `calendar_event_notification_set` | `(destroy: Option<&[Id]>)` | `SetResponse` |
| `calendar_event_notification_query` | `(filter, sort, position: Option<u64>, limit: Option<u64>)` | `QueryResponse` |
| `calendar_event_notification_query_changes` | `(since_query_state: &State, max_changes: Option<u64>)` | `QueryChangesResponse` |

### ParticipantIdentity

| Method | Signature | Returns |
|---|---|---|
| `participant_identity_get` | `(ids: Option<&[Id]>, properties: Option<&[&str]>)` | `GetResponse<ParticipantIdentity>` |
| `participant_identity_changes` | `(since_state: &State, max_changes: Option<u64>)` | `ChangesResponse` |
| `participant_identity_set` | `(create, update, destroy: Option<&[Id]>)` | `SetResponse<ParticipantIdentity>` |

### Principal

| Method | Signature | Returns |
|---|---|---|
| `principal_get_availability` | `(principal_id: &Id, utc_start: &UTCDate, utc_end: &UTCDate, show_details: Option<bool>, event_properties: Option<&[&str]>)` | `PrincipalGetAvailabilityResponse` |

`Id`, `State`, and `UTCDate` here are `jmap_types::Id`, `jmap_types::State`,
and `jmap_types::UTCDate`. `UTCDate` enforces RFC 8620 §1.4 format
validation at construction time via `UTCDate::new_validated`.

`filter` and `sort` parameters use typed conditions/comparators where defined,
falling back to `Option<serde_json::Value>` for spec extensions not yet bound.
`create` parameters are typed as `Option<HashMap<String, T>>` for the relevant
JMAP object `T`. `update` parameters are typed as
`Option<HashMap<Id, jmap_types::PatchObject>>` (RFC 8620 §5.3) — wire format is
unchanged from a plain JSON object because `PatchObject` is
`#[serde(transparent)]`. Pass `None` to omit any of these from the request.

## Extension trait

`JmapCalendarsExt` extends `jmap_base_client::JmapClient` with a single
method:

```rust
pub trait JmapCalendarsExt {
    fn with_calendars_session(&self, session: Session) -> SessionClient;
}
```

Import the trait to use it:

```rust
use jmap_calendars_client::JmapCalendarsExt;
```

`SessionClient` is the struct returned by `with_calendars_session`. It holds
a clone of the `JmapClient` and the fetched `Session`. All 19 Calendars
methods are implemented directly on `SessionClient` — there is no method
dispatch overhead.

`session_parts()` (internal) extracts `(api_url, account_id)` from the
session by looking up the primary account for
`urn:ietf:params:jmap:calendars`. If no such primary account exists in the
session, it returns `ClientError::InvalidSession`.

## Response types

| Type | Fields | Source |
|---|---|---|
| `GetResponse<T>` | `account_id`, `state`, `list: Vec<T>`, `not_found: Option<Vec<Id>>` | RFC 8620 §5.1 |
| `ChangesResponse` | `account_id`, `old_state`, `new_state`, `has_more_changes`, `created`, `updated`, `destroyed` | RFC 8620 §5.2 |
| `SetResponse<T>` | `account_id`, `old_state`, `new_state`, `created`, `updated`, `destroyed`, `not_created`, `not_updated`, `not_destroyed` | RFC 8620 §5.3 |
| `QueryResponse` | `account_id`, `query_state`, `can_calculate_changes`, `position`, `ids`, `total`, `limit` | RFC 8620 §5.5 |
| `QueryChangesResponse` | `account_id`, `old_query_state`, `new_query_state`, `total`, `removed`, `added` | RFC 8620 §5.6 |
| `CalendarEventParseResponse` | `account_id`, `parsed: Option<HashMap<Id, Vec<CalendarEvent>>>`, `not_found`, `not_parsable` | draft §5.13 |
| `PrincipalGetAvailabilityResponse` | `account_id`, `list: Vec<BusyPeriod>` | draft §2.2 |

`SetResponse<T>` defaults to `T = serde_json::Value` when the concrete type
is not needed. Use `SetResponse<Calendar>` or `SetResponse<CalendarEvent>`
when you need typed access to created/updated objects.

`CalendarEventGetParams` carries optional extra arguments for
`calendar_event_get`:

```rust
pub struct CalendarEventGetParams {
    pub expand_recurrences: Option<bool>,   // draft §5.11
    pub reduced_participants: Option<bool>, // draft §5.4
    pub fetch_calendars: Option<bool>,      // draft §5.4
}
```

Pass `None` for any field to omit it from the request.

## How it works

Each method on `SessionClient` runs the same pipeline:

1. Validate arguments (typed `&Id` / `&[Id]` makes invalid Ids unrepresentable;
   defence-in-depth empty-state guards return `InvalidArgument` before any I/O).
2. Resolve `(api_url, account_id)` from the bound session for
   `urn:ietf:params:jmap:calendars`.
3. Build the method-arguments JSON.
4. Wrap it into a `JmapRequest` via `JmapRequestBuilder` with
   `using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:calendars"]`.
5. POST it via `jmap_base_client::JmapClient::call`.
6. `extract_response::<T>` finds the typed result for call ID `"r1"`.

The `Jmap*Ext` extension trait (`JmapCalendarsExt`) adds the
`with_calendars_session(session)` accessor to `JmapClient`. The returned
`SessionClient` carries the session and exposes every Calendars method as a
typed `async fn`.

## Gotchas

- **Tests are wiremock smoke tests only.** There are no integration tests
  against a real JMAP server. The tests verify request shape (method name,
  call id, capability URIs, wire field names) and response deserialization
  against spec-derived JSON fixtures.

- **`calendar_event_parse` requires pre-uploaded blobs.** The method accepts
  blob IDs as strings; callers must upload blobs separately using
  `jmap_base_client::upload_blob` before calling this method. The client
  does not perform blob upload automatically.

- **`principal_get_availability` wire key is `"id"` (not `"principalId"`).**
  This is correct per draft §2.2 but non-obvious — the field name `"id"` in
  the wire request refers to the principal being queried. Do not change it
  to `"principalId"`.

- **`CalendarEventNotification/set` is destroy-only.** The
  `calendar_event_notification_set` method accepts only a `destroy` parameter.
  Create and update operations are not exposed because the server is required
  to reject them with `forbidden` SetErrors (draft §7.3); constructing such
  requests would be incorrect.

## Crate family

```
jmap-types
    └── jmap-base-client         HTTP transport, auth, session, blob
            └── jmap-calendars-client  ← this crate
```

`jmap-calendars-types` is a sibling dependency (via `jmap-calendars-client`'s
`Cargo.toml`) — response types reference `Calendar`, `CalendarEvent`,
`CalendarEventNotification`, `ParticipantIdentity`, and `BusyPeriod` from
that crate.

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[draft-ietf-jmap-calendars-26]** — JMAP Calendars binding (normative for
  method semantics, wire field names, capability URIs)
- **[RFC 8984]** — JSCalendar Event format (normative for `CalendarEvent`
  content)
- **[RFC 8620]** — JMAP Core (request format, response shapes, error types)

[draft-ietf-jmap-calendars-26]: https://datatracker.ietf.org/doc/draft-ietf-jmap-calendars/
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

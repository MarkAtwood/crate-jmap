# jmap-calendars-client — Implementation Plan

draft-ietf-jmap-calendars-26 (JMAP Calendars) method implementations on top
of `jmap-base-client`.

## Crate Family Position

```
jmap-types
    ├── jmap-calendars-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-calendars-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for every
JMAP Calendars operation defined in draft-ietf-jmap-calendars-26:
`Calendar/get`, `Calendar/set`, `Calendar/changes`, `Calendar/query`,
`CalendarEvent/get`, `CalendarEvent/set`, `CalendarEvent/changes`,
`CalendarEvent/query`, `CalendarEvent/queryChanges`, `CalendarEvent/copy`,
`CalendarEventNotification/get`, `CalendarEventNotification/changes`,
`CalendarEventNotification/set`, `CalendarEventNotification/query`,
`CalendarEventNotification/queryChanges`, `ParticipantIdentity/get`,
`ParticipantIdentity/changes`, `ParticipantIdentity/set`.

Consumers call `jmap-base-client::JmapClient::call()` directly or use the
typed helpers defined here. No new HTTP machinery — all network operations go
through `jmap-base-client`.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that's `jmap-base-client`)
- Not handling iCalendar, CalDAV, or other non-JMAP calendar protocols
- Not performing recurrence expansion locally (that is a server-side operation
  when `expandRecurrences` is true, or a consumer responsibility otherwise)

## Source Material

This is greenfield — no existing Rust implementation to extract from.

Design pattern to follow:
- `~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern,
  module layout, and test approach
- `~/PROJECT/crate-jmapchat-client/src/methods/` — how method request/response
  types are structured and how `JmapRequestBuilder` is used
- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt` —
  normative spec for all method signatures, arguments, and response fields

## Dependencies

```toml
jmap-types            = { path = "../crate-jmap-types" }
jmap-calendars-types  = { path = "../crate-jmap-calendars-types" }
jmap-base-client      = { path = "../crate-jmap-base-client" }
serde_json            = "1"
thiserror             = "2"
```

No direct reqwest/tokio dependency — all I/O goes through `jmap-base-client`.

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule). To add methods to
`JmapClient` from this crate, we use an **extension trait**:

```rust
pub trait JmapCalendarsExt {
    async fn calendar_get(...) -> Result<...>;
    // ...
}

impl JmapCalendarsExt for JmapClient {
    async fn calendar_get(...) -> Result<...> { ... }
}
```

Callers must bring the trait into scope: `use jmap_calendars_client::JmapCalendarsExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used — no `async-trait` crate
needed. This works because we do not need `dyn JmapCalendarsExt`. If dyn
dispatch is ever required, wrap with `async-trait 0.1` at that time.

## Planned Public API

```rust
use jmap_base_client::{ClientError, JmapClient};
use jmap_calendars_types::{
    Calendar, CalendarEvent, CalendarEventNotification, ParticipantIdentity,
};
use jmap_types::{Id, State};

/// Extension trait adding JMAP Calendars methods to [`JmapClient`].
///
/// Import to use: `use jmap_calendars_client::JmapCalendarsExt;`
pub trait JmapCalendarsExt {
    // ── Calendar ─────────────────────────────────────────────────────────────

    /// Calendar/get (draft §4.1). ids=None fetches all.
    async fn calendar_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<Calendar>, ClientError>;

    /// Calendar/changes (draft §4.2).
    async fn calendar_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// Calendar/set (draft §4.3).
    /// on_destroy_remove_events and on_success_set_is_default are extra args.
    async fn calendar_set(
        &self,
        account_id: &Id,
        req: SetRequest<Calendar>,
        on_destroy_remove_events: bool,
        on_success_set_is_default: Option<&str>,
    ) -> Result<SetResponse<Calendar>, ClientError>;

    /// Calendar/query (RFC 8620 §5.5, applied to Calendar).
    async fn calendar_query(
        &self,
        account_id: &Id,
        filter: Option<serde_json::Value>,
        sort: Option<&[CalendarComparator]>,
        position: Option<i64>,
        limit: Option<u64>,
    ) -> Result<QueryResponse, ClientError>;

    /// Calendar/queryChanges (RFC 8620 §5.6, applied to Calendar).
    async fn calendar_query_changes(
        &self,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<serde_json::Value>,
        sort: Option<&[CalendarComparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
    ) -> Result<QueryChangesResponse, ClientError>;

    // ── CalendarEvent ─────────────────────────────────────────────────────────

    /// CalendarEvent/get (draft §5.7). Includes extra arguments.
    async fn calendar_event_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        args: CalendarEventGetArgs,
    ) -> Result<GetResponse<CalendarEvent>, ClientError>;

    /// CalendarEvent/changes (draft §5.8).
    async fn calendar_event_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// CalendarEvent/set (draft §5.9).
    /// send_scheduling_messages is the extra draft argument.
    async fn calendar_event_set(
        &self,
        account_id: &Id,
        req: SetRequest<CalendarEvent>,
        send_scheduling_messages: bool,
    ) -> Result<SetResponse<CalendarEvent>, ClientError>;

    /// CalendarEvent/copy (draft §5.10, RFC 8620 §5.4).
    async fn calendar_event_copy(
        &self,
        from_account_id: &Id,
        to_account_id: &Id,
        req: CopyRequest<CalendarEvent>,
    ) -> Result<CopyResponse<CalendarEvent>, ClientError>;

    /// CalendarEvent/query (draft §5.11). Includes expandRecurrences and timeZone.
    async fn calendar_event_query(
        &self,
        account_id: &Id,
        req: CalendarEventQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    /// CalendarEvent/queryChanges (draft §5.12).
    async fn calendar_event_query_changes(
        &self,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<CalendarEventFilterCondition>,
        sort: Option<&[CalendarEventComparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        expand_recurrences: bool,
        time_zone: Option<&str>,
    ) -> Result<QueryChangesResponse, ClientError>;

    // ── CalendarEventNotification ─────────────────────────────────────────────

    /// CalendarEventNotification/get (draft §7.1).
    async fn notification_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<CalendarEventNotification>, ClientError>;

    /// CalendarEventNotification/changes (draft §7.2).
    async fn notification_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// CalendarEventNotification/set (draft §7.3). Only destroy is supported.
    /// The caller should only put ids in `destroy`; any create/update will
    /// be rejected by the server with forbidden.
    async fn notification_destroy(
        &self,
        account_id: &Id,
        ids: &[Id],
    ) -> Result<SetResponse<CalendarEventNotification>, ClientError>;

    /// CalendarEventNotification/query (draft §7.4).
    async fn notification_query(
        &self,
        account_id: &Id,
        filter: Option<NotificationFilterCondition>,
        sort: Option<&[NotificationComparator]>,
        position: Option<i64>,
        limit: Option<u64>,
    ) -> Result<QueryResponse, ClientError>;

    /// CalendarEventNotification/queryChanges (draft §7.5).
    async fn notification_query_changes(
        &self,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<NotificationFilterCondition>,
        sort: Option<&[NotificationComparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
    ) -> Result<QueryChangesResponse, ClientError>;

    // ── ParticipantIdentity ───────────────────────────────────────────────────

    /// ParticipantIdentity/get (draft §3.1). ids=None fetches all.
    async fn participant_identity_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<ParticipantIdentity>, ClientError>;

    /// ParticipantIdentity/changes (draft §3.2).
    async fn participant_identity_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// ParticipantIdentity/set (draft §3.3).
    /// on_success_set_is_default is the extra draft argument.
    async fn participant_identity_set(
        &self,
        account_id: &Id,
        req: SetRequest<ParticipantIdentity>,
        on_success_set_is_default: Option<&str>,
    ) -> Result<SetResponse<ParticipantIdentity>, ClientError>;
}

impl JmapCalendarsExt for JmapClient {
    // implementations in calendar.rs, event.rs, notification.rs, participant.rs
}
```

### Supporting request/response types

```rust
/// Extra arguments for CalendarEvent/get beyond the standard /get (draft §5.7).
pub struct CalendarEventGetArgs {
    /// Filter recurrence overrides to those before this UTC datetime.
    pub recurrence_overrides_before: Option<String>,
    /// Filter recurrence overrides to those on or after this UTC datetime.
    pub recurrence_overrides_after: Option<String>,
    /// If true, only return owner/self participants (default false).
    pub reduce_participants: bool,
    /// Time zone for utcStart/utcEnd of floating events (default "Etc/UTC").
    pub time_zone: Option<String>,
}

/// Full request struct for CalendarEvent/query (draft §5.11).
pub struct CalendarEventQueryRequest {
    pub filter: Option<CalendarEventFilterCondition>,
    pub sort: Option<Vec<CalendarEventComparator>>,
    pub position: Option<i64>,
    pub limit: Option<u64>,
    /// If true, server expands recurring events within filter window.
    /// Requires filter.after and filter.before to be set.
    pub expand_recurrences: bool,
    /// Time zone for before/after filter conditions (default "Etc/UTC").
    pub time_zone: Option<String>,
}

/// Standard JMAP get response.
pub struct GetResponse<T> {
    pub account_id: Id,
    pub state: State,
    pub list: Vec<T>,
    pub not_found: Vec<Id>,
}

/// Standard JMAP changes response.
pub struct ChangesResponse {
    pub account_id: Id,
    pub old_state: State,
    pub new_state: State,
    pub has_more_changes: bool,
    pub created: Vec<Id>,
    pub updated: Vec<Id>,
    pub destroyed: Vec<Id>,
}

/// Standard JMAP set response.
pub struct SetResponse<T> {
    pub account_id: Id,
    pub old_state: Option<State>,
    pub new_state: State,
    pub created: HashMap<String, T>,
    pub updated: HashMap<Id, Option<T>>,
    pub destroyed: Vec<Id>,
    pub not_created: HashMap<String, SetError>,
    pub not_updated: HashMap<Id, SetError>,
    pub not_destroyed: HashMap<Id, SetError>,
}

/// Standard JMAP set request.
pub struct SetRequest<T> {
    pub if_in_state: Option<State>,
    pub create: Option<HashMap<String, T>>,
    pub update: Option<HashMap<Id, serde_json::Value>>,  // PatchObject values
    pub destroy: Option<Vec<Id>>,
}

/// Standard JMAP query response.
pub struct QueryResponse {
    pub account_id: Id,
    pub query_state: State,
    pub can_calculate_changes: bool,
    pub position: u64,
    pub ids: Vec<Id>,
    pub total: Option<u64>,
    pub limit: Option<u64>,
}

/// Standard JMAP queryChanges response.
pub struct QueryChangesResponse {
    pub account_id: Id,
    pub old_query_state: State,
    pub new_query_state: State,
    pub total: Option<u64>,
    pub removed: Vec<Id>,
    pub added: Vec<AddedItem>,
}

/// Standard JMAP copy request.
pub struct CopyRequest<T> {
    pub if_from_in_state: Option<State>,
    pub if_in_state: Option<State>,
    pub create: HashMap<String, T>,
    pub on_success_destroy_original: bool,
    pub destroy_from_if_in_state: Option<State>,
}

/// Standard JMAP copy response.
pub struct CopyResponse<T> {
    pub account_id: Id,
    pub from_account_id: Id,
    pub old_state: Option<State>,
    pub new_state: State,
    pub created: HashMap<String, T>,
    pub not_created: HashMap<String, SetError>,
}
```

## Module Layout

```
src/
  lib.rs            pub trait JmapCalendarsExt; impl JmapCalendarsExt for JmapClient;
                    re-exports of all public types
  calendar.rs       Calendar/get, /changes, /set, /query, /queryChanges
                    CalendarComparator type
  event.rs          CalendarEvent/get (CalendarEventGetArgs), /changes, /set,
                    /copy, /query (CalendarEventQueryRequest),
                    /queryChanges. CalendarEventComparator type.
  notification.rs   CalendarEventNotification/get, /changes, /set (destroy),
                    /query, /queryChanges. NotificationFilterCondition,
                    NotificationComparator types.
  participant.rs    ParticipantIdentity/get, /changes, /set
  types.rs          GetResponse, ChangesResponse, SetResponse, SetRequest,
                    QueryResponse, QueryChangesResponse, CopyRequest,
                    CopyResponse, AddedItem, SetError
```

## Test Strategy

- All tests use `wiremock` (or equivalent mock HTTP layer) via
  `jmap-base-client`'s HTTP layer — no live network required
- Request serialization tests: construct a typed request, verify the JSON
  serialized to the mock server matches the spec example from draft §8
- Response deserialization tests: feed the spec example JSON responses from
  draft §8 into the typed methods and verify the resulting structs

### Primary test oracles

**draft §8.1 — Fetching initial data**: The spec provides both the full
`methodCalls` request array and the server response. Use these as:
- Serialize oracle: verify `calendar_get` + `participant_identity_get` +
  `calendar_event_query` + `calendar_event_get` produce the exact JSON in Fig. 7
- Deserialize oracle: feed the response JSON and verify `GetResponse<Calendar>`,
  `GetResponse<ParticipantIdentity>`, `QueryResponse`, `GetResponse<CalendarEvent>`

**draft §8.2 — Creating an event**: The spec shows a `CalendarEvent/set`
create request and response. Use as:
- Serialize oracle for `calendar_event_set` with a new CalendarEvent
- Deserialize oracle for `SetResponse<CalendarEvent>`

**draft §8.3 — Snoozing an alert**: Shows a `CalendarEvent/set` update with
a PatchObject that modifies alert acknowledged time and adds a snooze alert.
Use as oracle for PatchObject serialization in `SetRequest.update`.

**draft §8.4 — Changing the default calendar**: Shows `Calendar/set` with
`onSuccessSetIsDefault`. Use as oracle for that extra argument.

### Additional test cases

- `CalendarEventGetArgs` with `reduceParticipants: true` serializes correctly
- `CalendarEventQueryRequest` with `expandRecurrences: true` requires both
  `filter.after` and `filter.before` (validate at call site, not just server)
- `notification_destroy` produces a set request with only `destroy` populated
- `participant_identity_set` with `onSuccessSetIsDefault` serializes the extra
  field correctly
- `SetRequest` with a PatchObject update serializes the nested map correctly
- `CopyRequest` serializes `onSuccessDestroyOriginal` correctly

## Spec References

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt` —
  all method signatures, extra arguments, and wire format (normative)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base get/set/changes/query
  request and response shapes (normative for structural fields)

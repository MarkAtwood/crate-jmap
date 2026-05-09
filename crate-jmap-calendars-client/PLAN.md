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

## Extension Trait + SessionClient Pattern (shipped)

The shipped design has **drifted** from the original "fat extension trait"
sketched below. The extension trait was simplified to a single method:

```rust
pub trait JmapCalendarsExt {
    fn with_calendars_session(&self, session: Session) -> SessionClient;
}

impl JmapCalendarsExt for JmapClient { /* one-line clone + bind */ }
```

`SessionClient` then carries inherent (not trait) methods for all 19 calendar
operations. This shape was chosen because:

- Every calendar method needs `(api_url, account_id)` from the bound session;
  taking `&Session` on every call would be repetitive.
- The 19 inherent methods compose more naturally with `cargo doc`, autocomplete,
  and IDE navigation than 19 trait-method declarations.
- The single-method extension trait still satisfies the orphan rule for
  attaching `with_calendars_session` to `JmapClient`.

Callers do:
```rust
use jmap_calendars_client::JmapCalendarsExt;
let session = client.fetch_session(...).await?;
let sc = client.with_calendars_session(session);
let resp = sc.calendar_get(None, None).await?;
```

TODO bd:JMAP-231o.28 — the single-method trait could be replaced with an
inherent `JmapClient::with_calendars_session` if we ever want to drop the
`use jmap_calendars_client::JmapCalendarsExt;` import. Defensible either
way; left as-is for now.

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used for the underlying
backend trait calls — no `async-trait` crate needed.

## Public API (shipped sketch)

The full reference is `cargo doc -p jmap-calendars-client --no-deps`. This
section sketches the trait + 19 SessionClient methods as they exist today.
Naming and signatures that drifted from the original plan are marked
**(drift)**. Most drift is reflected in the design-decision log below
this section.

```rust
use jmap_base_client::{ClientError, JmapClient, Session};
use jmap_calendars_client::JmapCalendarsExt;

/// One-method extension trait. (drift: original plan put all 19 methods here.)
pub trait JmapCalendarsExt {
    fn with_calendars_session(&self, session: Session) -> SessionClient;
}

/// Bound (JmapClient, Session) pair. All 19 method calls live here as
/// inherent methods rather than trait methods. Cheap to clone.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient { /* fields are pub(crate) */ }

impl SessionClient {
    // ── Calendar (3 methods) ────────────────────────────────────────────
    pub async fn calendar_get(
        &self,
        ids: Option<&[&str]>,             // (drift: was Option<&[Id]>)
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<Calendar>, ClientError>;
    pub async fn calendar_changes(/* ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn calendar_set(
        &self,
        create: Option<HashMap<String, Calendar>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[&str]>,
        on_destroy_remove_events: Option<bool>,
    ) -> Result<SetResponse<Calendar>, ClientError>;

    // ── CalendarEvent (7 methods) ───────────────────────────────────────
    pub async fn calendar_event_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
        params: Option<CalendarEventGetParams>, // (drift: was CalendarEventGetArgs)
    ) -> Result<GetResponse<CalendarEvent>, ClientError>;
    pub async fn calendar_event_changes(/* ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn calendar_event_set(/* ... */) -> Result<SetResponse<CalendarEvent>, ClientError>;
    pub async fn calendar_event_copy(/* ... */) -> Result<SetResponse<CalendarEvent>, ClientError>;
    pub async fn calendar_event_query(
        &self,
        filter: Option<serde_json::Value>,  // (drift: was typed CalendarEventFilterCondition)
        sort: Option<serde_json::Value>,    // (drift: was Option<&[Comparator]>)
        position: Option<i64>,
        limit: Option<u64>,
        expand_recurrences: Option<bool>,
        time_zone: Option<&str>,
    ) -> Result<QueryResponse, ClientError>;
    pub async fn calendar_event_query_changes(/* ... */) -> Result<QueryChangesResponse, ClientError>;
    pub async fn calendar_event_parse(
        &self,
        blob_ids: &[&str],
        properties: Option<&[&str]>,
    ) -> Result<CalendarEventParseResponse, ClientError>;

    // ── CalendarEventNotification (5 methods) ───────────────────────────
    pub async fn calendar_event_notification_get(/* ... */) -> Result<GetResponse<CalendarEventNotification>, ClientError>;
    pub async fn calendar_event_notification_changes(/* ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn calendar_event_notification_set(
        &self,
        destroy: Option<&[&str]>,           // destroy-only per draft §7.3
    ) -> Result<SetResponse<CalendarEventNotification>, ClientError>;
    pub async fn calendar_event_notification_query(/* ... */) -> Result<QueryResponse, ClientError>;
    pub async fn calendar_event_notification_query_changes(/* ... */) -> Result<QueryChangesResponse, ClientError>;

    // ── ParticipantIdentity (3 methods) ─────────────────────────────────
    pub async fn participant_identity_get(/* ... */) -> Result<GetResponse<ParticipantIdentity>, ClientError>;
    pub async fn participant_identity_changes(/* ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn participant_identity_set(/* ... */) -> Result<SetResponse<ParticipantIdentity>, ClientError>;

    // ── Principal (1 method) ────────────────────────────────────────────
    pub async fn principal_get_availability(/* ... */) -> Result<PrincipalGetAvailabilityResponse, ClientError>;
}
```

### Drift summary (original plan → shipped)

- **Extension trait shape**: 19 trait methods → 1 trait method
  (`with_calendars_session`) + 19 inherent methods on `SessionClient`. See
  the rationale and JMAP-231o.28 above.
- **Id types**: `&Id` / `&[Id]` parameters → `&str` / `&[&str]`. Avoids
  forcing callers to construct `Id` from string literals; the empty-string
  guard is enforced inside each builder. TODO bd:JMAP-231o.6 — empty-string
  guards are inconsistent across methods.
- **State types**: `&State` parameters → `&str`. Same rationale as Id.
  TODO bd:JMAP-231o.3 — consider migrating state-bearing fields back to
  `jmap_types::State` newtype now that JMAP-231o was filed.
- **SetRequest<T> / CopyRequest<T> / CalendarEventQueryRequest builder
  structs** were dropped in favour of explicit per-method positional
  arguments and raw `serde_json::Value` for filter/sort. Trade-off: less
  type safety, simpler call sites, no nested-builder ergonomics issues.
  TODO bd:JMAP-231o.22 — revisit whether typed filter/sort would be
  worthwhile for 0.2.0.
- **CalendarEventGetArgs** (planned) → **CalendarEventGetParams** (shipped).
  Field names also changed: `reduce_participants: bool` → `reduced_participants:
  Option<bool>` etc. The Option<bool> shape is intentional (None = "do not
  send the field"); see TODO bd:JMAP-231o.32.
- **Method count**: 18 → 19. CalendarEvent/parse and Principal/getAvailability
  were added to the spec / draft after the original plan was written.
- **SetResponse fields**: shipped uses `Option<HashMap>` for created/updated
  (None when key absent on the wire) rather than always-present empty maps.
  TODO bd:JMAP-231o.27 — debate whether always-present (default to
  empty) would be friendlier to callers.
- **Wire-format hygiene**: /get methods now omit `ids` / `properties` when
  None rather than sending explicit JSON null (closed by JMAP-231o.10
  as of 2026-05-08).

## Module Layout (shipped)

```
src/
  lib.rs                       pub trait JmapCalendarsExt (one method);
                               impl for JmapClient; re-exports the response
                               types and SessionClient
  methods/
    mod.rs                     pub struct SessionClient + Clone + manual Debug;
                               build_request helper; CalendarEventGetParams,
                               CalendarEventParseResponse, PrincipalGet-
                               AvailabilityResponse type definitions; CALL_ID
                               + USING_* capability arrays; std response types
                               re-exported from jmap-base-client
                               (GetResponse, ChangesResponse, SetResponse,
                               QueryResponse, QueryChangesResponse)
    calendar.rs                Calendar/get, /changes, /set
    event.rs                   CalendarEvent/get, /changes, /set, /query,
                               /queryChanges
    event_copy.rs              CalendarEvent/copy
    event_notification.rs      CalendarEventNotification/get, /changes, /set,
                               /query, /queryChanges
    event_parse.rs             CalendarEvent/parse
    participant_identity.rs    ParticipantIdentity/get, /changes, /set
    principal_availability.rs  Principal/getAvailability
```

**Drift from earlier plan:**
- `notification.rs` → `event_notification.rs` (clearer scoping).
- `participant.rs` → `participant_identity.rs` (matches type name).
- `calendar.rs` no longer carries `Calendar/query` / `Calendar/queryChanges`
  — those methods are not registered in the shipped client.
- `types.rs` was never created. Standard JMAP response types
  (`GetResponse`, `SetResponse`, etc.) come from `jmap-base-client` and are
  re-exported through `methods/mod.rs`. Calendar-specific response types
  (`CalendarEventParseResponse`, `PrincipalGetAvailabilityResponse`) live in
  `methods/mod.rs` directly.
- `event_copy.rs`, `event_parse.rs`, `principal_availability.rs` are
  shipped modules not in the original plan.

## Test Strategy (shipped)

Tests live in two places:

- **Inline `#[cfg(test)] mod tests` blocks** in each `methods/*.rs` file.
  These currently exercise `build_request` shape (method name, capability
  URIs, CALL_ID), and a few argument-handling assertions. They do NOT
  drive the production `SessionClient` methods — see TODO below.
- **`tests/calendar_smoke_tests.rs`** at the crate root, using `wiremock`.
  These DO call the production `SessionClient` methods through a mocked
  HTTP layer.

### Primary test oracles

**draft §8.1–§8.4** — the spec provides full `methodCalls` request and
response pairs. The smoke tests use these as wire-format oracles. The
inline tests use hand-written JSON literals for the same shapes.

### Open follow-ups

- TODO bd:JMAP-231o.7 — guard tests assert the implementation's own guard
  rather than an independent oracle (vacuous).
- TODO bd:JMAP-231o.8 — many inline `_request_shape` tests build args
  by hand and pass them to `build_request`, never exercising the
  production `calendar_*` / `event_*` builder code paths. Those tests
  are reassuring-looking but vacuous; rewriting them to call the
  `SessionClient` methods is the work tracked in JMAP-231o.8.
- TODO bd:JMAP-231o.11 — the `helpers_compile` test runs three times
  because of `#[path]` include.

## Review Findings (JMAP-231o children)

The /review-rusty pass on this crate filed 37 findings under JMAP-231o.
P0/P1 children are closed. The list below records remaining open items
plus recently-closed items that touched public-API shape (so a reader
of this PLAN.md sees both the pending design TODOs and the most
recent design changes that motivate the shipped sketch above).

- **bd:JMAP-231o.3** (P2) — state fields should use `jmap_types::State`
  newtype.
- **bd:JMAP-231o.4** (P2) — this PLAN.md drift (closed by this rewrite).
- **bd:JMAP-231o.6** (P2) — empty-string guards inconsistent across builders.
- **bd:JMAP-231o.8** (P2) — inline tests build args by hand and never hit
  production methods; vacuous.
- **bd:JMAP-231o.9** (P2) — `calendar_event_notification_set` always sends
  `destroy: []` even when caller passes `None`; debate short-circuit vs
  documenting.
- **bd:JMAP-231o.10** (P2) — `/get` builders sent `ids: null` / `properties:
  null` explicitly when `None` (closed 2026-05-08; consistent with /set
  conditional-add idiom).
- **bd:JMAP-231o.11** (P3) — `helpers_compile` test runs three times.
- **bd:JMAP-231o.13** (P3) — `session_parts` lifetime ergonomics.
- **bd:JMAP-231o.14** (P2) — `CALL_ID` is hard-coded to `"r1"`; pipelining
  callers would collide.
- **bd:JMAP-231o.15** (P3) — README has no doc-test of usage example.
- **bd:JMAP-231o.16** (P3) — capability URI const naming.
- **bd:JMAP-231o.17** (P3) — wire key `id` (not `principalId`) for
  `Principal/getAvailability` — has explicit comment + test.
- **bd:JMAP-231o.18** (P3) — `calendar_event_notification_set` is
  destroy-only; doc nuance.
- **bd:JMAP-231o.19** (P2) — verbose `(*id).to_owned()` in 7 sites
  (closed 2026-05-08; replaced with `Value::from`).
- **bd:JMAP-231o.20** (P3) — `for id in id_slice.iter()` could drop `.iter()`.
- **bd:JMAP-231o.24** (P3) — `session_parts` returns `&str` pair.
- **bd:JMAP-231o.25** (P3) — `build_request` allocates `Vec<String>` per call.
- **bd:JMAP-231o.26** (P2) — `ClientError::InvalidArgument` carries free-form
  `String`; could be structured.
- **bd:JMAP-231o.27** (P2) — `SetResponse` fields default to `None`; debate
  defaulting to empty maps.
- **bd:JMAP-231o.28** (P3) — single-method extension trait shape.
- **bd:JMAP-231o.29** (P2) — `SessionClient` missing Debug, Clone derives
  (closed 2026-05-08; added Clone derive + manual Debug).
- **bd:JMAP-231o.30** (P4) — `as implicit implicit-fetch` doc typo + factual
  error.
- **bd:JMAP-231o.31** (P3) — `ChangesResponse` fields not Option (spec says
  may be empty arrays, treated as empty).
- **bd:JMAP-231o.32** (P4) — `Option<bool>` flags have no third state.
- **bd:JMAP-231o.33** (P4) — `methods/mod.rs` lacks module-level docs on
  public types.
- **bd:JMAP-231o.34** (P3) — public fallible methods missing `# Errors`
  rustdoc sections.
- **bd:JMAP-231o.35** (P4) — type names not backtick-quoted in doc comments.
- **bd:JMAP-231o.36** (P4) — `make_client` is async but has no `.await`.
- **bd:JMAP-231o.37** (P4) — `make_client` uses `.expect()` rather than
  propagating as test failure.

## Spec References

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt` —
  all method signatures, extra arguments, and wire format (normative)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base get/set/changes/query
  request and response shapes (normative for structural fields)

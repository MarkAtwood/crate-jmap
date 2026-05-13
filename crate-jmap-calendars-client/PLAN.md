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
- `~/PROJECT/crate-jmap/crate-jmap-mail-client/` — identical extension trait pattern,
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
use jmap_types::{Id, PatchObject, State};

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
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<Calendar>, ClientError>;
    pub async fn calendar_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;
    pub async fn calendar_set(
        &self,
        create: Option<HashMap<String, Calendar>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
        on_destroy_remove_events: Option<bool>,
    ) -> Result<SetResponse<Calendar>, ClientError>;

    // ── CalendarEvent (7 methods) ───────────────────────────────────────
    pub async fn calendar_event_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        params: Option<CalendarEventGetParams>, // (drift: was CalendarEventGetArgs)
    ) -> Result<GetResponse<CalendarEvent>, ClientError>;
    pub async fn calendar_event_changes(/* &State since_state, ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn calendar_event_set(
        &self,
        create: Option<HashMap<String, CalendarEvent>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
    ) -> Result<SetResponse<CalendarEvent>, ClientError>;
    pub async fn calendar_event_copy(
        &self,
        from_account_id: &Id,
        create: HashMap<String, CalendarEvent>,
    ) -> Result<SetResponse<CalendarEvent>, ClientError>;
    pub async fn calendar_event_query(
        &self,
        filter: Option<&CalendarEventFilterCondition>,
        sort: Option<&[CalendarEventComparator]>,
        position: Option<u64>,
        limit: Option<u64>,
        expand_recurrences: Option<bool>,
    ) -> Result<QueryResponse, ClientError>;
    pub async fn calendar_event_query_changes(/* &State since_query_state, ... */) -> Result<QueryChangesResponse, ClientError>;
    pub async fn calendar_event_parse(
        &self,
        blob_ids: &[Id],
        properties: Option<&[&str]>,
    ) -> Result<CalendarEventParseResponse, ClientError>;

    // ── CalendarEventNotification (5 methods) ───────────────────────────
    pub async fn calendar_event_notification_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<CalendarEventNotification>, ClientError>;
    pub async fn calendar_event_notification_changes(/* &State since_state, ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn calendar_event_notification_set(
        &self,
        destroy: Option<&[Id]>,             // destroy-only per draft §7.3
    ) -> Result<SetResponse, ClientError>;
    pub async fn calendar_event_notification_query(/* ... */) -> Result<QueryResponse, ClientError>;
    pub async fn calendar_event_notification_query_changes(/* &State since_query_state, ... */) -> Result<QueryChangesResponse, ClientError>;

    // ── ParticipantIdentity (3 methods) ─────────────────────────────────
    pub async fn participant_identity_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<ParticipantIdentity>, ClientError>;
    pub async fn participant_identity_changes(/* &State since_state, ... */) -> Result<ChangesResponse, ClientError>;
    pub async fn participant_identity_set(
        &self,
        create: Option<HashMap<String, ParticipantIdentity>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
    ) -> Result<SetResponse<ParticipantIdentity>, ClientError>;

    // ── Principal (1 method) ────────────────────────────────────────────
    pub async fn principal_get_availability(
        &self,
        principal_id: &Id,
        utc_start: &UTCDate,
        utc_end: &UTCDate,
        show_details: Option<bool>,
        event_properties: Option<&[&str]>,
    ) -> Result<PrincipalGetAvailabilityResponse, ClientError>;
}
```

### Drift summary (original plan → shipped)

- **Extension trait shape**: 19 trait methods → 1 trait method
  (`with_calendars_session`) + 19 inherent methods on `SessionClient`. See
  the rationale and JMAP-231o.28 above.
- **Id types**: parameters use `&Id` / `&[Id]` directly. The original plan
  for typed Id parameters was briefly reverted to `&str` / `&[&str]` for
  ergonomics (with internal `validate_id_field` / `validate_ids_field`
  helpers as a 0.1.x stopgap), then restored to typed Id in the 0.2.0
  refactor (bd:JMAP-6by7.1, 2026-05-09). The validate_*_field helpers and
  their 11 call sites were deleted as part of the same refactor — they
  became dead code under the typed parameters.
- **State types**: parameters use `&State`. Migrated alongside the Id
  refactor in bd:JMAP-6by7.1; closes the standalone bd:JMAP-231o.3.
- **CalendarEvent filter / sort**: the 0.2.0 refactor restored typed
  `&CalendarEventFilterCondition` and `&[CalendarEventComparator]` —
  `serde_json::Value` was a transitional shape. TODO bd:JMAP-231o.22 still
  applies to the *Notification/query* sort (kept as `&[serde_json::Value]`
  because the spec defines minimal sort properties for notifications).
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
- **UTCDate types**: `principal_get_availability` takes `utc_start` /
  `utc_end` as `&UTCDate` (migrated under `bd:JMAP-g7wu.9.4`). The
  workspace-wide UTCDate audit sweep continues under `bd:JMAP-g7wu.9.5`
  for any remaining `&str`/`String` sites that semantically represent
  UTCDates.

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

## Extras-preservation policy (JMAP-lbdy)

Every public method-response struct defined in this crate that appears on
the JMAP wire carries an `extra` field per the workspace
extras-preservation policy (see workspace `AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

This preserves vendor / site / private-extension fields across
deserialize/serialize round-trip. Wire format is byte-identical when extras
are empty. The `default` attribute is active for Deserialize method-response
structs; the attribute set is kept uniform with the canonical extension-client
template (`jmap-mail-client`) so the cookie-cutter shape across sibling
extension-client crates is preserved.

In scope in this crate (each has at least one `*_preserves_vendor_extras`
round-trip preservation test in `methods/mod.rs`):

- Method-response structs (Deserialize): `CalendarEventParseResponse`
  (response to `CalendarEvent/parse`, draft-ietf-jmap-calendars-26 §5.13),
  `PrincipalGetAvailabilityResponse` (response to `Principal/getAvailability`,
  draft-ietf-jmap-calendars-26 §2.2).

The crate also re-exports standard response wrappers (`GetResponse<T>`,
`SetResponse<T>`, `ChangesResponse`, `QueryResponse`,
`QueryChangesResponse`) from `jmap-types`; those carry their own `extra`
field per JMAP-lbdy.1 and are not re-documented here.

Out of scope (explicitly excluded by the workspace policy):

- Filter / comparator algebra types and control enums — see workspace
  AGENTS.md "Filter algebra and control enums are explicitly EXCLUDED"
  for the full rationale.
- Internal Rust state types (`SessionClient`) — not wire-format.
- Non-serde builder helpers — `CalendarEventGetParams` in
  `src/methods/mod.rs` is a Rust-side builder helper with
  `#[derive(Debug, Clone, Default)]` only and NO serde derive. Its fields
  are manually unpacked into JSON by `build_request`, so it never appears
  on the JMAP wire as a flattened struct and is correctly excluded from
  the extras policy.

### New-type rule

Any new public method-response struct (or new method-argument struct that
gains a serde derive) added to this crate that appears on the JMAP wire
MUST include the `extra` field from day one with the documented serde
attributes and at least one round-trip preservation test. Per the
canonical-template propagation rule (workspace AGENTS.md), the canonical
extension-client template is `jmap-mail-client`; shape changes in that
crate propagate here and to the other five sibling extension-client crates
in lock-step.

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
  newtype. **Closed by bd:JMAP-6by7.1** (2026-05-09). Every State-shaped
  parameter on `SessionClient` is now `&State`.
- **bd:JMAP-231o.4** (P2) — this PLAN.md drift (closed by this rewrite).
- **bd:JMAP-231o.6** (P2) — empty-string guards inconsistent across builders.
  Originally closed 2026-05-09 with Option A (full internal validation via
  `Id::new_validated`, `&str` API kept). Re-closed 2026-05-09 by
  bd:JMAP-6by7.1, which restored the typed-Id API: the helper functions
  and their 11 call sites are gone; validation now happens at the type
  boundary (`Id::new_validated`) rather than inside each method.
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

# jmap-calendars-types

Serde-annotated Rust types for JMAP Calendars ([draft-ietf-jmap-calendars-26]) and
JSCalendar ([RFC 8984]).

**Types only** — no method handlers, no async, no network I/O. Sits between
`jmap-types` (RFC 8620 primitives) and `jmap-calendars-server` /
`jmap-calendars-client`.

## What

| Type | Module | Spec |
|---|---|---|
| `Calendar` | `calendar` | draft §4 |
| `CalendarRights` | `calendar` | draft §4 |
| `IncludeInAvailability` | `calendar` | draft §4 |
| `CalendarFilterCondition` | `calendar` | draft §4.5 |
| `CalendarEvent` | `event` | draft §5, RFC 8984 §2.1, §4, §5.1 |
| `CalendarEventFilterCondition` | `event` | draft §5.11.1 |
| `CalendarEventComparator` | `event` | draft §5.11.2 |
| `RecurrenceRule` | `jscalendar` | RFC 8984 §4.3.3 |
| `NDay` | `jscalendar` | RFC 8984 §4.3.3 |
| `Location` | `jscalendar` | RFC 8984 §4.2.5 |
| `VirtualLocation` | `jscalendar` | RFC 8984 §4.2.6 |
| `Link` | `jscalendar` | RFC 8984 §1.4.11 |
| `Participant` | `jscalendar` | RFC 8984 §4.4.6 |
| `Alert` | `jscalendar` | RFC 8984 §4.5.2 |
| `AlertTrigger` | `jscalendar` | RFC 8984 §4.5.2 |
| `OffsetTrigger` | `jscalendar` | RFC 8984 §4.5.2 |
| `AbsoluteTrigger` | `jscalendar` | RFC 8984 §4.5.2 |
| `Relation` | `jscalendar` | RFC 8984 §1.4.10 |
| `LocalDateTime` | `jscalendar` | RFC 8984 §1.4.5 |
| `Duration` | `jscalendar` | RFC 8984 §1.4.6 |
| `SignedDuration` | `jscalendar` | RFC 8984 §1.4.7 |
| `CalendarEventNotification` | `notification` | draft §7 |
| `Person` | `notification` | draft §7 |
| `NotificationType` | `notification` | draft §7 |
| `NotificationFilterCondition` | `notification` | draft §7.4.1 |
| `CalendarAlert` | `notification` | draft §7 |
| `ParticipantIdentity` | `participant_identity` | draft §3 |
| `BusyPeriod` | `availability` | draft §2.2 |
| `CalendarsCapability` | `capability` | draft §1.5 |
| `CalendarsAccountCapability` | `capability` | draft §1.5 |
| `CalendarsParseCapability` | `capability` | draft §5.13 |
| `PrincipalCalendarsCapability` | `capability` | draft §1.5 |
| `PrincipalsAvailabilityCapability` | `capability` | draft §2.2 |
| `PrincipalsAvailabilityAccountCapability` | `capability` | draft §2.2 |

The types in the `jscalendar` module (rows above with module = `jscalendar`) live in
the [`jmap-jscalendar-types`] crate and are re-exported here for backward-compatible
access. The re-export is available both as flat re-exports (`jmap_calendars_types::Location`)
and as a module alias (`jmap_calendars_types::jscalendar::Location`). The same sub-types
are consumed by `jmap-tasks-types` (planned).

[`jmap-jscalendar-types`]: https://crates.io/crates/jmap-jscalendar-types

Property enum types re-exported from the `backend` sub-module:
`CalendarProperty`, `CalendarEventProperty`, `CalendarEventNotificationProperty`,
`ParticipantIdentityProperty`.

Capability URI string constants: `JMAP_CALENDARS_URI`,
`JMAP_CALENDARS_PARSE_URI`, `JMAP_PRINCIPALS_AVAILABILITY_URI`.

## Filter extensibility

Filter and comparator types in this crate — `CalendarFilterCondition`,
`CalendarEventFilterCondition`, `CalendarEventComparator`,
`NotificationFilterCondition`, and the generic `Filter<T>` / `Operator`
re-exported from `jmap-types` — are **intentionally not extensible** via
vendor "extras" fields. A filter clause the server does not understand
silently breaks query correctness: the client gets the wrong set of records
back with no error signal. So these types deliberately have no `extra`
catch-all field.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use `draft-ietf-jmap-metadata` (capability URI
  `urn:ietf:params:jmap:metadata`), which defines a `Metadata` / `Annotation`
  companion object keyed by `(relatedType, relatedId)` with capability-declared
  schema (`metadataTypes` / `maxDepth`) and a `Metadata/query` `textMatch`
  filter. This is the workspace's recommended path for vendor data that needs
  to be queryable; the implementation tracker is bd JMAP-06zp.
- **Pre-IETF escape.** If you cannot wait for the metadata draft, escape the
  filter tree to `serde_json::Value` or fork the `FilterCondition` types.
  See [`PLAN.md`](PLAN.md) in this crate for the hybrid sloppy-value pattern
  this crate already uses for JSCalendar-shaped fields.

This policy is part of the workspace extras-preservation policy documented in
the workspace [`AGENTS.md`](../AGENTS.md); the filter-algebra exclusion
decision is bd JMAP-lbdy.

## Spec coverage

### draft-ietf-jmap-calendars-26

| Section | Topic |
|---|---|
| §1.5 | `CalendarsCapability`, `CalendarsAccountCapability` |
| §2.2 | `Principal/getAvailability` — `BusyPeriod` |
| §3 | `ParticipantIdentity` |
| §4 | `Calendar`, `CalendarRights`, `IncludeInAvailability` |
| §5 | `CalendarEvent` JMAP properties (`id`, `calendarIds`, `isDraft`, `utcStart`, `utcEnd`, `useDefaultAlerts`, `blobId`, etc.) |
| §5.4 | Per-user properties (`keywords`, `color`, `freeBusyStatus`, `useDefaultAlerts`, `alerts`) |
| §5.7 | `iCalComponent` wire property |
| §5.11.1 | `CalendarEventFilterCondition` |
| §5.11.2 | `CalendarEventComparator` |
| §5.13 | `CalendarsParseCapability` |
| §7 | `CalendarEventNotification`, `Person`, `NotificationType` |
| §7.4.1 | `NotificationFilterCondition` |

### RFC 8984 (JSCalendar)

| Section | Topic |
|---|---|
| §1.4.5 | `LocalDateTime` newtype |
| §1.4.6 | `Duration` newtype |
| §1.4.7 | `SignedDuration` newtype |
| §1.4.10 | `Relation` |
| §1.4.11 | `Link` |
| §4.1 | Metadata properties (`uid`, `relatedTo`, `prodId`, `created`, `updated`, `sequence`) |
| §4.2 | What/where properties (`title`, `description`, `locations`, `virtualLocations`, `links`, `keywords`, `color`) |
| §4.3 | Recurrence properties (`recurrenceId`, `recurrenceRules`, `recurrenceOverrides`, `excluded`) |
| §4.3.3 | `RecurrenceRule`, `NDay` |
| §4.4 | Scheduling/sharing (`priority`, `freeBusyStatus`, `privacy`, `participants`, `replyTo`) |
| §4.4.6 | `Participant` |
| §4.5 | Alert properties (`alerts`) |
| §4.5.2 | `Alert`, `AlertTrigger`, `OffsetTrigger`, `AbsoluteTrigger` |
| §4.6 | Multilingual (`localizations`) |
| §4.7 | Time zone (`timeZone`, `timeZones`) |
| §4.7.2 | Custom time zone definitions (opaque `serde_json::Value` passthrough) |
| §5.1 | Event-specific (`start`, `duration`, `status`) |

## Usage

### Deserialize a `CalendarEvent`

```rust
use jmap_calendars_types::CalendarEvent;

let json = r#"{
    "id": "ev1",
    "uid": "abc-123@example.com",
    "title": "Team meeting",
    "start": "2024-06-15T10:00:00",
    "duration": "PT1H",
    "timeZone": "America/New_York",
    "calendarIds": { "cal1": true },
    "updated": "2024-06-01T00:00:00Z"
}"#;

let event: CalendarEvent = serde_json::from_str(json).unwrap();
assert_eq!(event.title.as_deref(), Some("Team meeting"));
```

### Deserialize a `Calendar`

```rust
use jmap_calendars_types::Calendar;

let json = r#"{
    "id": "cal1",
    "name": "Work",
    "color": "#4285f4",
    "isSubscribed": true,
    "isVisible": true,
    "isDefault": false,
    "includeInAvailability": "all",
    "myRights": {
        "mayReadFreeBusy": true,
        "mayReadItems": true,
        "mayWriteAll": true,
        "mayWriteOwn": true,
        "mayUpdatePrivate": true,
        "mayRSVP": true,
        "mayShare": false,
        "mayDelete": false
    },
    "sortOrder": 0
}"#;

let cal: Calendar = serde_json::from_str(json).unwrap();
assert_eq!(cal.name, "Work");
```

### Deserialize a `BusyPeriod`

```rust
use jmap_calendars_types::BusyPeriod;

let json = r#"{
    "utcStart": "2024-06-15T09:00:00Z",
    "utcEnd": "2024-06-15T10:00:00Z",
    "busyStatus": "busy"
}"#;

let period: BusyPeriod = serde_json::from_str(json).unwrap();
assert_eq!(period.utc_start, "2024-06-15T09:00:00Z");
```

## How it works

### `rename_all = "camelCase"`

Every struct carries `#[serde(rename_all = "camelCase")]`. Rust field names
use `snake_case`; the wire format is camelCase per JSCalendar and JMAP
conventions. The mapping is automatic: `calendar_ids` → `"calendarIds"`,
`recurrence_rules` → `"recurrenceRules"`, etc.

### `#[serde(rename = "@type")]` for JSCalendar type discriminators

JSCalendar embeds an `@type` field in sub-objects to identify their kind
(e.g. `"RecurrenceRule"`, `"Participant"`, `"Alert"`). The `@` character is
not valid in a Rust identifier, so each such field is declared as:

```rust
#[serde(rename = "@type")]
pub at_type: String,
```

`AlertTrigger` uses an internally-tagged serde enum (`#[serde(tag = "@type")]`)
so that `OffsetTrigger`, `AbsoluteTrigger`, and the forward-compatibility
`Unknown(serde_json::Value)` variant all dispatch on the same `@type` field.

### `#[non_exhaustive]` policy

All public structs and enums are `#[non_exhaustive]`. New fields may be added
as the draft evolves without breaking downstream crates. Construct structs
using `..Default::default()` or the builder pattern your crate provides.

### `serde_json::Value` for open-ended extension points

Several `CalendarEvent` fields use `Option<serde_json::Value>` rather than
concrete typed structs:

| Field | Why `Value` |
|---|---|
| `timeZones` | Custom VTIMEZONE definitions (RFC 8984 §4.7.2) are a complex nested format requiring a full timezone parser. Almost no caller needs to construct these. |
| `locations`, `virtualLocations`, `links`, `participants`, `alerts`, `recurrenceRules`, `excludedRecurrenceRules`, `keywords`, `categories`, `replyTo`, `relatedTo` | JSCalendar defines these as open-ended maps/arrays where values are rich nested objects. Concrete types for each exist in the `jscalendar` module; callers that need typed access deserialize the `Value` themselves using those types. |

The fields `recurrenceOverrides` and `localizations` use a typed envelope —
`Option<HashMap<String, jmap_types::PatchObject>>` — because they ARE
RFC 8620 §5.3 PatchObjects at the JMAP layer.  The outer envelope is
typed; the inner `PatchObject` leaves remain `serde_json::Value` to
preserve per-leaf JSCalendar shape flexibility.  Wire format is
byte-identical to a plain JSON object map via `PatchObject`'s
`#[serde(transparent)]`.

This is the correct representation, not laziness — do not change these to
typed structs without reading RFC 8984 §3.3 on extensibility.

### `iCalComponent` wire name

The `ical_component` field on `CalendarEvent` carries the raw iCalendar
representation of the event. The wire name is `"iCalComponent"` — not
`"icalComponent"` — because "iCal" is a brand abbreviation with mixed case.
The automatic `rename_all = "camelCase"` conversion would produce the wrong
name, so this field carries an explicit override:

```rust
#[serde(rename = "iCalComponent", skip_serializing_if = "Option::is_none")]
pub ical_component: Option<String>,
```

## Known Limitations

- **`timeZones`, `links`, `participants`, `alerts`, `locations`,
  `virtualLocations`** use `Option<serde_json::Value>` because JSCalendar
  defines these as open-ended maps where values are complex nested
  objects. Callers needing typed access must deserialize the `Value`
  themselves using the concrete types in the `jscalendar` module.
- **`recurrenceOverrides`, `localizations`** use
  `Option<HashMap<String, jmap_types::PatchObject>>`: the outer envelope
  is typed (RFC 8620 §5.3 PatchObject) while inner leaves remain `Value`
  for JSCalendar shape flexibility. Construct via
  `PatchObject::from_map(...)` or `Map::into()`.

- **`iCalComponent: Option<String>`** carries raw base64-encoded iCalendar data
  with no validation. The crate does not parse or validate the iCalendar content;
  that is the consumer's responsibility.

- **Draft expired.** draft-ietf-jmap-calendars-26 is an expired Internet-Draft,
  not a published RFC. Some ambiguous field semantics use best-judgment
  interpretation based on the JSCalendar RFC 8984 normative base. The type
  definitions will need review when (if) the draft is published.

- **`LocalDateTime` and `Duration` are unvalidated newtype wrappers** around
  `String`. Format correctness (RFC 8984 §1.4.5, §1.4.6) is the backend's
  responsibility; this crate does not parse the internal format.

## Crate family

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-calendars-types  ← this crate
            ├── jmap-calendars-server (method handlers)
            └── jmap-calendars-client (client extension trait)
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[draft-ietf-jmap-calendars-26]** — JMAP Calendars binding (Calendar,
  CalendarEvent additions, CalendarEventNotification, ParticipantIdentity,
  capability objects, error codes)
- **[RFC 8984]** — JSCalendar Event format (all JSCalendar Event properties,
  RecurrenceRule, Alert, Participant, Location, etc.)
- **[RFC 8620]** — JMAP Core (request format, PatchObject, SetError,
  ResultReference)

[draft-ietf-jmap-calendars-26]: https://www.ietf.org/archive/id/draft-ietf-jmap-calendars-26.txt
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0

# jmap-calendars-types — Implementation Plan

RFC 8984 (JSCalendar) data types plus JMAP Calendars binding types from
draft-ietf-jmap-calendars-26. Types only — no method handlers, no async, no
network I/O. This crate sits between `jmap-types` (shared JMAP base primitives)
and `jmap-calendars-server` / `jmap-calendars-client`.

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-calendars-types  ← this crate
            ├── jmap-calendars-server (method handlers)
            └── jmap-calendars-client (extension trait)
```

## What This Crate Covers

Each object type maps to one source module. The normative references are:

- `draft-ietf-jmap-calendars-26` — JMAP Calendars binding (Calendar object,
  CalendarEvent additions, CalendarEventNotification, ParticipantIdentity,
  CalendarRights, capability objects, error codes)
- RFC 8984 — JSCalendar Event format (the content of CalendarEvent)

| Module | Type(s) | Spec section |
|---|---|---|
| `calendar.rs` | `Calendar`, `CalendarRights`, `IncludeInAvailability` | draft §4 |
| `event.rs` | `CalendarEvent` | draft §5, RFC 8984 §2.1, §4, §5.1 |
| `notification.rs` | `CalendarEventNotification`, `Person`, `NotificationType` | draft §7 |
| `participant_identity.rs` | `ParticipantIdentity` | draft §3 |
| `jscalendar.rs` | JSCalendar value types (see below) | RFC 8984 §4 |
| `query.rs` | `CalendarEventFilterCondition`, `CalendarEventComparator`, `NotificationFilterCondition` | draft §5.11, §7.4 |
| `capability.rs` | `CalendarsCapability`, `CalendarsAccountCapability` | draft §1.5 |

### JSCalendar value types (`jscalendar.rs`)

These are sub-object types defined in RFC 8984 that are embedded in
`CalendarEvent`. They have no JMAP identity of their own.

| Type | RFC 8984 section |
|---|---|
| `RecurrenceRule` | §4.3.3 |
| `NDay` | §4.3.3 (byDay array entry) |
| `Location` | §4.2.5 |
| `VirtualLocation` | §4.2.6 |
| `Link` | §1.4.11 |
| `Participant` | §4.4.6 |
| `Alert` | §4.5.2 |
| `AlertTrigger` (enum) | §4.5.2 — OffsetTrigger, AbsoluteTrigger, UnknownTrigger |
| `OffsetTrigger` | §4.5.2 |
| `AbsoluteTrigger` | §4.5.2 |
| `Relation` | §1.4.10 |
| `TimeZoneRule` | §4.7.2 (custom time zones, if supported) |

## Type Table with Field References

### `Calendar` (draft §4)

| Field | Wire type | Notes |
|---|---|---|
| `id` | `Id` | immutable; server-set |
| `name` | `String` | MUST NOT be empty; max 255 UTF-8 octets |
| `description` | `String\|null` | default null |
| `color` | `String\|null` | CSS color name or `#rrggbb`; default null |
| `sortOrder` | `u64` | 0 ≤ n < 2^31; default 0 |
| `isSubscribed` | `bool` | per-user |
| `isVisible` | `bool` | default true; per-user; ignored when !isSubscribed |
| `isDefault` | `bool` | server-set; at most one per account |
| `includeInAvailability` | `IncludeInAvailability` | "all"\|"attending"\|"none" |
| `defaultAlertsWithTime` | `HashMap<String, Alert>\|null` | UUID keys recommended |
| `defaultAlertsWithoutTime` | `HashMap<String, Alert>\|null` | UUID keys recommended |
| `timeZone` | `String\|null` | IANA tz id; default null |
| `shareWith` | `HashMap<Id, CalendarRights>\|null` | default null |
| `myRights` | `CalendarRights` | server-set |

### `CalendarRights` (draft §4, after myRights definition)

| Field | Wire type | Notes |
|---|---|---|
| `mayReadFreeBusy` | `bool` | read free/busy for availability |
| `mayReadItems` | `bool` | read events in this calendar |
| `mayWriteAll` | `bool` | create/modify/destroy any event; implies all write rights |
| `mayWriteOwn` | `bool` | create/modify/destroy events the user owns |
| `mayUpdatePrivate` | `bool` | modify per-user properties on all events |
| `mayRSVP` | `bool` | update participant status for own identities |
| `mayShare` | `bool` | modify shareWith |
| `mayDelete` | `bool` | delete the calendar itself |

### `CalendarEvent` (draft §5, RFC 8984 §2.1, §4, §5.1)

`CalendarEvent` is a JSCalendar Event object with additional JMAP-specific
properties. All JSCalendar properties are optional at the wire level (partial
responses) except where mandated by the spec.

**JMAP-added properties** (draft §5):

| Field | Wire type | Notes |
|---|---|---|
| `id` | `Id` | immutable; server-set |
| `baseEventId` | `Id\|null` | immutable; server-set; only for synthetic expanded instances |
| `calendarIds` | `HashMap<Id, bool>` | keys are calendar ids; values always true |
| `isDraft` | `bool` | default false; once false, cannot be set true |
| `isOrigin` | `bool` | server-set |
| `utcStart` | `UTCDateTime` | computed; not returned by default |
| `utcEnd` | `UTCDateTime` | computed; not returned by default |
| `useDefaultAlerts` | `bool` | default false |
| `mayInviteSelf` | `bool` | default false; draft §5.1.1 |
| `mayInviteOthers` | `bool` | default false; draft §5.1.2 |
| `hideAttendees` | `bool` | default false; draft §5.1.3 |
| `scheduleSequence` | `u64` | server-managed; draft §5.2.1 |
| `scheduleUpdated` | `UTCDateTime\|null` | server-managed; draft §5.2.2 |
| `blobId` | `Id\|null` | iCalendar representation; draft §10.9.14 |

**JSCalendar metadata properties** (RFC 8984 §4.1):

| Field | Wire type | Notes |
|---|---|---|
| `uid` | `String` | globally unique; mandatory |
| `relatedTo` | `HashMap<String, Relation>\|null` | UIDs of related objects |
| `prodId` | `String\|null` | product identifier |
| `created` | `UTCDateTime\|null` | creation time |
| `updated` | `UTCDateTime` | mandatory; last modification |
| `sequence` | `u64` | default 0 |

**JSCalendar what/where properties** (RFC 8984 §4.2):

| Field | Wire type | Notes |
|---|---|---|
| `title` | `String\|null` | default empty string |
| `description` | `String\|null` | default empty string |
| `descriptionContentType` | `String\|null` | default "text/plain" |
| `showWithoutTime` | `bool\|null` | default false (all-day event flag) |
| `locations` | `HashMap<String, Location>\|null` | |
| `virtualLocations` | `HashMap<String, VirtualLocation>\|null` | |
| `links` | `HashMap<String, Link>\|null` | attachments, images, etc. |
| `locale` | `String\|null` | BCP 47 language tag |
| `keywords` | `HashMap<String, bool>\|null` | values always true |
| `categories` | `HashMap<String, bool>\|null` | URI keys; values always true |
| `color` | `String\|null` | CSS color |

**JSCalendar recurrence properties** (RFC 8984 §4.3):

| Field | Wire type | Notes |
|---|---|---|
| `recurrenceId` | `String\|null` | LocalDateTime; identifies override instance |
| `recurrenceIdTimeZone` | `String\|null` | required if recurrenceId present |
| `recurrenceRules` | `Vec<RecurrenceRule>\|null` | |
| `excludedRecurrenceRules` | `Vec<RecurrenceRule>\|null` | |
| `recurrenceOverrides` | `HashMap<String, jmap_types::PatchObject>\|null` | typed PatchObject envelope; keys are LocalDateTime strings (RFC 8620 §5.3 PatchObject; inner leaves remain `Value`) |
| `excluded` | `bool\|null` | default false; marks excluded override |

**JSCalendar scheduling/sharing properties** (RFC 8984 §4.4):

| Field | Wire type | Notes |
|---|---|---|
| `priority` | `u8\|null` | 0–9; 0 = undefined |
| `freeBusyStatus` | `String\|null` | "free"\|"busy"; default "busy" |
| `privacy` | `String\|null` | "public"\|"private"\|"secret"; default "public" |
| `replyTo` | `HashMap<String, String>\|null` | method→URI |
| `sentBy` | `String\|null` | addr-spec |
| `participants` | `HashMap<String, Participant>\|null` | |
| `requestStatus` | `String\|null` | iTIP request status |

**JSCalendar alert properties** (RFC 8984 §4.5):

| Field | Wire type | Notes |
|---|---|---|
| `alerts` | `HashMap<String, Alert>\|null` | |

**JSCalendar multilingual** (RFC 8984 §4.6):

| Field | Wire type | Notes |
|---|---|---|
| `localizations` | `HashMap<String, jmap_types::PatchObject>\|null` | lang→typed PatchObject envelope (RFC 8620 §5.3 PatchObject; inner leaves remain `Value`) |

**JSCalendar time zone** (RFC 8984 §4.7):

| Field | Wire type | Notes |
|---|---|---|
| `timeZone` | `String\|null` | IANA tz id |
| `timeZones` | `serde_json::Value\|null` | custom tz definitions; per-entry value typed-decodable to `jmap_jscalendar_types::TimeZone` (re-exported as `TimeZone`; see JMAP-x014) |

**JSCalendar Event-specific properties** (RFC 8984 §5.1):

| Field | Wire type | Notes |
|---|---|---|
| `start` | `String\|null` | LocalDateTime string |
| `duration` | `String\|null` | Duration string (ISO 8601 subset) |
| `status` | `String\|null` | "confirmed"\|"cancelled"\|"tentative" |

### `RecurrenceRule` (RFC 8984 §4.3.3)

| Field | Wire type | Notes |
|---|---|---|
| `@type` | `String` | always "RecurrenceRule" |
| `frequency` | `String` | "yearly"\|"monthly"\|"weekly"\|"daily"\|"hourly"\|"minutely"\|"secondly" |
| `interval` | `u64\|null` | default 1; must be ≥ 1 |
| `rscale` | `String\|null` | calendar system; default "gregorian" |
| `skip` | `String\|null` | "omit"\|"backward"\|"forward"; default "omit" |
| `firstDayOfWeek` | `String\|null` | "mo"–"su"; default "mo" |
| `byDay` | `Vec<NDay>\|null` | |
| `byMonthDay` | `Vec<i32>\|null` | |
| `byMonth` | `Vec<String>\|null` | "1"–"12", with optional "L" suffix |
| `byYearDay` | `Vec<i32>\|null` | |
| `byWeekNo` | `Vec<i32>\|null` | |
| `byHour` | `Vec<u8>\|null` | |
| `byMinute` | `Vec<u8>\|null` | |
| `bySecond` | `Vec<u8>\|null` | |
| `bySetPosition` | `Vec<i32>\|null` | |
| `count` | `u64\|null` | |
| `until` | `String\|null` | LocalDateTime |

### `NDay` (RFC 8984 §4.3.3)

| Field | Wire type |
|---|---|
| `@type` | `String` — always "NDay" |
| `day` | `String` — "mo"\|"tu"\|"we"\|"th"\|"fr"\|"sa"\|"su" |
| `nthOfPeriod` | `i32\|null` — non-zero |

### `Participant` (RFC 8984 §4.4.6)

| Field | Wire type | Notes |
|---|---|---|
| `@type` | `String` | always "Participant" |
| `name` | `String\|null` | |
| `email` | `String\|null` | addr-spec |
| `description` | `String\|null` | |
| `sendTo` | `HashMap<String, String>\|null` | method→URI |
| `kind` | `String\|null` | "individual"\|"group"\|"location"\|"resource" |
| `roles` | `HashMap<String, bool>` | mandatory; "owner"\|"attendee"\|"optional"\|"informational"\|"chair"\|"contact" |
| `locationId` | `String\|null` | |
| `language` | `String\|null` | BCP 47 |
| `participationStatus` | `String\|null` | default "needs-action" |
| `participationComment` | `String\|null` | |
| `expectReply` | `bool\|null` | default false |
| `scheduleAgent` | `String\|null` | "server"\|"client"\|"none"; default "server" |
| `calendarAddress` | `String\|null` | iTIP scheduling address |
| `invitedBy` | `String\|null` | participant id |
| `delegatedTo` | `HashMap<String, bool>\|null` | |
| `delegatedFrom` | `HashMap<String, bool>\|null` | |
| `memberOf` | `HashMap<String, bool>\|null` | |
| `links` | `HashMap<String, Link>\|null` | |

### `Alert` (RFC 8984 §4.5.2)

| Field | Wire type | Notes |
|---|---|---|
| `@type` | `String` | always "Alert" |
| `trigger` | `AlertTrigger` | OffsetTrigger or AbsoluteTrigger |
| `acknowledged` | `UTCDateTime\|null` | |
| `relatedTo` | `HashMap<String, Relation>\|null` | for snooze chains |
| `action` | `String\|null` | "display"\|"email"; default "display" |

`AlertTrigger` is an internally tagged enum (`@type` field):
- `OffsetTrigger`: `offset: String` (SignedDuration), `relativeTo: String\|null` ("start"\|"end")
- `AbsoluteTrigger`: `when: UTCDateTime`
- `UnknownTrigger`: passthrough `serde_json::Value` for forward compatibility

### `Location` (RFC 8984 §4.2.5)

| Field | Wire type |
|---|---|
| `@type` | `String` — always "Location" |
| `name` | `String\|null` |
| `description` | `String\|null` |
| `locationTypes` | `HashMap<String, bool>\|null` |
| `relativeTo` | `String\|null` — "start"\|"end" |
| `timeZone` | `String\|null` |
| `coordinates` | `String\|null` — geo: URI |
| `links` | `HashMap<String, Link>\|null` |

### `CalendarEventNotification` (draft §7)

| Field | Wire type | Notes |
|---|---|---|
| `id` | `Id` | |
| `created` | `UTCDateTime` | when this notification was created |
| `changedBy` | `Person` | who made the change |
| `comment` | `String\|null` | comment from changer |
| `type` | `NotificationType` | "created"\|"updated"\|"destroyed" |
| `calendarEventId` | `Id` | base event id |
| `isDraft` | `bool\|null` | present for created/updated |
| `event` | `CalendarEvent` | data before change (updated/destroyed) or after (created) |
| `eventPatch` | `jmap_types::PatchObject\|null` | typed PatchObject (RFC 8620 §5.3); present for updated only |

`Person` sub-object (draft §7):

| Field | Wire type |
|---|---|
| `name` | `String` |
| `email` | `String\|null` |
| `principalId` | `Id\|null` |
| `calendarAddress` | `String\|null` |

### `ParticipantIdentity` (draft §3)

| Field | Wire type | Notes |
|---|---|---|
| `id` | `Id` | immutable; server-set |
| `name` | `String` | default empty string |
| `calendarAddress` | `String` | iTIP URI (e.g., mailto:…) |
| `isDefault` | `bool` | server-set; at most one per account |

### `CalendarsCapability` and `CalendarsAccountCapability` (draft §1.5)

`CalendarsCapability` (session-level, `urn:ietf:params:jmap:calendars`): empty object.

`CalendarsAccountCapability` (per-account):

| Field | Wire type |
|---|---|
| `maxCalendarsPerEvent` | `u64\|null` |
| `minDateTime` | `UTCDateTime` |
| `maxDateTime` | `UTCDateTime` |
| `maxExpandedQueryDuration` | `String` — Duration |
| `maxParticipantsPerEvent` | `u64\|null` |
| `mayCreateCalendar` | `bool` |

### Query types (`query.rs`)

`CalendarEventFilterCondition` (draft §5.11.1):

| Field | Wire type |
|---|---|
| `inCalendar` | `Id\|null` |
| `after` | `String\|null` — LocalDateTime |
| `before` | `String\|null` — LocalDateTime |
| `text` | `String\|null` |
| `title` | `String\|null` |
| `description` | `String\|null` |
| `location` | `String\|null` |
| `owner` | `String\|null` |
| `attendee` | `String\|null` |
| `uid` | `String\|null` |

`NotificationFilterCondition` (draft §7.4.1):

| Field | Wire type |
|---|---|
| `after` | `UTCDateTime\|null` |
| `before` | `UTCDateTime\|null` |
| `type` | `String\|null` |
| `calendarEventIds` | `Vec<Id>\|null` |

`CalendarEventComparator` (draft §5.11.2): only `start` property is required
to be supported. Struct has `property: String` and `isAscending: bool`
(default true).

## What Is Out of Scope

- Method handlers — those live in `jmap-calendars-server`
- iCalendar parsing and conversion — consumer responsibility
- Free/busy calculation — server concern
- Recurrence expansion — server concern
- Transport and network I/O — no tokio, no reqwest
- JMAP Sharing principal types — live in `jmap-sharing-types` (see `crate-jmap-sharing-types/`)

## Key Design Decisions

JSCalendar typed sub-types live in `jmap-jscalendar-types`. See
`crate-jmap-jscalendar-types/PLAN.md` for the hybrid-design rationale.
This crate's `CalendarEvent` keeps its sloppy `Option<serde_json::Value>`
fields as the wire-format anchor; consumers obtain typed views via
`serde_json::from_value::<jmap_calendars_types::Location>(event.locations.unwrap())`
(the re-export) or the equivalent `jmap_jscalendar_types::Location`
direct path.

## Module Layout

```
src/
  lib.rs                   re-exports; CalendarEvent, Calendar, etc.
  calendar.rs              Calendar, CalendarRights, IncludeInAvailability
  event.rs                 CalendarEvent (all fields; see §5 of draft + RFC 8984)
  notification.rs          CalendarEventNotification, Person, NotificationType
  participant_identity.rs  ParticipantIdentity
  query.rs                 CalendarEventFilterCondition, CalendarEventComparator,
                           NotificationFilterCondition
  capability.rs            CalendarsCapability, CalendarsAccountCapability
```

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values from the code
under test. Acceptable sources:

1. Hand-written JSON fixtures constructed directly from draft-ietf-jmap-calendars-26
   field descriptions, committed in `tests/fixtures/`.
2. Literal JSON from the spec examples: draft §8 (Fetching initial data,
   Creating an event, Snoozing an alert, Parsing an iCalendar file) provides
   complete request/response JSON suitable for deserialization tests.
3. RFC 8984 §6 examples (Simple Event, All-Day Event, Recurring Event with
   Overrides, Recurring Event with Participants) for CalendarEvent content.

All tests are `#[test]` (no tokio). Roundtrip tests (`serialize → deserialize`)
verify serde consistency but are not a substitute for spec-grounded oracle tests.

Priority test cases:

- `Calendar`: deserialize spec §8.1 response; verify all fields
- `CalendarRights`: verify all 8 boolean fields serialize/deserialize correctly
- `CalendarEvent`: deserialize RFC 8984 §6.9 recurring event with overrides;
  verify `recurrenceOverrides` keys and typed `PatchObject` values via
  `patch.as_map().get(...)`
- `RecurrenceRule`: verify `frequency`, `byDay` with NDay objects, `count`,
  `until`
- `Alert` with `OffsetTrigger`: verify `@type` tag and `offset`/`relativeTo`
- `Alert` with `AbsoluteTrigger`: verify `@type` tag and `when`
- `AlertTrigger` unknown type: round-trips via `Unknown(Value)` variant
- `CalendarEventNotification`: deserialize draft §7 example; verify all fields
  including `eventPatch`
- `ParticipantIdentity`: serialize/deserialize with `isDefault: true`
- `CalendarEvent` all-fields-None: partial response deserializes without error
- `IncludeInAvailability`: all three variants serialize to correct strings

## Source Material

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt` —
  JMAP Calendars binding (normative for Calendar, notification, participant
  identity, method signatures, error codes)
- `~/PROJECT/jmap-chat-spec/references/rfc8984.txt` — JSCalendar (normative
  for CalendarEvent content: all JSCalendar properties, RecurrenceRule, Alert,
  Participant, Location, etc.)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — JMAP base protocol (for
  Filter, Comparator, PatchObject, and session types)

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
```

No tokio, no async, no network deps. No iCalendar parser.

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

Every public `Deserialize` struct that appears on the JMAP wire carries an
`extra` field per the workspace extras-preservation policy (see workspace
`AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

In scope in this crate (each has a round-trip preservation test):

- `BusyPeriod` (availability.rs).
- `CalendarRights`, `Calendar` (calendar.rs).
- `CalendarEvent` (event.rs).
- `Person`, `CalendarEventNotification`, `CalendarAlert` (notification.rs).
- `ParticipantIdentity` (participant_identity.rs).

The 10 typed sub-types in the `jscalendar` module alias (re-exported from
`jmap-jscalendar-types`) — `NDay`, `RecurrenceRule`, `Location`,
`VirtualLocation`, `Link`, `Relation`, `Participant`, `OffsetTrigger`,
`AbsoluteTrigger`, `Alert` — also carry `extra` fields by virtue of
the policy applied in their home crate.

Out of scope:

- Filter and comparator algebra (`CalendarFilterCondition`,
  `CalendarEventFilterCondition`, `CalendarEventComparator`,
  `NotificationFilterCondition`) — silent-drop of an unknown filter
  clause is a query-correctness bug. The existing rustdoc on each of
  these types already documents the IETF-track and pre-IETF escape paths
  per the workspace AGENTS.md "filter algebra excluded" decision.
- Capability objects (`CalendarsCapability`,
  `CalendarsAccountCapability`, `PrincipalsAvailabilityCapability`,
  `PrincipalsAvailabilityAccountCapability`, `CalendarsParseCapability`,
  `PrincipalCalendarsCapability`) — capability objects live in the
  Session response and follow Session's evolution rules, matching the
  precedent set in `jmap-mail-types` where Sieve capability objects
  were excluded.
- `IncludeInAvailability` and `NotificationType` — string enums.

### New-type rule

Any new public `Deserialize` struct added to this crate that appears on
the JMAP wire MUST include the `extra` field from day one with the
documented serde attributes and at least one round-trip preservation
test.

## JMAP Object Metadata `relatedType` declarations

The JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/))
defines a companion `Metadata` object keyed by `(relatedType, relatedId)`
that attaches vendor-defined `Annotation`s — and, for some types, WebDAV
`WebDavMetadata` records — to objects defined elsewhere in the workspace.

The data types in this crate that are valid `relatedType` values:

| relatedType | Flavours supported by spec |
|---|---|
| `Calendar` | `Annotation`; `WebDavMetadata` (draft §2.1.3 SHOULD) |
| `CalendarEvent` | `Annotation`; `WebDavMetadata` (draft §2.1.3 SHOULD) |
| `CalendarEventNotification` | `Annotation` |

Servers that declare `urn:ietf:params:jmap:metadata` MAY restrict the
set of supported `relatedType`s via the capability's `dataTypes`
property.

Implementation crates: `jmap-metadata-types`, `jmap-metadata-server`,
`jmap-metadata-client` (bd JMAP-06zp).

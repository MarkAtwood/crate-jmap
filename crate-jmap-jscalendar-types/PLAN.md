# jmap-jscalendar-types — Implementation Plan

RFC 8984 JSCalendar typed sub-types for the `jmap-*` crate family.
Pure types — no method handlers, no async, no network I/O.

## Crate family position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-jscalendar-types  ← this crate
            ├── jmap-calendars-types (path-dep + re-export)
            └── jmap-tasks-types     (path-dep + re-export)
```

## What this crate is

The JSCalendar sub-object types defined in RFC 8984. They are embedded
inside larger JMAP objects (`CalendarEvent` in `jmap-calendars-types`,
`Task` in `jmap-tasks-types`) and have no JMAP identity of their own.

Previously these types lived inside `crate-jmap-calendars-types/src/jscalendar.rs`.
They were extracted into this dedicated crate (per bd:JMAP-x59i) so
`jmap-tasks-types` can also consume them without taking on a
`jmap-calendars-types` dependency — which would be both
semantically wrong (Tasks does not depend on Calendars) and would
risk a dependency cycle if Calendars later wanted to consume any
Tasks type.

## What this crate is not

- Not the JMAP Calendars binding (that is `jmap-calendars-types`)
- Not the JMAP Tasks binding (that is `jmap-tasks-types`)
- Not opinionated about CalDAV, iCalendar, or iTIP semantics — only the
  wire-format JSON shape defined by RFC 8984

## Dependencies

```toml
jmap-types = { workspace = true }   # for Id only
serde      = { workspace = true }
serde_json = { workspace = true }
```

No other dependencies.

## Public API

Single module (`src/lib.rs`). All types are `#[non_exhaustive]` and derive
`Debug, Clone, PartialEq, Serialize, Deserialize` (plus `Eq, Hash` where
the inner types permit it). Wire-format JSON uses
`#[serde(rename_all = "camelCase")]` on every struct, and the JSCalendar
`@type` discriminator is mapped to a `String` field named `at_type` with
`#[serde(rename = "@type")]`.

### Scalar wrappers (newtype around `String`)

| Type | RFC 8984 § | Wire shape |
|---|---|---|
| `LocalDateTime` | §1.4.5 | `"YYYY-MM-DDTHH:MM:SS"` (no `Z`, no `±offset`) |
| `Duration` | §1.4.6 | ISO 8601 duration subset, e.g. `"PT1H"`, `"P1DT2H"` |
| `SignedDuration` | §1.4.7 | optional `+`/`-` prefix on Duration |

Each implements `From<String>`, `From<&str>`, `AsRef<str>`,
`Debug + Clone + PartialEq + Eq + Hash + Serialize + Deserialize`.
Validation of the internal format is left to the backend; these wrappers
exist to document intent at the type level.

### Object types

| Type | RFC 8984 § | Notes |
|---|---|---|
| `NDay` | §4.3.3 | `byDay` array entry (day + nthOfPeriod) |
| `RecurrenceRule` | §4.3.3 | Used in `recurrenceRules` and `excludedRecurrenceRules` |
| `Location` | §4.2.5 | Physical location |
| `VirtualLocation` | §4.2.6 | Online meeting (with mandatory `uri`) |
| `Link` | §1.4.11 | Attachment, image, or URL (may use JMAP blob id) |
| `Relation` | §1.4.10 | UID-based relationship to another object |
| `Participant` | §4.4.6 | Event participant (attendee/organizer/etc.) |
| `OffsetTrigger` | §4.5.2 | Trigger offset from event `start`/`end` |
| `AbsoluteTrigger` | §4.5.2 | Trigger at an absolute UTC date-time |
| `AlertTrigger` | §4.5.2 | enum: OffsetTrigger \| AbsoluteTrigger \| Unknown |
| `Alert` | §4.5.2 | Alert to display or email |

### `AlertTrigger` round-trip preservation

`AlertTrigger` is an `#[non_exhaustive]` enum with three variants:

- `OffsetTrigger(OffsetTrigger)` — wire `@type: "OffsetTrigger"`
- `AbsoluteTrigger(AbsoluteTrigger)` — wire `@type: "AbsoluteTrigger"`
- `Unknown(serde_json::Value)` — any other `@type` value

The `Unknown` variant preserves unrecognised trigger types as raw JSON
for round-trip fidelity, per RFC 8984 §4.5.2: "Implementations MUST NOT
trigger for trigger types they do not understand but MUST preserve them."

Serde is implemented manually because
`#[serde(tag = "@type", other)]` with tuple variants is not supported
by serde's derive macros — `other` only works with unit variants in
internally-tagged enums.

## Module layout

Single `lib.rs` for now. The original `jscalendar.rs` in `jmap-calendars-types`
was a single 559-line file, and a pure extraction preserves that shape.

If a future bead splits this into sub-modules (e.g. `recurrence.rs`,
`location.rs`, `participant.rs`, `alert.rs`), it will be a follow-up after
this extraction is verified, to keep blast radius narrow.

## Spec reference

```
~/PROJECT/jmap-chat-spec/references/rfc8984.txt   ← normative
```

## History

- bd:JMAP-x59i.1 created this crate by extracting the 14 RFC 8984 types
  from `crate-jmap-calendars-types/src/jscalendar.rs`. The extraction
  is purely mechanical — no behavioural change, no public API
  modifications to `jmap-calendars-types` (that migration is tracked
  separately under bd:JMAP-x59i.2).
- bd:JMAP-x59i.2 (follow-up) migrates `jmap-calendars-types` to consume
  this crate (path-dep + re-export) and unblocks `jmap-tasks-types`
  consumption (bd:JMAP-g7wu.8).

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

- `NDay`, `RecurrenceRule` (RFC 8984 §4.3.3)
- `Location`, `VirtualLocation` (RFC 8984 §4.2.5, §4.2.6)
- `Link` (RFC 8984 §1.4.11)
- `Relation` (RFC 8984 §1.4.10)
- `Participant` (RFC 8984 §4.4.6)
- `OffsetTrigger`, `AbsoluteTrigger`, `Alert` (RFC 8984 §4.5.2)

Out of scope:

- `LocalDateTime`, `Duration`, `SignedDuration` — newtypes around `String`;
  no field-shaped extension surface.
- `AlertTrigger` — outer dispatch enum; vendor-extras handling lives on
  the variant structs (OffsetTrigger / AbsoluteTrigger). The
  `AlertTrigger::Unknown(Value)` variant is the existing pass-through
  for unrecognised `@type` strings and is independent of this policy.

### New-type rule

Any new public `Deserialize` struct added to this crate MUST include the
`extra` field from day one with the documented serde attributes and at
least one round-trip preservation test.

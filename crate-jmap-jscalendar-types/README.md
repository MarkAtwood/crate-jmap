# jmap-jscalendar-types

RFC 8984 JSCalendar typed sub-types for the `jmap-*` crate family.

Consumed by `jmap-calendars-types` and `jmap-tasks-types` (planned).
Pure data types: no method handlers, no async, no network I/O.

## What it is

This crate provides the JSCalendar sub-object types defined in RFC 8984.
They are embedded inside larger JMAP objects (such as `CalendarEvent`
in `jmap-calendars-types`) and have no JMAP identity of their own.

| Type | RFC 8984 § |
|---|---|
| `LocalDateTime`, `Duration`, `SignedDuration` | §1.4.5–§1.4.7 |
| `NDay`, `RecurrenceRule` | §4.3.3 |
| `Location`, `VirtualLocation` | §4.2.5, §4.2.6 |
| `Link` | §1.4.11 |
| `Relation` | §1.4.10 |
| `Participant` | §4.4.6 |
| `OffsetTrigger`, `AbsoluteTrigger`, `AlertTrigger`, `Alert` | §4.5.2 |

## Why a separate crate

Previously these types lived inside `jmap-calendars-types`. Splitting
them out lets `jmap-tasks-types` consume them without depending on
`jmap-calendars-types`, which is both semantically wrong (Tasks does
not depend on Calendars) and risks a dep cycle if Calendars later
wants to consume any Tasks type.

## What it's for

These typed sub-types are consumed by `jmap-calendars-types` — which re-exports
them as the `jscalendar` module alias — and are planned for `jmap-tasks-types`
(bd:JMAP-yfpq) so Tasks can share the same shapes without taking a Calendars
dep. The crate carries pure data-types only: no JMAP-method semantics, no
async, no network I/O. JMAP wire envelopes and identity types come from
`jmap-types`; the sub-types here go inside larger JMAP objects.

## How to use

```toml
[dependencies]
jmap-jscalendar-types = "0.1"
```

Transitively pulls in `jmap-types`, `serde`, `serde_json`. Parse a
`RecurrenceRule` directly from a JSON `Value`:

```rust
use jmap_jscalendar_types::RecurrenceRule;
use serde_json::json;

fn main() -> Result<(), serde_json::Error> {
    let rule: RecurrenceRule = serde_json::from_value(json!({
        "@type": "RecurrenceRule",
        "frequency": "weekly",
        "count": 10
    }))?;
    println!("frequency = {}", rule.frequency);
    Ok(())
}
```

The same pattern works for `Participant`, `Location`, `Alert`, `Link`,
`Relation`, and the trigger types.

## How it works

- Sealed sub-type set: every public sub-type that appears on the wire is
  defined in this crate's `src/lib.rs` and the set is closed at the spec's
  RFC 8984 §1.4 / §4.2 / §4.3 / §4.4 / §4.5 boundary.
- `#[non_exhaustive]` on every public struct, so additive spec evolution is
  a non-breaking change.
- Wire field `"@type"` is mapped to the Rust field `at_type: String` via
  `#[serde(rename = "@type")]`. The field is kept as `String` (not a closed
  enum) to preserve forward-compatibility with new sub-object types.
- `#[serde(rename_all = "camelCase")]` on all structs.
- No async. `#[forbid(unsafe_code)]` at the crate root.
- Dependencies limited to `jmap-types`, `serde`, `serde_json`.

## Gotchas

- `LocalDateTime`, `Duration`, and `SignedDuration` are newtype wrappers
  around `String`. Internal-format validation (the RFC 8984 ABNF) is left
  to the backend — these types document intent at the type level but do
  not parse the value.
- `AlertTrigger` is an enum with a third `Unknown(serde_json::Value)`
  variant that catches any `@type` value outside `OffsetTrigger` /
  `AbsoluteTrigger`. This is independent of the workspace
  extras-preservation policy and exists because `#[serde(tag = "@type",
  other)]` with tuple variants is not supported by serde.
- The Sloppy-Value pattern in `jmap-calendars-types` (per the workspace
  `AGENTS.md`) means consumers see some calendar-event fields as
  `serde_json::Value` rather than typed sub-types from this crate. To
  reach the typed shape, deserialise the value through one of the types
  here (e.g. `serde_json::from_value::<RecurrenceRule>(...)`).

## References

- [RFC 8984] — JSCalendar (normative for every type in this crate)
  - §1.4 — common data types (LocalDateTime, Duration, SignedDuration, Link, Relation)
  - §4.2 — Location, VirtualLocation
  - §4.3 — RecurrenceRule, NDay
  - §4.4 — Participant
  - §4.5 — Alert, OffsetTrigger, AbsoluteTrigger

[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984

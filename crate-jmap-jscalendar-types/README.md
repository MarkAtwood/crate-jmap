# jmap-jscalendar-types

RFC 8984 JSCalendar typed sub-types for the `jmap-*` crate family.

Consumed by `jmap-calendars-types` and `jmap-tasks-types` (planned).
Pure data types: no method handlers, no async, no network I/O.

## What

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

## Dependencies

```toml
jmap-jscalendar-types = "0.1"
```

Transitively pulls in `jmap-types`, `serde`, `serde_json`.

## License

MIT OR Apache-2.0

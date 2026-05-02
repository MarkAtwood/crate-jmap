# jmap-calendars-server — Implementation Plan

Backend-agnostic JMAP Calendars method handlers.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt`

## Crate Family Position

```
jmap-types
    └── jmap-calendars-types
            └── jmap-calendars-server  ← this crate
```

## What This Crate Is

Method handlers for JMAP Calendars, plugged into `jmap-server::Dispatcher` via
`register_calendars_handlers(dispatcher, backend)`.

## Methods (draft-ietf-jmap-calendars-26)

- `Calendar/get`, `Calendar/set`, `Calendar/changes`, `Calendar/query`
- `CalendarEvent/get`, `CalendarEvent/set`, `CalendarEvent/changes`, `CalendarEvent/query`, `CalendarEvent/queryChanges`, `CalendarEvent/copy`
- `CalendarEventNotification/get`, `CalendarEventNotification/changes`, `CalendarEventNotification/query`
- `ParticipantIdentity/get`, `ParticipantIdentity/set`

## Backend Trait

```rust
pub trait CalendarsBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn get_objects<O>(...) -> Result<...>;
    async fn set_objects<O>(...) -> Result<...>;
    // etc.
}
```

## Module Layout

```
src/
  lib.rs            CalendarsBackend trait, register_calendars_handlers
  calendar.rs       Calendar/get, set, changes, query
  event.rs          CalendarEvent/get, set, changes, query, queryChanges, copy
  notification.rs   CalendarEventNotification/get, changes, query
  participant.rs    ParticipantIdentity/get, set
  backend.rs        CalendarsBackend trait, error types
  helpers.rs        shared utilities
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-server/` — identical handler structure.

# jmap-calendars-client — Implementation Plan

JMAP Calendars method implementations on top of `jmap-base-client`.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt`

## Crate Family Position

```
jmap-types
    └── jmap-base-client
            └── jmap-calendars-client  ← this crate
```

## What This Crate Is

Extension trait `JmapCalendarsExt` over `jmap_base_client::JmapClient` that adds typed
methods for every JMAP Calendars operation.

## Planned Public API

```rust
pub trait JmapCalendarsExt {
    async fn calendar_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<Calendar>, ClientError>;
    async fn calendar_event_get(&self, account_id: &Id, ids: Option<&[Id]>, props: &[&str])
        -> Result<GetResponse<CalendarEvent>, ClientError>;
    async fn calendar_event_set(&self, account_id: &Id, req: SetRequest<CalendarEvent>)
        -> Result<SetResponse<CalendarEvent>, ClientError>;
    // ... all Calendar, CalendarEvent, Notification, ParticipantIdentity methods
}
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern.

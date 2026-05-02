# jmap-calendars-types — Implementation Plan

Data types for the JMAP Calendars extension.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-calendars-26.txt`

## Crate Family Position

```
jmap-types
    └── jmap-calendars-types  ← this crate
            ├── jmap-calendars-server
            └── jmap-calendars-client
```

## What This Crate Is

Serde-serializable data types for JMAP Calendars:
- `Calendar` — container for events (analogous to Mailbox for email)
- `CalendarEvent` — JSCalendar Event object (RFC 8984)
- `CalendarEventNotification` — change notifications for events
- `CalendarPreferences` — per-account calendar settings

No async, no I/O. Depends only on `jmap-types` and `serde`.

## Key Types (draft-ietf-jmap-calendars-26)

- `Calendar` — `id`, `name`, `color`, `isDefault`, `isSubscribed`, `isVisible`, `shareWith`, `myRights`
- `CalendarEvent` — JSCalendar Event (RFC 8984): `uid`, `title`, `start`, `duration`, `timeZone`, `participants`, `recurrenceRules`, etc.
- `CalendarEventNotification` — `id`, `calendarEventId`, `changedBy`, `comment`, `changes`
- `ParticipantIdentity` — `id`, `name`, `email`

## Source Material

The JSCalendar Event format is defined in RFC 8984. The JMAP Calendars binding is in
draft-ietf-jmap-calendars-26. This is a very mature draft (rev 26) close to publication.

# jmap-tasks-types — Implementation Plan

Data types for the JMAP Tasks extension.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-tasks-06.txt`

## Crate Family Position

```
jmap-types
    └── jmap-tasks-types  ← this crate
            ├── jmap-tasks-server
            └── jmap-tasks-client
```

## What This Crate Is

Serde-serializable data types for JMAP Tasks:
- `TaskList` — container for tasks (analogous to Mailbox for email, Calendar for calendars)
- `Task` — JSCalendar Task object (RFC 8984 §8)

No async, no I/O. Depends only on `jmap-types` and `serde`.

## Key Types (draft-ietf-jmap-tasks-06)

Tasks reuses JSCalendar Task (RFC 8984 §8) as its wire format. The JMAP binding adds:

- `TaskList` — `id`, `name`, `color`, `isSubscribed`, `isVisible`, `shareWith`, `myRights`
- `Task` — JSCalendar Task: `uid`, `title`, `due`, `estimatedDuration`, `status`, `progress`,
  `progressUpdated`, `keywords`, `categories`, `priority`, `recurrenceRules`, `participants`
- `TaskNotification` — change notifications

## Source Material

JSCalendar Task is defined in RFC 8984 §8. The JMAP Tasks binding is in
draft-ietf-jmap-tasks-06. This is an earlier draft (rev 06) and the API surface may change.
The spec deliberately mirrors JMAP Calendars wherever possible.

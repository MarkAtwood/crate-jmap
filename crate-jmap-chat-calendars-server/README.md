# jmap-chat-calendars-server

The Chat binding for `jmap-calendars-server`: the rules of
draft-atwood-jmap-chat-calendars-00 that apply when one server offers
`urn:ietf:params:jmap:chat:calendars` with `urn:ietf:params:jmap:chat` and
`urn:ietf:params:jmap:calendars`. `jmap-calendars-server` and
`jmap-chat-server` do not depend on each other; this crate depends on the
Calendars server and the Chat types.

## What it is

| Item | Spec |
|---|---|
| `may_advertise` | chat-calendars §Capability: advertise the capability only when the account also advertises `urn:ietf:params:jmap:chat` and `urn:ietf:params:jmap:calendars` |
| `space_extension_properties` | chat-calendars §Account-Level Capability Object `mayBindCalendar`: register `calendarId` as a Space extension property only when binding is allowed |
| `check_calendar_id` | chat-calendars §calendarId, §Binding and unbinding, §Federation: `null` unbinds; otherwise the value must name a Calendar on the Space's own account (`invalidProperties` naming `calendarId`) |
| `create_space_calendar` | chat-calendars §Binding and unbinding auto-bind on create: create a Calendar and set the Space's `calendarId` before it is stored |

## What it's for

A deployment that implements both `CalendarsBackend` and `ChatBackend`
calls these functions:

| Where | Call |
|---|---|
| Session object, per account | `may_advertise` |
| `ChatBackend::space_extension_properties` | `space_extension_properties` |
| `ChatBackend::create_object::<Space>` and `apply_space_metadata_patch`, for a `calendarId` value | `check_calendar_id` |
| `ChatBackend::create_object::<Space>`, before the insert, for auto-bind | `create_space_calendar` |

## How to use

```rust,ignore
// Inside ChatBackend::apply_space_metadata_patch, after the manage_space gate:
if let Some(value) = patch.extra.get(SPACE_CALENDAR_ID_PROPERTY) {
    check_calendar_id(&self.calendars, &calendars_caller, account_id, value)
        .await
        .map_err(|e| BackendSetError::Other(e.into()))?
        .map_err(BackendSetError::SetError)?;
}
```

## How it works

`calendarId` travels through the `jmap-chat-server` Space extension hook:
it is stored in `Space::extra` on create and patched through
`apply_space_metadata_patch`, whose `manage_space` gate covers the
draft's "members holding the `"manage_space"` permission". This crate only
checks the value against the Calendars backend.

## Gotchas

- Space destroy has no hook here: the Calendar is never destroyed with
  the Space (§Binding and unbinding "SHOULD NOT be automatically
  destroyed"). `Space/set` carries no way to request it; a member destroys
  the Calendar with `Calendar/set`.
- `check_calendar_id` reads with the caller's Calendars context, so a
  Calendar the caller cannot see is refused like a missing one. The read
  is not atomic with the Space write.
- With `mayBindCalendar: false`, `Space/set` create drops a `calendarId`
  rather than refusing it; update refuses it (JMAP-zmqp).
- `create_space_calendar` keeps a client-supplied `calendarId` (check it
  first) and names a new Calendar after the Space.

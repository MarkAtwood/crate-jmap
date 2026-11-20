# JMAP-Net/JMAP.Net issue (filed 2026-11-20 as https://github.com/JMAP-Net/JMAP.Net/issues/1)

**Title:** CalendarRights has `mayAdmin` instead of `mayShare`

`Capabilities/Calendars/Types/CalendarRights.cs` serializes `mayAdmin`, and
the `calendar.json` fixture carries it in `myRights` and `shareWith`. The
draft this library targets defines no `mayAdmin` on CalendarRights; it
defines `mayShare`:

- draft-ietf-jmap-calendars-26 (in this repo's `rfcs/`), CalendarRights:
  "*mayShare*: Boolean / The user may modify the "shareWith" property
  for this calendar."
- draft-ietf-jmap-calendars-34 keeps `mayShare` and has no `mayAdmin`.

A server that follows the draft omits `mayAdmin` and sends `mayShare`, so
JMAP.Net would drop that right, and a JMAP.Net-built object is missing a
property other implementations require. (`mayAdmin` is the Tasks and
AddressBook spelling, which may be where it came from.)

Found while using the fixtures as cross-implementation test data in
crate-jmap (Rust). The other fixtures we checked (Core protocol shapes,
Principal, ShareNotification, CalendarEvent, CalendarEventNotification)
agree with our reading of the drafts, apart from -26 vs -34 revision
differences.

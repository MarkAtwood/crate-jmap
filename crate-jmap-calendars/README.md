# jmap-calendars

JMAP Calendars ([draft-ietf-jmap-calendars-34]) in one dependency: re-exports
[`jmap-calendars-types`] always, [`jmap-calendars-client`] with the `client`
feature and [`jmap-calendars-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_calendars::types` |
| `client` | `jmap_calendars::client` |
| `server` | `jmap_calendars::server` |
| `memory` | `server`, plus `jmap_calendars::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapCalendarsExt` extension trait over
  `jmap-base-client`.
- `server`: method handlers for `Calendar/*`, `CalendarEvent/*` and the other Calendars methods over `CalendarsBackend`.

## What it's for

Calendars hold JSCalendar events, synchronised and shared over JMAP. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Bridges to other families (for example the
`jmap-chat-*-server` crates) are not re-exported; depend on them directly.

## How to use

```toml
[dependencies]
jmap-calendars = { version = "0.5", features = ["client"] }
```

```rust
use jmap_calendars::types::JMAP_CALENDARS_URI;

assert_eq!(JMAP_CALENDARS_URI, "urn:ietf:params:jmap:calendars");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_calendars::server::X` is the same item as `jmap_calendars_server::X`.
The component crates version in lockstep with this one on the shared
`jmap-*` line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-calendars-34] — JMAP Calendars
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-calendars-34]: https://datatracker.ietf.org/doc/draft-ietf-jmap-calendars/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-calendars-types`]: https://crates.io/crates/jmap-calendars-types
[`jmap-calendars-client`]: https://crates.io/crates/jmap-calendars-client
[`jmap-calendars-server`]: https://crates.io/crates/jmap-calendars-server

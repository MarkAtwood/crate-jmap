# jmap-object-history-server

Helpers for JMAP server backends that keep object history
(draft-ietf-jmap-object-history): which versions a history `Foo/get`
returns, a per-object version log with retention, and how a restore shows
up in `Foo/changes`. Scaffold only for now.

## What it is

An empty crate reserving the place of the pure, synchronous helpers
planned in `docs/design/object-history.md` §7: `select_versions`,
`VersionLog`, retention, the §8.2 rights cut and `fold_restore`
(bd JMAP-tna1.3.2). No handlers, no backend trait, no async.

## What it's for

Reference backends (the `memory` features of the data-type server crates)
use it to implement `jmap_server::history::ObjectHistoryBackend`
consistently. Production backends may use it or keep history in their own
storage.

## How to use

Nothing to call yet. Depend on it from a backend once bd JMAP-tna1.3.2
lands.

## How it works

It will work on the wire types of `jmap-object-history-types` and depend
on nothing async, so the `jmap-server` handlers never depend on it.

## Gotchas

- Scaffold: the crate has no public items yet.

## References

- [draft-ietf-jmap-object-history-03] — JMAP Object History
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-object-history-03]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

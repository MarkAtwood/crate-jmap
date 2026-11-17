# jmap-object-history-client

Client bindings for JMAP Object History (draft-ietf-jmap-object-history)
over `jmap-base-client`: reading earlier and destroyed versions,
searching destroyed objects and restoring them. Scaffold only for now.

## What it is

An empty crate reserving the place of the extension trait planned in
`docs/design/object-history.md` §8 (bd JMAP-tna1.3.12): a sealed
`JmapObjectHistoryExt::with_object_history_session` returning a
`SessionClient` with generic history `Foo/get`, destroyed `Foo/query` and
restore on the base client's `SetCall`.

## What it's for

Applications that show an object's history or offer undelete against a
server advertising `urn:ietf:params:jmap:object-history`.

## How to use

Nothing to call yet; see bd JMAP-tna1.3.12.

## How it works

It will add the object-history URI to `using` and build on
`jmap-object-history-types` for every wire value.

## Gotchas

- Scaffold: the crate has no public items yet.

## References

- [draft-ietf-jmap-object-history-03] — JMAP Object History
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-object-history-03]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

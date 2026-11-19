# jmap-search

A reserved name for the planned JMAP search extension: a generic
filter-expression language for every JMAP data type's `/query`. This
release has **no public items and no dependencies**; it holds the name on
the shared `jmap-*` version line until the extension's crates exist.

## What it is

An empty crate. Nothing is re-exported yet, because the
`jmap-search-types`, `jmap-search-client` and `jmap-search-server` crates
it is meant to re-export are not in the workspace. When they are, this
crate takes the umbrella shape of `jmap-quotas`:

| Feature | Re-export (planned) |
|---|---|
| (none) | `jmap_search::types` |
| `client` | `jmap_search::client` |
| `server` | `jmap_search::server` |

## What it's for

The planned extension adds a `where` property to the FilterCondition of
each listed type's `/query`: field tests as JSON Pointers into the `/get`
representation, comparison operators (`$eq`, `$ne`, `$gt`, `$gte`, `$lt`,
`$lte`, `$in`, `$nin`, `$exists`, `$prefix`, `$contains`), array rules,
logical operators (`$and`, `$or`, `$not`), `$collation` and `$text`, under
a capability of its own. It generalises the graph-independent part of
[draft-atwood-jmap-graph-search-00], so one filter language covers every
JMAP type, and Graph search keeps only its structural operators.

## How to use

There is nothing to use yet. For Graph search today, depend on
[`jmap-graph`] (or `jmap-graph-types` and `jmap-graph-server`) and use
their `search` modules, which implement
`urn:ietf:params:jmap:graph:search`.

## How it works

It does not work yet: `src/lib.rs` holds only crate documentation. The
design is tracked in the workspace's issue tracker (bd JMAP-nn3i).

## Gotchas

- Depending on this crate gives you nothing. Do not add it to a build
  expecting search types; they are not here.
- The capability URI of the planned extension is not settled, so this
  crate defines no `CAPABILITY` constant.
- The 0.0.1 placeholder named `jmap-search-types`, `-client` and `-server`
  0.0.1 as dependencies; 0.5.0 drops them.

## References

- [draft-atwood-jmap-graph-search-00] — JMAP Graph Search (the source of
  the filter language)
- [RFC 8620] — JMAP Core (`/query`, §5.5)
- [RFC 6901] — JSON Pointer

[draft-atwood-jmap-graph-search-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6901]: https://www.rfc-editor.org/rfc/rfc6901
[`jmap-graph`]: https://crates.io/crates/jmap-graph

# jmap-search

JMAP Search (`urn:ietf:params:jmap:search`): one filter language for the
`/query` of every JMAP data type. This crate holds the capability object,
the expression AST, the checks a server runs before evaluating, and
(feature `eval`) the evaluator.

## What it is

The type-neutral core of the `where` language specified by the local
draft [docs/specs/jmap-search.md] (draft-atwood-jmap-search-00), which is
the built-in language of [draft-atwood-jmap-graph-search-00] without its
structural operators:

| Item | Spec section |
|---|---|
| `JMAP_SEARCH_URI`, `SearchCapability`, `DataTypeSearchInfo` | §Capability |
| `Expression` (`parse`, `to_value`, `depth`, `visit`), `FieldTest`, `Bound`, `is_json_pointer` | §The Filter Language, §Nesting Depth |
| `WhereError` → `invalidArguments` / `unsupportedFilter`, `QUERY_TOO_COMPLEX` | §Errors |
| `check_where`, `check_where_filter`, `COLLATIONS`, `is_supported_collation` | §Errors, §Opting In a Data Type, §Collation |
| `where_values`, `filter_uses_where` | §The where FilterCondition Property |
| `Candidate`, `Expression::matches`, `FieldTest::matches`, `collate` (feature `eval`) | §Comparison Operators, §Arrays, §Logical Operators, §Collation, §Full-Text Matching |

## What it's for

- Servers: check every `where` of a `/query` filter against the account's
  capability and the request's `using`, then evaluate it over the objects
  the caller may fetch. `jmap-mail-server` (Email) and `jmap-chat-server`
  (Message) do this through their `search_capability` backend hooks.
- Clients: build expressions and add the capability to `using` when a
  filter sets `where` (`jmap-mail-client`, `jmap-chat-client`).
- Graph search: `jmap-graph-types` and `jmap-graph-server` build the
  draft-atwood-jmap-graph-search-00 language (structural operators,
  `GraphView`) on `FieldTest`, `Bound`, `WhereError`, `QUERY_TOO_COMPLEX`
  and the evaluator here.

## How to use

```rust
use jmap_search::{DataTypeSearchInfo, SearchCapability, check_where_filter};
use serde_json::json;

let cap = SearchCapability::new(8).with_data_type("Email", DataTypeSearchInfo::new(true));
let filter = json!({ "inMailbox": "M1", "where": { "/keywords/$flagged": true } });
let using = vec!["urn:ietf:params:jmap:search".to_owned()];
assert_eq!(check_where_filter(Some(&filter), "Email", &using, Some(&cap)), Ok(true));
```

With feature `eval`, evaluate a checked expression with
`Expression::matches` over a `Candidate`: `serde_json::Value` is one (fields
by JSON Pointer, `$text` over every string), or implement the trait to give
`$text` the type's own meaning (Email's RFC 8621 `text`).

## How it works

`Expression::parse` walks the JSON once and reports the first defect as
`WhereError::Malformed` (`invalidArguments`) or
`WhereError::UnknownOperator` (`unsupportedFilter`), so a future operator
reads as unsupported rather than malformed. `check_where` adds the
capability checks: the type in `dataTypes`, `$text` against
`supportsText`, `$collation` in `COLLATIONS`, and nesting depth against
`maxDepth` (`queryTooComplex`). The evaluator applies the array rule
(non-array operand tested per element, not recursively) and prepares
strings per collation (`i;ascii-casemap`, `i;octet`, `i;unicode-casemap`)
before comparing them as code points.

## Gotchas

- `where` is raw JSON in each family's FilterCondition (`where_`), not an
  `Expression`, because serde cannot tell `invalidArguments` from
  `unsupportedFilter`.
- `COLLATIONS` is a fixed list, not the consumer's `collationAlgorithms`.
- The evaluator has no index or work budget; each candidate is scanned.
  Servers that need one refuse expensive queries with `queryTooComplex`.
- GraphNode and GraphEdge are not listed in `dataTypes` (the draft says
  MUST NOT): they use `urn:ietf:params:jmap:graph:search`.
- The draft is local to this repository for now; its home may move.

## References

- [docs/specs/jmap-search.md] — JMAP Search (draft-atwood-jmap-search-00)
- [draft-atwood-jmap-graph-search-00] — JMAP Graph Search
- [RFC 8620] — JMAP Core (`/query`, §5.5; `/queryChanges`, §5.6)
- [RFC 6901] — JSON Pointer
- [RFC 4790], [RFC 5051] — collations

[docs/specs/jmap-search.md]: https://github.com/MarkAtwood/crate-jmap/blob/main/docs/specs/jmap-search.md
[draft-atwood-jmap-graph-search-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6901]: https://www.rfc-editor.org/rfc/rfc6901
[RFC 4790]: https://www.rfc-editor.org/rfc/rfc4790
[RFC 5051]: https://www.rfc-editor.org/rfc/rfc5051

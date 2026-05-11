# jmap-metadata-client — Implementation Plan

Client-side method bindings for the JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01]) on top of `jmap-base-client`. Cookie-cut
from `jmap-mail-client` (canonical extension-client template per workspace
`AGENTS.md`).

## Crate Family Position

```
jmap-types
    ├── jmap-metadata-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-metadata-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for
each `draft-ietf-jmap-metadata-01` operation:

| Method | Function | Returns |
|---|---|---|
| `Metadata/get` | `metadata_get` | `GetResponse<Metadata>` |
| `Metadata/changes` | `metadata_changes` | `ChangesResponse` |
| `Metadata/set` | `metadata_set` | `SetResponse<Metadata>` |
| `Metadata/query` | `metadata_query` | `QueryResponse` |
| `Metadata/queryChanges` | `metadata_query_changes` | `QueryChangesResponse` |

All methods follow the standard five-step pattern documented in
`src/methods/metadata.rs`.

## What This Crate Is Not

- Not a server-side crate (see `jmap-metadata-server`).
- Not a standalone HTTP client (no auth, no transport — that's
  `jmap-base-client`).
- Not the implementation of the `/get` / `/set` argument extensions
  defined in draft §4 (`fetchMetadata`, `metadataTypes`,
  `metadataProperties`, `metadataChanges`, `metadataFilter`) on other
  data types' methods. Those extensions extend e.g. `Email/get` and
  `Mailbox/get`, and are tracked separately by bd JMAP-06zp.5
  (consumer-crate integration).

## Module Layout

```
src/
  lib.rs                  pub trait JmapMetadataExt; impl for JmapClient;
                          re-exports of response/parameter types.
  methods/
    mod.rs                SessionClient, build_request, USING_METADATA,
                          MetadataChangesParams, response type re-exports.
    metadata.rs           5 method impls on SessionClient: metadata_get,
                          metadata_changes, metadata_set, metadata_query,
                          metadata_query_changes.
tests/
  helpers.rs              wiremock-backed make_session / make_client.
  metadata_tests.rs       wiremock end-to-end tests, one per method, plus
                          a focused test for the §3.3 filter args.
```

## Method-specific Notes

### Metadata/changes — filter extras

Per draft §3.3, `Metadata/changes` accepts two extra optional
arguments beyond the standard RFC 8620 §5.2 set:

- `filterRelatedType: String|null` — restrict the response's
  `created`/`updated`/`destroyed` arrays to Metadata whose
  `relatedType` equals the value.
- `filterMetadataType: String[]|null` — restrict to Metadata whose
  `@type` is in the array.

These are surfaced via `MetadataChangesParams`. Per §3.3 the response's
state token is independent of the filters; callers MUST re-use the same
filter values across subsequent `Metadata/changes` calls to ensure
consistent synchronisation. The crate enforces nothing here — it is the
caller's responsibility per spec.

### Metadata/set — no method-level extras

Per draft §3.1, `Metadata/set` is a standard RFC 8620 §5.2 `/set`
call with no additional method-level arguments. Uniqueness, quota,
`maySetPrivate` gating, and related-object validation are all
server-side concerns reported back via the standard SetError catalogue.

### Metadata/query — filter / sort / sort-property contract

Filter and sort are passed through as `serde_json::Value` so callers
can mix typed `MetadataFilterCondition` with `FilterOperator<T>`
algebra without the API forcing a single type. The canonical typed
filter is `jmap_metadata_types::MetadataFilterCondition` (draft §3.4.1).

Per draft §3.4.2, the `id` property MUST be supported for sorting on
every server; `@type`, `relatedType`, `relatedId`, and `isPrivate`
SHOULD be supported. This crate does not constrain `sort` to those
values — clients are free to pass server-specific sort properties.

## Extras-preservation policy (JMAP-lbdy)

This crate has one in-scope locally-defined wire-format struct:
`MetadataChangesParams`. It carries the standard
`#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>` field per the
workspace policy.

Wire-format types reach callers through re-exports rather than locally
defined types:

- Standard response wrappers (`GetResponse<T>`, `SetResponse<T>`,
  `ChangesResponse`, `QueryResponse`, `QueryChangesResponse`) are
  re-exported from `jmap-types` and carry their own `extra` per
  JMAP-lbdy.1.
- The `Metadata` data type is defined in `jmap-metadata-types` and
  carries `extra` on each variant per the JMAP-lbdy policy applied
  to that crate.

### New-type rule

If a future method requires a locally-defined method-argument or
method-response struct, that new struct MUST include the `extra` field
from day one and at least one round-trip preservation test, mirroring
`MetadataChangesParams`.

## Test Strategy

- All tests use `wiremock` via `jmap-base-client`'s HTTP layer — no
  live network.
- Inline tests in `src/methods/mod.rs` cover `build_request`,
  `USING_METADATA`, `session_parts`, response-type deserialisation
  oracles from RFC 8620 examples, and `MetadataChangesParams` serialise
  shape.
- Inline tests in `src/methods/metadata.rs` cover `Annotation` and
  `Metadata` round-trip from spec-example JSON.
- `tests/metadata_tests.rs` covers each of the 5 methods end-to-end:
  request shape verification + response deserialisation. Oracles are
  hand-written JSON literals taken from the draft and RFC 8620 — never
  derived from the code under test.

## References

- **[draft-ietf-jmap-metadata-01]** — JMAP Object Metadata (normative
  for all method names, argument shapes, and response formats).
- **[RFC 8620]** — JMAP Core (request format, response shapes, `/get`,
  `/set`, `/changes`, `/query`, `/queryChanges`).

[draft-ietf-jmap-metadata-01]: https://www.ietf.org/archive/id/draft-ietf-jmap-metadata-01.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## bd tracker

`JMAP-06zp.4` — this crate's creation task under the parent epic
`JMAP-06zp` (implement `draft-ietf-jmap-metadata` as a new
`jmap-metadata-*` crate family).

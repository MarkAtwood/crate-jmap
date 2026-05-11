# jmap-metadata-types — Implementation Plan

JMAP Object Metadata extension (draft-ietf-jmap-metadata-01) data types.
Types only — no method handlers, no async, no network I/O. This crate sits
between `jmap-types` (shared JMAP base primitives) and the planned
`jmap-metadata-server` / `jmap-metadata-client`.

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-metadata-types  ← this crate
            ├── jmap-metadata-server (planned, bd JMAP-06zp.3)
            └── jmap-metadata-client (planned, bd JMAP-06zp.4)
```

## Why this crate exists

The base JMAP spec has no metaprotocol mechanism for a server to declare
what vendor extras it supports or whether they can be filtered.
`draft-ietf-jmap-metadata-01` is the IETF-track answer:

- A new capability URI `urn:ietf:params:jmap:metadata` with discoverable
  scope (`dataTypes`, `metadataTypes`, `maxDepth`, `maySetPrivate`).
- A `Metadata` companion object (three flavours: `Annotation`,
  `ImapMetadata`, `WebDavMetadata`) keyed by `(relatedType, relatedId)`.
- A `Metadata/query` filter that includes `textMatch` over vendor
  string properties — coarse but standardised filterability.

This crate is the foundation of the workspace's implementation tracker
**bd JMAP-06zp**.

## What This Crate Covers

| Module | Type(s) | Draft section |
|---|---|---|
| `metadata.rs` | `Metadata`, `Annotation`, `ImapMetadata`, `WebDavMetadata` | §2 |
| `capability.rs` | `MetadataCapability`, `JMAP_METADATA_URI` | §1.2.1 |
| `filter.rs` | `MetadataFilterCondition` | §3.4.1 |
| `backend.rs` | `MetadataProperty`, `JmapObject` / `GetObject` / `SetObject` / `QueryObject` impls | (internal) |

Generic query types (`Filter<T>`, `FilterOperator<T>`, `Operator`) live
in `jmap-types::query` because they are defined by RFC 8620 §5.5, not
this draft.

## What Is Out of Scope

- Method handlers (`Metadata/get`, `Metadata/set`, etc.) — those live in
  `jmap-metadata-server` (bd JMAP-06zp.3).
- The `/get` and `/set` argument extensions (`fetchMetadata`,
  `onSuccessCreateMetadata`, `onSuccessUpdateMetadata`, the
  `metadata: Metadata[]` response field) — those belong in the
  consumer crates (mail-server, calendars-server, etc.) because the
  argument names are namespaced to each data type's `/get` and `/set`
  methods. Tracked separately as bd JMAP-06zp.5 (consumer-crate
  integration).
- Transport and network I/O — no tokio, no reqwest.

## Full Type Reference

### Metadata (tagged union, §2.1)

`Metadata` is a `#[serde(tag = "@type")]` internally-tagged enum over
three concrete object types. The wire `@type` discriminator is
generated/consumed automatically; each variant struct does NOT
redeclare `@type` as a field.

| Variant | `@type` wire value | Section |
|---|---|---|
| `Annotation` | `"Annotation"` | §2.1.1 |
| `ImapMetadata` | `"ImapMetadata"` | §2.1.2 |
| `WebDavMetadata` | `"WebDavMetadata"` | §2.1.3 |

Additional metadata types MAY be defined by future specifications
(§2.1). New variants are non-breaking due to `#[non_exhaustive]`.

#### Common properties (§2.2.1)

All three variants carry the same five common properties:

| Field | Rust field | Wire | Notes |
|---|---|---|---|
| `@type` | (enum tag) | mandatory String | per §2.2.1.1 |
| `id` | `id: Option<Id>` | server-set Id | absent on create requests; present in responses |
| `relatedType` | `related_type: String` | mandatory String | type name of the related object |
| `relatedId` | `related_id: Id` | mandatory Id | id of the related object |
| `isPrivate` | `is_private: Option<bool>` | optional bool, default false | absent when false (§2.2.1.5) |

`relatedType` and `relatedId` constraints (e.g. ImapMetadata requires
`relatedType=="Mailbox"` per §2.1.2; WebDavMetadata accepts specific
WebDAV-backed types per §2.1.3) are NOT enforced at the type level —
they are server-side validation, documented per-variant in rustdoc.

### Annotation (§2.1.1)

Carries five common properties plus a flatten `extra` map for
vendor-specific properties (§2.2.1.6). Vendor properties MUST be
domain-prefixed per spec (e.g. `acme.example.com:color`); the crate
does not enforce the prefix at the type level — the catch-all just
preserves keys verbatim.

### ImapMetadata (§2.1.2)

Carries the five common properties plus `metadata: BTreeMap<String,
String>` (§2.2.2.1) plus `extra` for forward compatibility.
`BTreeMap` ordering keeps round-trip output deterministic.

### WebDavMetadata (§2.1.3)

Carries the five common properties plus `metadata: BTreeMap<String,
String>` (§2.2.3.1, expanded-name keys like
`"{namespace-uri}localname"`) plus `extra`.

### MetadataCapability (§1.2.1)

Account-level capability struct for the `urn:ietf:params:jmap:metadata`
key in `accountCapabilities`. Fields:

| Field | Wire type | Notes |
|---|---|---|
| `dataTypes` | `String[]\|null` | null = all data types |
| `metadataTypes` | `String[]` | accepted `@type` values |
| `maxDepth` | `UnsignedInt\|null` | null = no nesting limit |
| `maySetPrivate` | `Boolean` (default true) | absent → None in Rust; server-side default is true |

`dataTypes` and `maxDepth` are required-and-nullable (always serialise
the `null` literal); `maySetPrivate` is omitted when `None`.

### MetadataFilterCondition (§3.4.1)

Out-of-scope-for-extras per workspace policy (see "Type-design
constraints" below). Field map:

| Filter field | Wire type | Semantics |
|---|---|---|
| `@type` | `String[]` | match if `@type` is in this set |
| `relatedType` | `String` | exact match on `relatedType` |
| `relatedIds` | `Id[]` | match if `relatedId` is in this list (requires `relatedType`) |
| `isPrivate` | `Boolean` | exact match on `isPrivate` |
| `textMatch` | `String` | server-defined text search over vendor string properties |

The `@type` wire field requires `#[serde(rename = "@type")]` because it
is not a valid Rust identifier; the Rust field is named `type_names`.
The `relatedIds`-requires-`relatedType` coupling is a server-side
validation rule and is not enforced at the type level.

Sort properties (§3.4.2): `id` MUST be supported; `@type`,
`relatedType`, `relatedId`, `isPrivate` SHOULD be supported. The
sort comparator uses the standard RFC 8620 `Comparator` shape; this
crate does not define a separate `MetadataComparator` struct because
the standard comparator with `property: String` covers it.

## Key Design Decisions

### Tagged-enum representation

The spec defines three distinct object shapes that share five common
properties and differ in their type-specific payload (`metadata` for
IMAP/WebDAV; vendor extras for Annotation). Two natural Rust shapes
were considered:

- **`#[serde(tag = "@type")]` enum** (selected) — faithful to the
  spec's "three object types" framing, expresses the wire-shape
  discrimination directly, lets each variant carry exactly the
  right fields. Common-properties duplication is the cost.
- **Single struct with `kind: MetadataKind` enum + nested
  payload** (rejected) — collapses three distinct types into one,
  forces `metadata` to be `Option<Map>` even where the spec
  mandates it, doesn't match the wire `@type`-at-top-level shape
  without custom serde.

The tagged enum is the better fit and matches the bead description.

### `Annotation` is Deserialize-fail-on-missing-relatedType

§2.2.1.3 lists `relatedType` as mandatory. The struct enforces this
at the type level. The §7.2 spec example demonstrates that
`metadataProperties` filtering can omit `relatedType` from a
response — that response does NOT deserialise into a
spec-faithful `Annotation`. The crate documents this; clients
consuming partial responses must use `serde_json::Value` or a
project-local partial struct.

### `BTreeMap` for `metadata` field

`ImapMetadata` and `WebDavMetadata` use `BTreeMap<String, String>` for
the `metadata` property to keep round-trip output deterministic.
The spec doesn't mandate ordering; deterministic ordering simplifies
preservation tests and reproducible builds.

### Standalone `Annotation` does not emit `@type`

When constructing an `Annotation` struct directly (not via the
`Metadata` enum), the `@type` discriminator is NOT serialised. This
mirrors how the spec puts the tag on the enclosing `Metadata`
object. Tests cover this explicitly.

## Module Layout

```
src/
  lib.rs        re-exports
  metadata.rs   Metadata enum + Annotation, ImapMetadata, WebDavMetadata
  capability.rs MetadataCapability + JMAP_METADATA_URI
  filter.rs     MetadataFilterCondition
  backend.rs    MetadataProperty + JmapObject trait impls
```

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values
from the code under test. Acceptable sources:

1. Hand-written JSON fixtures constructed directly from
   `draft-ietf-jmap-metadata-01` field descriptions.
2. Literal JSON copied from the §7 example section (whitespace
   normalised).
3. The capability example sketched from §1.2.1's field-by-field
   description.

Tests are named with `_draft_01_` to pin them to the current draft
revision; when the draft revs, expect to update or replace these
tests rather than mutate them silently.

All tests are `#[test]` (no tokio). Round-trip tests verify serde
consistency but are not a substitute for spec-grounded oracle tests.

Key cases covered:

- Capability URI matches the spec's `urn:ietf:params:jmap:metadata`.
- `MetadataCapability` with `null` and explicit `dataTypes` /
  `maxDepth`.
- `Annotation` with vendor-prefixed extras (§7.1 example).
- Partial §7.2 response missing `relatedType` fails to
  deserialise (pinned behaviour).
- §7.5 atomic-create response shape with server-assigned `id`.
- `ImapMetadata` for both `/private/` and `/shared/` namespaces.
- IMAP empty-string entry round-trips.
- `WebDavMetadata` with expanded-name keys including XML content.
- `Metadata` enum dispatches on `@type` for all three known
  variants.
- Unknown / missing `@type` fails to deserialise.
- `MetadataFilterCondition` wire field name `@type` (not
  `typeNames`).
- Filter has no `extra` flatten field; unknown vendor field on a
  filter is silently dropped on round-trip (filter-algebra
  exclusion).
- `JmapObject::TYPE_NAME == "Metadata"` matches IANA §9.2.

## Spec References

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-metadata-01.txt` — normative
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol (Id, State, SetError, /get + /set + /query method shapes)

## Dependencies

```toml
jmap-types = { workspace = true }
serde      = { workspace = true }
serde_json = { workspace = true }
# No tokio, no async, no network deps
```

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

Every public `Deserialize` struct that appears on the JMAP wire
carries an `extra` field per the workspace extras-preservation
policy (see workspace `AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

In scope in this crate (each has a round-trip preservation test):

- `Annotation`, `ImapMetadata`, `WebDavMetadata` (metadata.rs).

Out of scope:

- `MetadataFilterCondition` (filter.rs) — filter algebra, per workspace
  policy "filter algebra excluded". Note: the whole point of the
  Metadata extension is to give vendors a capability-declared
  filterable extras mechanism; vendor extras on a filter clause
  would defeat that.
- `MetadataCapability` (capability.rs) — capability object,
  consistent with the canonical extension-types template treating
  capabilities as Session-shape objects rather than data objects.

### New-type rule

Any new public `Deserialize` struct added to this crate that appears
on the JMAP wire MUST include the `extra` field from day one with
the documented serde attributes and at least one round-trip
preservation test.

### Draft-version risk

`draft-ietf-jmap-metadata-01` is an active IETF Working Group draft
(expires June 2026). The wire format may evolve. All structs and
enums are `#[non_exhaustive]` so adding fields and variants when
the draft revs is non-breaking. Major wire-shape changes between
revisions will require coordinated updates; test names
(`*_draft_01_*`) carry the version pin to make rev-related
breakage easy to spot.

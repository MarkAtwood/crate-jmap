# jmap-metadata-types

Serde-annotated Rust types for the JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01]). Types only — no method handlers,
no async, no network I/O.

## What

| Type | Description |
|---|---|
| `Metadata` | Tagged union of `Annotation`, `ImapMetadata`, `WebDavMetadata` (`@type` discriminator) |
| `Annotation` | General-purpose vendor metadata (§2.1.1) — round-trips arbitrary domain-prefixed properties via `extra` |
| `ImapMetadata` | IMAP METADATA extension mapping (§2.1.2) |
| `WebDavMetadata` | WebDAV dead-property mapping (§2.1.3) |
| `MetadataCapability` | Account-level capability object (§1.2.1) |
| `MetadataFilterCondition` | Filter for `Metadata/query` (§3.4.1) — filter-algebra; not extras-preserving |
| `MetadataProperty` | Property selector enum used by `/get` / `/set` |

Capability URI constant:

| Constant | Value |
|---|---|
| `JMAP_METADATA_URI` | `"urn:ietf:params:jmap:metadata"` |

## Why this crate exists

`jmap-types` and the extension-types crates apply an
[extras-preservation policy](../AGENTS.md) that captures vendor
fields in a flatten `extra` map on every wire-format Deserialize
struct. That policy explicitly **excludes filter and comparator
algebra**: silently round-tripping an unknown filter clause through
the client gives the wrong record set back with no error signal.

`draft-ietf-jmap-metadata-01` is the IETF-track answer. It defines a
companion `Metadata` object keyed by `(relatedType, relatedId)` with
schema discovery via the capability and a `Metadata/query` filter
that includes `textMatch` over vendor string properties — coarse
but standardised filterability that the base extras policy cannot
offer.

This crate is the foundation of the workspace's implementation
tracker, bd JMAP-06zp.

## Spec coverage

**draft-ietf-jmap-metadata-01 sections implemented:**

- §1.2.1 — `MetadataCapability` and `JMAP_METADATA_URI`
- §2.1 — `Metadata` tagged union; three variants
- §2.1.1 / §2.2.1.6 — `Annotation` with vendor-extension support
- §2.1.2 / §2.2.2 — `ImapMetadata` with `/private/` and `/shared/`
  namespace mapping
- §2.1.3 / §2.2.3 — `WebDavMetadata` with expanded-name property keys
- §2.2.1 — Common properties (`id`, `@type`, `relatedType`,
  `relatedId`, `isPrivate`)
- §3.4.1 — `MetadataFilterCondition`
- IANA §9.1 — capability URI string

Not in this crate (tracked separately under bd JMAP-06zp):

- Method handlers `Metadata/get`, `Metadata/set`, `Metadata/changes`,
  `Metadata/query`, `Metadata/queryChanges` (live in
  `jmap-metadata-server`, bd JMAP-06zp.3).
- The standard-method extensions (`fetchMetadata`,
  `onSuccessCreateMetadata`, `onSuccessUpdateMetadata`,
  `metadata: Metadata[]` response field) live in each consumer
  crate's `/get` and `/set` argument structs (bd JMAP-06zp.5).

## Filter-algebra exclusion

`MetadataFilterCondition` does NOT carry a `extra` flatten field — the
workspace extras-preservation policy explicitly excludes filter
algebra (see workspace [AGENTS.md](../AGENTS.md) and bd JMAP-lbdy).
Vendor extras that need to be filterable belong in an `Annotation`
payload and are queried via the spec's own `textMatch` filter, not
via a vendor-extended filter condition.

## Usage

```rust
use jmap_metadata_types::{Annotation, Metadata};

// Deserialize an Annotation (the §7.5 atomic-create response shape).
let meta: Metadata = serde_json::from_str(r#"{
    "@type": "Annotation",
    "id": "MD789",
    "relatedType": "Email",
    "relatedId": "EM456",
    "isPrivate": true,
    "acme.example.com:workflowState": "pending-review"
}"#).unwrap();

match meta {
    Metadata::Annotation(Annotation { related_type, ref extra, .. }) => {
        assert_eq!(related_type, "Email");
        assert_eq!(
            extra.get("acme.example.com:workflowState").and_then(|v| v.as_str()),
            Some("pending-review"),
        );
    }
    _ => unreachable!(),
}
```

## How it works

The `Metadata` enum is `#[serde(tag = "@type")]` — the wire `@type`
discriminator is consumed by serde automatically. Each variant
struct carries the common properties (id, relatedType, relatedId,
isPrivate) as typed fields plus an `extra` flatten map for
vendor / future-spec fields.

All structs carry `#[serde(rename_all = "camelCase")]` to produce
camelCase JSON field names as required by the JMAP wire format.
`MetadataFilterCondition.type_names` is named for clarity in Rust
but serialises as the literal wire name `"@type"` via
`#[serde(rename = "@type")]`.

`ImapMetadata` and `WebDavMetadata` use `BTreeMap<String, String>`
for their `metadata` field so round-trip output is deterministic.

## Draft-version pin

Tests are named with `_draft_01_` to pin them to the current
revision. When draft-ietf-jmap-metadata revs, expect to update or
replace these tests alongside the wire-format changes. The
`#[non_exhaustive]` derive on every struct and enum keeps additive
spec evolution non-breaking.

## References

- **[draft-ietf-jmap-metadata-01]** — JMAP Object Metadata (normative for all type definitions)
- **[RFC 8620]** — JMAP Core (Id, State, request/response shapes,
  `/get`, `/set`, `/query`)

[draft-ietf-jmap-metadata-01]: https://www.ietf.org/archive/id/draft-ietf-jmap-metadata-01.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0

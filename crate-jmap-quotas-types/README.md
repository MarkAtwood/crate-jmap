# jmap-quotas-types

Serde-annotated Rust types for JMAP Quotas ([RFC 9425]): `Quota`, `Scope` and
`ResourceType`. Types only — no method handlers, no async, no network I/O.

## What it is

| Type | Module | Source |
|---|---|---|
| `Quota` | `quota` | RFC 9425 §4.1 |
| `Scope` | `quota` | RFC 9425 §3.1 |
| `ResourceType` | `quota` | RFC 9425 §3.2 |
| `QuotaFilterCondition`, `QuotaFilter` | `quota` | RFC 9425 §4.4 |
| `QuotaComparator`, `QuotaComparatorProperty` | `quota` | RFC 9425 §4.4 |
| `QuotaProperty` | `backend` | RFC 9425 §4.1 — property enum for `Quota/get` |
| `QuotaCapability` | crate root | RFC 9425 §2.1 — capability object (`{}`) |
| `JMAP_QUOTA_URI` const (`"urn:ietf:params:jmap:quota"`) | crate root | RFC 9425 §2.1 |

## What it's for

Consumed by `jmap-quotas-server` (method handlers and the backend trait)
and `jmap-quotas-client` (typed method bindings). A Quota reports an
account's usage of some resource against a limit; RFC 9425 defines
`Quota/get`, `/changes`, `/query` and `/queryChanges`, and no `/set`:
every property is server-set.

## How to use

```rust
use jmap_quotas_types::{Quota, ResourceType, Scope};

let json = r#"{
    "id": "2a06df0d-9865-4e74-a92f-74dcc814270e",
    "resourceType": "count",
    "used": 1056,
    "warnLimit": 1600,
    "softLimit": 1800,
    "hardLimit": 2000,
    "scope": "account",
    "name": "bob@example.com",
    "types": ["Mail", "Calendar", "Contact"]
}"#;

let q: Quota = serde_json::from_str(json).unwrap();
assert_eq!(q.resource_type, ResourceType::Count);
assert_eq!(q.scope, Scope::Account);
assert_eq!(q.soft_limit, Some(1800));
```

## How it works

- All structs carry `#[serde(rename_all = "camelCase")]`; wire names match
  RFC 9425 exactly. The filter condition's `type` is the Rust field
  `type_name`.
- `warnLimit`, `softLimit` and `description` are optional
  (`UnsignedInt|null` / `String|null`): `null` and absent both decode to
  `None`, and `None` is omitted on the wire.
- `Scope`, `ResourceType` and `QuotaComparatorProperty` keep unknown values
  in an `Other(String)` variant so they round-trip.
- `Quota` and `QuotaCapability` keep unknown fields in `extra`. The filter
  condition rejects unknown fields (`deny_unknown_fields`), the workspace
  filter policy (bd JMAP-lbdy): an unknown filter clause must fail, not be
  ignored.

## Gotchas

- `types` holds names from the IANA "JMAP Data Types" registry. A server
  filters it to the types whose capability the request's `using` lists,
  and leaves out a Quota left with none (RFC 9425 §4.1); that is a server
  concern, not this crate's.
- A `/get` with `properties` returns partial objects, which do not decode
  as `Quota` (the RFC 9425 §5.2 response lists only `id` and `used`).
  Decode those as `serde_json::Value`.

## Crate family

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-quotas-types  ← this crate
            ├── jmap-quotas-server (method handlers)
            └── jmap-quotas-client (client extension trait)
```

## References

- **[RFC 9425]** — JMAP for Quotas
- **[RFC 8620]** — JMAP Core (request format, Id, Filter, Comparator)

[RFC 9425]: https://www.rfc-editor.org/rfc/rfc9425
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

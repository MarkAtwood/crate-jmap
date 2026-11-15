# jmap-quotas-server

Backend-agnostic JMAP Quotas ([RFC 9425]) method handlers for the
`jmap-server` dispatcher: `Quota/get`, `Quota/changes`, `Quota/query` and
`Quota/queryChanges`.

## What it is

| Item | Purpose |
|---|---|
| `register_quotas_handlers` | Registers the four `Quota/*` methods on a `Dispatcher` |
| `handle_quota_get` / `_changes` / `_query` / `_query_changes` | The handlers, callable directly |
| `QuotasBackend` | The storage trait (a `JmapBackend` plus `recognized_types`) |
| `registry_types` | The default `recognized_types`: registered type names whose capability is in `using` |
| `memory::MemoryBackend` | In-memory reference backend (feature `memory`) |

## How to use

```rust
use std::sync::Arc;
use jmap_quotas_server::{memory::MemoryBackend, register_quotas_handlers};
use jmap_server::Dispatcher;
use jmap_types::Id;

let backend = MemoryBackend::new().with_account(&Id::from("u1"));
backend.seed_quota(&Id::from("u1"), serde_json::json!({
    "id": "q1", "resourceType": "octets", "used": 1024, "hardLimit": 4096,
    "scope": "account", "name": "bob@example.com", "types": ["Email"]
}));
let mut dispatcher = Dispatcher::new();
register_quotas_handlers(&mut dispatcher, Arc::new(backend));
```

Advertise `urn:ietf:params:jmap:quota` with the value `{}` in the Session
`capabilities` and in each account's `accountCapabilities` (RFC 9425 §2.1).

## How it works

RFC 9425 §4.1: "The server MUST filter out any types for which the client
did not request the associated capability in the "using" section of the
request. Further, the server MUST NOT return Quota objects for which there
are no types recognized by the client."

- `/get` cuts each Quota's `types` to the recognized types and leaves out a
  Quota left with none; one the client asked for by id goes in `notFound`.
- `/query` and `/queryChanges` AND the client's filter with an OR of
  `type` conditions on the recognized types, and turn a client `type`
  condition on an unrecognized type into a condition that never matches.
  So `position`, `total` and `queryChanges` count only visible Quotas.
- `/changes` is the generic handler. A backend sets
  `ChangesResult::updated_properties` to `["used"]` when only usage changed
  (§4.3).

A type is recognized when the "JMAP Data Types" registry
(`jmap_types::data_types`) lists its capability in `using`. Override
`QuotasBackend::recognized_types` for private types.

## Gotchas

- The backend's `query_objects` sees a filter on every query, even when
  the client sent none, so it must evaluate `type` and the `AND` / `OR` /
  `NOT` operators.
- RFC 9425 §5.1's example `types` (`Mail`, `Calendar`, `Contact`) are not
  registered type names; only `Calendar` is recognized from them.

## References

- **[RFC 9425]** — JMAP for Quotas
- **[RFC 8620]** — JMAP Core

[RFC 9425]: https://www.rfc-editor.org/rfc/rfc9425
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

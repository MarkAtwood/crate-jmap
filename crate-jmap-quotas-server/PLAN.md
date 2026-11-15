# jmap-quotas-server — Implementation Plan

RFC 9425 Quota method handlers. Tracked by bd JMAP-1igi (epic) and
JMAP-1igi.2.

## Module layout

| Module | Contents | RFC 9425 |
|---|---|---|
| `lib.rs` | `register_quotas_handlers` | §4 |
| `backend.rs` | `QuotasBackend` (`recognized_types`), `registry_types` | §4.1 |
| `quota.rs` | `handle_quota_get` / `_changes` / `_query` / `_query_changes`, the `types` rule | §4.1–§4.5 |
| `memory.rs` (feature `memory`) | `MemoryBackend`: `seed_quota`, `put_quota`, `set_used`, `remove_quota` | §4.3, §4.4 |

## Out of scope

- `Quota/set` — RFC 9425 defines none.
- Localized `name` / `description` and Accept-Language selection — bd
  JMAP-cayv.11.
- Session capability mounting and the testjig — bd JMAP-1igi.4.

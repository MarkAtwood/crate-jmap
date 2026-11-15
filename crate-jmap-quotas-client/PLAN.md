# jmap-quotas-client — Implementation Plan

RFC 9425 client bindings. Tracked by bd JMAP-1igi (epic) and JMAP-1igi.3.

| Module | Contents |
|---|---|
| `lib.rs` | `JmapQuotasExt` (sealed), re-exports |
| `methods.rs` | `SessionClient`: `with_type_capabilities`, `with_i18n`, `quota_get`, `quota_get_properties`, `quota_changes`, `quota_query`, `quota_query_changes` |

Out of scope: localized `name` / `description` (bd JMAP-cayv.11).

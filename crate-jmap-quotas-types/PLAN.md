# jmap-quotas-types — Implementation Plan

RFC 9425 (JMAP for Quotas) data types. Types only — no method handlers,
no async, no network I/O. Tracked by bd JMAP-1igi (epic) and JMAP-1igi.1.

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-quotas-types  ← this crate
            ├── jmap-quotas-server (method handlers, JMAP-1igi.2)
            └── jmap-quotas-client (typed bindings, JMAP-1igi.3)
```

## What This Crate Covers

| Module | Type(s) | RFC 9425 section |
|---|---|---|
| `quota.rs` | `Quota`, `Scope`, `ResourceType`, `QuotaFilterCondition`, `QuotaComparator` | §3, §4.1, §4.4 |
| `capability.rs` | `QuotaCapability`, `JMAP_QUOTA_URI` | §2.1 |
| `backend.rs` | `QuotaProperty`, `JmapObject` / `GetObject` / `QueryObject` impls | §4 |

## What Is Out of Scope

- Method handlers (`Quota/get`, `/changes`, `/query`, `/queryChanges`) and
  the `types` filtering by `using` — `jmap-quotas-server`
- `Quota/set` — RFC 9425 defines none
- Localization of `name` / `description` (draft-atwood-jmap-updates-00) —
  bd JMAP-cayv.11, after the server exists

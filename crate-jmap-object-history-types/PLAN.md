# jmap-object-history-types — Implementation Plan

Design: `docs/design/object-history.md` §3 (placement), §4 (wire types), §10 (tests).
Spec: `~/PROJECT/IETF/specs/draft-ietf-jmap-object-history-03.txt`, plus the -04 editor's copy changes
in the design note §2a.

## Crate Family Position

```
jmap-types (data_types registry: "Supports History")
    └── jmap-object-history-types  ← this crate
            ├── jmap-server (history module)
            ├── the data-type -types crates (impl HistoryObject)
            ├── jmap-object-history-server
            └── jmap-object-history-client
```

## Contents (bd JMAP-tna1.3.1, done)

| Item | Spec |
|---|---|
| `JMAP_OBJECT_HISTORY_URI` | §7.1 |
| `ObjectHistoryCapability`, `ObjectHistoryAccountCapability` | §2.1 |
| `HistoryGetArguments` | §3 |
| `ObjectHistoryInfo`, `HistoryEntry<T>` | §3.1 |
| `DestroyedQueryArguments` | §4 (and queryChanges, -04) |
| `RestoreOutcome` | §5.1 |
| `HistoryObject`, `is_history_type`, `PENDING_HISTORY_REGISTRATION` | §7.2; decisions 5, 14, 23 |

Later: bd JMAP-tna1.3.26 moves Task/TaskList from the pending list to the registry.

## Test Oracle Strategy

Hand-written JSON from the spec text and examples (§3.2.1 verbatim); `is_history_type` against §7.2 Table 1.

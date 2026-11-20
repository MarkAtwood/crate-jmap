# jmap-object-history-types

Wire types for JMAP Object History (draft-ietf-jmap-object-history): the
capability objects, the history arguments of `Foo/get`, `Foo/query` and
`Foo/queryChanges`, the `objectHistory` property, the `restored` and
`notRestored` results of `Foo/set`, and which data types support history.

## What it is

| Item | Spec |
|---|---|
| `JMAP_OBJECT_HISTORY_URI` | §7.1 |
| `ObjectHistoryCapability` (Session value, empty) and `ObjectHistoryAccountCapability` (`new`, `maxHistoryDuration`, `supportedTypes`, `supports::<O>()`) | §2.1 |
| `HistoryGetArguments` (`includeReplaced`, `includeDestroyed`, `historyAfter`, `historyBefore`, `historySkip`, `historyLimit`) | §3 |
| `ObjectHistoryInfo` (`version`, `replaced`, `hasOlder`, `expires`) and `HistoryEntry<T>` (a Foo plus `objectHistory`) | §3.1 |
| `DestroyedQueryArguments` (`onlyDestroyed`, `destroyedAfter`, `destroyedBefore`) | §4; on `Foo/queryChanges` per the -04 editor's copy |
| `RestoreOutcome` (`restored`, `notRestored`) with `from_set_extra` | §5.1 |
| `HistoryObject` marker, `is_history_type`, `PENDING_HISTORY_REGISTRATION` | §7.2 |

## What it's for

`jmap-server`'s history module, the data-type crates (which implement
`HistoryObject` for their history types), `jmap-object-history-server`
and `jmap-object-history-client` all share these types.

## How to use

```rust
use jmap_object_history_types::{ObjectHistoryAccountCapability, is_history_type};

// A server builds the value it advertises; a client deserializes it
// from the Session object.
let cap = ObjectHistoryAccountCapability::new(
    Some(2_592_000),
    vec!["ContactCard".to_owned(), "Email".to_owned()],
);
assert!(cap.supports_type("Email"));
assert!(is_history_type("Mailbox"));
assert!(!is_history_type("Thread"));
```

## How it works

Plain serde structs, every one `#[non_exhaustive]` with a flattened
`extra` for unknown members (except `HistoryEntry`, whose `T` keeps them,
and `RestoreOutcome`, a projection of the `/set` response). Arguments
are left out when they have the spec's default; `replaced`, `expires`
and `maxHistoryDuration` are always written, since `null` means
something. `is_history_type` reads the "Supports History" column of the
`jmap_types::data_types` registry plus the pending list. No async.

## Gotchas

- `ObjectHistoryAccountCapability` has no constructor: build it from
  JSON.
- Task and TaskList are history types ahead of their registration
  (`PENDING_HISTORY_REGISTRATION`): draft-ietf-jmap-tasks-06 registers
  no data types. Mark's updated Tasks draft registers them, and then they
  move to the registry (bd JMAP-tna1.3.26).
- The `restore` request argument is not here: it is the base client's
  generic `SetCall.restore`.
- Follows -03 plus the -04 editor's copy changes listed in
  `docs/design/object-history.md` §2a (queryChanges arguments).

## References

- [draft-ietf-jmap-object-history-03] — JMAP Object History
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-object-history-03]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

# jmap-object-history-server

Helpers for JMAP server backends that keep object history
(draft-ietf-jmap-object-history): which versions a history `Foo/get`
returns, a per-object version log with retention, and how a restore shows
up in `Foo/changes`. Pure and synchronous.

## What it is

| Item | Spec |
|---|---|
| `HistorySelector`, `select_versions`, `Version`, `Selection` | §3 selection: flags, strict `historyAfter`/`historyBefore`, `historySkip` from the newest (live counts), `historyLimit` keeps the newest, `notFound` only when nothing is left after the skip; §3.1 `hasOlder` |
| `VersionLog<V>`, `HistoryRetention`, `DestroyedObject` | §3.1 version numbers, distinct `replaced` (equal-millisecond collapse), `expires`; §2.1 retention, oldest first (decision 24); -04 §3.1 newest retained version is final; §5.1 restore continues the history; §8.2 rights cut |
| `fold_restore`, `ChangeEvent`, `ChangeKind` | -04 §5.1 `Foo/changes` for a restored id |
| `utc_date_from_millis`, `millis_from_utc_date` | RFC 8620 §1.4, millisecond precision (§3.1 SHOULD NOT exceed it) |

No handlers, no backend trait, no async: those live in `jmap-server`.

## What it's for

Reference backends (the `memory` features of the data-type server crates)
use it to implement `jmap_server::history::ObjectHistoryBackend`.
Production backends may use it or keep history in their own storage.

## How to use

```rust
use jmap_object_history_server::{HistorySelector, Selection, VersionLog};
use jmap_types::Id;
use serde_json::json;

let (account, id) = (Id::from("a1"), Id::from("e1"));
let mut log = VersionLog::default();
// The backend calls these from its update and destroy paths.
log.record_replace("Email", &account, &id, json!({"keywords": {}}), 1_000);
log.record_destroy("Email", &account, &id, json!({"keywords": {"$seen": true}}), 2_000);
let sel = HistorySelector::new().include_destroyed(true);
let Selection::Entries(e) = log.select("Email", &account, &id, None, &sel, None).unwrap()
else { unreachable!() };
assert_eq!(e[0].1.version, 2); // only the final version
assert_eq!(log.restore("Email", &account, &id), Some(json!({"keywords": {"$seen": true}})));
```

## How it works

`VersionLog` keeps only replaced and destroyed versions, keyed by (type
name, account, id); the live object stays in the backend's own store and
is passed to `select`. Times are milliseconds since the epoch and the
clock is injected, so tests are deterministic. The log is `Clone`, so a
backend whose conditional rollback restores a snapshot of its state
rolls the history back too.

## Gotchas

- Two versions replaced in the same millisecond are collapsed into the
  newer one, and a clock that goes backwards is held at the last value,
  so `replaced` stays strictly increasing per object.
- `prune` discards oldest first. `discard_version` can discard any
  version; discarding a destroyed object's final version makes the next
  newest the final version (-04 §3.1).
- `fold_restore` returns `None` for an object that exists neither at
  `sinceState` nor now (the -04 SHOULD NOT report), and for no changes;
  the backend's ordinary fold handles ids that were never restored.
- `select` with neither flag set returns a live object's live version:
  callers normally take the plain `Foo/get` path instead.

## References

- [draft-ietf-jmap-object-history-03] — JMAP Object History (plus the
  -04 editor's copy changes in `docs/design/object-history.md` §2a)
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-object-history-03]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

# jmap-object-history-client

Client bindings for JMAP Object History (draft-ietf-jmap-object-history)
over `jmap-base-client`: reading earlier and destroyed versions,
searching destroyed objects and restoring them.

## What it is

A sealed extension trait, `JmapObjectHistoryExt`, on
`jmap_base_client::JmapClient`. Its `with_object_history_session(session)`
gives a `SessionClient` whose calls work for any data type the server
keeps history for:

- `capability(account)` and `supports(account, type_name)`: the §2.1
  account capability.
- `get_history::<T>(type_name, &HistoryGetCall)`: `Foo/get` with
  `includeReplaced`, `includeDestroyed`, `historyBefore` and
  `historyLimit` (§3), each entry a `HistoryEntry<T>`.
- `query_destroyed(type_name, &DestroyedQueryCall)` and
  `query_destroyed_changes(type_name, &DestroyedQueryChangesCall)`:
  `Foo/query` and `Foo/queryChanges` with `onlyDestroyed`,
  `destroyedAfter` and `destroyedBefore` (§4 and the -04 editor's copy).
- `restore(type_name, &SetCall, extra_using)`: `Foo/set` with the base
  client's `SetCall::restore` (§5.1), returning the response and its
  `RestoreOutcome`.
- `older_page_before(entries)` and `has_older(entries)`: paging one id's
  versions.

## What it's for

Applications that show an object's history or offer undelete against a
server advertising `urn:ietf:params:jmap:object-history`.

## How to use

```rust,no_run
# async fn demo(client: jmap_base_client::JmapClient, session: jmap_base_client::Session)
# -> Result<(), jmap_base_client::ClientError> {
use jmap_object_history_client::{DestroyedQueryCall, JmapObjectHistoryExt};
use jmap_types::Id;

let history = client.with_object_history_session(session);
let account = Id::from("abc");
if history.supports(&account, "Email")? {
    let call = DestroyedQueryCall::new(account, "urn:ietf:params:jmap:mail").limit(Some(50));
    let destroyed = history.query_destroyed("Email", &call).await?;
    println!("{} recently destroyed", destroyed.ids.len());
}
# Ok(()) }
```

## How it works

Each call adds `urn:ietf:params:jmap:core`, the type's capability URI and
`urn:ietf:params:jmap:object-history` to `using`. Wire values are the
types of `jmap-object-history-types`, used directly; this crate does not
re-export them. `restore` goes through `JmapClient::set_call`, so it
combines with the conditional options (`ifUnchangedBy`, `atomic`) of the
same `SetCall`.

## Gotchas

- History is best effort (§3.1): do not rely on `expires`, `hasOlder` or
  version numbers across requests; the server may purge at any time.
- Page one id per call (§3), and pass `replaced` back as `historyBefore`
  exactly as received: that is what `older_page_before` returns.
- Under `atomic`, a failed restore fails the whole `/set` with
  `atomicFailure`; decode it with
  `jmap_base_client::ConditionalError::from_method_error`, whose
  `AtomicFailure::not_restored()` lists the restores that failed.
- Do not present a destroy as permanent to users of a server that keeps
  history (§8.4).

## References

- [draft-ietf-jmap-object-history-03] — JMAP Object History
- [draft-ietf-jmap-conditional-02] — JMAP Conditional Updates
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-object-history-03]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03
[draft-ietf-jmap-conditional-02]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-conditional-02
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

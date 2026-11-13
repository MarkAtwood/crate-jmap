# Design: draft-ietf-jmap-object-history-03 in crate-jmap (bd:JMAP-tna1.3)

Spec: `~/PROJECT/IETF/specs/draft-ietf-jmap-object-history-03.xml` (posted 2026-11-02).

Status: final. Mark's decisions of 2026-11-13 are recorded in `docs/design/DECISIONS.md`.
This note applies decisions 5 (type scope), 6 (spec-gap stopgaps), 14 (shared data-type
registry), 18 (history arguments without the URI), 22 (private-type history) and 23 (Tasks
registrations from Mark's own draft). Where they touch this extension it also follows decisions 1
(opt-in registration), 4 (blob crate names), 7 (conditional error codes), 8 (generic `SetCall`),
10 (`objectHistory` in EmailPush `properties`), 11 (legacy RFC 9404 Blob/lookup) and 17 (the
conditional attach call). Every place a decision changed the
draft is marked "(decision N)". Companion notes: `docs/design/conditional.md`,
`docs/design/emailpush.md`, `docs/design/blobext.md`.

## 1. Summary

Object history (`urn:ietf:params:jmap:object-history`) adds history arguments to standard `Foo/get`,
destroyed-object search to `Foo/query`, and a `restore` argument to `Foo/set`, for any data type that opts in.
It defines no methods of its own. New crates: `jmap-object-history-types` (wire types, `HistoryObject` marker),
`jmap-object-history-server` (pure version-selection, retention and change-fold helpers for reference backends) and
`jmap-object-history-client`. `jmap-server` gets a `history` module: the `ObjectHistoryBackend` trait, argument
parsing, generic handlers and the restore phase. Each data-type server crate opts in with
`register_<x>_handlers_with_history`. Deployments that keep calling `register_<x>_handlers` see no wire change.
Conditional-02 adopts the same opt-in pattern (decision 1) and composes with it (§9).

First-round scope (decision 5): the 11 Table 1 "Yes" types; Task and TaskList; and Mark's private
types Chat, Message, Space, ChatContact, CustomEmoji, SpaceBan, SpaceInvite (chat), Graph, GraphNode,
GraphEdge (graph), SceneAsset, SceneRegion, SceneObject (scene), VTCCall, VTCRecording, VTCLivestream
(vtc); the private-type list is confirmed by decision 22. Excluded (decision 22): GraphNodeOp,
PresenceStatus, ReadPosition, SceneAvatar, VTCParticipant and TaskNotification, which are log,
ephemeral, presence or notification types, by analogy with CalendarEventNotification and
EmailDelivery "No", and KeyPackage (chat-mimi), "No" (decision 22). Task and TaskList history
registrations come from Mark's own updated Tasks draft (decision 23, JMAP-jkl8.9).

## 2. Spec coverage

Section numbers are from -03. "OHT" = jmap-object-history-types, "OHS" = jmap-object-history-server, "srv" =
`jmap_server::history`, "Bk" = the `ObjectHistoryBackend` contract (implemented by the consumer, and by the memory
backends for reference).

| § | Requirement | Where |
|---|---|---|
| 2.1 | Session `capabilities` value "MUST be an empty object" | OHT `ObjectHistoryCapability` (empty + `extra`); consumer Session builder; testjig test |
| 2.1 | Account value "MUST contain" `maxHistoryDuration: "UnsignedInt\|null"` and `supportedTypes: "String[]"` | OHT `ObjectHistoryAccountCapability` (both always serialized); Bk `object_history_capability` |
| 2.1 | In any `accountCapabilities` ⇒ "MUST also include the property in the "capabilities" property" | consumer Session builder (testjig is the reference + test); documented on the OHT type |
| 2.1 | Retention best effort, "MAY discard versions sooner"; "MAY retain history for only some objects" | Bk doc; OHS `VersionLog` retention. Discarding the final version of a destroyed object discards the object: no older version stands in for it (decision 6(d)) |
| 2.1 | Clients "MUST ignore type names they do not recognise" | client `supports::<O>()` is a string lookup only |
| 3 | `includeReplaced`, `includeDestroyed`, `historyAfter`, `historyBefore`, `historySkip`, `historyLimit` | srv `HistoryGetRequest::parse` |
| 3 | Same-id entries "MUST be ordered by objectHistory.version (ascending)"; ids MAY interleave | Bk contract; srv stable-sorts each id's entries by `version` |
| 3 | Destroyed: final version only unless `includeReplaced`; every destroyed entry has non-null `replaced` | Bk; OHS `select_versions` |
| 3 | Destruction by any route (IMAP EXPUNGE, POP3 DELE, auto-empty Trash, `onDestroyRemoveEmails`) counts; other protocols create versions | Bk doc; memory backends record every cascade (`onDestroyRemoveEmails`, `onDestroyRemoveContents`, `onDestroyRemoveTasks`, Calendar, FileNode, Space, Graph and GraphNode cascades) |
| 3 | after/before strict; live version kept by `historyAfter`, dropped by `historyBefore`; `historySkip` from newest, live counts as one; limit "MUST return the most recent entries" | OHS `select_versions`; Bk contract |
| 3 | Both flags false ⇒ the four history* arguments are ignored | srv: decides the path from the two flags first and never parses the other four on the plain path |
| 3 | `ids: null` with `includeDestroyed` returns every retained destroyed object | Bk; memory |
| 3 | `maxObjectsInGet` / `requestTooLarge` count ids, not entries | srv: existing `enforce_max_objects_in_get` on `ids`, unchanged |
| 3 | No version left after flags, after/before and skip ⇒ `notFound` (`historyLimit` is not in that list) | OHS `select_versions`; Bk. A destroyed object whose final version was discarded is `notFound` too (decision 6(d)) |
| 3.1 | `objectHistory: ObjectHistoryInfo {version, replaced, hasOlder, expires}` | OHT `ObjectHistoryInfo` |
| 3.1 | Live version highest `version`, `replaced: null`, `expires: null`; `replaced` distinct per object; SHOULD NOT exceed ms precision; version numbers SHOULD be consistent across requests | OHS `VersionLog` (ms clock, collapses equal-ms versions, stable numbering); Bk doc |
| 3.1 | `objectHistory` returned "if and only if" a flag is true, whatever `properties` says; MAY be listed in `properties` | srv strips it from `properties` before the backend and per-type validators, then `attach_object_history`; `/set` rejects it as a stored property (§7.3/7.4 row) so the plain path can never emit it |
| 3.1 | Which changes make a version is server dependent; historical server-computed values SHOULD be the then-current ones | Bk doc; memory snapshots whole objects |
| 3.1 | Blob of a returned version "MUST be downloadable"; non-current versions are not blob references (GC, `blobHasReference`, `Blob/lookup`); a version whose blob was discarded "MUST NOT" be returned | Bk contract; blobext hook contracts (§9); memory Email blob store |
| 3.1 | Flag true and type not in `supportedTypes` ⇒ "MUST reject the call with an "invalidArguments" error" | srv |
| 3.1 | Supported type, no retained history ⇒ "MUST still return the current version" with `replaced: null` | Bk; `select_versions` |
| 4 | `onlyDestroyed`, `destroyedAfter`, `destroyedBefore`; a restored object is not destroyed | srv `DestroyedQueryRequest::parse`; Bk `query_destroyed` |
| 4 | `invalidArguments` when `onlyDestroyed` and type unsupported, or `destroyedAfter`/`destroyedBefore` "is supplied and onlyDestroyed is false" | srv |
| 4 | filter and sort apply to the final version; unsupported ones are `unsupportedFilter` / `unsupportedSort` | Bk returns `BackendQueryError::Method` |
| 4 | Null or empty sort ⇒ by destruction time, most recent first | Bk; memory |
| 4 | Arguments for current objects only (`collapseThreads`, …) are ignored | per-crate handlers (Email `collapseThreads`, CalendarEvent `expandRecurrences`) |
| 4 | `Foo/queryChanges` "works as for any other query": leaves on restore or discard, enters on destroy; MAY `cannotCalculateChanges` | -03 adds no queryChanges arguments (spec gap). Stopgap: a destroyed query's `queryState` encodes the query kind, and srv routes `Foo/queryChanges` by `sinceQueryState` through Bk `query_destroyed_changes` (decision 6(c); §5) |
| 5 | Restoring an earlier version of a live object is an ordinary update | N/A (existing `/set`) |
| 5.1 | `restore: "Id[PatchObject\|null]\|null"`; empty PatchObject ≡ null | srv `RestoreRequest::parse` |
| 5.1 | `restored: "Id[Foo\|null]\|null"` (diff against the final version with the patch applied, or null); `notRestored: "Id[SetError]\|null"` | srv `restored_diff` + `append_restore_results` |
| 5.1 | Non-null `restore` on an unsupported type ⇒ "MUST reject the call with an "invalidArguments" error" | srv |
| 5.1 | Restores run "before any create, update, or destroy in the same call" | srv `run_restore_phase`, first stage of every history-aware `/set` |
| 5.1 | Final values + patch; patch follows update rules; server-set values kept unless impossible, new values returned in `restored` | Bk `restore_object`; srv `apply_restore_patch`, or the type's own update-patch parser through `RestoreHooks` (no current user: Space needed it, but a destroyed Space is purged and never restored, decision 22) |
| 5.1 | History continues; destroyed versions become previous versions | Bk; OHS `VersionLog::restore` |
| 5.1 | SetErrors `notFound` (incl. a current id), `invalidPatch`, `invalidProperties`, `forbidden`, `overQuota`, or any update/create error | srv (shape errors); Bk (the rest) |
| 5.1 | RFC 8620 `maxObjectsInSet` names only create, update and destroy (spec gap) | Stopgap: restore entries count toward it (decision 6(a)); srv `enforce_max_objects_in_set_with_restore` |
| 5.1 | "MUST NOT reuse the id of a destroyed object for which it retains any history, other than by restoring" | Bk contract on every id-minting path; memory: uuid (mail, contacts, calendars), monotonic `node-N` (filenode), each private crate's minting audited in its bead; test-helper insert-with-id paths refuse logged ids |
| 5.1 | `Foo/changes`: restored id in `updated` if it existed at `sinceState`, else `created`; never `destroyed` | Bk contract on `get_changes`; OHS `fold_restore` for memory change logs |
| 5.2 | Email keeps `blobId`; SieveScript comes back inactive (activate with `onSuccessActivateScript`); missing container fails as an update would; conflicts use the normal error (`alreadyExists`) | mail-server Email/SieveScript beads; filenode `parentId`/name; parent-first restore order (§5) |
| 6 | Non-normative | N/A |
| 7.1 | Capability URI | OHT `JMAP_OBJECT_HISTORY_URI` |
| 7.2 | "Supports History" column; default "No" for registrations without a value | The shared data-type registry in `jmap-types` carries the column (decision 14). `impl HistoryObject` exactly for the decision-5 types: the registry's "Yes" rows (Table 1, plus Mark's drafts once H14 adds their lines), and Task/TaskList on OHT's documented pending list until Mark's updated Tasks draft registers them with "Supports History: Yes" (decisions 5, 23; JMAP-jkl8.9, then bead H24). Never for a "No" or excluded type. A test in each type crate checks the impls against the registry |
| 7.3/7.4 | `objectHistory` reserved in JSContact (Card) and JSCalendar (Event, Task, Group) | history-aware `/set` rejects an `objectHistory` key in create, update and restore patch with `invalidProperties`, on every history type (also needed for the §3.1 "if and only if"). Task is in scope (decision 5) |
| 7.4 | Other specs SHOULD reserve the name | N/A (spec authors). Mark's drafts define no separate property registry |
| 8.1 | Retention limits SHOULD; history in quota NOT RECOMMENDED; a restored object counts | Bk doc; memory never counts history; `overQuota` from `restore_object` |
| 8.2 | History MUST respect current access; never a version unreadable when current; destroyed existence hidden likewise | Bk contract on `get_object_history`, `query_destroyed`, `query_destroyed_changes`, `restore_object`; srv applies the per-caller property projection (VTCLivestream `streamKey`/`streamUri`, SceneRegion `invitedUserIds`, chat-tasks hidden Task properties) to every entry; memory: conservative rights cut |
| 8.3 | SHOULD offer an admin purge | out of protocol: consumer; memory `purge_object_history` (test-helpers) |
| 8.4 | Make retention visible (SHOULD); clients SHOULD NOT call destroy permanent | `maxHistoryDuration`, `expires`; client crate docs |
| 8.5 | MAY decline history, MAY refuse restore with `forbidden` | Bk `restore_object` contract |

Type-specific rules from Mark's drafts (decisions 5, 22):
- draft-atwood-jmap-chat-00 §Message (`editHistory` note, l.782): a server listing Message "SHOULD NOT
  return any prior or destroyed version of a Message once that Message is deleted (`deletedAt` set)
  or hard-deleted, and SHOULD permanently remove those versions at the same time". Bk contract for
  Message; the chat memory backend purges them (decision 22). A Message therefore never appears in
  `includeDestroyed`, `onlyDestroyed` or `restore` (`notFound`); only edits of live, undeleted
  Messages have history.
- draft-atwood-jmap-chat-00 §Space/set (l.1395): a Space destroy "Hard-deletes all records; no
  tombstones are retained". The server purges the history of the Space and of everything in it
  (its Chats and their Messages, its SpaceBans, SpaceInvites and Space-scoped CustomEmojis) at the destroy, so none of them
  appears in `includeDestroyed`, `onlyDestroyed` or `restore` (`notFound`) afterwards (decision 22).
- graph-edit §Compaction ("When a node is destroyed, its ops are destroyed with it"): a destroyed
  GraphNode may be restored. It comes back at its final snapshot (`blobId`/`snapshotSeq`), and its
  op log restarts empty with its head at `snapshotSeq`; edits after the final snapshot are not
  recoverable, and the graph crate documents that (decision 22).
- draft-atwood-jmap-graph-provenance-00 §Retraction: the same rule for a retracted provenance
  record (a GraphNode). There is no provenance crate yet; the graph bead documents the rule on the
  Bk contract so a provenance implementation inherits it.

## 3. Crate placement

| Crate | New? | Contents | Depends on | Publish |
|---|---|---|---|---|
| `jmap-object-history-types` (`crate-jmap-object-history-types/`) | new | URI, both capability structs, `ObjectHistoryInfo`, `HistoryEntry<T>`, argument structs, `RestoreOutcome`, `HistoryObject` marker, `is_history_type(name)` over the registry plus the pending list (decision 14). No async | `jmap-types`, serde, serde_json | yes, 0.5.0 |
| `jmap-object-history-server` (`crate-jmap-object-history-server/`) | new | Pure helpers for backends: `HistorySelector` + `select_versions`, `VersionLog<V>` (`Clone`; retention, `expires`, equal-ms collapse, rights cut, restore, oldest-first prune), `fold_restore`. No handlers, no backend trait, no async | OHT, `jmap-types` | yes, 0.5.0 |
| `jmap-object-history-client` (`crate-jmap-object-history-client/`) | new | `JmapObjectHistoryExt::with_object_history_session` → `SessionClient` with generic history `/get`, destroyed `/query`, restore built on base-client `SetCall` (decision 8), paging helpers | `jmap-base-client`, `jmap-types`, OHT | yes, 0.5.0 |
| `jmap-types` | extend | `UTCDate::cmp_instant(&self, &UTCDate) -> Result<Ordering, ValidationError>`: exact instant order (seconds, then fractional digits compared as decimals). `to_epoch_seconds` truncates fractions, and string order is wrong (`…00.5Z` < `…00Z`); a nanos conversion would still collapse >9 fractional digits, which `new_validated` accepts. The shared data-type registry with its "supports history" column is blobext's bead blobext:B1 (decision 14); this epic only reads it and sets the values for Mark's drafts' rows (H15) | n/a | yes |
| `jmap-base-client` | extend (conditional epic) | `SetCall.restore` (decision 8), added by conditional bead C6; nothing object-history specific beyond that field | n/a | yes |
| `jmap-server` | extend | new `history.rs` module (trait, params, parsing, generic handlers, restore phase, envelope, `apply_restore_patch`, `enforce_max_objects_in_set_with_restore`, `object_history_gaps`). Existing `helpers.rs` functions unchanged | + OHT | yes |
| mail, contacts, calendars, filenode, tasks, chat, graph, scene, vtc `-types` | extend | `impl HistoryObject` in `src/backend.rs` for the decision-5 types only (decision 5) | + OHT | yes |
| the matching `-server` crates | extend | `*_with_history` handlers and register fn; `ObjectHistoryBackend` for `MemoryBackend` | + OHS (optional, `memory` feature) | yes |
| `jmap-testjig` | extend | Session capability, registration, end-to-end examples | n/a | no |

Why this split: it follows the metadata extension, the one cross-cutting per-type extension in the tree. `jmap-server`
holds the handler-level helpers (`MetadataProjection`, `handle_get_with_metadata`), and `jmap-metadata-server` holds
pure helpers that only `memory` features pull in. The `ObjectHistoryBackend` trait has to sit in `jmap-server`, because the
data-type crates' handlers must name it and those crates depend on `jmap-server`. `HistoryObject` lives in OHT, not
`jmap-types`, so the foundation carries only the shared registry data (decision 14), not this extension's API.
The type crates implement it for their own types (orphan rule satisfied).
This adds a new dependency edge `*-types → OHT`. It is not the metadata edge: the type crates use `jmap-metadata-types`
for `MetadataTextMatch` filter fields, not for a marker. The edges are: OHT ← {jmap-server, 9 type crates, OHS, client}, and
OHS ← the server crates' `memory` feature only. OHT depends on nothing async and never on `jmap-server`. No cycles.
`jmap-base-client` does not depend on OHT: the object-history client adds the URI to `SetCall`'s
`extra_using` (decision 8; conditional note §8).

## 4. Wire types (OHT)

```rust
pub const JMAP_OBJECT_HISTORY_URI: &str = "urn:ietf:params:jmap:object-history";

/// Implemented exactly for the decision-5 types: registry "Supports History: Yes" rows
/// (§7.2 Table 1 and Mark's drafts, read from the jmap-types registry, decision 14) and the
/// types on PENDING_HISTORY_REGISTRATION.
pub trait HistoryObject: jmap_types::GetObject {}

/// Types implemented ahead of their registration (decision 5): draft-ietf-jmap-tasks-06 registers
/// no "Supports History" value, so the registry default is "No". Mark's updated Tasks draft
/// registers them (decision 23, JMAP-jkl8.9); bead H24 then moves them to the registry.
pub const PENDING_HISTORY_REGISTRATION: &[&str] = &["Task", "TaskList"];

/// True for a registry "Yes" row or a pending type. For consumers building `supportedTypes`
/// and for each type crate's impl cross-check test.
pub fn is_history_type(type_name: &str) -> bool;

/// Session `capabilities` value: "MUST be an empty object" (§2.1).
#[non_exhaustive] #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectHistoryCapability {
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")] pub extra: Map<String, Value>,
}

#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectHistoryAccountCapability {
    /// "maxHistoryDuration": "UnsignedInt|null" (seconds). Always serialized (MUST contain).
    #[serde(default)] pub max_history_duration: Option<u64>,
    /// "supportedTypes": "String[]". Open vocabulary (private types), so String.
    pub supported_types: Vec<String>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")] pub extra: Map<String, Value>,
}
impl ObjectHistoryAccountCapability { pub fn supports<O: JmapObject>(&self) -> bool; }
// No `new`: built by deserializing JSON (AGENTS "Public-API construction"); memory backends take the
// value from the caller, as `register_account(.., VtcCapability)` does.

/// "objectHistory": "ObjectHistoryInfo" (§3.1). replaced/expires always serialized (null is meaningful).
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectHistoryInfo {
    pub version: u64,                 // "version": UnsignedInt
    pub replaced: Option<UTCDate>,    // "replaced": UTCDate|null (null = live)
    pub has_older: bool,              // "hasOlder": Boolean
    pub expires: Option<UTCDate>,     // "expires": UTCDate|null (null for live)
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")] pub extra: Map<String, Value>,
}

/// One history `Foo/get` list entry: an ordinary Foo plus "objectHistory".
/// No own `extra`: T's flattened `extra` already keeps unknown keys (two catch-alls would race).
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry<T> {
    #[serde(flatten)] pub object: T,
    #[serde(rename = "objectHistory")] pub object_history: ObjectHistoryInfo,
}

/// Foo/get additions (§3). Spec defaults; skip-serialize when default.
#[non_exhaustive] #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryGetArguments {
    pub include_replaced: bool,            // "includeReplaced" (default false)
    pub include_destroyed: bool,           // "includeDestroyed" (default false)
    pub history_after: Option<UTCDate>,    // "historyAfter"  (default null)
    pub history_before: Option<UTCDate>,   // "historyBefore" (default null)
    pub history_skip: u64,                 // "historySkip"   (default 0)
    pub history_limit: Option<u64>,        // "historyLimit"  (default null)
    #[serde(flatten, ...)] pub extra: Map<String, Value>,
}
/// Foo/query additions (§4). Same derives.
pub struct DestroyedQueryArguments {
    pub only_destroyed: bool,              // "onlyDestroyed" (default false)
    pub destroyed_after: Option<UTCDate>,  // "destroyedAfter"
    pub destroyed_before: Option<UTCDate>, // "destroyedBefore"
    pub extra: Map<String, Value>,
}
/// Foo/set response additions (§5.1), read from `SetResponse::extra`. A projection of that
/// object, so no own `extra`. Local copies of the null⇄empty serde helpers (the jmap-types ones are private).
#[non_exhaustive] #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub restored: BTreeMap<Id, Option<PartialObject>>,   // "restored": Id[Foo|null]|null (a diff)
    pub not_restored: BTreeMap<Id, SetError>,            // "notRestored": Id[SetError]|null
}
impl RestoreOutcome { pub fn from_set_extra(extra: &Map<String, Value>) -> Result<Self, serde_json::Error>; }
```
The `restore` request argument is base-client `SetCall.restore: BTreeMap<Id, Option<PatchObject>>`
(decision 8). `SetResponse` in `jmap-types` is **not** changed: `restored` and `notRestored`
round-trip through its `extra`, so non-history `/set` bytes stay the same. In a conditional
`atomicFailure`, `notRestored` is read through `AtomicFailure::not_restored()` in `jmap-types`'
conditional module (decision 6(b); conditional note §4). Every struct gets the extras round-trip
test, and `RestoreOutcome` gets the BTreeMap order test.

## 5. Server design (`jmap_server::history`)

**Opt-in without touching plain handlers.** Rust has no specialization, so history support is a separate set of
pub handlers bounded by `B: XBackend + ObjectHistoryBackend`. `/get`, `/query` and `/queryChanges` are wrappers. They
peel off the history arguments and delegate to the existing plain handler when no history mode is active, so
those bodies do not move. Only `/set` needs a body change, because the restore stage sits inside it. Each crate's
`handle_<x>_set` body moves into a private `<x>_set_impl<B, R: RestoreStage<B>>`. The pub plain handler passes
`NoRestore` (no stage, no `restored`/`notRestored` keys), and `handle_<x>_set_with_history` passes `WithRestore`.
Plain handler signatures and bytes are unchanged.

**Foo/get.** (1) `accountId` and `account_exists` as today. (2) Read only `includeReplaced` and `includeDestroyed`.
They must be booleans or absent, otherwise `invalidArguments`. Without the URI in `using`, a `true` flag is
`invalidArguments`, from the dispatcher check before the handler runs (decision 18). (3) Both false ⇒ remove the six history keys and `objectHistory` from `properties`, then call the plain
path (`handle_get` / `handle_get_with_metadata` / the per-type getter). The four others are ignored, not validated (§3).
(4) `!cap.supports::<O>()` (`None` capability included) ⇒ `invalidArguments`. (5) Parse the four: `UTCDate`
through `new_validated`, skip and limit as UnsignedInt, `invalidArguments` naming the argument. (6)
`enforce_max_objects_in_get` on `ids`. (7) `get_object_history` with `properties` minus `objectHistory`. (8) Serialize,
apply the per-type projection to every entry (`MetadataProjection`; Email body-value processing, factored out of
`handle_email_get` into a helper; the per-caller blanking of VTCLivestream `streamKey`/`streamUri`, SceneRegion
`invitedUserIds` and chat-tasks hidden Task properties, so no version shows what the current one hides, §8.2),
then `attach_object_history`. The `properties` filter keeps `id` and `objectHistory`.
(9) Stable-sort each id's entries by `version` and reply with `notFound`. Generic: `handle_get_with_history::<O,B>(backend,
caller, args, using)`. Custom getters (Email, Mailbox, and private-type getters with projections) use the pub pieces
`HistoryGetRequest::parse`, `strip_object_history_property` and `attach_object_history`.

**Foo/query.** `DestroyedQueryRequest::parse` takes the three arguments out of `args`. Without the URI in `using` the
dispatcher check has already rejected `onlyDestroyed: true` and a non-null `destroyedAfter`/`destroyedBefore`
(decision 18). If `onlyDestroyed` is false or absent, a non-null `destroyedAfter`/`destroyedBefore` ⇒ `invalidArguments`. An explicit `null`
is not "supplied", since the default is null. Otherwise it delegates to the plain handler. `onlyDestroyed: true` on an unsupported type ⇒
`invalidArguments`. Otherwise `query_destroyed` gets `DestroyedQueryParams { query: QueryParams<O>, destroyed_after,
destroyed_before }` (`#[non_exhaustive]`, builder, like `QueryParams`). The response has the `handle_query` shape. Per-type
handler-side checks (`reject_unsupported_sort`, metadata filter gating) still run before the backend sees the filter.
Destroyed `/query` exists only for types with a `/query`: Mailbox, Email, SieveScript (handler-side, §9), ContactCard,
CalendarEvent, FileNode, Task, Chat, Message, Space, ChatContact, CustomEmoji, Graph, GraphNode, GraphEdge, SceneAsset,
SceneRegion, SceneObject, VTCCall, VTCRecording and VTCLivestream. Identity, VacationResponse, AddressBook, Calendar,
ParticipantIdentity, TaskList, SpaceBan and SpaceInvite have none.

**Foo/queryChanges.** -03 adds no queryChanges arguments, and a spec-following client sends only `sinceQueryState`,
`filter` and `sort`. So the destroyed query is identified by its state, not by arguments: every `queryState` that
`query_destroyed` mints encodes the query kind (decision 6(c)). `handle_query_changes_with_history`
first calls `query_destroyed_changes`. `Ok(None)` ("not a state minted by `query_destroyed`") falls through to the plain
handler, and `Ok(Some(r))` or an error is the answer. `onlyDestroyed`/`destroyed*` sent to queryChanges are ignored.
This is a stopgap, raised upstream (decision 6(c); bead H22). The existing memory `query_changes` derives the query state from the type state. If the
destroyed state were routed there, it would answer with live changes, which is wrong data. That is why this method has
no default (§6).

**Foo/set.** `run_restore_phase::<O,B>(backend, caller, account, restore_arg, created_ids, using, hooks) ->
RestoreAccumulators { restored, not_restored, mutated }`. It runs after the `ifInState` check and before create; conditional's
`ifUnchangedBy` runs earlier still, in the dispatcher before the handler (§9). A non-null `restore` that is not an object ⇒ `invalidArguments`. Per entry: a key
that is not a valid `Id` ⇒ `notFound` (§5.1: "not the id of a destroyed object"). A value that is neither null nor an object ⇒
`invalidPatch` (as the update loop does), and `{}` becomes `None`. A `check_patch_paths` failure ⇒ `invalidPatch`. An
`objectHistory` key ⇒ `invalidProperties`. The per-type update pre-checks (`#creation` key refs, `check_metadata_entry(..,
false, using)`) run through `hooks: &dyn RestoreHooks`, which also lets a type supply its own update-patch parser
instead of `apply_restore_patch`, for a type whose updates are not plain PatchObjects (Space's structural `SpacePatchOp` was the case, but a destroyed Space is purged and never restored, decision 22). **Order:** for tree types (Mailbox, FileNode) the phase fetches the final versions of the
restore keys (`get_object_history`, `include_destroyed`) and processes a parent before a child whose final `parentId` is
another restore key. Without that, §5.2 "restore that object first, in the same call" fails for a subtree. Then
`restore_object`. On `Ok(Restored{stored, baseline})`, `restored[id] = restored_diff(baseline, stored)` (top-level
properties whose values differ, or `null`). On `Err`, `backend_set_error_value`. `append_restore_results(&mut
out.primary, acc)` adds `"restored"` and `"notRestored"` (`null` when empty) whenever the URI is in `using` (§5.1 "The
response has the following additional properties"). `mutated` feeds the crate's `finalize_set_response`, and `oldState` is
the value read before the phase. Later create, update and destroy see the restored object (§5.1 "so these can refer to a
restored object"). Count: `enforce_max_objects_in_set_with_restore` = create + update + destroy + restore entries
(decision 6(a)), used only by the history path. The foundation `enforce_max_objects_in_set` stays as it is.

**Helpers (in srv, because they need `apply_patch_object`).** `apply_restore_patch::<O>(final_json, patch) -> Result<Map,
SetError>` applies all or nothing. A changed `SERVER_SET` or `IMMUTABLE` value ⇒ `invalidProperties`, and an identical
value is allowed (as `check_read_only_patch` does for update). Also `restored_diff`.

**Dispatcher check (decision 18).** Like conditional's decision 2, one dispatcher check runs on `*/get`, `*/query`
and `*/set` for every deployment, whatever the registration: without the object-history URI in `using`,
`includeReplaced: true`, `includeDestroyed: true`, `onlyDestroyed: true`, a non-null `destroyedAfter` or
`destroyedBefore`, or a non-empty `restore` ⇒ method `invalidArguments`, and the handler is not called. `false`,
`null` and empty values are accepted and removed. A deployment without history therefore no longer returns live data
for `includeDestroyed: true` or drops a `restore` silently. No crate or draft uses these names for anything else
(decision 18). Bead H23, after H3. The `_with_history` handlers keep their own check as defence in depth.

**Error mapping.** Argument shape, unsupported type, a supplied `destroyed*` without `onlyDestroyed`, a non-object `restore`,
and a history argument sent without the URI (decision 18) ⇒ method `invalidArguments`. Too many ids ⇒ `requestTooLarge`
(existing). Too many set entries including restores ⇒ `requestTooLarge` with description `maxObjectsInSet` (decision 6(a)).
`BackendQueryError::Method` passes through (`unsupportedFilter`, `unsupportedSort`, `anchorNotFound`).
`BackendChangesError::CannotCalculate` ⇒ `cannotCalculateChanges`. A backend `Other` ⇒ redacted `serverFail`
(`server_fail_from_backend`). Per-id restore errors go into `notRestored` through `backend_set_error_value`, which redacts
`Other`.

**Registration.** `register_<x>_handlers_with_history<B: XBackend + ObjectHistoryBackend>(dispatcher, backend)` calls
`register_<x>_handlers` and then re-registers, last-wins as documented on `register_mail_handlers`, the history-aware
`/get` and `/set` of each `HistoryObject` type, plus `/query` and `/queryChanges` where the type has them. Sieve gets
`register_sieve_handlers_with_history`, because the Sieve methods are registered separately. The decision-5 crates add
`register_tasks_handlers_with_history`, `register_chat_handlers_with_history`, `register_graph_handlers_with_history`
(`register_graph_edit_handlers` is unchanged: GraphNodeOp is excluded), `register_scene_handlers_with_history` and
`register_vtc_handlers_with_history` (decision 5). `/changes`, `/copy` and the
"No" types are unchanged, since restore classification in `/changes` is change-log behaviour. Identity, VacationResponse and
SieveScript handlers are registered without `using` today, so their history variants bind `using` from `CallContext`.
Each history-aware registration goes through `Dispatcher::register_history_aware(name, handler)`, which marks the
method; a later plain `register` of the same name clears the mark. Conditional-02 opts in with a separate attach call,
`register_<x>_conditional`, made after `register_<x>_handlers_with_history`; it wraps whichever `/set` is registered, so the
two opt-ins compose without a combined variant (decisions 1, 17; conditional note §5.1).

**Capability advertisement.** The kit has no Session builder. The consumer calls `object_history_capability(caller,
account_id)` per account and puts `Some(c)` under `accountCapabilities[URI]`. If any account has one, it sets
`capabilities[URI] = {}` and adds the URI to its `check_known_capabilities` list. The handlers read the same method, so the
advertisement and the behaviour agree. `Dispatcher::object_history_gaps(supported_types)` lists the `/get`, `/set`,
`/query` and `/queryChanges` methods of the supported types that are not marked history-aware (for example because a
plain `register_<x>_handlers` ran last and silently dropped `restore`). A consumer MUST NOT list a type whose gaps are
non-empty, mirroring conditional's `conditional_gaps`. testjig is the reference and asserts no gaps.

## 6. Backend hooks (`jmap_server::history::ObjectHistoryBackend: JmapBackend`)

This is a separate trait, as `SieveBackend: MailBackend` is, so a backend that offers history implements every semantic method
(checked at compile time). Backends without history are untouched. Decision 1 adopts this opt-in pattern for
conditional-02 too, naming `register_<x>_handlers_with_history` as its model, so the separate trait stands (decision 1).

| Method | Contract | Default? |
|---|---|---|
| `object_history_capability(&self, caller, account_id) -> Option<ObjectHistoryAccountCapability>` (sync, like `metadata_info`) | What the Session advertises. `None` = no history in this account. Handlers gate on `supported_types`. Listing a type for which `is_history_type` is false is a contract violation (debug-asserted in the memory backends) | **No.** Implementing the trait means stating it. A `None` default would make the trait a silent no-op |
| `get_object_history<O: HistoryObject>(&self, caller, account_id, HistoryGetParams<'_>) -> Result<HistoryGetResult<O>, BackendQueryError<E>>` | §3 selection exactly: flags, strict after/before, skip from newest (live counts), limit newest, `notFound` only when nothing remains after skip. With `include_destroyed` false a destroyed id is `notFound`; with `include_replaced` false a live id gets only its live version. Ascending `version` per id; live highest with `replaced: null, expires: null`; `replaced` distinct per object. `hasOlder` counts retained older versions whether returned or not (MAY be false). A destroyed object whose final version was discarded is gone: `notFound`, even if older versions remain (decision 6(d)). Never a version whose blob is gone; blobs of returned versions downloadable. §8.2 (never a version unreadable when current; hide destroyed objects unreadable at destruction). Supported type without retained history ⇒ live version only. `ids: None` + `include_destroyed` ⇒ also every retained destroyed object. Type rules from Mark's drafts (§2: Message, retracted provenance records) | **No.** A default would answer "no history" or wrong `notFound` |
| `query_destroyed<O: HistoryObject + QueryObject>(&self, caller, account_id, DestroyedQueryParams<'_, O>) -> Result<QueryResult, BackendQueryError<E>>` | Over retained destroyed objects only (restored ones and those whose final version was discarded excluded, decision 6(d)). filter and sort on the final version. `destroyed_after`/`_before` strict on the final `replaced`. Empty sort ⇒ destruction time descending. Anchor and position as `query_objects`. Unsupported filter or sort ⇒ `Method(unsupportedFilter/unsupportedSort)`. §8.2 visibility. The returned `query_state` MUST encode that it is a destroyed query, so `query_destroyed_changes` recognises it (decision 6(c)) | **No.** A default would claim "nothing was destroyed" |
| `query_destroyed_changes<O: HistoryObject + QueryObject>(&self, caller, account_id, QueryChangesParams<'_, O>) -> Result<Option<QueryChangesResult>, BackendChangesError<E>>` | `Ok(None)` iff `since_query_state` does not encode a destroyed query (decision 6(c)). Otherwise destroy ⇒ added, restore or discard ⇒ removed, or `Err(CannotCalculate)` (§4 MAY), in which case `query_destroyed` should report `can_calculate_changes: false` | **No.** An `Ok(None)` default sends destroyed states into the live `query_changes`, which is wrong data. A backend that cannot track writes `Ok(Some(..))` → `Err(CannotCalculate)` for its own states |
| `restore_object<O: HistoryObject + SetObject>(&self, caller, account_id, id, patch: Option<PatchObject>) -> Result<Restored<O>, BackendSetError<E>>` | Atomically: write access (`forbidden`). `notFound` unless `id` is a destroyed object with retained history whose final version is still retained (a current id ⇒ `notFound`; final version discarded ⇒ `notFound`, decision 6(d)). Start from the final version (decision 6(d)) and apply the patch with `apply_restore_patch` or the type's own patch parser (`invalidPatch`/`invalidProperties`). Validate as an update: references to missing objects ⇒ `invalidProperties` naming the property; conflicts ⇒ the normal error (`alreadyExists` etc.). Keep server-set values unless impossible (e.g. new `threadId`, recomputed Mailbox counts). Email keeps `blobId`; SieveScript inactive. Quota counts the restored object (`overQuota`). Harmful content ⇒ `forbidden`. Store under the old id, continue its history, advance `get_state::<O>` and record the change so `get_changes` reports `updated` if the id existed at `sinceState`, else `created`, never `destroyed`. Side effects on other types (Thread, Mailbox counts) advance their states. Inside a conditional scope the write is part of the scope (`SetOp::Restore`, decision 6(b)). Returns `Restored { stored, baseline }` (baseline = final version with the patch applied) | **No.** It is a write with spec semantics |

Contracts added to **existing** methods (doc only, in each crate that opts in):
- Every id-minting path (`create_object`, `create_email`, `import_email`, `copy_object`, Task/copy, and so on) MUST NOT mint an id that
  belongs to a destroyed object with retained history (§5.1).
- `update_object`, `destroy_object`, server cascades and other protocols record versions. This is a storage concern, and the
  doc states the §3 "counts as destroyed" rule.
- `get_changes`: the restore classification above. A restored id is never metadata-only (`updatedProperties` is null for it).
- The blobext hooks (tna1.2: `destroy_blob` / `blobHasReference`, `Blob/lookup` for both blob2 and the legacy RFC 9404
  capability, GC) consider current objects only (decision 11).
- Message (chat): deleting (`deletedAt` set) or hard-deleting a Message purges its versions (§2, decision 22).
- Space (chat): destroying a Space purges the history of the Space and everything in it (§2, decision 22).
- GraphNode (graph): `restore_object` restores a destroyed node at its final snapshot with an empty op log (§2,
  decision 22).

No permission or policy hooks are added. Access is decided inside the methods above (backend-canonical rule), and §8.5 policy
is part of `restore_object`. Support types: `HistoryGetParams<'a>`, `DestroyedQueryParams<'a,O>`,
`HistoryGetResult<O>{entries: Vec<HistoryEntry<O>>, not_found: Vec<Id>}`, `Restored<O>{stored, baseline}`. All are
`#[non_exhaustive]` with `new` and setters (the params-struct rule).

## 7. Memory reference backend scope

OHS provides the reusable parts, and each opted-in crate's `MemoryBackend` wires them in.
- `VersionLog<V = serde_json::Value>` keyed by (type name, account, id): `Vec<Version{version, snapshot, replaced_ms}>`
  plus `destroyed_at`. The live version stays in the existing store. Methods: `record_replace`, `record_destroy`, `restore(id) ->
  final`, `prune(now)` and `select(id, &HistorySelector, live)`. `prune` discards oldest first, so the final version of a
  destroyed object goes last, and when it goes the whole destroyed object goes with it (decision 6(d)).
  Equal-millisecond versions are collapsed (§3.1). `expires = replaced + max_duration`. The clock is
  injected (`now: i64` ms), so tests are deterministic. `VersionLog` is `Clone` and lives in each memory backend's
  `Inner`, so a conditional rollback discards versions written by a rolled-back restore (conditional note §7).
- `HistoryRetention { max_duration: Option<Duration>, max_versions_per_object: Option<usize> }`. Off by default.
  `MemoryBackend::with_object_history(capability: ObjectHistoryAccountCapability, retention)` turns it on, so existing tests do
  not change.
- §8.2 conservative cut: a per-container `rights_changed_at` (Mailbox, AddressBook, Calendar, TaskList, the FileNode
  parent chain, Space for chat types, Graph for GraphNode/GraphEdge, SceneRegion for SceneObject).
  Versions replaced before it are not returned, and destroyed objects whose container's rights changed after destruction
  are hidden.
- `query_destroyed` reuses each crate's existing `query` match/compare functions over final snapshots, with the default sort
  by destruction time descending. Destroyed query states carry a private kind prefix (decision 6(c)), and `query_destroyed_changes` answers from a
  per-account destroyed/restored log, so the §4.1 example gets `canCalculateChanges: true`.
- Change log: each crate's private `ChangeType` gains `Restored`, and its `IdFate` fold calls OHS `fold_restore`: destroyed
  and then restored in the window ⇒ `updated`; created in the window or destroyed before it ⇒ `created`. There is no shared
  `ChangeKind` today, and the mail canonical keeps these private.
- Email (mail memory): the blob store keeps blobs referenced by retained versions alive. Destroying a blob drops the versions
  that reference it, and every version of an Email shares its `blobId`. `Blob/lookup` sees current Emails only.
  Blob-referencing private types (Chat, Message, Space, CustomEmoji, GraphNode, SceneAsset, SceneObject, VTCRecording;
  registry "Can Reference Blobs: Yes", decision 14) follow the same rule: a version whose blob is gone is not returned, and
  if that was the final version of a destroyed object, the object is gone (decision 6(d)).
- Chat memory purges Message versions on delete or hard delete, and every version of a Space and its contents on Space
  destroy (§2, decision 22). Graph memory restores a destroyed GraphNode at its final `blobId`/`snapshotSeq` with an
  empty op log whose head is `snapshotSeq` (§2, decision 22).
- `test-helpers` knobs: `set_history_clock_for_test`, `discard_version_for_test`, `purge_object_history` (the §8.3 stand-in),
  and `set_rights_changed_at_for_test`.

## 8. Client bindings (`jmap-object-history-client`)

- Canonical client shape (mail-client): sealed `JmapObjectHistoryExt::with_object_history_session(session) ->
  SessionClient`. It uses OHT types directly (no re-exports; the one-path rule) and offers `supports::<O>(cap)`.
- Generic `SessionClient` methods, `O: JmapObject + DeserializeOwned`, with method name `format!("{}/get", O::TYPE_NAME)`.
  `using` is the caller's data-type URIs plus `JMAP_OBJECT_HISTORY_URI`. More than four inputs go into a params struct
  (Public API rule 3):
  - `get_history::<O>(&HistoryGetCall) -> GetResponse<HistoryEntry<O>>` (`HistoryGetCall` = account, using, ids, properties, `HistoryGetArguments`)
  - `query_destroyed::<O>(&DestroyedQueryCall<O>) -> QueryResponse`; `query_changes` is the ordinary call (state-routed, §5)
  - `restore::<O>(&SetCall, extra_using) -> (SetResponse<PartialObject>, RestoreOutcome)`: the request is base-client's
    generic `SetCall` with its `restore` option (decision 8), sent through `JmapClient::set_call` with
    `JMAP_OBJECT_HISTORY_URI` added to `extra_using`. One `SetCall` can carry `restore` together with creates, updates,
    destroys and conditional options; an `atomicFailure` is decoded with base-client's `ConditionalError`, and its
    `notRestored` read through `AtomicFailure::not_restored()` (decision 6(b)).
- Paging: `older_page_before(entries, id) -> Option<&UTCDate>` returns the oldest `replaced` exactly as received (§3 SHOULD
  not re-serialise). `has_older(entries, id)` reads `hasOlder` on the oldest entry.
- Arguments go through `to_args`. The rustdoc covers §8.4 (do not call destroy permanent) and §3.1 (no reliance on
  `expires`, `hasOlder` or version numbers across requests).
- No per-type sugar in the extension clients. The generic calls cover every type. The next breaking release gives the
  30 typed `*_set` fns one growable options parameter carrying conditional and object-history options together
  (decision 8; conditional bead C13).

## 9. Interactions and breaking changes

**conditional-02** (JMAP-y1yu; `docs/design/conditional.md`):
- Registration composes (decision 1): `register_<x>_handlers_with_history`, then `register_<x>_conditional`, which attaches
  `SetSupport` to the history-aware `/set`. testjig and the in-repo reference servers do both.
- Order for one `Foo/set`: `ifInState` → `ifUnchangedBy` (both against the state at method start, conditional §3.1 rules 1
  and 5, evaluated by the dispatcher before the handler) → restore → create → update → destroy. A destroyed object "does
  not exist" for `ifUnchangedBy`, so `{"x": null}` holds and `{"x": {}}` fails before a restore of `x`. `ifUnchangedBy`
  compares the plain `Foo/get` representation, never `objectHistory`.
- `atomic` (decision 6(b)): conditional §4 binds `atomic` to "every creation, update, and destroy", and `atomicFailure`
  "MAY carry `notCreated`, `notUpdated`, and `notDestroyed`". Restores are not named in either draft. Stopgap: `atomic`
  covers restores, and `atomicFailure` carries `notRestored` (in `AtomicFailure.extra`, typed accessor
  `not_restored()`); raised on both drafts (bead H22). The restore phase runs inside the handler, so inside the scope;
  deferred failures map through `SetOp::Restore`. A backend that cannot restore inside the transaction ⇒
  `cannotApplyAtomically`. Both error codes are `&str` consts (`ATOMIC_FAILURE`, `CANNOT_APPLY_ATOMICALLY`) in
  `jmap-types`' conditional module, not `MethodErrorType` variants (decision 7).
- Conditional adds no shared `/set` pipeline, so `run_restore_phase` works with the per-crate `SetAccumulators`/
  `finalize_set_response` copies. H3 adds a new `history.rs` and leaves `helpers.rs` alone, so the only file shared with
  conditional's foundation beads is the `jmap-server` `lib.rs` re-export list (a trivial rebase).
- Without the URI in `using`, conditional's arguments are rejected by one dispatcher check on every deployment (decision 2),
  and history arguments by a second check of the same shape (decision 18, §5).

**blobext-03** (tna1.2; crates `jmap-blob-types` / `jmap-blob-server` / `jmap-blob-client`, decision 4): `Blob/lookup`
`matchedIds` and `blobHasReference` count current objects only (§3.1), for blob2 and for the legacy RFC 9404
`urn:ietf:params:jmap:blob` Blob/lookup that blobext serves (decision 11). The blobext design states the same thing. A blob
that only history references may be destroyed through `Blob/set`. Afterwards, versions referencing it are not returned. For an
Email that means every version, so the object leaves `includeDestroyed`, `onlyDestroyed` and `restore` (`notFound`,
decision 6(d)). History `expires` and Blob `expires` are independent. A blob of a returned version must pass the consumer's
download access check (Bk contract). The data-type registry (decision 14) lives in `jmap-types` and is shared: blobext reads
"Can Reference Blobs", this extension reads "Supports History" (bead blobext:B1 creates it; H15 sets Mark's drafts' values).

**emailpush-05** (JMAP-r44n; `docs/design/emailpush.md`): a restored Email is not a delivery. emailpush §3 excludes Emails
"created as the result of a JMAP method call". So a restore sends no EmailPush and leaves EmailDelivery state unchanged. It
still advances Email, Thread and Mailbox state (StateChange). `objectHistory` in `EmailPushConfig.properties` is accepted
where this deployment's Email/get accepts it and dropped from the stored list during normalization, so it is never pushed
and pushes stay a true prefix (decision 10). The emailpush memory backend's `email_history` flag follows the Email history
capability once H5 lands.

**Existing crates (Table 1).** metadata: historical versions carry `metadata`/`privateMetadata` as stored, projected per entry, and a
restore is never metadata-only. mail: `onDestroyRemoveEmails` and auto-expunge destroys are recorded. `VacationResponse`
is get-history only (singleton, never destroyed, no `/query`; every restore is `notFound`). `Thread`, `EmailSubmission`
and `SearchSnippet` are "No". SieveScript has no `QueryObject` and filters in the handler, so its destroyed query is
`get_object_history(ids: None, include_destroyed)` plus the same filter and paging. A restored script id is a valid
`onSuccessActivateScript` target in the same call. No `SieveScript/changes` is registered today. contacts:
`onDestroyRemoveContents` cascades recorded. calendars: `CalendarEvent/query` ignores `expandRecurrences` with
`onlyDestroyed`; Calendar destroy cascades recorded. filenode: missing `parentId` ⇒ `invalidProperties`, sibling name
clash ⇒ `alreadyExists`, parent-first restore order; the inherited-rights move logic (bda149e4) feeds
`rights_changed_at`. sharing: Principal and ShareNotification are "No".

**Decision-5 crates (decision 5).**
- tasks: Task and TaskList. TaskList has no `/query`. `onDestroyRemoveTasks` cascades are recorded; a TaskList destroy
  refused for remaining tasks records nothing. Task/copy is an id-minting path. TaskNotification is excluded
  (decision 22). The chat-tasks hidden Task properties are blanked per entry (§5). Task/TaskList are implemented ahead
  of their registration: draft-ietf-jmap-tasks-06 has no "JMAP Data Types" entries at all. The registrations come
  from Mark's own updated Tasks draft (decision 23, JMAP-jkl8.9: TaskList, Task, TaskNotification with Supports
  History Yes, Yes, No), not from an upstream ask; until it exists Task/TaskList sit on OHT's pending list, and bead
  H24 moves them to the registry.
- chat: Chat, Message, Space, ChatContact, CustomEmoji, SpaceBan, SpaceInvite. SpaceBan and SpaceInvite have no `/query`.
  Message history follows the chat-00 purge rule (§2, decision 22). A Space destroy cascades to its channel Chats and
  their Messages, and chat-00 says it "Hard-deletes all records; no tombstones are retained", so the destroy also purges
  the history of the Space and everything in it, and none of it can be restored (decision 22; §2). A Space restore is
  therefore always `notFound`, and Space/set's structural patch ops need no restore parser; `includeReplaced` on a live
  Space still returns its earlier versions. Restore of a SpaceInvite keeps its
  server-set `code` and `uses`. ReadPosition and PresenceStatus are excluded (decision 22). Bridge checks a consumer runs on create (chat-filenode `check_root_create`, chat-vtc
  `check_call_create`) also apply to a restore of the same object, since §5.1 allows "any of the errors that can be
  returned by an update or create"; each bridge crate's README says so.
- graph: Graph, GraphNode, GraphEdge. Graph destroy cascades to nodes and edges, node destroy to edges; all recorded.
  An edge restore whose `fromId`/`toId` node is still destroyed ⇒ `invalidProperties` (graph's one integrity rule). It
  succeeds when a `GraphNode/set` that restores the node runs earlier in the same request; restores are per method, so
  nodes and edges go in two calls, nodes first. GraphNodeOp is excluded (decision 22); its ops die with their node
  (graph-edit §Compaction), so a restored GraphNode comes back at its final `blobId`/`snapshotSeq` with an empty op log
  whose head is `snapshotSeq`; edits after the final snapshot are not recoverable and the graph crate documents that
  (decision 22).
  Nodes of a projected Graph (graph-projection) are derived views: the backend retains no history for them and answers
  with the live version only (§2.1 "MAY retain history for only some objects"). Retracted provenance records follow §2.
- scene: SceneAsset, SceneRegion, SceneObject. SceneRegion `invitedUserIds` is blanked per entry. SceneAvatar is
  excluded (decision 22). `SceneObject/interact` is not a `/set` and is untouched.
- vtc: VTCCall, VTCRecording, VTCLivestream. VTCLivestream `streamKey`/`streamUri` are blanked per entry. VTCRecording
  versions follow the blob rule (§7). VTCParticipant is excluded (decision 22). VTC side-effect events are not emitted
  for history reads; a restore emits what an update would.
- chat-mimi: KeyPackage is "No" (decision 22): an MLS KeyPackage is single-use key material, and restoring a consumed
  one would invite reuse. H14 writes "No" in chat-mimi-00, and KeyPackage gets no `HistoryObject` impl.

**Spec repository.** Mark's drafts in `~/PROJECT/jmap-chat-spec` that register data types (chat-00, chat-mimi-00,
graph-00, graph-edit-00, scene-00, vtc-00) get a "Supports History" line in every registration, with the values decision
22 confirmed (decisions 5, 22; bead H14), so the registry rows (decision 14) cite text that exists. The Tasks
registrations go in Mark's own updated Tasks draft (decision 23, JMAP-jkl8.9), not in this set.

**Breaking changes.**
- Public API: additive. New crates. `jmap-server` gets a new module, dependency and the additive
  `Dispatcher::register_history_aware` / `object_history_gaps`, and `jmap-types` gets `UTCDate::cmp_instant`.
  The nine type crates get new impls and a dependency. The server crates get new pub `*_with_history` functions and register
  functions, and their `/set` bodies move into private `*_set_impl`, so existing signatures stay the same. The `memory` API
  gains `with_object_history`. Snapshots only add lines.
- Wire, every deployment (CHANGELOG "Changed", as for conditional's decision 2): a history argument sent without the URI
  (`includeReplaced: true`, `includeDestroyed: true`, `onlyDestroyed: true`, non-null `destroyedAfter`/`destroyedBefore`,
  non-empty `restore`) now fails with `invalidArguments` instead of being ignored (decision 18). Otherwise none for
  deployments using the plain register functions. With `_with_history`: `/set` responses carry `restored` and
  `notRestored` whenever the URI is in `using`; `objectHistory` in create/update ⇒ `invalidProperties`; restores count
  toward `maxObjectsInSet` (decision 6(a)).

## 10. Test plan

Oracles are hand-written JSON from the spec text, never derived from the code under test. Tests use the memory backend with
an injected clock and go through the dispatcher.
- **Spec examples, byte for byte except `state`:** §3.2.1 ContactCard (three versions, `hasOlder` false/true/true, `expires` =
  replaced + 2592000 s); §3.2.2 `Email/changes` → `#ids` `/destroyed` → `Email/get includeDestroyed`; §4.1 `Email/query
  onlyDestroyed destroyedAfter limit 50` ⇒ `["email57","email42"]` with `canCalculateChanges: true`, then
  `Email/queryChanges` with only `sinceQueryState` after a restore ⇒ `removed: ["email42"]`; §5.3 restore with
  `"restored": {"email42": null}` and every other map `null`.
- **OHT:** round-trip, extras preservation, `replaced: null` and `maxHistoryDuration: null` serialized, `HistoryEntry<T>`
  flatten with T's `extra`, `RestoreOutcome::from_set_extra`, BTreeMap order; `is_history_type` true for every Table 1
  "Yes" row and Task/TaskList, false for Thread, EmailSubmission and an unknown name. **jmap-types:** `cmp_instant` with `…00.5Z` >
  `…00Z` and `…00.0000000001Z` > `…00Z`.
- **Per type crate:** every `impl HistoryObject` names a type with `is_history_type` true, and every decision-5 type of the
  crate has one (registry cross-check, decision 14).
- **OHS `select_versions`, table-driven from §3 prose:** live and destroyed × the four flag combinations; strict
  `historyAfter` (equal excluded, live kept); `historyBefore` drops live; "historySkip: 20 … any id with 20 or fewer matching
  entries is returned in notFound"; live counts as one skipped entry; `historyLimit` keeps the newest; `historyLimit: 0`
  puts the id in neither list (literal reading); `hasOlder` true for an unreturned older version; equal-ms collapse;
  `fold_restore` cases; `prune` discards oldest first and drops a destroyed object with its final version (decision 6(d)).
- **srv contract tests (mock backend):** flags false ⇒ `historyLimit: "x"` ignored, not rejected; a flag on an unsupported
  type ⇒ `invalidArguments`; `objectHistory` in `properties` with no flag ⇒ no error and not returned; returned with a flag
  even when missing from `properties`; `maxObjectsInGet` counts ids; `destroyedAfter` without `onlyDestroyed` ⇒
  `invalidArguments`; `destroyedAfter: null` alone is fine; destroyed queryChanges routed by state kind and a live state
  falls through (decision 6(c)); `restore: {"x": {}}` ≡
  `null`; non-object patch ⇒ `invalidPatch`; non-object `restore` ⇒ `invalidArguments`; restore on an unsupported type ⇒
  `invalidArguments`; restore runs before an update of the same id; `oldState` is from before the restore;
  `restored`/`notRestored` present (null) whenever the URI is in `using`, absent from plain handlers; `objectHistory` in a create ⇒
  `invalidProperties`; create + update + destroy + restore one over `maxObjectsInSet` ⇒ `requestTooLarge` (decision 6(a));
  backend `Other` redacted; `object_history_gaps` lists a method re-registered plain after `_with_history`.
- **Dispatcher check (decision 18, H23):** without the URI, `includeDestroyed: true`, `includeReplaced: true`,
  `onlyDestroyed: true`, `destroyedBefore: "…"` and `restore: {"x": null}` ⇒ `invalidArguments` with the handler not
  called, under plain and `_with_history` registration alike; `includeDestroyed: false`, `destroyedAfter: null` and
  `restore: {}` reach the handler without the key; with the URI the check does nothing.
- **Per-crate (memory):** restored id in `/changes` is `updated` (existed at `sinceState`) or `created`, never `destroyed`;
  the id is never reused; restoring a current id ⇒ `notFound`; a destroyed object whose final version was discarded ⇒
  `notFound` for get, absent from `onlyDestroyed`, `notFound` for restore, even with older versions left (decision 6(d));
  restoring something that references a destroyed container ⇒
  `invalidProperties`, and it succeeds once the container is restored earlier in the request; Mailbox/FileNode parent+child
  in one `restore` map succeed in either key order; Email keeps `blobId` and `threadId`, and a vanished Thread gives a new
  `threadId` in `restored`; SieveScript comes back inactive and a name clash ⇒ `alreadyExists`; FileNode name clash ⇒
  `alreadyExists`; `onlyDestroyed` default order is destruction time descending; cascade destroys appear in
  `includeDestroyed`; the §8.2 cut hides versions from before a rights change.
- **Decision-5 crates (memory):** Task/copy never mints a logged id; `onDestroyRemoveTasks` cascade appears in
  `includeDestroyed`; an edited Message returns its earlier versions, and after `deletedAt` is set (or a hard delete)
  returns none and restore ⇒ `notFound`; a live Space returns its earlier versions with `includeReplaced`; GraphEdge restore with a
  destroyed endpoint ⇒ `invalidProperties`, then succeeds after the node's restore; a projected GraphNode returns its live
  version only; VTCLivestream history entries blank `streamKey`/`streamUri` for a caller who cannot see them now, and
  SceneRegion entries blank `invitedUserIds`; the excluded types and KeyPackage (decision 22) answer a history flag with
  `invalidArguments`; after a Space destroy, the Space and its Chats, Messages, SpaceBans, SpaceInvites and CustomEmojis
  are `notFound` for `includeDestroyed`, absent from `onlyDestroyed` and `notFound` for restore (decision 22); a destroyed
  GraphNode edited past its last snapshot restores at its final `blobId`/`snapshotSeq` with an empty op log (decision 22).
- **Cross-extension (testjig, after tna1.2 and conditional):** a blob referenced only by a destroyed Email ⇒ `Blob/lookup`
  empty (blob2 and legacy RFC 9404, decision 11), `Blob/set destroy` succeeds, then the Email is gone and restore ⇒
  `notFound`; `ifUnchangedBy {"x": null}` with `restore x` succeeds; `atomic` with one failing restore ⇒ `atomicFailure`
  with `notRestored` and state unchanged, and the rolled-back restore leaves no version (decision 6(b)); a restored Email
  sends no EmailPush; an `EmailPushConfig` with `objectHistory` in `properties` is echoed without it (decision 10); Session
  has the URI in `capabilities` as `{}` exactly when some account has it; `object_history_gaps` is empty.

## 11. Bead breakdown (children of JMAP-tna1.3, now the object-history sub-epic of JMAP-tna1)

| # | Title | Crates / files | Size | Depends |
|---|---|---|---|---|
| D1 | Decision: confirm the private-type history list. Closed: resolved by decision 22 | none | S | none |
| D2 | Decision: history arguments without the URI, handler-only or dispatcher-wide. Closed: resolved by decision 18 (dispatcher-wide, bead H23) | none | S | none |
| H1 | object-history-types + scaffold: wire types, `HistoryObject`, `PENDING_HISTORY_REGISTRATION`, `is_history_type` over the jmap-types registry (decision 14), `UTCDate::cmp_instant`; empty OHS and client crates; workspace `members` + `[workspace.dependencies]` for all three; root AGENTS crate-map and dependency-tree rows for all three | new OHT; OHS/client stubs; jmap-types `id.rs`; root Cargo.toml, AGENTS.md | M | blobext:B1 |
| H2 | object-history-server: `select_versions`, `VersionLog` (Clone, oldest-first prune, decision 6(d)), retention, rights cut, `fold_restore` | OHS only | M | H1 |
| H3 | jmap-server `history.rs`: trait, params, parsing, generic get/query/queryChanges wrappers (state-kind routing, decision 6(c)), restore phase + `RestoreHooks`, envelope, `apply_restore_patch`, `enforce_max_objects_in_set_with_restore` (decision 6(a)), `register_history_aware` + `object_history_gaps` | jmap-server (`history.rs`, dispatcher mark, one `lib.rs` export line) | L | H1 |
| H4 | mail-server canonical: `*_set_impl` split for Mailbox/Identity/VacationResponse, history Mailbox/Identity/VacationResponse handlers, `register_mail_handlers_with_history`, memory `ObjectHistoryBackend` (VersionLog in `Inner`, change fold); AGENTS.md object-history pattern section | mail-types `backend.rs`, mail-server, AGENTS.md | L | H2, H3 |
| H5 | mail-server Email: history get (body values per version), onlyDestroyed query + state-routed queryChanges, restore (blob/thread), memory blob retention, cascade recording | mail-server | L | H4 |
| H6 | mail-server SieveScript history (feature `sieve`), `register_sieve_handlers_with_history` | mail-server | S | H5 (same `lib.rs`/`memory/mod.rs`) |
| H7 | contacts-server: AddressBook, ContactCard (§3.2.1 example), `onDestroyRemoveContents` | contacts-types, contacts-server | M | H4 |
| H8 | calendars-server: Calendar, CalendarEvent, ParticipantIdentity | calendars-types, calendars-server | M | H4 |
| H9 | filenode-server: FileNode (parent order, name clash, inherited-rights cut) | filenode-types, filenode-server | M | H4 |
| H10 | object-history-client (restore on base-client `SetCall`, decision 8) | OHC | M | H1, conditional:C6 |
| H11 | testjig: Session capability, `_with_history` registration for mail, contacts, calendars, filenode, tasks (before any `register_<x>_conditional`), gap assertion, spec examples end to end | jmap-testjig | M | H5–H9, H13 |
| H12 | cross-extension tests (blobext incl. legacy 9404 lookup, conditional atomic + restore, emailpush) | jmap-testjig `tests/` | S | H11; JMAP-tna1.2; conditional:C11; emailpush:8 |
| H13 | tasks-server: Task, TaskList (decision 5) | tasks-types, tasks-server, chat-tasks-server README | M | H4 |
| H14 | jmap-chat-spec: "Supports History" lines in the six registering drafts, cite object-history-03, render-check; values per decision 22 (KeyPackage No) | `~/PROJECT/jmap-chat-spec` drafts | S | D1 |
| H15 | jmap-types registry: "Supports History" values for Mark's drafts' rows (decisions 5, 14, 22) | jmap-types registry module | S | blobext:B1, D1, H14 |
| H16 | chat-server: Chat, Message, Space, ChatContact, CustomEmoji, SpaceBan, SpaceInvite; Message purge per chat-00 l.782, Space destroy purges its history (decision 22) | chat-types, chat-server, bridge READMEs | L | H4, D1, H15 |
| H17 | graph-server: Graph, GraphNode, GraphEdge; GraphNode restore at its final snapshot with an empty op log (decision 22) | graph-types, graph-server | M | H4, D1, H15 |
| H18 | scene-server: SceneAsset, SceneRegion, SceneObject | scene-types, scene-server | M | H4, D1, H15 |
| H19 | vtc-server: VTCCall, VTCRecording, VTCLivestream | vtc-types, vtc-server | M | H4, D1, H15 |
| H20 | testjig: chat `_with_history` registration and Message history end to end | jmap-testjig | S | H11, H16 |
| H21 (follow-up) | upstream: ask the Tasks authors to register Task/TaskList "Supports History: Yes". Closed: superseded by decision 23 (Mark's own Tasks draft, JMAP-jkl8.9) | none | S | none |
| H22 (follow-up) | upstream: draft the decision-6 stopgap messages/issues for the object-history and conditional authors | none (drafted text) | S | none |
| H23 | jmap-server: dispatcher check for history arguments without the URI on `*/get`, `*/query`, `*/set` (decision 18) | jmap-server (dispatcher, tests) | S | H3 |
| H24 (follow-up, P3) | Task/TaskList history from Mark's Tasks draft registrations: registry rows, drop `PENDING_HISTORY_REGISTRATION` (decision 23) | OHT, jmap-types registry | S | H1, blobext:B1, JMAP-jkl8.9 |

Parallelism: H2 ∥ H3 ∥ H10 after H1. H1 owns every shared manifest edit, so they touch disjoint crates. H7–H9, H13 and
H16–H19 run in parallel after H4 (separate crates). H4 → H5 → H6 run in series because they share
`mail-server/src/lib.rs` and `memory/mod.rs`. H4 goes first because mail-server is the canonical extension server: its
`*_set_impl` and `register_*_with_history` shape is the one the siblings copy. D1 is closed (decision 22), so the
private-type work (H14 → H15 → H16–H19) is unblocked by it. H23 follows H3 and runs in parallel with H4 onwards. Each bead updates its own crate's README/AGENTS/PLAN and
`public-api/<crate>.txt`. The root `CHANGELOG.md` is the one shared file: each bead adds one bullet under `[Unreleased]`,
and rebase conflicts there are expected and trivial. testjig: H11 and conditional C11 both edit `http.rs`; whichever lands
second keeps the order `register_<x>_handlers_with_history` then `register_<x>_conditional`.

## 12. Open questions for Mark

Decided and removed (see `docs/design/DECISIONS.md`): old Q1 trait shape (separate trait and `_with_history`
registration, adopted as the model in decision 1); old Q2 type scope (decision 5); old Q4 spec gaps (decision 6(a)–(c));
old Q5 discarded final version (decision 6(d)); old Q6 `objectHistory` in `EmailPushConfig.properties` (decision 10);
history arguments without the URI (decision 18: one dispatcher check, bead H23); KeyPackage (decision 22: No); history
after a Space destroy (decision 22: purged with the Space); restoring an edited GraphNode (decision 22: final snapshot,
empty op log); the conditional registration shape (decision 17: the `register_<x>_conditional` attach call).

Remaining: none.

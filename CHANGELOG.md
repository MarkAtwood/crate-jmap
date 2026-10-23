# Changelog

All notable changes to the publishable `jmap-*` crates in this workspace are
recorded here. The crates are versioned together (see `RELEASING.md`), so one
entry covers every crate. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) with the pre-1.0
convention that a minor bump may break the API.

Entries marked **BREAKING** need source changes in downstream code, or change
wire behaviour that a conforming peer can observe.

## [Unreleased]

### Added

- **jmap-chat-server**: **BREAKING** `build_chat_message_push` takes a
  trailing `unread_count: Option<u64>` that becomes the payload's
  `unreadCount`, and the new `owner_unread_count` returns the owner's
  total unread count across non-muted Chats to pass there
  (draft-atwood-jmap-chat-push-00 §ChatMessagePush: "Servers SHOULD
  include this for platforms that support atomic badge updates"). Pass
  `None` to keep the old output (bd JMAP-3rpv).
- **jmap-chat-mimi-client**: `decrypt_group_info_and_tree` decrypts a
  verified GroupInfoResponse's `encrypted_groupinfo_and_tree` with the
  `groupInfoPublicKey` private key (RFC 9420 §5.1.3 DecryptWithLabel,
  label "GroupInfo and ratchet_tree encryption", context `room_id`) and
  parses the GroupInfoRatchetTreeTBE into `GroupInfoAndTree`: the
  GroupInfo, a `RatchetTreeOption` and the `PendingProposal`s
  (draft-ietf-mimi-protocol-06 §5.6). `GroupInfoResponseError` gains
  `DecryptionFailed` and `MalformedGroupInfoAndTree` (bd JMAP-6k5d.13).

- **jmap-chat-vtc-server**: `may_access_call` decides the Chat rules of
  per-call access for a `VtcBackend`'s `get_objects`, `query_objects` and
  `query_changes` (bd JMAP-8mem.13, draft-atwood-jmap-vtc-00 §Joining a
  Room or Scheduled Call, §Chat-Bound Calls): `false` when the initiator
  blocked the caller, the caller is not a member of the `chatId` Chat or
  the bound Space, or lacks `"view"` on the bound channel.
- **jmap-metadata-server**: a reusable metadata baseline for backends
  (bd JMAP-5kpy). `MetadataCondition::take_from` removes the six
  draft-ietf-jmap-metadata-04 §3.6 fields from a filter condition,
  rejecting a malformed path (`invalidArguments`) or a
  `privateMetadata*` condition without `supportsPrivate`
  (`unsupportedFilter`), and `matches` evaluates them.
  `changed_properties`, `ChangedProperties` and `MetadataOnlyChanges`
  track per-id changes over a `/changes` window for
  `ignoreMetadataOnlyChanges` and `updatedProperties` (§3, §3.3).
  `check_metadata_write` and `check_metadata_depth` apply the §3.2
  `/set` rules 2–4 and the §2.1.1 migration case, returning
  `InvalidMetadataWrite`.
- **jmap-base-client**: `to_args(&impl Serialize) -> Result<Value,
  ClientError>` serializes a method argument, mapping failure to
  `ClientError::Serialize` (bd JMAP-tjvm.11). It is the one way extension
  clients put a caller-supplied value into a request.
- **jmap-server**: `json_type_name(&Value) -> &'static str` names a JSON
  value's top-level type (`null`, `boolean`, `number`, `string`, `array`,
  `object`) for error descriptions (bd JMAP-zegc).
- **jmap-types**: `impl_string_enum!` now emits an inherent
  `pub fn as_str(&self) -> &str`, and its `Serialize` and `Display` impls
  call it, so every string-backed wire enum has one wire mapping (bd
  JMAP-axxb). The hand-written `as_str()` copies on `BodyType`,
  `BroadcastScope`, `ChannelPermissionTargetType`, `ChatMemberRole`,
  `ChatPermission`, `EphemeralDataType` (jmap-chat-types), `QuotaScope`,
  `QuotaResourceType` (jmap-chat-client), `VtcEventType` (jmap-vtc-types),
  `SceneEventType` (jmap-scene-types) and `GraphEditDataType`
  (jmap-graph-types) are gone; their signature and output are unchanged.
- **jmap-server**: `limit_or_max(Option<u64>) -> usize` converts an RFC 8620
  `limit` / `maxChanges` argument to an item count, saturating instead of
  truncating on 32-bit targets (bd JMAP-pgj7). The mail, calendars, chat,
  contacts, filenode, graph, scene, sharing, tasks and vtc servers use it in
  place of bare `n as usize` casts, and the mail-server anchor branches of
  `Email/query`, `Mailbox/query`, `EmailSubmission/query` and
  `SieveScript/query` no longer use `as` casts (or an overflowing
  `anchor_idx + anchorOffset`) to compute the start index.
- **jmap-server**: `backend_set_error_value(BackendSetError<E>) -> Value` is
  the one RFC 8620 §5.3 `notCreated`/`notUpdated`/`notDestroyed` mapping for
  backend `/set` failures: a `SetError` verbatim, `Other` as the redacted
  `serverFail` (bd JMAP-jfia.36). Every `*-server` crate now calls it instead
  of its own three-arm match or private copy, and the unreachable
  `"unhandled backend error variant"` fallback is gone from those sites
  (bd JMAP-1xze).
  The last bespoke catch-all, `jmap-chat-federation-server`'s Peer/* write
  failure mapping, now yields the same redacted `serverFail` ("internal
  error") instead of "the backend refused the write" (bd JMAP-510h.18).
- **workspace**: committed public-API snapshots (`public-api/<crate>.txt`,
  `scripts/public-api.sh`) and a CI `public-api` job that fails when they
  are stale, so every public-surface change appears in review diffs
  (bd JMAP-698j.1).
- **jmap-server**: `parse_wire_id(raw, property)` validates a client-supplied
  id against RFC 8620 §1.2 and returns an `invalidProperties` SetError naming
  `property` when it is not an Id (bd JMAP-k9va).
- **jmap-server**: `Dispatcher::dispatch` emits one `tracing::error!` event
  when a method handler panics, with only the `method` and `call_id` fields
  (never the panic payload or the arguments), so operators can tell a caught
  panic from a handler's own `serverFail`. `jmap-server` now depends on
  `tracing`; the `dispatch` signature is unchanged (bd JMAP-jfia.29).
- **jmap-server**: `parse_request_bytes(body: &[u8], max_calls)` parses the
  raw HTTP body with `serde_json::from_slice`, maps a parse failure to the
  RFC 8620 §3.6.1 `notJSON` request error, then runs `parse_request`
  (bd JMAP-jfia.21).
- **jmap-types**: `check_property_classification::<T>()` checks a fully
  populated object against `T`'s `SetObject` property lists, and
  `PushSubscription` implements `JmapObject` and `SetObject`
  (bd JMAP-x2gd.87).
- **jmap-server**: `reject_server_set::<O>()` and
  `reject_read_only_patch::<O>(patch, current)` build the RFC 8620 §5.3
  `invalidProperties` SetError for a `/set` create carrying a non-null
  server-set property and for an update patch naming a server-set or
  immutable property, from `O`'s `SetObject` lists. Given the stored
  object, `reject_read_only_patch` lets a key repeat its stored value.
  `is_client_updatable::<O>(prop)` says whether an update may change a
  property (bd JMAP-x2gd.87).
- **jmap-types**: `Filter::matches` evaluates an RFC 8620 §5.5
  `FilterOperator` tree (`AND`/`OR`/`NOT`) over a caller's
  per-condition predicate (bd JMAP-skk9.2).
- **jmap-chat-types**, **jmap-graph-types**, **jmap-mail-types**,
  **jmap-scene-types**, **jmap-vtc-types**: crate-root re-exports for
  the items that had only a module path (bd JMAP-wf92.23): the chat
  `*Filter`/`*Comparator` aliases, `*FilterCondition`s,
  `*ComparatorProperty`s and `SPEC_*`/`CHAT_CAP_*`/
  `COMPANION_EPHEMERAL_DATA_TYPES` vocabulary constants; the graph
  `/query` filters and comparators, the graph-edit stream, cursor and
  `GraphNode/op` query types and constants (`OPS_COMPACTED`,
  `DATA_TYPE_OPS`, ...), and the graph-search filter language
  (`Expression`, `FieldTest`, `EdgeTest`, `ReachTest`, `Bound`,
  `Direction`, `CandidateKind`, `WhereError`, `QUERY_TOO_COMPLEX`); the
  mail system keyword constants (`DRAFT`, `SEEN`, `FLAGGED`,
  `ANSWERED`, `FORWARDED`, `PHISHING`, `JUNK`, `NOT_JUNK`) and, with the
  `mdn` feature, the RFC 9007 MDN types and `JMAP_MDN_URI`; the scene and
  vtc `/query` filters and comparators (`WithinRadius`, `WithinBounds`,
  `VtcCallFilter`, ...). Each crate also re-exports `Filter`,
  `FilterOperator` and `Operator` at its root; jmap-chat-types also
  re-exports the generic `Comparator<P>` behind its `*Comparator`
  aliases.
- **jmap-base-client**, **jmap-calendars-client**, **jmap-chat-client**:
  crate-root re-exports for the items that had only a module path
  (bd JMAP-wf92.23): `HttpErrorKind`, `WebSocketErrorKind`,
  `connect_ws_with_limit`, `DEFAULT_WS_MAX_MESSAGE_BYTES`;
  `CalendarEventParseParams`, `PrincipalGetAvailabilityParams`;
  `CALL_ID`, `server_retry_after`, `ServerRetryAfterError`,
  `parse_chat_ws_frame`, `ChatCapabilityRole`, `Endpoint`.

- **jmap-chat-mimi-server**: `pub use openmls;` and
  `pub use openmls_rust_crypto;` (now a normal dependency);
  **jmap-chat-mimi-client**: `pub use openmls;`. openmls 0.x is a
  recorded public dependency of both crates, which take a major bump
  whenever they move to a new openmls minor version (bd JMAP-skk9.11).
- **jmap-types**: `Urgency` implements `Serialize` and `Deserialize` as
  its RFC 8030 §5.3 header string; any other string is a deserialize
  error. `jmap-chat-types`: `UrgencyLevel::to_urgency` (bd JMAP-skk9.5).
- **jmap-chat-server**: `MemoryBackend::set_space_everyone_permissions`
  sets the implicit `@everyone` role's permissions of one Space,
  replacing the `set_everyone_permissions` set there, in
  `ChatBackend::everyone_permissions` and in the backend's own
  permission checks (draft-atwood-jmap-chat-00 §SpaceRole: the role
  "which every member of a Space holds"; bd JMAP-wf92.22).
- **jmap-calendars-server**: `CalendarsBackend::privacy_conflicts`
  receives every valid create, update and destroy of one
  `CalendarEvent/set` before any is applied and returns the
  `PrivacyConflicts` (creation ids and event ids) touching a uid whose
  events would end up with different `privacy`; the handler rejects
  those with `invalidProperties` naming `privacy` and applies the rest
  (draft-ietf-jmap-calendars-33 §5.9). `may_copy_calendar_event` lets
  the backend refuse a `CalendarEvent/copy` of a `private` event out of
  an account that does not belong to the user; the handler answers
  `forbidden` (§5.10). Both default to fail-open (bd:JMAP-bghk).
- **jmap-mail-types**: `SearchSnippetGetResponse`, the RFC 8621 §5.1
  `SearchSnippet/get` response (`accountId`, `list`, nullable
  `notFound`).
- **jmap-types**: `PartialObject` (`id: Option<Id>` plus a flattened
  `extra` map) for the partial objects a server echoes in `/set`
  `created` / `updated` and `/copy` `created` (RFC 8620 §5.3, §5.4), and
  `CopyResponse<T>`, the RFC 8620 §5.4 `/copy` response shape with
  `fromAccountId` and no `updated` / `destroyed`.
- **jmap-server**: `CallContext::created_ids` carries the request's
  creation id -> server id map (client `createdIds` plus every earlier
  `/set` creation) into each call, and `resolve_creation_ref` resolves a
  `#creationId` Id-typed value against it (RFC 8620 §3.3, §5.3).
  `CallContext::with_created_ids` sets it. Extension `/set` handlers
  must call it on their Id-typed properties to honour `#` references.
- **jmap-chat-mimi-server**: `notify_from_hub(room_id, sender_uri)`
  accepts a `/notify` only when the transport-authenticated sender's
  authority is the room ID's authority, the room's hub; `store_fanout`
  docs require the check for `/notify` (draft-atwood-jmap-chat-mimi-00
  §application/mimi-fanout, jmap-chat-spec 29e9a41 L439).
- **jmap-filenode-server**: `FileNode/copy` responses carry `destroyed`,
  the ids of destination nodes that onExists `replace` or `newest`
  destroyed, `onDestroyRemoveChildren` cascades included; omitted when
  nothing was destroyed (draft-ietf-jmap-filenode-16 L655-657, L677-679,
  L773-775). **jmap-filenode-client** already decodes it as
  `SetResponse::destroyed`.
- **jmap-did-resolver**: `Resolver::resolve_with_hint` uses the account
  capability's `didDocumentUri`. For `did:webvh` the log comes from the
  hint on any origin and is verified as usual; the witness file still
  comes from the method-derived URL (draft-atwood-jmap-did-00 L194 at
  jmap-chat-spec 29e9a41). For `did:web` the hint is used only when its
  origin is the method-derived URL's origin. `did:key` and `did:plc`
  ignore it. An unreachable hint (`FetchError::Status` or `Transport`)
  falls back to the method-derived URL; an oversized body
  (`FetchError::TooLarge`) is an error, not a fallback.
- **jmap-server**: `parse_max_changes`, the shared RFC 8620 §5.2
  `maxChanges` parser (absent/`null` -> `None`; `0`, negative or
  non-integer -> `invalidArguments`). `handle_changes`,
  `handle_query_changes` and the custom `/changes` and `/queryChanges`
  handlers in jmap-mail-server and jmap-chat-server now use it instead of
  inline copies.
- **jmap-chat-mimi-client**: `SessionClient::key_package_get`,
  `key_package_set` (create and destroy) and `key_material_fetch`, the
  `KeyPackage/get`, `KeyPackage/set` and `KeyMaterial/fetch` bindings of
  draft-atwood-jmap-chat-mimi-00.
- **jmap-filenode-server**: `FileNodeBackend::files_rights` (default
  `Ok(None)`, fail-open). `FileNode/get` reports a `Some` result as the
  node's `myRights`, including the ancestors `fetchParents` appends; an
  `Err` is `serverFail`. Unless `properties` excludes `myRights`, each
  `FileNode/get` now makes one extra `get_objects` read. `FileNode/set`
  and `FileNode/copy` report a `Some` result in each `created` entry
  (RFC 8620 §5.3); there an `Err` leaves the create standing and the
  entry reports the stored `myRights`.
- **jmap-chat-filenode-server**: a deployment returns `space_files_rights`
  from `FileNodeBackend::files_rights`, so `FileNode/get`, `FileNode/set`
  and `FileNode/copy` report the §Permission Mapping rights without
  patching their reads.
- **jmap-graph-types**: `GraphProjectionCapability`, the account-level
  `urn:ietf:params:jmap:graph:projection` object (`graphId`) of
  draft-atwood-jmap-graph-projection-00 §Account-Level Capability Object.
- **jmap-graph-server**: the `GraphBackend` rustdoc states the
  `get_changes` duty: Graph, GraphNode, GraphEdge and GraphNodeOp
  `/changes` SHOULD omit what the caller may not read and report gained or
  lost `mayRead` as created or destroyed (draft-atwood-jmap-graph-00
  §Security Considerations). `MemoryBackend` does not scope `/changes`.
- **jmap-scene-server**: the `SceneBackend` rustdoc states that
  `get_changes` is per account, not scoped to the caller: Scene visibility
  filtering "does not change the JMAP state model" and a hidden object MAY
  be reported in `notFound` from `/get` (draft-atwood-jmap-scene-00
  §Visibility Contract).

- **jmap-server**: `Dispatcher::set_guard` installs an optional per-call
  guard, run with the method name and caller after result references
  resolve and before handler lookup; an `Err` becomes that call's error
  response. Without a guard every call reaches its handler, as before.
- **jmap-chat-federation-server**: `method_access_guard`, the peer-role
  check `check_method_access` shaped for `Dispatcher::set_guard`, so the
  `forbidden` rules of draft-atwood-jmap-chat-federation-00 §Account-Level
  Capability and §Server-to-Server Methods cover every registered handler.

- **jmap-chat-server**: `ChatBackend::maintains_unread_count` (default
  `false`, today's exact upkeep). A backend that returns `true` keeps
  `Chat.unreadCount` itself and may defer, coalesce or cap it, as the
  advisory count of draft-atwood-jmap-chat-00 §Chat allows (jmap-chat-spec
  45d611c); `Chat/get` then returns the stored value and Message or
  ReadPosition writes no longer rewrite the Chat. The backend must still
  advance the `Chat` state whenever the stored value changes (RFC 8620
  §5.1).

- **jmap-graph-projection-server** (new): Mail and Contacts
  binding for draft-atwood-jmap-graph-projection-00 — `Projectable` for the
  seven RFC 8621/9610 types with registered object URI type names,
  `Reference` edges typed `<DataType>/<property>` from `Id`, `Id|null`,
  `Id[]` and `Id[T]` properties, `object_uri`, `projected_node`,
  `projected_edge`, the GraphNode `objectUri` filter (`node_matches`,
  `node_object_uri`), `projected_graph_rights` (no writes accepted) and
  `visible_ids` (visibility by native `/get`).

- **jmap-graph-types**: `search` module for draft-atwood-jmap-graph-search-00
  (`urn:ietf:params:jmap:graph:search`) — `JMAP_GRAPH_SEARCH_URI`,
  `GraphSearchCapability` (`languages`, `maxDepth`, `supportsText`), the
  `where`, `language` and `expression` fields of `GraphNodeFilterCondition`
  and `GraphEdgeFilterCondition` (`where` as raw JSON), the built-in filter
  language AST `Expression` (`FieldTest`, `Bound`, `EdgeTest`, `ReachTest`,
  `Direction`) with `Expression::parse` (well-formedness per candidate kind;
  `WhereError` maps to `invalidArguments` or `unsupportedFilter`),
  `Expression::to_value` and `Expression::traversal_depth`, and the
  `queryTooComplex` method error name.

- **jmap-graph-server**: draft-atwood-jmap-graph-search-00 server side —
  `GraphBackend::graph_search_capability` (default `None`, which keeps the
  `where`/`language`/`expression` filter properties `unsupportedFilter`),
  `search::check_search_filter` (well-formedness, `languages`,
  `supportsText`, `$collation` against `search::COLLATIONS`, `maxDepth` as
  `queryTooComplex`), the `search::GraphView` `where` evaluator over the
  caller-visible contents of one Graph, `canCalculateChanges: false` on
  search `/query` results and `cannotCalculateChanges` on search
  `/queryChanges`, `JMAP_GRAPH_SEARCH_URI` re-export, and
  `MemoryBackend::register_search`. No advertised language is implemented.

- **jmap-graph-client**: draft-atwood-jmap-graph-edit-00 client bindings —
  `SessionClient::graph_node_op_get`, `graph_node_op_changes`, create-only
  `graph_node_op_set`, `graph_node_op_query`, `graph_node_op_query_changes`
  and `graph_node_cursor`, sent with `urn:ietf:params:jmap:graph:edit` in
  `using`; `ws` module with the sealed `GraphEditWsExt` on `WsSession`
  (`send_graph_edit_stream_enable`, `send_graph_edit_stream_disable`,
  `next_graph_edit_frame`), `GraphEditWsFrame` and
  `parse_graph_edit_ws_frame`.
- **jmap-graph-server**: draft-atwood-jmap-graph-edit-00 server support —
  `register_graph_edit_handlers` with `GraphNodeOp/get`, `/changes`,
  create-only `/set` (backend-assigned `seq`, `baseSeq`, base64url `data`
  and `maxOpSize` checks), `/query` and `/queryChanges` (single
  `nodeId`/`afterSeq` condition, `seq` ascending, `opsCompacted`), and
  `GraphNode/cursor`; the GraphNode `editFormat`/`snapshotSeq` rules when
  the account has the edit capability; `GraphBackend` hooks
  `graph_edit_capability`, `edit_log` (`EditLog`), `projected_graph_id`,
  `publish_cursor` and `is_sender_blocked`, all with defaults; the
  `stream` module (`GraphEditSubscription`, `should_deliver`) for the
  WebSocket op and cursor stream; and in `MemoryBackend` an op log,
  front-only `compact_ops` honoring `opRetention`, and op destroy
  cascades.

- **jmap-graph-types**: `GraphCursorEvent::new`.

- **jmap-graph-types**: `edit` module for draft-atwood-jmap-graph-edit-00
  (`urn:ietf:params:jmap:graph:edit`) — `JMAP_GRAPH_EDIT_URI`,
  `GraphEditCapability`, the `GraphNodeOp` object with its `JmapObject`
  impls, `GraphNodeOpFilterCondition` (a single condition with a required
  `nodeId`) and `GraphNodeOpComparator` (`seq`), `GraphNode::edit_format`
  and `GraphNode::snapshot_seq` (read from `extra`), the `opsCompacted`
  method error name, `GraphNode/cursor` request and response, and the
  WebSocket frames `GraphEditStreamEnable`/`GraphEditStreamDisable`
  (`GraphEditStreamMessage`) and `GraphNodeOpEvent`/`GraphCursorEvent`
  (`GraphEditEvent`).

- **jmap-graph-client** (new): draft-atwood-jmap-graph-00
  client bindings — `JmapGraphExt::with_graph_session` and `SessionClient`
  `/get`, `/changes`, `/set`, `/query` and `/queryChanges` of `Graph`,
  `GraphNode` and `GraphEdge`, typed with the `jmap-graph-types` filters and
  comparators, and `for_account` for a shared Graph's account (§Accounts).
  GraphNode and GraphEdge `/query` take a required single filter condition.
  A GraphNode filter with `objectUri` adds
  `urn:ietf:params:jmap:graph:projection` to `using`.

- **jmap-graph-types**: `JMAP_GRAPH_PROJECTION_URI`
  (`urn:ietf:params:jmap:graph:projection`) and
  `GraphNodeFilterCondition::object_uri`, the
  draft-atwood-jmap-graph-projection-00 §GraphNode/query Filter. In
  **jmap-graph-server** a condition with `objectUri` matches no node: its
  nodes are never projected, so their `objectUri` is `null`.

- **jmap-graph-server** (new): draft-atwood-jmap-graph-00
  method handlers — `register_graph_handlers` and the standard `/get`,
  `/changes`, `/set`, `/query` and `/queryChanges` of `Graph`, `GraphNode`
  and `GraphEdge` over a `GraphBackend` trait, with a `memory` reference
  backend. Rights come from the caller's `Graph.myRights` (§Graph/set:
  own `isSubscribed` with `mayRead`, `name`/`annotations` with `mayWrite`,
  `shareWith` and destroy with `mayAdmin`; GraphNode/GraphEdge `/set` with
  `mayWrite`); a Graph the caller may not read is `notFound`, with its
  nodes and edges (§Access Control). Destroying a Graph destroys its nodes
  and edges, destroying a node its edges; edge `fromId`/`toId` must name
  nodes of the edge's Graph; annotation keys are non-empty and
  `maxAnnotationSize` is `tooLarge`; `uri` must be a URI; GraphNode and
  GraphEdge `/query` need a single `graphId` condition
  (`unsupportedFilter`).

- **jmap-graph-types** (new): draft-atwood-jmap-graph-00
  wire types — `JMAP_GRAPH_URI` (`urn:ietf:params:jmap:graph`),
  `GraphCapability` (`maxAnnotationSize`), `PrincipalGraphCapability`,
  `Graph` and `GraphRights` (RFC 9670 `shareWith`/`myRights`), `GraphNode`
  (annotations, optional blob content, optional `uri`), `GraphEdge`
  (`fromId`/`toId`, `type`), the `query` filters and comparators and the
  `JmapObject` impls. The GraphNode and GraphEdge filters are single
  conditions with a required `graphId`, so a FilterOperator or a filter
  without `graphId` fails to decode (`unsupportedFilter`, draft §GraphNode/query).
  `JMAP_URI_GRAPH_TYPE`, `JMAP_URI_GRAPH_NODE_TYPE` and
  `JMAP_URI_GRAPH_EDGE_TYPE` name the registered JMAP Object URI types.

- **jmap-scene-client**: `JmapSceneExt::with_scene_session` and a
  `SessionClient` binding the `/get`, `/changes`, `/set`, `/query` and
  `/queryChanges` methods of `SceneAsset`, `SceneRegion`, `SceneObject`
  and `SceneAvatar` (draft-atwood-jmap-scene-00 §Methods), typed with the
  `jmap-scene-types` objects, filters and comparators.
  `SessionClient::for_account` targets a region's hosting account
  (§Accounts: "Every `accountId` in a Scene method call ... is the id of
  the hosting account"). `jmap-types` is now a normal dependency.

- **jmap-scene-server**: `SceneBackend::may_see_region_avatars` hook
  (draft-atwood-jmap-scene-00 §Security Considerations: for a region
  bound to an E2EE VTCCall the server "SHOULD return `notFound` for
  `SceneAvatar/get`, exclude the records from `SceneAvatar/query`, and
  suppress `SceneAvatarEvent` ... delivery" toward non-participants).
  `SceneAvatar/get` reports the avatars of a region answering `false` in
  `notFound` and `should_deliver` drops their `SceneAvatarEvent`s; the
  backend's `query_objects`/`query_changes` must omit them. The default
  hides nothing (fail-open); `MemoryBackend` hides what
  `hide_region_avatars` records.
- **jmap-scene-vtc-server**: `may_see_region_avatars`, the VTC answer for
  that hook: `true` unless the region's VTCCall has `e2eeEnabled` and the
  user has no VTCParticipant record in it.

- **jmap-scene-types**: `SceneStreamEnable::id` (draft-atwood-jmap-scene-wss-00
  §SceneStreamEnable: "A `RequestError` caused by this message carries
  this value as its `requestId`"). An `id` on the wire now lands in this
  field instead of `extra`.

- **jmap-scene-server**: `SceneBackend::channel_in_space` hook.
  `SceneRegion/set` calls it when a create or update sets `channelId` or
  `spaceId` and the resulting region has both non-null, and fails with
  `invalidProperties` naming `channelId` when it returns `false`
  (draft-atwood-jmap-scene-00 §Updating a Region: "a non-null `channelId`
  is a channel of the Space named by `spaceId`"). The default accepts
  every pair (fail-open); `MemoryBackend` accepts only pairs recorded
  with `set_channel_space`.

- **jmap-scene-types**: `SceneCapability::supports_scene_asset`
  (`supportsSceneAsset`, draft-atwood-jmap-scene-00 §Account-Level
  Capability Object). A capability object without it deserializes as
  `true`.
- **jmap-scene-server**: every `SceneAsset/*` handler returns
  `unknownMethod` when `SceneBackend::scene_capability` reports
  `supportsSceneAsset: false` for an existing account ("When `false`, a
  call to any SceneAsset method returns an `unknownMethod` error").
- **jmap-scene-server**: `SceneObject/interact`
  (draft-atwood-jmap-scene-00 §SceneObject/interact), registered by
  `register_scene_handlers` as `handle_scene_object_interact`. Each
  interaction is `invalidProperties` for a missing or wrongly typed
  property, `notFound` for a missing, inaccessible or invisible region or
  object, `forbidden` without an active avatar or for an object with
  `interactable: false`, and otherwise goes to the new
  `SceneBackend::accept_interaction` hook as a `SceneInteractionEvent`
  with the caller's userId. The hook returns `false` for `rateLimit` and
  hands accepted events to the WebSocket transport; its default accepts
  everything and delivers nothing (fail-open).

- **jmap-scene-types**: `SceneInteraction`, `SceneObjectInteractRequest`
  and `SceneObjectInteractResponse`, the `SceneObject/interact` arguments
  and response.

- **jmap-chat-vtc-server**: `eject_banned_user` for the Space ban rule
  of draft-atwood-jmap-vtc-00 §Chat-Bound Calls. On a SpaceBan it sets
  `leftAt` on the banned user's active VTCParticipant records in calls
  bound to the Space and returns one `"left"` `VTCParticipantEvent` per
  record, for the caller to deliver. Like a `VTCParticipant/set` leave,
  it ends a ring or scheduled call left with no active participant and
  hands on the moderator role (§Leaving a Call). Call it from
  `ChatBackend::on_space_ban_created` after `contact_identity`.

- **jmap-vtc-server**: `after_leave`, the call side effects of
  draft-atwood-jmap-vtc-00 §Leaving a Call that `VTCParticipant/set`
  applies after a leave, for code that sets `leftAt` another way. A ring
  or scheduled call left with no active participant ends with `endReason`
  `"completed"` (`"cancelled"` while `"ringing"`), and when the last
  moderator leaves, the earliest-joined authenticated participant gets
  `"moderator"`.

- **jmap-vtc-client**: `VtcWsExt`, `VtcWsFrame` and `parse_vtc_ws_frame`
  for draft-atwood-jmap-vtc-wss-00 on a `jmap_base_client::WsSession`.
  `send_vtc_stream_enable` and `send_vtc_stream_disable` send the
  `VTCStreamEnable` and `VTCStreamDisable` frames; `next_vtc_frame` types
  the eight VTC event types and passes every other frame through, so an
  unrecognized `@type` or a malformed event never fails the connection
  (§Forward Compatibility).

- **jmap-vtc-types**: `VtcStreamEnable::id`, the optional client-specified
  frame identifier of draft-atwood-jmap-vtc-wss-00 §VTCStreamEnable. The
  transport echoes it as the `requestId` of a `RequestError` the frame
  causes (RFC 8887 §4.3.4). It was previously kept only in `extra`.

- **jmap-vtc-server**: ring fork and ring timeout of
  draft-atwood-jmap-vtc-00 §Multi-Device Forking and §Ring Timeout.
  Answering a ring delivers a `VTCCallEndEvent` with `endReason`
  `"answered_elsewhere"` through `VtcBackend::deliver_event`, for the
  answering user's other devices. `expire_ring` applies the timeout when
  the deployment's timer fires, with an injected clock: a ringing call
  ends with `endReason: "missed"`, an active one continues, and a
  `"timeout"` `VTCCallEndEvent` goes to the still-pending targets.
  `RING_TIMEOUT` is the recommended 30 seconds.
- **jmap-vtc-server**: the `VTCParticipant/*` handlers of
  draft-atwood-jmap-vtc-00, registered by `register_vtc_handlers`.
  `/set` create joins a room, scheduled or ring call (lobby `"waiting"`
  for non-moderators when `lobbyEnabled`, `muteOnEntry` /
  `videoOffOnEntry` and the `participantsCan*` policy applied to
  `mediaState`, `maxConcurrentCalls` when the join activates a pending
  call) or, with `gatewayData`, dials out through a gateway whose
  `supportsDialOut` is `true` (moderators only, `forbidden` on an E2EE
  call). `/set` update answers and declines rings, enforces the call
  policy on a participant's own `mediaState`, leaves (server time),
  kicks (`kickedBy`), reconnects (refused while `kickedBy` is set), and
  holds lobby admission, role changes, `breakoutCallId` and
  `unmuteRequested` to moderators. A moderator's
  `gatewayData/pendingSignal` is delivered as an outbound
  `VTCGatewaySignal` and not stored. The call transitions follow: answer
  and join activate, all targets declining ends with `"declined"`, the
  last departure ends a ring or scheduled call, and the last moderator's
  departure promotes the earliest-joined authenticated participant.
  Destroy is `forbidden`. `/query` defaults to `joinedAt` ascending and
  refuses a filter scoped by neither `callId` nor `userId` with
  `unsupportedFilter`; `query::participant_matches` and
  `query::compare_participants` evaluate it. `MemoryBackend` implements
  the participant contracts and records delivered events
  (`events_for_test`).
- **jmap-vtc-types**: `VtcParticipant` implements `SetObject` and
  `QueryObject`.
- **jmap-vtc-client** (new crate): binds every
  draft-atwood-jmap-vtc-00 method — `/get`, `/changes`, `/set`, `/query`
  and `/queryChanges` for VTCCall, VTCParticipant, VTCRecording and
  VTCLivestream — on `SessionClient` via `JmapVtcExt::with_vtc_session`.
  `/get` decodes the `jmap-vtc-types` objects; `/query` and `/queryChanges`
  take the `jmap-vtc-types` filters and comparators; `/set` returns the
  untyped `SetResponse` because RFC 8620 §5.3 created entries carry only
  server-set properties. `SessionClient::for_account` targets a call's
  hosting account (§Hosting Account), the primary VTC account staying the
  default.
- **jmap-vtc-types**: `VtcParticipantFilter`,
  `VtcParticipantFilterCondition`, `VtcParticipantSortProperty` and
  `VtcParticipantComparator` for `VTCParticipant/query`.
- **jmap-chat-mimi-client** (new crate): the draft-atwood-jmap-chat-mimi-00
  client. `JmapMimiExt::with_mimi_session` binds a `jmap-base-client`
  session to its primary `urn:ietf:params:jmap:chat:mimi` account;
  `SessionClient` carries `room_create`, `room_update`, `room_send`,
  `room_unbind`, `group_info_fetch` and `mimi_capability`.
  `account_has_left` tells a client that a commit removed the last leaf
  whose credential identity is `userUri`, after which it SHOULD call
  `Room/unbind` (§Chat.roomId). `verify_group_info_response` parses a
  GroupInfoResponse (draft-ietf-mimi-protocol-06 §5.6) and verifies its
  `GroupInfoResponseTBS` signature with `hub_sender` over the received
  octets, and `group_info_names_hub_sender` checks that the GroupInfo's
  `external_senders` contains that `hub_sender` (§GroupInfo/fetch).
- **jmap-chat-mimi-server** (new crate): `KeyPackage/get` and
  `KeyPackage/set` of draft-atwood-jmap-chat-mimi-00 over a `MimiBackend`
  trait, with a `memory` reference backend. `check_key_package_create`
  parses and verifies the KeyPackage with openmls 0.9.1 and refuses it
  with `invalidProperties` naming `keyPackage` when it does not parse,
  does not verify, uses a cipher suite outside `cipherSuites`, has an
  expired lifetime, has a non-Basic credential, or has an identity other
  than `userUri`. It derives `lastResort` from the `app_data_dictionary`
  `last_resort_key_package` component and refuses a differing claim
  (`invalidProperties` naming `lastResort`). `MimiBackend::create_key_package`
  binds the signature key to the account, refuses a KeyPackageRef already
  stored or handed out, replaces the client's previous last-resort
  KeyPackage, and returns `overQuota` beyond `maxKeyPackages`. Every
  update is `forbidden`. KeyPackage hand-out (§Handing Out KeyPackages):
  `MimiBackend::claim_key_packages` returns one `ClientKeyMaterial` per
  client, atomically destroying the compatible non-last-resort record it
  hands out or else keeping the last-resort one, with a `clientUri` stable
  per signature key, and keeps every handed-out KeyPackageRef so it cannot
  be uploaded again. `KeyMaterialRequirements::fit` judges a stored
  KeyPackage against a request's acceptable cipher suites and
  `RequiredCapabilities` and treats an expired one as unavailable.
  `Room/create` (§Room/create): `check_room_create` applies the seven
  checks that need no server state (GroupInfo signature and cipher suite,
  tree hash, exactly one leaf with identity `userUri`, `external_senders`
  exactly `hubSender`, participant list exactly `userUri`, `room_uri` of
  `roomIdPrefix` and a 16-octet base32 room ID, 16-octet group ID,
  `franking_agent` of `hubSender` and a parsing `base_room_policy`) and
  `MimiBackend::create_room` applies the signature-key binding and room ID
  and group ID uniqueness atomically with creating the bound Chat; every
  refusal is `invalidArguments`. `may_update_room_chat`,
  `may_create_room_message` and `may_update_room_message` return the
  §Chat.roomId and §message/mls refusals for a `jmap-chat-server`
  `ChatBackend`'s per-Chat hooks. New dependency `openmls_memory_storage
  =0.6.0`. FanoutMessages (§application/mimi-fanout, §Chat.roomId):
  `parse_fanout_messages` splits a `/notify` body of concatenated
  FanoutMessages (draft-ietf-mimi-protocol-06 §5.5) into `Fanout`s with
  the `senderMsgId` ULID (`timestamp` plus the first 10 octets of the
  SHA-256 of the FanoutMessage octets), `sentAt`, the `message` octets and,
  for a Welcome, the KeyPackageRefs of its secrets, and refuses a body
  that does not parse with `FanoutParseError`.
  `MimiBackend::store_fanout` stores a `Fanout` atomically in every bound
  Chat of the room, or for a Welcome only in the accounts holding a named
  KeyPackageRef, rebinding or creating their Chat. It drops a repeated
  `senderMsgId` and a byte-identical `message`, and everything from a
  blocked hub. It attributes each Message to the account's hub
  ChatContact and creates that ChatContact when it is missing.
  `MimiBackend::unbind_room` unbinds a room Chat. A SemiPrivateMessage
  does not parse, because openmls 0.9.1 does not decode it. New
  dependency `sha2 0.10`.
  `KeyMaterial/fetch`, `Room/update`, `Room/send`, `Room/unbind` and
  `GroupInfo/fetch` (§KeyMaterial/fetch, §Rooms, §GroupInfo/fetch):
  `handle_key_material_fetch`, `handle_room_update`, `handle_room_send`,
  `handle_room_unbind` and `handle_group_info_fetch`, registered by
  `register_mimi_handlers`. Each decodes its MIMI request
  (draft-ietf-mimi-protocol-06 §5.2-§5.6) and refuses one that does not
  parse, or a room no Chat of the account is bound to
  (`MimiBackend::room_bound`), with `invalidArguments`. The request goes
  out through `MimiBackend::mimi_request` (`MimiEndpoint`, `HubReply`),
  whose `HubUnavailable` is `serverUnavailable`, and the hub's response is
  decoded into the method response (`serverFail` when it does not parse).
  `KeyMaterial/fetch` also checks `requestingUser` and `roomId`, and in a
  room this server hosts answers a local target
  (`MimiBackend::local_key_material_account`) from `claim_key_packages`.
  On success `Room/update` and `Room/send` build the FanoutMessage
  (`accepted_timestamp`, the handshake with its `moreProposals` or an
  empty `externalProposals`, or the application message with its `frank`)
  and store it with `store_fanout`, attributed to the hub. `Room/unbind`
  refuses a room no Chat of the account has, else calls `unbind_room`.
- **jmap-chat-mimi-types**: `convert::ulid`, the ULID of a 48-bit time and
  an 80-bit random component, which `sender_msg_id` now uses.
- **jmap-mail-types**, **jmap-calendars-types**, **jmap-contacts-types**,
  **jmap-filenode-types**, **jmap-tasks-types**: `may_set_metadata:
  Option<bool>` on `MailboxRights`, `CalendarRights`, `AddressBookRights`,
  `FilesRights` and `TaskRights`, the `maySetMetadata` right of
  draft-ietf-jmap-metadata-04 §4.1. It also applies to `shareWith` entries.
  `None` means the server omitted it. A `maySetMetadata` key now decodes
  into this field instead of `extra`.
- **jmap-metadata-server**: `is_well_formed_metadata_path`, which checks
  that a metadata filter path uses `~` only as the RFC 6901 escapes `~0`
  and `~1`. A `/query` handler rejects a path that fails it with
  `invalidArguments`, per draft-ietf-jmap-metadata-04 §3.6.
- **jmap-did-resolver**: `did:webvh` resolution. `Resolver` fetches the
  DID log from the DIF did:webvh v1.0 DID-to-HTTPS URL, and the
  `did-witness.json` beside it when an entry needs witnessing, and
  returns the verified log as `Resolved::Webvh`.
  `Resolver::resolve_webvh_version` returns the version an object was
  accepted under (draft-atwood-jmap-did-00 §Resolving `did:webvh`). A
  fetched log lacking the latest `versionId` seen before fails with
  `ResolveError::WebvhRollback`, and a log with an invalid entry is not
  cached. New `ResolveError` variants `Webvh`, `WebvhRollback` and
  `VersionNotFound`.
- **jmap-did-types**: `webvh_log_url`, the DIF did:webvh v1.0
  DID-to-HTTPS Transformation of a `did:webvh` DID.
- **jmap-did-types**: `verify_webvh_log`, a pure `did:webvh` verifier
  over supplied `did.jsonl` and `did-witness.json` bytes, following DIF
  did:webvh v1.0 as draft-atwood-jmap-did-00 §Resolving `did:webvh`
  defers to it. It returns a `WebvhLog` whose valid versions
  (`WebvhVersion`, with the `WebvhParameters` in effect) resolve by
  `versionId`, with the first invalid entry and its `WebvhError`
  reported separately. The crate now enables serde_json's
  `float_roundtrip` feature, which workspace feature unification turns
  on for every crate.
- **jmap-chat-tasks-server**: `may_advertise`, which returns `true` only
  when an account advertises both `urn:ietf:params:jmap:chat` and
  `urn:ietf:params:jmap:tasks` (draft-atwood-jmap-chat-tasks-00
  §Relationship to JMAP Tasks, §Capability), mirroring
  `jmap-chat-calendars-server`.
- **jmap-filenode-server**: `FileNodeBackend::cid_advertised` (default
  `false`). When it returns `true`, `FileNode/get` and each `FileNode/set`
  `created` entry emit `"sha256": null` for directories and for files
  without a stored digest, since with
  `urn:ietf:params:jmap:cid` advertised FileNode objects "MUST include"
  `sha256` (draft-atwood-jmap-cid-00 §FileNode Extension).
  `MemoryBackend` returns `true` once a digest is seeded for the account.
- **jmap-chat-filenode-server**: `space_files_rights` maps a member's Space
  permissions to the `FilesRights` of a node in the Space file tree, and
  `check_space_create`, `check_space_update` and `check_space_destroy`
  refuse with `forbidden` a FileNode create, update or destroy those
  rights do not grant, including any `shareWith` change and a root
  destroy without `manage_space` (draft-atwood-jmap-chat-filenode-00
  §Permission Mapping).
- **jmap-chat-federation-types**: `is_jmap_object_uri`, whether a
  MessageAction or Endpoint `uri` has the `jmap` scheme
  (draft-atwood-jmap-chat-federation-00 §User References in Peer Methods).
- **jmap-chat-types**: `ChatStreamEnable.id`, the optional client-chosen
  message `id` that a resulting `RequestError` echoes as its `requestId`
  (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable). A wire `id` used to
  land in `extra`; a non-string `id` (e.g. `"id": 7`) is now a parse
  error, since the draft types it as a String.
- **jmap-chat-types**: `Chat.active_call_account_id`, the server-set
  `activeCallAccountId` property (draft-atwood-jmap-chat-00 §Chat): the
  account hosting the VTCCall named by `activeCallId`, serialized as
  `null` when no call is active.
- **jmap-chat-vtc-server**: `sync_active_call_id` sets and clears
  `Chat.activeCallAccountId` together with `activeCallId` in one Chat
  update (draft-atwood-jmap-vtc-00 §Active call banner).
- **jmap-chat-server**: `verify_did_binding`, the DID-bound verification
  of a DID-form peer identity (draft-atwood-jmap-chat-did-00 §DID-Bound
  Verification). Run after the request signature has verified against
  the peer domain's `peerSigningKeys`, it takes the key that verified it
  (`Accepted::key`) and succeeds only if the resolved DID Document's
  `authentication` relationship (embedded, absolute or `#fragment`
  references; `publicKeyJwk` or Ed25519 `publicKeyMultibase`) holds an
  Ed25519 key equal to it octet for octet. It verifies no signature and
  records no nonce. No document (`DidAuthRejection::Unresolved`), a
  document whose `id` is not the DID (`DocumentIdMismatch`) or an absent
  key (`KeyNotBound`) is a rejection. The crate now depends on
  `jmap-peer-auth`. The caller passes the document as
  `DidDocument::Cached` or `DidDocument::Refreshed`; a key absent from a
  cached document gives `DidAuthRejection::RefreshRequired`, so the
  caller refreshes and checks once more (§Caching: "If the key that
  verified a request signature is not in the `authentication`
  verification relationship of the cached DID Document ..., the server
  MUST resolve the DID again").
- **jmap-chat-federation-server**: `AuthenticatedPeer::did_verified` and
  `AuthenticatedPeer::with_did_verified`. The `Peer/*` handlers now accept
  a peer identity that is a DID without a host (`did:key`, `did:plc`)
  when the backend sets the marker after `verify_did_binding` succeeds
  (draft-atwood-jmap-chat-federation-00 §Identity Binding rule 3:
  "A DID-form identity is accepted only after the DID-bound verification
  of {{JMAP-CHAT-DID}} succeeds"). Without the marker such an identity is
  `forbidden` as before; the marker does not affect any other identity.
- **jmap-chat-federation-server**: `assigneeIds` in the metadata of a
  `urn:ietf:params:jmap:chat:cap:task` MessageAction is mapped as a user reference
  (draft-atwood-jmap-chat-tasks-00 §Federation). `Peer/deliver` resolves
  each identity to a local id (owner, ChatContact or placeholder) and
  rejects a non-identity entry with `invalidArguments`;
  `SenderIdentities::actions` maps local ids to identity strings for
  sending.
- **jmap-chat-federation-server**: the optional presence methods of
  draft-atwood-jmap-chat-federation-00. `handle_peer_subscribe_presence`
  answers with the owner's presence (`"invisible"` as `"offline"`) and the
  TTL the backend grants; `handle_peer_unsubscribe_presence` succeeds with
  or without a subscription; `handle_peer_presence` updates the sender's
  ChatContact `presence`, `lastActiveAt`, `statusText` and `statusEmoji`
  (`null` clears, absent keeps), drops updates from a blocked contact
  silently, and fires the local presence event. They run over the new
  `FederationPresenceBackend` trait, which only deployments offering the
  methods implement.
- **jmap-peer-auth**: `fetch_address_allowed`, the §Blob Fetch and SSRF
  address rule: refuses loopback, unspecified, link-local and private-network
  (RFC 1918, RFC 4193) addresses, including IPv4-mapped forms.
- **jmap-chat-federation-types**: `owner_endpoints` reads a probed Session's
  `ownerEndpoints` as the `endpoints` of the matching ChatContact
  (draft-atwood-jmap-chat-00 §ChatContact).
- **jmap-chat-federation-server**: the sending side of
  draft-atwood-jmap-chat-federation-00, for a backend's
  `on_outbound_event`. `outbound_reference` translates a stored `replyTo` or
  `threadRootId` to the referenced Message's `senderMsgId` and omits it when
  none is found (§Outbound Message Preparation); `other_participants` lists
  the `Peer/retract` and `Peer/deliver` recipients (§Peer/retract "Sender
  Behavior"); `peer_group_update_args` builds `Peer/groupUpdate` from an
  `OutboundEvent::GroupUpdate`, mapping member ids to identity strings via
  `SenderIdentities` (`OutboundError` for an id with no identity);
  `aggregate_delivery_state` gives the best state any recipient reached,
  `"failed"` only when every recipient failed (§Delivery State
  Transitions); `is_transient` classifies a `DeliveryFailure` for retry
  (§Retry and Backoff, §Error Handling).
- **jmap-chat-federation-server**: new crate for the server rules of
  draft-atwood-jmap-chat-federation-00. `FederationCaller` is implemented
  by a dispatcher `CallerCtx` to say whether the caller is a peer on a
  `role: "peer"` account; `check_method_access` returns `forbidden` for a
  peer calling a method outside `PEER_METHODS` (§Account-Level Capability)
  and for any other caller calling a `Peer/*` method (§Server-to-Server
  Methods).
- **jmap-chat-federation-server**: `handle_peer_deliver`, the `Peer/deliver`
  handler of draft-atwood-jmap-chat-federation-00: §New Message Validation
  steps 1 to 14, §Edit and Reaction Validation, identity mapping with
  placeholder ChatContacts created only for accepted requests (§User
  References in Peer Methods), and the direct-chat race of §Direct Chat
  Simultaneous Initiation (`min(X, Y)`, reported as `canonicalChatId`). It
  runs over the new `FederationBackend` trait, a `ChatBackend` that also
  supplies the `AuthenticatedPeer`, the `@everyone` permission floor,
  attachment fetching (`FetchedBlob`; the SSRF policy stays with the
  deployment), fresh Message ids for silent drops, and Chats under a
  peer-chosen id (`create_chat`).
- **jmap-chat-federation-server**: `handle_peer_receipt`,
  `handle_peer_typing`, `handle_peer_retract` and `handle_peer_group_update`,
  the receivers of draft-atwood-jmap-chat-federation-00 §Peer/receipt,
  §Peer/typing, §Peer/retract and §Peer/groupUpdate. `Peer/receipt` stores
  the receipt in `deliveryReceipts` under the reader's ChatContact id for
  every Chat kind, raises `deliveryState`, and records only `deliveredAt`
  when the owner's effective `receiptSharing` is `false`. `Peer/typing`
  checks membership, `blocked` and `receiveTypingIndicators`, then calls the
  new required `FederationBackend::forward_typing`. `Peer/retract`
  tombstones the sender's own Message. `Peer/groupUpdate` creates group
  Chats with placeholder ChatContacts for unknown members and applies member,
  role and metadata changes, `leave` and `close` with the draft's admin and
  `closedAt` checks.
- **jmap-chat-federation-types**: `session` module for
  draft-atwood-jmap-chat-federation-00 §Session Object Extensions for
  Federation. `FederationSession` holds `ownerUserId`, `peerSigningKeys`
  (raw JWK entries) and the optional `ownerDirectAddress`, read from a
  Session's extension map by `from_session_extra`.
  `anonymous_well_known_session` builds the Session answered to an unsigned
  GET of `/.well-known/jmap` (§Key Publication): empty `accounts` and
  `primaryAccounts`, empty `username`, the federation fields. It returns
  `AnonymousSessionError` for an empty key list or a key carrying the
  private member `d`.
- **jmap-chat-federation-types**: `peer` module with the request and
  response arguments of the draft-atwood-jmap-chat-federation-00 `Peer/*`
  methods: `PeerDeliverArgs` (`PeerMessage`, `PeerEdit`,
  `PeerReactionUpdate`, `ReactionAction`, `PeerAttachment` with
  `fetchUrl`, `PeerMention`), `PeerDeliverResponse`, `PeerReceiptArgs`,
  `PeerTypingArgs`, `PeerRetractArgs`, `PeerRetractResponse`,
  `PeerGroupUpdateArgs` (`GroupUpdateAction`, `PeerChatMember`,
  `PeerRoleUpdate`), `PeerSubscribePresenceArgs`,
  `PeerSubscribePresenceResponse`, `PeerUnsubscribePresenceArgs`,
  `PeerPresenceArgs` and the `accountId`-only `PeerAck`. User-reference
  fields are identity strings (§User References in Peer Methods).
- **jmap-chat-mimi-types**: `convert` module with the converting-party
  rules of draft-atwood-jmap-chat-mimi-00: `sender_msg_id` (ULID from the
  hub acceptance time and octets 1-10 of the MIMI message ID), `RefKey`
  (the 80-bit reference and alias key, also readable from a stored
  `senderMsgId`), `check_ref_key` / `RefKeyCollision` (§Security
  Considerations collision rule), `may_record_alias`, `topic_id`,
  `is_media_reaction` and `never_sets_read`.
- **jmap-chat-client**: RSVP helpers for draft-atwood-jmap-chat-calendars-00
  calendar-event MessageActions: `RsvpStatus`, `rsvp_patch` building the
  `participants/{key}/participationStatus` patch for `CalendarEvent/set`, and
  `own_participant_key` matching the user's ParticipantIdentity
  `calendarAddress` values after RFC 3986 §6.2.2 normalisation.
- **jmap-chat-types**: draft-atwood-jmap-chat-tasks-00 types:
  `JMAP_CHAT_TASKS_URI`, `ChatTasksCapability` (`mayBindTaskList`,
  `mayBackReferenceChat`), `Space::task_list_id` reading the `taskListId`
  extension (`SPACE_TASK_LIST_ID_PROPERTY`) from `Space::extra`,
  `task_chat_id` reading the Task `chatId` extension
  (`TASK_CHAT_ID_PROPERTY`) from a Task's extension map, the
  `urn:ietf:params:jmap:chat:cap:task` MessageAction metadata shape `TaskMetadata`, and
  `JMAP_URI_TASK_TYPE`, the type name of the `jmap:task:<accountId>:<taskId>`
  object URI (parse it with `jmap_types::JmapUri`).
- **jmap-chat-types**: draft-atwood-jmap-chat-calendars-00 types:
  `JMAP_CHAT_CALENDARS_URI`, `ChatCalendarsCapability` (`mayBindCalendar`,
  `supportsIcsParsing`, `supportsAvailabilityLookup`), `Space::calendar_id`
  reading the `calendarId` extension (`SPACE_CALENDAR_ID_PROPERTY`) from
  `Space::extra`, and the MessageAction metadata shapes
  `CalendarEventMetadata`, `AvailabilityMetadata` and `LookupWindow`.
- **jmap-chat-tasks-server**: new crate, the Chat binding for
  `jmap-tasks-server` (draft-atwood-jmap-chat-tasks-00).
  `space_extension_properties` registers `taskListId` only with
  `mayBindTaskList: true`; `check_task_list_id` accepts `null` (unbind) or
  a TaskList on the Space's own account, else `invalidProperties` naming
  `taskListId`; `create_space_task_list` auto-binds a new Space. The bound
  TaskList is never destroyed with its Space. Destroy cascades:
  `clear_space_task_list_id` (from `TasksBackend::destroy_object::<TaskList>`)
  removes `taskListId` from the Spaces bound to a destroyed TaskList (§taskListId),
  and `clear_task_chat_ids` (from `ChatBackend::on_chat_destroyed`) removes
  `chatId` from the Tasks that reference a destroyed Chat (§Lifecycle). A Task
  destroy leaves its Chat in place. The crate now depends on `jmap-chat-server`.
  Task `chatId` rules: `check_task_chat_id` accepts `null` or any valid `Id`
  without looking the Chat up (§chatId "MUST NOT validate `chatId` existence
  at write time") and refuses every value as `invalidProperties` naming
  `chatId` under `mayBackReferenceChat: false`; `redact_task_chat_ids`
  removes `chatId` from Tasks returned to a reader whose
  `ChatBackend::may_view_chat` is `false` (§Chat back-reference leakage MAY),
  hiding nothing by default.
- **jmap-chat-calendars-server**: new crate, the Chat binding for
  `jmap-calendars-server` (draft-atwood-jmap-chat-calendars-00).
  `may_advertise` allows the capability only alongside
  `urn:ietf:params:jmap:chat` and `urn:ietf:params:jmap:calendars`;
  `space_extension_properties` registers `calendarId` only with
  `mayBindCalendar: true`; `check_calendar_id` accepts `null` (unbind) or a
  Calendar on the Space's own account, else `invalidProperties` naming
  `calendarId`; `create_space_calendar` auto-binds a new Space. The bound
  Calendar is never destroyed with its Space.
- **jmap-chat-filenode-server**: new crate, the Chat binding for
  `jmap-filenode-server`. `check_root_create`, called before a FileNode
  insert, refuses a Space file tree root without `spaceId`
  (`invalidProperties`), for a Space that does not exist or that the caller
  is not a member of (`forbidden`), for a Space that already has a root
  (`alreadyExists` with `existingId`), or without `manage_space`
  (`forbidden`) (draft-atwood-jmap-chat-filenode-00 §Creating the File Tree
  Root, §Permission Mapping).
- **jmap-chat-filenode-server**: `check_space_quota` refuses a FileNode
  create that would take a Space file tree's total `size` over
  `maxSpaceStorageBytes` with `overQuota`, and `create_space_file_tree`
  inserts a Space's file tree root for a server advertising
  `autoCreateFileTree: true` (draft-atwood-jmap-chat-filenode-00
  §Account-Level Capability Object).
- **jmap-chat-server**: `ChatBackend::filenode_attachment`, consulted by
  `Message/set` create for each Attachment carrying `filenodeId`, returns
  the Attachment with its absent `blobId`, `filename`, `contentType`,
  `size` and `sha256` filled in, or a SetError. The default returns
  `None`, leaving the five fields required as before. The
  `maxAttachmentsPerMessage` check now runs before any entry is resolved
  (draft-atwood-jmap-chat-filenode-00 §filenodeId).
- **jmap-chat-filenode-server**: `resolve_filenode_attachment`, called from
  `ChatBackend::filenode_attachment`, requires a `filenodeId` to name a file
  in the channel's Space file tree (`invalidProperties` naming
  `filenodeId`) that the sender can read under §Permission Mapping
  (`forbidden`), rejects a conflicting `blobId`, `size` or `sha256`
  (`invalidProperties` naming `attachments`), and fills the absent fields
  from the FileNode (draft-atwood-jmap-chat-filenode-00 §filenodeId,
  §Attachment filenodeId Validation).
- **jmap-chat-filenode-server**: `destroy_space_file_tree`, called from
  `ChatBackend::on_space_destroyed`, destroys the Space's file tree root and
  every FileNode within it, descendants first, through
  `FileNodeBackend::destroy_object`, so `FileNode/changes` reports them.
  No grace period is offered (draft-atwood-jmap-chat-filenode-00 §Space
  Deletion).
- **jmap-filenode-types**, **jmap-filenode-server**:
  `FileNodeFilterCondition.space_id` (`spaceId` on the wire), matched by the
  in-memory backend against the node's `spaceId` property, so
  `FileNode/query` can locate one Space's file tree root among several
  (draft-atwood-jmap-chat-filenode-00 §The `spaceId` Extension Property).
- **jmap-filenode-types**, **jmap-filenode-server**: typed
  `FileNode.space_id` (`spaceId` on the wire, omitted when `None`).
  `FileNode/set` and `FileNode/copy` reject a create that carries `spaceId`
  on a node whose `role` is not `chatspace`, and an update
  that changes `spaceId` or moves a node that has one off that role, with
  `invalidProperties` (draft-atwood-jmap-chat-filenode-00 §The `spaceId`
  Extension Property: "immutable ... Absent on all FileNode objects that are
  not Space file tree roots").
- **jmap-chat-scene-server**: new crate, the Chat binding for
  `jmap-scene-server`. `should_deliver_scene` decides delivery of a Scene
  ephemeral event over the union of the `ChatStreamEnable` `"scene"` path
  and the `SceneStreamEnable` subscription, once per event and connection
  (draft-atwood-jmap-scene-wss-00 §Interoperability with JMAP Chat
  WebSocket).
- **jmap-scene-vtc-server**: new crate, the VTC binding for
  `jmap-scene-server` (draft-atwood-jmap-scene-00 with
  draft-atwood-jmap-vtc-00). `clear_active_call_id`, called from
  `VtcBackend::update_object`, clears `SceneRegion.activeCallId` to `null`
  and advances the SceneRegion state when the referenced VTCCall ends,
  whether through `VTCCall/set` or `jmap_vtc_server::end_call`.
- **jmap-chat-vtc-server**: new crate, the Chat binding for
  `jmap-vtc-server` (draft-atwood-jmap-vtc-00 with draft-atwood-jmap-chat-00).
  `check_call_create` applies the `chatId` access and membership rules and
  the `start_call` permission to a `VTCCall/set` create;
  `sync_active_call_id` sets `Chat.activeCallId` when a bound call becomes
  `"active"` and clears it when the call ends; `clear_space_references` and
  `clear_channel_references` clear `spaceId` / `channelId` on VTCCalls when
  the Space or channel is destroyed; `contact_identity` maps a ChatContact
  id, or the accountId, to the identity string VTC uses as `userId`;
  `is_blocked_by_contact` answers
  `VtcBackend::is_user_blocked` from `ChatContact.blocked`.
- **jmap-chat-vtc-server**: `should_deliver_vtc` decides delivery of a VTC
  ephemeral event over the union of the `ChatStreamEnable` `"vtc"` path and
  the `VTCStreamEnable` subscription, once per event and connection
  (draft-atwood-jmap-vtc-wss-00 §Interoperability with JMAP Chat
  WebSocket).
- **jmap-scene-client**: `ws` module for draft-atwood-jmap-scene-wss-00.
  `SceneWsExt` on `jmap_base_client::WsSession` adds `next_scene_frame`,
  `send_scene_stream_enable`, `send_scene_stream_disable` and
  `send_scene_interaction`. `parse_scene_ws_frame` types the
  `SceneAvatarEvent`, `SceneObjectEvent` and `SceneInteractionEvent`
  frames and passes every other frame through unchanged as
  `SceneWsFrame::Other`, including `RequestError`, interleaved VTC/Chat
  events and Scene events that fail to parse.
- **jmap-scene-types**: `SceneInteractionEvent::new` from the required
  fields, so a client can build the frame it submits.
- **jmap-scene-server**: `should_deliver`, the delivery-time check of
  draft-atwood-jmap-scene-wss-00 for one Scene event on one connection —
  `eventTypes` and `regionIds` filters, region access and an active avatar
  of the recipient in the event's region, the `SceneObject/get`
  visibility contract for object and interaction events, and
  blocked-sender suppression (`userId`, or a non-null `updatedBy`) only
  when the account has `urn:ietf:params:jmap:chat`. `validate_interaction`
  checks a client-sent `SceneInteractionEvent` before relay: `userId` is
  the caller, an active avatar in `regionId`, and an `interactable`
  object in that region.
- **jmap-scene-server**: `ephemeral` module for draft-atwood-jmap-scene-wss-00
  §SceneStreamEnable. `SceneEphemeralSubscription::replace_with` replaces
  the connection's subscription in its entirety, drops unrecognized
  `eventTypes` next to recognized ones, and returns `invalidArguments`
  (leaving the subscription unchanged) for an empty or all-unrecognized
  `eventTypes`. `RegionScope::All` (`regionIds: null`, dynamic) or
  `RegionScope::Only` (static, entries kept even without an active avatar)
  and `SceneStreamFilter::includes` give the filters.
- **jmap-scene-types**: draft-atwood-jmap-scene-wss-00 wire types.
  `JMAP_SCENE_WEBSOCKET_URI` (`urn:ietf:params:jmap:scene:websocket`);
  the `event` module with `SceneAvatarEvent`, `SceneObjectEvent`,
  `SceneInteractionEvent`, `AvatarEventKind`, `ObjectEventKind` and the
  `@type`-dispatched `SceneEvent`; the `stream` module with
  `SceneStreamEnable`, `SceneStreamDisable`, the `@type`-dispatched
  `SceneStreamMessage` and the `SCENE_EVENT_TYPES` vocabulary. Unknown
  `@type` values decode to an `Unknown` variant and re-serialize verbatim.
- **jmap-scene-client** (new crate): `AssetFetcher` for
  draft-atwood-jmap-scene-00 §assetUri Is Untrusted. `fetch` refuses an
  `assetUri` whose scheme is not `https:` before connecting
  (`AssetFetchError::NotHttps`), does not follow redirects to non-HTTPS
  endpoints, stops at a caller-supplied byte limit and checks the body
  against an expected `sha256` (`AssetFetchError::DigestMismatch`).
- **jmap-scene-server**: `push` module for §Push Notifications.
  `SCENE_DATA_TYPES` lists the four state-change types; `SceneEvent`,
  `Urgency` and `push_urgency` give the §Push Urgency recommendation for a
  `StateChange` (the highest of its events), with an avatar enter or leave
  already announced by a bound Chat's push lowered to `low` (§Interaction
  with Chat Push).
- **jmap-scene-types**: `set_error` module for draft-atwood-jmap-scene-00
  §Content Moderation for User-Created Objects: the `CONTENT_POLICY`
  (`contentPolicy`) SetError type, the `RETRY_AFTER` property name and the
  `retry_after` accessor, which reads the `rateLimit` retry delay in
  seconds from `SetError.extra` (`RetryAfterError::Malformed` when it is
  not a non-negative integer). `jmap-types` is unchanged.
- **jmap-scene-server**: §Avatar Identity and Impersonation reserved
  names. `SceneBackend` gains the fail-open `reserved_display_names`
  (default: none); `SceneAvatar/set` refuses a create or update whose
  client-supplied `displayName` is on the list with `forbidden`.
  `MemoryBackend` gains `reserve_display_name`.

- **jmap-scene-server**: draft-atwood-jmap-scene-00 §Visibility Contract,
  §Region Access and §Blocked-Contact Spatial Presence. `SceneBackend`
  gains the fail-open `may_access_region`, `is_object_visible` and
  `is_user_blocked` (the ChatContact lookup belongs to the Chat binding
  crate). `SceneRegion/get`, `SceneObject/get` and `SceneAvatar/get`
  report a record the caller may not see in `notFound`, in request order
  like a missing id; `SceneObject/set` refuses an update or destroy of a
  hidden object with `notFound`, never `forbidden`, as do
  `SceneObject/set` create, `SceneRegion/set` update and destroy and
  `SceneAvatar/set` update in a region without access. `query_objects` and
  `query_changes` must omit the same records; `MemoryBackend` does, and
  gains `hide_object` and `block`. `activeAvatarCount` stays the true
  count.

- **jmap-scene-server**: `SceneAvatar/get`, `/changes`, `/set`, `/query`
  and `/queryChanges` handlers, registered by `register_scene_handlers`:
  create enters a region (`notFound` for a missing region, `forbidden`
  from `may_enter_region` or for another user's `userId`, spawn
  `position`/`orientation` unless supplied); `alreadyExists` with
  `existingId` for a second active avatar in the region, reconnect to a
  departed record, auto-eject from another region, `overQuota` at
  `maxAvatarsPerRegion` and `activeAvatarCount` upkeep, all atomic in the
  backend; own-avatar updates, leave by `leftAt` (`invalidProperties`
  on a second leave), eject through `may_eject_avatar`, `forbidden`
  destroy, and the `joinedAt`-ascending default sort with the §Filter
  Validation `invalidArguments` checks. `SceneBackend` gains the required
  `caller_user_id`, the fail-open `may_enter_region` and
  `may_eject_avatar`, and `avatar_display_name` (default: the userId);
  `query` gains `avatar_matches` and `compare_avatars`.

- **jmap-scene-types**: new crate for draft-atwood-jmap-scene-00 —
  `JMAP_SCENE_URI` (`urn:ietf:params:jmap:scene`) and `SceneCapability`,
  the `Position`, `Orientation` and `Scale` coordinate formats with
  `IDENTITY_ORIENTATION` and `IDENTITY_SCALE`, `SceneBounds`, `GeoAnchor`,
  the `SceneAsset`, `SceneRegion` (`ViewHint`, `AccessPolicy`),
  `SceneObject` (`PhysicsMode`) and `SceneAvatar` data types, and the
  `query` filters and comparators of all four, including the
  `WithinRadius` and `WithinBounds` spatial filters.
- **jmap-scene-types**: `JmapObject`, `GetObject`, `SetObject` and
  `QueryObject` impls for `SceneAsset`, `SceneRegion`, `SceneObject` and
  `SceneAvatar`.
- **jmap-scene-server**: `SceneObject/get`, `/changes`, `/set`, `/query`
  and `/queryChanges` handlers, registered by `register_scene_handlers`:
  `forbidden` without `mayCreateObject`, required `regionId` and finite
  `position`, `orientation` within 1e-3 of unit length and stored
  normalized, `physicsMode` and `visualRef`/`visualType` checks,
  `notFound` for a missing region, blob or parent, a region-scoped,
  acyclic scene graph at most 16 levels deep, immutable `regionId`,
  recursive destroy, the `createdAt`-ascending default sort and the
  §Filter Validation `invalidArguments` checks. `SceneBackend` gains the
  fail-open `may_modify_object`; `create_object` sets a SceneObject's
  `ownerId` and applies `maxObjectsPerRegion`, `destroy_object` removes
  its descendants. `query` gains `object_matches`, `compare_objects` and
  `region_local_position`, which maps a child's parent-relative position
  into region-local coordinates for `withinRadius` and `withinBounds`.
- **jmap-scene-server**: `SceneRegion/get`, `/changes`, `/set`, `/query`
  and `/queryChanges` handlers, registered by `register_scene_handlers`:
  required `name` and `bounds` with `min <= max`, the `accessPolicy`,
  `spawnPosition` and unit-quaternion `spawnOrientation` checks,
  `forbidden` without `mayCreateRegion`, `overQuota` at
  `maxRegionsPerAccount`, Chat and VTC binding checks, owner-or-admin
  update, owner-only destroy that removes the region's SceneObjects and
  SceneAvatars, the `createdAt`-descending default sort and
  `invalidArguments` for an unknown `accessPolicy` filter value.
  `SceneBackend` gains the required `has_capability` and the fail-open
  `region_binding` (returning the new `BindingTarget`),
  `may_update_region` and `may_destroy_region`; `query` gains
  `region_matches` and `compare_regions`.
- **jmap-scene-server**: new crate for draft-atwood-jmap-scene-00 —
  `SceneAsset/get`, `/changes`, `/set`, `/query` and `/queryChanges`
  handlers with `register_scene_handlers`, the `SceneBackend` trait
  (`scene_capability`, `blob_size`, `create_object`, `update_object`,
  `destroy_object`, fail-open `may_modify_asset`), `GLTF_BINARY` accepted
  as the mandatory-to-implement format, `notFound` for a missing blob,
  `overQuota` above `maxAssetSizeBytes`, the `query::asset_matches` and
  `query::compare_assets` functions, and a `memory` reference backend.
- **jmap-vtc-types**: draft-atwood-jmap-vtc-wss-00 stream control
  frames — `JMAP_VTC_WEBSOCKET_URI`
  (`urn:ietf:params:jmap:vtc:websocket`), `VtcStreamEnable` (`callIds`
  and `eventTypes`, each defaulting to `null`), `VtcStreamDisable`, the
  `@type`-dispatched `VtcStreamMessage` with a verbatim `Unknown` arm, and
  the `VTC_EVENT_TYPES` vocabulary of recognized `eventTypes` values.
- **jmap-vtc-server**: `ephemeral` module with
  `VtcEphemeralSubscription`, the per-connection `VTCStreamEnable` state of
  draft-atwood-jmap-vtc-wss-00 — whole-frame replace semantics, `callIds`
  as a `CallScope`, unrecognized `eventTypes` dropped, and
  `invalidArguments` without a state change when no recognized
  `eventTypes` value remains.
- **jmap-vtc-server**: `should_deliver`, the delivery-time check of
  draft-atwood-jmap-vtc-wss-00 for one VTC event on one connection —
  `eventTypes` filter, per-user ring events outside `callIds`, per-call
  events only for listed calls the recipient is a current participant in,
  blocked-sender suppression (never for `VTCRecordingStateEvent`),
  `VTCGatewaySignal` to moderators only, and no `VTCActiveSpeakerEvent`
  on E2EE calls. `VtcBackend::is_user_blocked`, a fail-open hook
  defaulting to `Ok(false)`.
- **jmap-vtc-types**: `VtcEvent::type_name`, the event's `@type`.
- **jmap-vtc-server**: new crate with the draft-atwood-jmap-vtc-00
  `VTCCall/get`, `/changes`, `/set`, `/query` and `/queryChanges`
  handlers, the `VtcBackend` trait and a `memory` reference backend.
  `VTCCall/set` create applies the ring, room and scheduled rules and the
  `mayCreateCall` / `supports*Calls` / `supportedMediaTypes` gates; update
  enforces the §State Machine, sets `endedAt` and `endReason` when a call
  ends, and gates ending, `lobbyEnabled`, `policy`, `e2eeEnabled` and
  breakout rooms on `VtcBackend::is_moderator` (fail-open default).
  Destroy takes the same permission as ending the call, and the memory
  backend drops a destroyed breakout room from its parent's
  `breakoutRoomIds`.
  `call_already_active_on_chat` is the L573 one-active-call-per-Chat
  SetError; `query::call_matches` and `query::compare_calls` evaluate the
  `/query` filter and sort. Depends on RFC 8620 only.
- **jmap-vtc-server**: `VTCRecording/*` and `VTCLivestream/*` handlers
  (draft-atwood-jmap-vtc-00 §VTCRecording Methods, §VTCLivestream
  Methods). Every `/set` operation is moderator-only; a create is
  `forbidden` without `supportsRecording` / `supportsLivestream` or on an
  `e2eeEnabled` call (no key-holding bridge is implemented), and
  `invalidProperties` naming `callId` unless the call is `"active"`.
  Recordings follow `recording` ⇄ `paused` → `stopped` (setting
  `stoppedAt`), each change followed by a `VTCRecordingStateEvent`;
  livestreams can only be stopped, and reconfigured only while stopped
  or failed. Destroying a `"recording"` or `"paused"` recording, or a
  `"starting"` or `"live"` livestream, stops it first.
  `VTCLivestream/get` returns `streamKey` and `streamUri` as `""` to
  non-moderators. `VtcBackend` gains the required
  `create_object` and `deliver_recording_state_event`;
  `register_vtc_handlers` wires the ten new methods; `query` gains
  `recording_matches`, `livestream_matches`, `compare_recordings` and
  `compare_livestreams`.
- **jmap-vtc-server**: call-end cascades. Ending a call through
  `VTCCall/set` first ends every breakout room beneath it, deepest first
  (jmap-vtc-implementer-guide §Breakout room cleanup), then stops each
  ended call's `"recording"` and `"paused"` VTCRecordings, setting
  `stoppedAt` and delivering a `VTCRecordingStateEvent`
  (jmap-vtc-recording-guide §Auto-stop on call end). The new `end_call`
  runs the same cascade for a server-initiated end, such as an inactivity
  timeout, with the `endReason` the deployment passes.
- **jmap-vtc-types**: `VtcRecordingFilter`, `VtcLivestreamFilter`,
  `VtcMediaComparator` and `VtcMediaSortProperty` (§VTCRecording/query,
  §VTCLivestream/query), and `JmapObject` impls for `VtcRecording`
  (`"VTCRecording"`) and `VtcLivestream` (`"VTCLivestream"`).
- **jmap-vtc-types**: `query` module with `VtcCallFilter`,
  `VtcCallFilterCondition`, `VtcCallComparator` and `VtcCallSortProperty`
  (§VTCCall/query), and `JmapObject` impls for `VtcCall` (`"VTCCall"`) and
  `VtcParticipant` (`"VTCParticipant"`).
- **jmap-vtc-types**: new crate for draft-atwood-jmap-vtc-00.
  `JMAP_VTC_URI` and `VtcCapability` with `VtcGateway` and
  `VtcDialInEntry` (§Capability, §VTCGateway); the embedded
  `VtcMediaState` and `VtcCallPolicy`; the `VtcCall`, `VtcParticipant`,
  `VtcRecording` and `VtcLivestream` data types with their state, role and
  reason enums; `VtcCallPush` in its full and content-free forms; and
  `VtcEvent`, which dispatches on `@type` over `VtcRingEvent`,
  `VtcCallEndEvent`, `VtcGatewaySignal`, `VtcParticipantEvent`,
  `VtcMediaStateEvent`, `VtcActiveSpeakerEvent`, `VtcUnmuteRequestEvent`
  and `VtcRecordingStateEvent`. Checked against the draft's JSON examples.
- **jmap-peer-auth-io**: new support crate for the I/O half of
  draft-atwood-jmap-chat-federation-00 §Peer Authentication Model.
  `PeerKeys` fetches `https://<peer-domain>/.well-known/jmap` unsigned and
  queries `_jmap-peer.<peer-domain>` TXT, applies `usable_keys`, caches for
  the RFC 9111 freshness lifetime (at most one hour and the DNS TTL), and
  refetches on an unknown `kid` or an expired entry at most once per 60
  seconds per peer domain, whatever the freshness lifetime.
  `HttpsFetcher` is HTTPS-only with `https`-only redirects and a caller
  address policy for §Blob Fetch and SSRF; `HickoryLookup` validates DNSSEC
  in-process; `MemoryReplayStore` implements check 5; `authenticate` runs
  the whole receive path. Per §Replay, `ReplayStore::remembers_since`
  reports when a store's memory starts (`MemoryReplayStore::new` at
  startup, or `started_at` with an injected clock), and `authenticate`
  rejects a request whose `created` precedes it with
  `Rejection::PrecedesRestart`.
- **jmap-peer-auth**: new I/O-free crate for draft-atwood-jmap-chat-federation-00
  §Peer Authentication Model. `sign` produces the RFC 9421
  `Signature-Input` / `Signature` (Ed25519, `tag="jmap-chat-peer"`) and RFC
  9530 `Content-Digest` values; `SignedRequest::parse` and `verify` run
  verification checks 1 to 6 with the clock, the peer's usable keys and a
  replay lookup passed in, returning the authenticated peer domain and the
  nonce record to keep and the signature's `created`. `PeerSigningKey` reads and writes `peerSigningKeys`
  JWKs (RFC 7638 `kid`), `usable_keys` applies the DNSSEC precedence rule,
  and `bind_identity` applies §Identity Binding. Checked against RFC 9421
  Appendix B.2.6, RFC 8037 Appendix A.3 and RFC 9530 Appendix D.
- **jmap-web-push**: new support crate for Web Push delivery of RFC 8620
  §7.2 pushes. `encrypt` (RFC 8291 `aes128gcm`, checked against the
  RFC 8291 Appendix A example), `Vapid` (RFC 8292 `Authorization: vapid`
  header), and `Sender::send`, which POSTs a JSON payload with the RFC
  8030 `TTL` / `Urgency` / `Topic` headers, retries `429` / `5xx` /
  transport errors with backoff, and returns `Outcome::Gone` on `404` or
  `410` so the caller can destroy the subscription. No `*-types` or
  `*-server` crate depends on it.
- **jmap-did-types**: `PlcState::document()` presents a verified `did:plc`
  state as its DID document (did:plc v0.3.0 §Presentation as DID Document),
  matching the document plc.directory serves at `/<did>`.
- **jmap-chat-federation-types**: new crate for
  draft-atwood-jmap-chat-federation-00. `contact_server_url` and
  `contact_direct_address` read the federation `ChatContact.serverUrl` and
  `ChatContact.directAddress` fields (§ChatContact Type Extensions for
  Federation) from `ChatContact.extra`; `jmap-chat-types` is unchanged.
- **jmap-base-client**: `connect_ws_allow_plaintext(url, auth_header,
  max_message_bytes)`, the explicit opt-in that also accepts plaintext
  `ws://` for local test rigs and examples.
- **jmap-chat-server**: `fit_chat_message_push(push, max_bytes)` fits a
  `ChatMessagePush` to a platform payload limit per
  draft-atwood-jmap-chat-push-00 §Delivery. It returns the payloads to
  deliver: an oversized batch is split into one payload per message, an
  entry that still does not fit loses only its `bodySnippet`, and a `None`
  element marks a message that gets a `StateChange` in its place.

- **jmap-chat-types**: `ChatContact.did` (`Option<jmap_did_types::Did>`),
  the optional server-set DID bound to a ChatContact for the pseudonymous
  case (draft-atwood-jmap-chat-did-00 §Optional `did` field), and
  `ChatContact::has_consistent_did`, which is `false` when `identity`
  parses as a DID different from `did`, for backends that set `did`.
  jmap-chat-types now depends on jmap-did-types.
- **jmap-chat-server**: `ChatContact/set` create accepts only `identity`;
  a create carrying `did` (any value, `null` included) is rejected with
  `invalidProperties` naming `did`, because `did` is server-set
  (draft-atwood-jmap-chat-did-00 §Optional `did` field).

- **jmap-chat-types**: `JMAP_CHAT_DID_URI` (`urn:ietf:params:jmap:chat:did`
  marker capability, value `{}`), `MANDATORY_DID_METHODS` (`web`, `key`,
  `plc`) and `may_advertise_chat_did`, which is `true` only when
  `urn:ietf:params:jmap:did` is advertised and its `supportedMethods` lists
  all three mandatory methods (draft-atwood-jmap-chat-did-00 §Relationship
  to JMAP DID, §Capability, §Mandatory DID Methods).
- **jmap-did-resolver** (new support crate): `Resolver` resolves
  `did:key` (derived locally), `did:web` (HTTPS GET of the did:web
  document URL, `id` must equal the DID) and `did:plc` (PLC directory
  `/<did>/log/audit`, verified with `verify_plc_audit_log`), caching
  fetched results for a fixed TTL up to a capped entry count.
  `Resolver::refresh` drops the cached entry and refetches, for the
  draft-atwood-jmap-chat-did-00 §Caching rule that a signing key absent
  from the cached document forces a resolution that bypasses the cache;
  a failed refresh leaves nothing cached (§Resolution failure handling).
  It refetches one DID at most once per `DEFAULT_REFRESH_INTERVAL`
  (60 s, §Caching "SHOULD NOT resolve one DID again for this reason more
  often than once every 60 seconds"; `Resolver::with_refresh_interval`
  changes it) and otherwise returns what `Resolver::resolve` does. The
  last refresh time is kept apart from the cache entry. Network access goes through
  the `Fetch` trait; `HttpFetcher` is the HTTPS-only reqwest
  implementation. No `*-types` or `*-server` crate depends on it.
- **jmap-base-client**: `Session::did_capability` and
  `AccountInfo::did_capability`, typed accessors returning
  `jmap_did_types::DidCapability` / `DidAccountCapability` for
  `urn:ietf:params:jmap:did` (draft-atwood-jmap-did-00 §Capability).
  `jmap-base-client` now depends on `jmap-did-types`.
- **jmap-server**: `reject_did_bearing_properties`, which rejects a
  `/set` create object that includes a DID-bearing property, or an update
  PatchObject that changes one, with an `invalidProperties` SetError
  listing the property. An update that repeats the stored value passes
  (draft-atwood-jmap-did-00 §The `did` Property: server-set; RFC 8620
  §5.3).
- **jmap-did-types** (new crate): the `urn:ietf:params:jmap:did`
  capability of draft-atwood-jmap-did-00 §Capability: `JMAP_DID_URI`,
  the session-level `DidCapability` (`supportedMethods`,
  `verificationEndpoint`) and the account-level `DidAccountCapability`
  (`did`, `didDocumentUri`, `previousDid`, `keyCustody`), with
  `KeyCustody` (`server` / `user` / `Other`). An empty
  `supportedMethods` is rejected ("Servers MUST support at least one
  method").
- **jmap-did-types**: pure resolution helpers for the three methods
  draft-atwood-jmap-chat-did-00 §Mandatory DID Methods requires, with no
  I/O: `DidKey` (`from_did`, `key_type`, `public_key`, `document`) and
  `KeyType` decode a `did:key` and derive its DID document by the
  did:key Method v0.9 algorithms; `Did::web_document_url` maps a
  `did:web` DID to its `did.json` URL; `verify_plc_audit_log` verifies a
  PLC directory audit log (genesis hash, DAG-CBOR CIDs, low-S ECDSA
  signatures by rotation keys, the 72-hour recovery rule, `nullified`
  flags and `createdAt` order) and returns `PlcStatus` / `PlcState`.
  `jmap-did-types` now depends on `base64`, `k256`, `p256` and `sha2`.
- **jmap-did-types**: `Did`, a DID validated against the W3C DID Core
  §3.1 ABNF (`DidError` on rejection), with `method`,
  `method_specific_id` and `web_domain`, the host of a `did:web` DID
  that draft-atwood-jmap-did-00 §Phishing via `did:web` says clients
  "SHOULD highlight".
- **jmap-cid-types**: base64 conversion for `Sha256`, so a client can
  compare it with an RFC 9404 `Blob/get` `digest:sha-256` value, per
  draft-atwood-jmap-cid-00 §Relationship to JMAP Blob Management
  Extensions ("convert encodings before comparing"): `to_base64` /
  `from_base64` for padded RFC 4648 §4 base64, and `to_bytes` for the 32
  raw digest bytes. `from_base64` rejects non-canonical base64 and
  digests that are not 32 bytes with the new `Sha256Base64Error`. The
  crate now depends on `base64`.
- **jmap-cid-types**: CIDv1 conversion for `Sha256`, per
  draft-atwood-jmap-cid-00 §Mapping to CIDv1: `to_cidv1` /
  `from_cidv1` for the `b`-prefixed lowercase base32 string form and
  `to_cidv1_bytes` / `from_cidv1_bytes` for the 36-byte binary form
  (`0x01 0x55 0x12 0x20` followed by the digest). The reverse direction
  rejects any other multibase, version, codec or hash, and
  non-canonical base32, with the new `CidV1Error`.
- **jmap-filenode-client**: `SessionClient::filenode_download` and
  `FileNodeDownload`, per draft-atwood-jmap-cid-00 §FileNode Extension
  "Client Usage". A `File` node with a `sha256` downloads as `Verified`
  after a constant-time digest compare (mismatch is
  `BlobIntegrityMismatch`); a `File` with null `sha256` is a legacy entry
  and downloads as `Unverified`. Any other `@type`, including a directory
  and an untyped node, is `InvalidArgument` with no request sent, since
  "Clients MUST check the FileNode type". The crate now depends on `bytes`.
- **jmap-filenode-server**: `FileNodeBackend::blob_sha256`, a defaulted
  hook returning the stored blob's digest when the server advertises
  `urn:ietf:params:jmap:cid` (default `Ok(None)`: not advertised).
  `FileNode/set` create and a `blobId` update now set the server-set
  `sha256` from it, per draft-atwood-jmap-cid-00 §FileNode Extension; a
  client-supplied `sha256` is dropped on create, an update that sets it to
  anything but its current value is `invalidProperties` (RFC 8620 §5.3),
  and a `blobId` update without a digest clears the old one. An update echoes the new `sha256` in
  `updated`. `MemoryBackend::seed_blob_sha256` seeds digests for tests.
  The crate now depends on `jmap-cid-types`.
- **jmap-filenode-types**: **BREAKING** `FileNode.sha256: Option<Sha256>`
  (draft-atwood-jmap-cid-00 §FileNode Extension), typed with
  `jmap_cid_types::Sha256`. It is emitted only when `Some` and only for
  `File` (or unrecognised `@type`) nodes, since the draft says it "MUST be
  `null` for directory FileNodes". A `sha256` that is not 64 lowercase hex
  characters now fails `FileNode` deserialization instead of landing in
  `extra`. The crate now depends on `jmap-cid-types`.
- **jmap-chat-mimi-types**: new crate for draft-atwood-jmap-chat-mimi-00.
  It holds `JMAP_CHAT_MIMI_URI`, the account-level `MimiCapability`
  (`userUri`, `roomIdPrefix`, `hubSender`, `cipherSuites`, `maxKeyPackages`),
  `MIMI_FANOUT_BODY_TYPE` and `MLS_MESSAGE_BODY_TYPE`, plus `Base64Url`, the
  unpadded base64url wire shape of every byte-string property ("Servers MUST
  NOT re-encode these octets"). An empty `cipherSuites` fails to
  deserialize, because the draft says it "MUST NOT be empty".
- **jmap-chat-mimi-types**: the `KeyPackage` object (§KeyPackage Object
  Fields) with `KeyPackageCreate` for `KeyPackage/set` creates and
  `KeyPackageCreated` for the `created` entries of its response, used as
  `jmap_types::SetResponse<KeyPackageCreated>`. `KeyPackage` implements
  `JmapObject`, `GetObject` and `SetObject`. The crate now depends on
  `jmap-types`.
- **jmap-chat-mimi-types**: `KeyMaterialFetchRequest` and
  `KeyMaterialFetchResponse` for `KeyMaterial/fetch`, with
  `ClientKeyMaterial` entries and the `KeyMaterialUserCode` and
  `KeyMaterialClientCode` status enums named as in
  draft-ietf-mimi-protocol-06 §5.2. Unknown code names round-trip as
  `Other`.
- **jmap-chat-mimi-types**: request and response types for `Room/create`,
  `Room/update`, `Room/send` and `Room/unbind` (draft §Rooms), with the
  `UpdateResponseCode` and `SubmitResponseCode` enums named as in
  draft-ietf-mimi-protocol-06 §5.3 and §5.4, and `chat_room_id`, which reads
  the `Chat.roomId` extension field from `Chat.extra`.
- **jmap-chat-mimi-types**: `GroupInfoFetchRequest` and
  `GroupInfoFetchResponse` for `GroupInfo/fetch` (draft §GroupInfo/fetch),
  with the `GroupInfoCode` status enum named as in
  draft-ietf-mimi-protocol-06 §5.6. The `response` is the whole
  GroupInfoResponse as a `Base64Url`, passed through unchanged.
- **jmap-chat-client**: `TypingIndicators` and `TYPING_DECAY` track typing
  indicators per `(chatId, senderId)` and hide one after 10 s without a
  typing event, per draft-atwood-jmap-chat-00 §Chat/typing: "if no typing
  event is received for a given `(chatId, senderId)` pair within
  approximately 10 seconds, the client SHOULD hide the typing indicator for
  that pair".
- **jmap-chat-types**: `Mention::new(id)` and `BroadcastMention::new(scope)`
  constructors.
- **jmap-chat-client**: `mentions_from_spans` and
  `broadcast_mentions_from_spans` derive a composed Message's `mentions` and
  `broadcastMentions` from its rich-body `"mention"` and `"broadcast"` spans,
  one entry per distinct user or scope, per draft-atwood-jmap-chat-00 §Rich
  Body Format: "A client composing a rich body MUST list each user it marks
  with a `"mention"` span in `mentions`".
- **jmap-chat-types**: `RichBody`, `Span` and `SpanType` model the parsed
  `body` of an `"application/jmap-chat-rich"` Message
  (draft-atwood-jmap-chat-00 §Rich Body Format). An unrecognized span type
  parses as `SpanType::Other` and round-trips with its fields unchanged, per
  "Servers MUST NOT reject messages solely because they contain unrecognized
  span types".
- **jmap-chat-types**: `vocabulary::COMPANION_EPHEMERAL_DATA_TYPES` lists the
  companion-registered `ChatStreamEnable` `dataTypes` values `"vtc"`
  (draft-atwood-jmap-vtc-wss-00) and `"scene"`
  (draft-atwood-jmap-scene-wss-00), per draft-atwood-jmap-chat-wss-00
  §Enabling Ephemeral Push.
- **jmap-chat-server**: `EphemeralSubscription` holds one WebSocket
  connection's normalized ephemeral subscription (draft-atwood-jmap-chat-wss-00
  §ChatStreamEnable). `replace_with` replaces it in full from a
  `ChatStreamEnable` frame and maps an absent `chatIds` / `contactIds` to
  `IdScope::All`, like `null`. It ignores unrecognized `dataTypes`, and
  rejects a frame with neither `"typing"` nor `"presence"` with an
  `invalidArguments` error that leaves the prior subscription unchanged.
- **jmap-chat-server**: `should_deliver_typing` decides, per recipient and at
  delivery time, whether a `ChatTypingEvent` may reach that owner's
  connections (draft-atwood-jmap-chat-wss-00 §ChatTypingEvent,
  draft-atwood-jmap-chat-00 §Chat/typing). It drops the event when the
  recipient is the sender, is no longer a member of the Chat, has
  `receiveTypingIndicators: false` on a direct or group Chat (channels ignore
  it), or has blocked the sender on its own contact list.
- **jmap-chat-server**: `project_presence` returns, per recipient and at
  delivery time, the `ChatPresenceEvent` that owner may receive
  (draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent, draft-atwood-jmap-chat-00
  §Presence Events). It drops the event when the contact is blocked on the
  recipient's own contact list, and delivers `"invisible"` as `"offline"`.
- **jmap-types**: `JmapUri` and `JmapUriError` parse and build the
  `jmap:<type>:<accountId>:<objectId>` JMAP object URI
  (draft-atwood-jmap-object-uri-00 §JMAP Object URIs), validating the type
  name and both ids. Exactly one object id follows the account: a URI with
  further segments, such as the retired
  `jmap:task:<accountId>:<taskListId>:<taskId>` form, is rejected
  (§Other Forms). The type lives in the foundation crate so chat,
  calendars, tasks and graph crates share it; `jmap-chat-types` does not
  re-export it.
- **jmap-chat-types**: `vocabulary::CHAT_CAP_VTC`,
  `CHAT_CAP_CALENDAR_EVENT`, `CHAT_CAP_AVAILABILITY` and `CHAT_CAP_TASK`
  name the `urn:ietf:params:jmap:chat:cap:*` Endpoint / MessageAction types.
- **jmap-chat-server**: `ChatBackend::may_create_message`,
  `may_update_message` and `may_update_chat` (default: permit) let an
  extension restrict Chats it binds, given the stored Chat (with its
  extension properties in `Chat::extra`) and the client's patch keys.
  `Message/set` consults them before any other per-Chat check on a create
  or update, and `Chat/set` before the closed-group and admin checks. A
  MIMI backend uses them for draft-atwood-jmap-chat-mimi-00 §Chat.roomId
  and §message/mls: `forbidden` for a create, or a metadata or member
  update, in a room Chat, and `invalidProperties` naming `body`,
  `bodyType` or `reactions` for an update that changes them.
- **jmap-chat-server**: `ChatBackend::on_outbound_event` (default: no-op)
  reports an `OutboundEvent` when the owner's action must reach other
  participants' servers: `Retract` for a `deletedForAll` update, before it
  is written (draft-atwood-jmap-chat-federation-00 §Peer/retract "Sender
  Behavior"); `Edit` after an edit (draft-atwood-jmap-chat-00 §Editing a
  Message); and one `GroupUpdate` per `GroupUpdateAction` after a group
  Chat create or a member, role or metadata change, with the recipients
  draft-atwood-jmap-chat-00 §Chat/set names. A federation backend turns
  them into `Peer/retract`, `Peer/deliver` `edit` and `Peer/groupUpdate`.
- **jmap-chat-server**: `on_outbound_event` also reports `NewMessage` after
  each `Message/set` create (draft-atwood-jmap-chat-00 §Message/set
  "enqueues the message for outbound delivery"), and one `ReactionUpdate`
  per `reactions/<senderReactionId>` patch key, with `ReactionAction::Add`
  or `ReactionAction::Remove` carrying the removed reaction, after the
  update is written (§Adding and Removing Reactions). A federation backend
  turns them into `Peer/deliver` `message` and `reactionUpdate`.
- **jmap-chat-server**: `on_outbound_event` also reports `Receipt` after the
  owner sets a non-null `readAt` on a received Message, carrying `readAt` and
  `readDisposition`, and suppressed when the effective `receiptSharing`
  (`Chat.receiptSharing`, else `PresenceStatus.receiptSharing`) is `false`
  (draft-atwood-jmap-chat-federation-00 §Peer/receipt "Sender Behavior");
  `Typing` after each validated `Chat/typing` (draft-atwood-jmap-chat-00
  §Chat/typing); and `Presence` after a `PresenceStatus/set` update naming
  `presence`, `statusText` or `statusEmoji`
  (draft-atwood-jmap-chat-federation-00 §Peer/subscribePresence). A backend
  turns them into `Peer/receipt`, local `typing` push and `Peer/typing`, and
  `Peer/presence`.
- **jmap-chat-server**: `ChatBackend::on_chat_destroyed`,
  `on_space_destroyed` and `on_space_ban_created` (default: no-op) are
  called after a Chat is destroyed (by `Chat/set`, `removeChannels`, or a
  Space destroy cascading to its channels), a Space is destroyed, or a
  SpaceBan is created. Companion extensions use them to clear `chatId`,
  `spaceId` and `channelId` references (draft-atwood-jmap-chat-tasks-00
  §Lifecycle, draft-atwood-jmap-vtc-00 §VTCCall), destroy a Space's file
  tree (draft-atwood-jmap-chat-filenode-00 §Space Deletion), and end a
  banned user's call participation (draft-atwood-jmap-vtc-00 §Access
  Control). `Space/set` destroy now reads the Space's channel Chats first
  and reports `serverFail` for that id if the read fails.
- **jmap-chat-server**: `ChatLimits::encrypted_body_types` (default
  `ENCRYPTED_BODY_TYPES`, now public: `"application/mls-ciphertext"`) and
  `with_encrypted_body_types` set which `bodyType` values the
  draft-atwood-jmap-chat-00 §Fields Inside the Ciphertext empty-field rule
  covers on `Message/set` create and update. A MIMI deployment adds
  `"application/mimi-fanout"`, which draft-atwood-jmap-chat-mimi-00
  §MLS Messages makes an encrypted body type.
- **jmap-chat-server**: `store_server_message` stores a Message that the
  server creates, such as a MIMI fanout Message
  (draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout) or a federation
  delivery. It returns `StoreOutcome::Duplicate` with the stored id, and
  writes nothing, when the Chat already holds that `senderMsgId`. After a
  store it creates the owner's ReadPosition and recomputes `unreadCount`
  as `Message/set` does. `find_message_by_sender_msg_id` looks up a
  Message by `(chatId, senderMsgId)` (draft-atwood-jmap-chat-federation-00).
- **jmap-chat-server**: `ChatBackend::space_extension_properties` lets a
  Space extension own extra top-level Space properties, such as
  `calendarId` (draft-atwood-jmap-chat-calendars-00) and `taskListId`
  (draft-atwood-jmap-chat-tasks-00). `Space/set` stores a listed key on
  create and passes it to `apply_space_metadata_patch` (behind the
  `manage_space` gate) on update, where `null` removes it. The default
  list is empty, so other keys are still rejected as `invalidProperties`.
- **jmap-chat-server**: `sweep_expired` (module `expiry`) applies the
  time-driven rules of draft-atwood-jmap-chat-00 for one account at a
  caller-supplied time: it hard-deletes Messages past `senderExpiresAt`,
  older than the Chat's `messageExpirySeconds` (from `receivedAt`), past
  `readAt` + `expiresAfterReadSeconds`, or read with `burnOnRead` (the
  two read rules skip the account's own sent copies), through
  `ChatBackend::expire_message`, and writes each affected Chat's
  recomputed `unreadCount` so the `Chat` state advances; clears `muted` and `muteUntil` once
  `muteUntil` passes; and at `PresenceStatus.expiresAt` nulls `statusText`
  and `statusEmoji`, resets `presence` to `"online"` and clears
  `expiresAt`. The kit still runs no scheduler; deployments call it from
  their own timer.
- **jmap-chat-server**: `resolve_channel_permissions` implements the
  five-step channel permission resolution of draft-atwood-jmap-chat-00
  §Space Permission Resolution (role deny, role allow, member deny,
  member allow over the Space-level union). New `VIEW` and `SEND`
  permission constants. New `ChatBackend::may_send_message` and
  `ChatBackend::may_view_chat` hooks, which are fail-open by default:
  `Message/set` create consults `may_send_message`; `Message/get` and
  `chatId`-scoped `Message/query` consult `may_view_chat`. A backend that
  answers `Message/query` without `chatId` must omit unviewable channel
  messages in `query_objects` itself.
- **jmap-chat-server**: `ChatLimits::supports_threads` (default `false`,
  builder `with_supports_threads`) carries the account's `supportsThreads`
  value. When `true`, `Message/get` returns the server-computed
  `replyCount` and, on thread roots, `unreadReplyCount`
  (draft-atwood-jmap-chat-00 §Message), and `Message/query` accepts the
  `threadRootId` filter, which the reference `MemoryBackend` implements.
- **jmap-chat-server**: `Chat/get` computes `unreadCount` from the
  owner's ReadPosition (draft-atwood-jmap-chat-00 §Chat): the number of
  Messages in the Chat whose id sorts after `lastReadMessageId`, or all of
  them when it is absent. It was previously the stored value, always `0`.
  `Space/join` creates the owner's ReadPosition for each channel of the
  joined Space (§ReadPosition). `ReadPosition/set` updates and
  `Message/set` creates and destroys write the recomputed count to the
  affected Chat, so the `Chat` state advances and `Chat/changes` lists
  that Chat (RFC 8620 §5.1).
- **jmap-chat-server**: `ChatLimits::max_body_bytes` (default 100,000),
  `max_attachment_bytes` (default 50,000,000) and
  `max_attachments_per_message` (default 16), with `with_*` builders,
  carry the account's draft-atwood-jmap-chat-00 capability values. New
  `ChatBackend::attachment_blob` hook and `AttachmentBlob` type report a
  stored blob's size and SHA-256; the default reports nothing, so no blob
  is checked. `Message/set` create rejects an Attachment whose `size` or
  `sha256` does not match with `invalidProperties` naming `attachments`
  (§Attachment), comparing the digest in constant time.

### Changed

- **jmap-chat-server**: **BREAKING** `Message/set` reports
  `OutboundEvent::Retract` to `ChatBackend::on_outbound_event` after the
  `deletedForAll` update is written, not before, so a failed write sends
  no `Peer/retract`. The ordering requirement the old docs quoted
  ("before marking the local record") is gone from
  draft-atwood-jmap-chat-federation-00 §Peer/retract "Sender Behavior";
  a backend that read the Message's content at event time now sees the
  updated record (bd JMAP-e5kx).
- **BREAKING** **jmap-chat-server**: `handle_chat_set`,
  `message::handle_message_set`, `handle_position_set`,
  `handle_invite_set`, `handle_ban_set` and `handle_emoji_set` take a new
  third argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_chat_handlers` does). They now
  resolve `#creationId` references (RFC 8620 §5.3) in Chat `contactId` and
  the `memberIds`, `pinnedMessageIds` and `removeMembers` elements,
  Message `chatId` / `replyTo` / `threadRootId`, ReadPosition
  `lastReadMessageId`, SpaceInvite `spaceId` / `defaultChannelId`, and
  SpaceBan and CustomEmoji `spaceId`. `Message/set` runs a create whose
  `replyTo` or `threadRootId` is `#k` after the create `k` of the same
  call, as §5.3 requires. An unknown reference fails that entry with
  `invalidProperties` naming the property. Ids nested inside entry objects
  (`addMembers`, `updateMemberRoles`, Message `mentions` and
  `attachments`, `Space/set` patch entries) are not resolved yet (bd
  JMAP-3x9e) (bd JMAP-1p88).
- **BREAKING** **jmap-scene-server**: `handle_scene_region_set`,
  `handle_scene_object_set` and `handle_scene_avatar_set` take a new third
  argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_scene_handlers` does). They now
  resolve `#creationId` references (RFC 8620 §5.3) in SceneRegion `chatId`
  / `spaceId` / `channelId` / `activeCallId`, SceneObject `regionId` /
  `parentId` and SceneAvatar `regionId`, on create and update.
  `SceneObject/set` runs a create whose `parentId` is `#k` after the create
  `k` of the same call, as §5.3 requires. An unknown reference fails that
  entry with `invalidProperties` naming the property (bd JMAP-1p88).
- **BREAKING** **jmap-vtc-server**: `handle_vtc_call_set`,
  `handle_vtc_participant_set`, `handle_vtc_recording_set` and
  `handle_vtc_livestream_set` take a new third argument, `created_ids:
  &BTreeMap<Id, Id>` (pass `CallContext::created_ids`;
  `register_vtc_handlers` does). They now resolve `#creationId` references
  (RFC 8620 §5.3) in VTCCall `parentCallId` / `chatId` / `spaceId` /
  `channelId` / `calendarEventId`, VTCParticipant `callId` /
  `breakoutCallId` and VTCRecording / VTCLivestream `callId`, on create
  and update. `VTCCall/set` runs a create whose `parentCallId` is `#k`
  after the create `k` of the same call, as §5.3 requires. An unknown
  reference fails that entry with `invalidProperties` naming the property
  (bd JMAP-1p88).
- **BREAKING** **jmap-filenode-server**: `handle_filenode_set` takes a new
  third argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_filenode_handlers` does). A
  FileNode `parentId` or `spaceId` that is a `#creationId` reference (RFC
  8620 §5.3) on create or update now resolves to the record created
  earlier in the request, and a create whose `parentId` is `#k` runs after
  the create `k` of the same call, as §5.3 requires. An unknown reference
  fails that entry with `invalidProperties` naming the property (bd
  JMAP-1p88).
- **BREAKING** **jmap-calendars-server**: `handle_calendar_event_set` takes
  a new third argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_calendars_handlers` does). A
  CalendarEvent `calendarIds` key or `calendarIds/<id>` patch path that is
  a `#creationId` reference (RFC 8620 §5.3) now resolves to the Calendar
  created earlier in the request instead of being stored literally; an
  unknown reference fails that entry with `invalidProperties` naming
  `calendarIds` (bd JMAP-1p88).
- **BREAKING** **jmap-contacts-server**: `handle_contact_card_set` takes a
  new third argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_contacts_handlers` does). A
  ContactCard `addressBookIds` key or `addressBookIds/<id>` patch path that
  is a `#creationId` reference (RFC 8620 §5.3) now resolves to the
  AddressBook created earlier in the request instead of being stored
  literally; an unknown reference fails that entry with
  `invalidProperties` naming `addressBookIds` (bd JMAP-1p88).
- **BREAKING** **jmap-tasks-server**: `handle_task_set` takes a new third
  argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_tasks_handlers` does). A Task
  `taskListId` that is a `#creationId` reference (RFC 8620 §5.3) on create
  or update now resolves to the TaskList created earlier in the request
  instead of being stored literally; an unknown reference fails that entry
  with `invalidProperties` naming `taskListId` (bd JMAP-1p88).
- **BREAKING** **jmap-graph-server**: `handle_graph_node_set`,
  `handle_graph_edge_set` and `handle_graph_node_op_set` take a new third
  argument, `created_ids: &BTreeMap<Id, Id>` (pass
  `CallContext::created_ids`; `register_graph_handlers` and
  `register_graph_edit_handlers` do). They now resolve `#creationId`
  references (RFC 8620 §5.3) in GraphNode `graphId`, GraphEdge `graphId` /
  `fromId` / `toId` (create and update) and GraphNodeOp `nodeId`, as
  draft-atwood-jmap-graph-00 L277, L306 and the L354-387 example require,
  instead of rejecting `#x` as a malformed id. A reference that matches no
  earlier creation fails that entry with `invalidProperties` naming the
  property (bd JMAP-1p88).
- **BREAKING** **jmap-mail-client**, **jmap-chat-client**,
  **jmap-chat-mimi-client**, **jmap-calendars-client**,
  **jmap-tasks-client**, **jmap-contacts-client**,
  **jmap-filenode-client**, **jmap-sharing-client**, **jmap-graph-client**,
  **jmap-scene-client**, **jmap-vtc-client**: a method argument that fails
  to serialize (for example a caller-supplied filter condition type whose
  `Serialize` impl errors) now returns `ClientError::Serialize`. Before, it
  returned `ClientError::InvalidArgument` or `ClientError::Parse` depending
  on the method, and `ids` / `properties` / `destroy` / `upToId` arguments
  panicked through `.expect`. Code that matched `InvalidArgument` or
  `Parse` for this case must match `Serialize` (bd JMAP-tjvm.11).
- **BREAKING** (Rust API): **jmap-chat-server**, **jmap-scene-server** and
  **jmap-vtc-server** move the `MemoryBackend::*_for_test` methods behind a
  new `test-helpers` feature (implies `memory`). With only `memory` enabled
  they no longer exist; enable `test-helpers` to keep calling them. The
  canonical jmap-mail-server exposes no such knobs (bd JMAP-ic0j.73).
- **BREAKING** (wire): **jmap-calendars-types**, **jmap-chat-types**,
  **jmap-contacts-types**, **jmap-filenode-types**, **jmap-sharing-types**
  and **jmap-tasks-types** comparators omit `isAscending` when it is
  `true`, as `EmailComparator` does; `false` is still sent and an absent
  key still decodes as ascending (RFC 8620 §5.5 "default: true"). Byte
  comparisons, snapshots or hashes of an ascending sort change (bd
  JMAP-18qe).
- **jmap-mail-server**, **jmap-chat-server**, **jmap-calendars-server**,
  **jmap-tasks-server**, **jmap-sharing-server**, **jmap-scene-server**,
  **jmap-vtc-server**, **jmap-graph-server**, **jmap-chat-mimi-server**:
  the `memory` feature's `MemoryBackend::register_account`, and
  **jmap-filenode-server**'s `MemoryBackend::with_account`, now
  `debug_assert!` that the account id is non-empty (RFC 8620 §1.2), as
  jmap-contacts-server already did (bd JMAP-dx0s). Release builds are
  unchanged.
- **BREAKING** **jmap-mail-server**: `SieveBackend::validate_sieve_script`
  takes the script bytes (`script: &[u8]`) instead of a `blob_id`. The
  `SieveScript/set` create and update paths and `SieveScript/validate` now
  read each script blob once with `get_sieve_blob`, check `maxSizeScript`,
  then pass the bytes to the parser, instead of fetching the blob a second
  time inside the backend. The handler now reports a missing blob as
  `invalidSieve` ("blob not found"); backends no longer handle that case. A
  backend error while fetching or validating a create or update script is
  now a per-item `serverFail` rather than a method-level error that dropped
  creates already persisted in the same call (bd JMAP-j7pa.5).
- **jmap-mail-server**, **jmap-chat-server**, **jmap-calendars-server**,
  **jmap-contacts-server**, **jmap-filenode-server**, **jmap-sharing-server**,
  **jmap-graph-server**, **jmap-scene-server**, **jmap-vtc-server**:
  `MemoryBackend` replaces `#[derive(Clone)]` with a manual `Clone` impl
  whose docs state the shared-state contract: a clone is an `Arc` clone of
  the same store, never a deep copy. No behaviour change (bd JMAP-q7k6).
- **jmap-mail-server**, **jmap-chat-server**, **jmap-calendars-server**,
  **jmap-contacts-server**, **jmap-tasks-server**, **jmap-filenode-server**,
  **jmap-sharing-server**: the `MemoryBackend` module docs now state the
  lock-poisoning policy: each `Mutex::lock` ends in a deliberate
  `.unwrap()`, because a poisoned lock means an earlier panic left state
  half-updated. No behaviour change (bd JMAP-pk88).
- **BREAKING** **jmap-mail-server**: `handle_mailbox_set`,
  `handle_email_set` and `handle_submission_set` take a new third argument,
  `created_ids: &BTreeMap<Id, Id>` (pass `CallContext::created_ids`;
  `register_mail_handlers` does). They now resolve `#creationId`
  references (RFC 8620 §5.3) in Mailbox `parentId` (create and update),
  Email `mailboxIds` keys and `mailboxIds/<id>` patch paths, and
  EmailSubmission `identityId` / `emailId`, instead of treating `#x` as a
  literal id. `Mailbox/set` runs a create whose `parentId` is `#k` after
  the create `k` of the same call, as §5.3 requires. A reference that
  matches no earlier creation fails that entry with `invalidProperties`
  naming the property (bd JMAP-j7uo).
- **BREAKING** **jmap-server**: `Dispatcher::dispatch` now answers a method
  call whose `filter` nests FilterOperator `conditions` more than 32 levels
  deep with `unsupportedFilter` (RFC 8620 §5.5, rfc8620.txt:2626-2629),
  before the handler runs. This one cap bounds the stack use of every
  filter walker in the `*-server` crates, which used to recurse as deep as
  the JSON parser allowed. Handlers called directly, outside the
  dispatcher, are not capped (bd JMAP-xmvz).
- **BREAKING** **jmap-server**: `PushSubscription/get` now rejects a
  `properties` entry that is neither an RFC 8620 §7.2 property nor listed in
  the new `PushSubscriptionBackend::PUSH_SUBSCRIPTION_EXTENSION_PROPERTIES`
  (default empty) with `invalidArguments`, per RFC 8620 §5.1
  (rfc8620.txt:1609-1611); it used to drop unknown names silently. A backend
  that stores extension properties must list them. **jmap-chat-server**'s
  `MemoryBackend` lists `chatPush` (bd JMAP-yt1w.39).
- **jmap-mail-server**, **jmap-filenode-server**: the `MailBackend` and
  `FileNodeBackend` trait docs gain a "Permission enforcement (backend
  canonical)" section (bd JMAP-510h.44). It names `principal_id` as the only
  identity seam and maps each write to the RFC 8621 §2 or
  draft-ietf-jmap-filenode-16 `myRights` it needs. It also states that
  handlers do no authorization, so a backend that follows the rustdoc alone
  enforces ACLs. Docs only; no API change.
- **jmap-mail-server**, **jmap-chat-server**, **jmap-calendars-server**,
  **jmap-contacts-server**, **jmap-filenode-server**,
  **jmap-sharing-server**: each mutating backend method's rustdoc gains an
  "Authorisation contract" section, matching **jmap-tasks-server**
  (bd JMAP-8zcl). It states that the method must check the caller's rights
  atomically with the write and return `forbidden` otherwise, because
  handlers do no permission checking. A `None` `principal_id` permits
  every write (fail-open, bd JMAP-bghk); **jmap-tasks-server**'s wording now
  says so too. Docs only; no API change.
- **workspace**: every crate moves to Rust edition 2024 (bd JMAP-i3cz),
  unlocking let-chains. MSRV stays 1.91 (edition 2024 needs 1.85), and the
  public API is unchanged. The source was migrated with
  `cargo fix --edition`, reformatted under the 2024 style edition, and nested
  `if let` blocks were collapsed into let-chains per clippy
  `collapsible_if`.
- **BREAKING** **jmap-server**, **jmap-mail-server**: wire ids are validated
  against RFC 8620 §1.2 (1-255 octets of `A-Za-z0-9-_`) instead of being
  wrapped unchecked with `Id::from` (bd JMAP-k9va). A malformed `/set`
  `update` key or `destroy` element now fails that entry with
  `invalidProperties` (`properties: ["id"]`) instead of reaching the
  backend; a malformed Id-typed create or patch property (`parentId`,
  `mailboxIds` keys, `identityId`, `emailId`, `blobId`, `Email/copy` `id`)
  fails with `invalidProperties` naming it; a malformed `anchor`, `upToId`,
  `fromAccountId` or `SieveScript/validate` `blobId` argument is
  `invalidArguments`. `query_anchor_args` returns `invalidArguments` for a
  malformed `anchor` where it used to pass it through to `anchorNotFound`.
- **BREAKING** **jmap-chat-server**, **jmap-calendars-server**,
  **jmap-tasks-server**, **jmap-contacts-server**, **jmap-filenode-server**,
  **jmap-sharing-server**, **jmap-vtc-server**, **jmap-scene-server**,
  **jmap-graph-server**, **jmap-chat-mimi-server**,
  **jmap-chat-filenode-server**: the same RFC 8620 §1.2 wire-id validation
  as `jmap-mail-server` (bd JMAP-k9va). `/set` `update` keys and `destroy`
  elements fail with `invalidProperties` (`properties: ["id"]`); Id-typed
  create and patch properties (for example `spaceId`, `userId`, `chatId`,
  `parentId`, `blobId`, `regionId`, `visualRef`, `callId`,
  `parentCallId`, `nodeId`, `graphId`, Chat `memberIds`, `contactId`,
  `avatarBlobId` and `pinnedMessageIds`, Message `replyTo` and
  `threadRootId`, SpaceInvite `defaultChannelId`, `/copy` `id`) fail with
  `invalidProperties` naming them; the CalendarEvent/copy and
  ContactCard/copy `fromAccountId`, `chatId`, Space/join `spaceId` and
  `Principal/getAvailability` `id` arguments are `invalidArguments`. A
  malformed `onSuccessSetIsDefault` id is skipped like an unknown one.
  Read-only lookups that run before `accountId` validation (Message/changes
  `filter.chatId`, the Graph and VTC `accountId` pre-checks) are unchanged:
  a malformed value there matches nothing.
- **jmap-calendars-server**: **BREAKING** `AvailabilityError` and
  `QueryCalendarEventsError` are removed. `get_availability` and
  `query_calendar_events` return `jmap_server::BackendQueryError<Self::Error>`,
  the same shape as `query_objects` in every server crate. A backend
  returns the draft-ietf-jmap-calendars-33 §2.2 errors (`notFound`,
  `forbidden`, `tooLarge`, `rateLimit`) and the §10.8.3/§10.8.4 errors
  (`expandDurationTooLarge`, `cannotCalculateOccurrences`) as
  `BackendQueryError::Method(JmapError)`; the wire errors are unchanged.
  The crate no longer depends on `thiserror` (bd JMAP-ic0j.37).
- **BREAKING** **jmap-server** and every extension server crate:
  `BackendSetError::SetError` now holds `Box<SetError>`, so a backend
  `Result`'s `Err` arm is pointer-sized instead of over 200 bytes.
  `From<SetError>` boxes; a new `From<Box<SetError>>` passes through, so
  `?` and `.into()` call sites are unchanged. Code that builds the variant
  directly needs `Box::new(..)` (or `.into()`); code that matches it gets a
  `Box<SetError>`. Public pre-validation helpers that feed it now return
  `Result<_, Box<SetError>>`: `validate_chat_push` (jmap-chat-server),
  `check_task_chat_id` (jmap-chat-tasks-server), and
  `check_key_package_create`, `may_update_room_chat`,
  `may_create_room_message` and `may_update_room_message`
  (jmap-chat-mimi-server). All 54 `#[allow(clippy::result_large_err)]`
  are gone (bd JMAP-s9xo).
- **BREAKING** **jmap-types**: `ValidationError`'s inner `String` is now
  private, matching `Id`, `UTCDate`, `Date` and `State`. Read the message
  through `Display` (`err.to_string()`) instead of `err.0` or tuple
  destructuring.
- **BREAKING** **jmap-server**, **jmap-chat-server**, **jmap-vtc-server**:
  a non-object `filter` (`42`, `"x"`, `true`, `[]`) on the generic
  `handle_query` / `handle_query_changes` (Tasks, Contacts, FileNode,
  Sharing, Chat, VTC and the other objects built on them, Graph aside),
  including the `VTCCall` and `VTCParticipant` `/query` and
  `/queryChanges` pre-checks, and on `ChatContact/query`,
  `CustomEmoji/query`, `Message/query`, `Message/queryChanges` and
  `Space/query` is now `invalidArguments`, not `unsupportedFilter`. RFC
  8620 §5.5 types `filter` as "FilterOperator|FilterCondition|null", both
  objects, and keeps `unsupportedFilter` for a filter that "is
  syntactically valid, but the server cannot process it". This matches
  the Mail `/query` and Calendars handlers (`jmap_server::filter_arg`);
  an object that fails to decode is still `unsupportedFilter`. Graph
  handlers keep `unsupportedFilter`, as the graph draft mandates for any
  filter other than its single condition (bd JMAP-98zb).
- **jmap-server**: the `Dispatcher::dispatch` docs now state the
  cancellation contract as intended behaviour: dropping the dispatch
  future does not cancel method calls already running in their spawned
  tasks; they run to completion and only the response is lost, so a
  client disconnect never interrupts a backend mid-write. No
  cancellation token will be added (bd JMAP-wlip.34).
- **BREAKING** **jmap-types**, **jmap-server**: `RESERVED_SET_ERROR_WIRE_NAMES`
  is no longer public, and `jmap_server` no longer re-exports it. A public
  list would make every new typed `SetError` field a silent break for
  code that relied on its contents. `SetError::with_extra` (debug assert)
  and `SetError::validate_extras` still reject the reserved keys
  (bd JMAP-jfia.18).
- **jmap-types** and every `jmap-*-client` crate: **BREAKING**
  `SetResponse::{created, updated, destroyed, not_created, not_updated,
  not_destroyed}` and `CopyResponse::{created, not_created}` are plain
  `BTreeMap` / `Vec`, not `Option`. RFC 8620 §5.3 / §5.4 define each as
  "null if none", so `None` and an empty collection meant the same thing
  and every caller had to flatten the `Option`. A `null` or absent field
  decodes as empty and an empty one encodes as `null`, the RFC wire form
  (bd JMAP-6xs8.24).
- **jmap-chat-types**, **jmap-chat-federation-types**: **BREAKING**
  `Attachment.sha256` is `jmap_cid_types::Sha256`,
  `AttachmentCreate.sha256` is `Option<Sha256>`, and `PeerAvatar.sha256`
  is `Sha256` (`PeerAvatar::new` takes a `Sha256`), the same typed
  digest FileNode carries. Both crates now depend on `jmap-cid-types`. A
  digest that is not 64 lowercase hex characters (draft-atwood-jmap-chat-00
  §Attachment) is rejected at deserialize; `Message/set` and
  `Peer/deliver` answer it with the same `invalidProperties` and
  `invalidArguments` errors as before (bd JMAP-tzvc).
- **jmap-base-client** and every `jmap-*-client` crate: **BREAKING**
  account and blob ids are typed `jmap_types::Id` on the client side, as
  they already are in server backend traits and in `BlobUploadResponse`.
  `Session.accounts` is a `BTreeMap<Id, AccountInfo>`,
  `Session.primary_accounts` is a `BTreeMap<String, Id>`,
  `Session::primary_account_id` returns `Option<&Id>`, and the
  `account_id` / `blob_id` fields of `UploadBlobParams`,
  `UploadBlobSessionParams`, `UploadBlobStreamParams`,
  `DownloadBlobParams` and `DownloadBlobSessionParams` are `&Id`, so a
  `BlobUploadResponse.blob_id` passes straight into a download. The
  `*_account_id()` accessors on every extension `SessionClient` return
  `Result<&Id, ClientError>`, and `ChatSessionExt::chat_account_id`
  returns `Option<&Id>`. Map lookups by `&str` still work through
  `Id: Borrow<str>`; use `.as_ref()` where a `&str` is needed
  (bd JMAP-ops7.49).
- **jmap-types** and every types crate: **BREAKING** `SetObject` has
  three required associated consts, `SERVER_SET`, `CLIENT_SETTABLE` and
  `IMMUTABLE`, naming each property's RFC 8620 §1.1 attribute from its
  spec. Every `SetObject` impl in the workspace declares them, and a
  per-type test fails when a field is in neither or both of `SERVER_SET`
  and `CLIENT_SETTABLE`. Downstream `SetObject` impls must add them
  (bd JMAP-x2gd.87).
- **jmap-mail-server**: **BREAKING** (wire) `Email/set`, `Mailbox/set`,
  `Identity/set`, `EmailSubmission/set` and `SieveScript/set` reject a
  create carrying a non-null server-set property (e.g. Email `size`,
  EmailSubmission `sendAt`, Identity `mayDelete`) with `invalidProperties`
  instead of ignoring it (RFC 8620 §1.1, §5.3). Update patches are checked
  through `jmap_server::reject_read_only_patch`, so a key pointing inside
  a server-set property (`myRights/mayDelete`) is rejected too, every
  offending property is listed, and `VacationResponse/set` rejects a patch
  to `id`. The `IMMUTABLE_EMAIL_FIELDS` and Mailbox/Identity read-only
  lists are gone (bd JMAP-x2gd.87).
- **jmap-calendars-server**, **jmap-contacts-server**,
  **jmap-tasks-server**, **jmap-filenode-server**,
  **jmap-sharing-server**: **BREAKING** (wire) `/set` creates reject a
  non-null server-set property and updates reject a patch to a
  server-set or immutable property, with `invalidProperties` (RFC 8620
  §1.1, §5.3), from each type's `SetObject` lists. Calendar, AddressBook,
  TaskList, ParticipantIdentity and Principal creates no longer require
  the client to send `isDefault`, `myRights`, `capabilities` or
  `accounts`; the server supplies them. A client `isOrigin` (CalendarEvent)
  or `sha256` (FileNode) on create is rejected instead of replaced.
  AddressBook `isDefault` changes only through `onSuccessSetIsDefault`
  (RFC 9610 §2), and the book it demotes now appears in `updated`
  whatever the backend's `update_object` returns (bd JMAP-x2gd.87).
- **jmap-chat-types**, **jmap-vtc-types**: **BREAKING** the `SetObject`
  lists also follow each `/set` method's text: Chat `members`, `spaceId`
  and the channel properties, Space `roles`, `members`, `categories` and
  `uncategorizedChannelIds`, ChatContact `endpoints`, VTCCall `joinUri`
  and VTCParticipant `e2eeFingerprint` are server-set; Message
  `attachments`, `actions`, `replyTo`, `threadRootId` and `sentAt`,
  CustomEmoji `spaceId` and VTCParticipant `gatewayData` are immutable
  (bd JMAP-x2gd.87).
- **jmap-chat-server**, **jmap-graph-server**, **jmap-scene-server**,
  **jmap-vtc-server**: **BREAKING** (wire) the hand-written read-only and
  update-allowed lists are gone; `/set` handlers read the `SetObject`
  lists through `jmap_server::reject_server_set`,
  `reject_read_only_patch` and `is_client_updatable`. Creates reject a
  non-null server-set property, including on Chat, Message, Space,
  CustomEmoji, SpaceInvite and SpaceBan, which ignored some before. A
  ChatContact patch to a server-set property such as `presence` is now
  `invalidProperties` instead of being dropped, and a key pointing inside
  a read-only property is rejected. Graph, GraphNode, GraphEdge,
  SceneObject and SceneRegion updates accept a server-set or immutable
  property that repeats its stored value (RFC 8620 §5.3)
  (bd JMAP-x2gd.87).
- **jmap-chat-mimi-server**: `KeyPackage/set` create takes its allowed
  properties from `KeyPackage::CLIENT_SETTABLE` (bd JMAP-x2gd.87).
- **jmap-vtc-client**, **jmap-scene-client**, **jmap-graph-client**:
  **BREAKING** every `/query` and `/queryChanges` method (`vtc_call_*`,
  `vtc_participant_*`, `vtc_recording_*`, `vtc_livestream_*`,
  `scene_asset_*`, `scene_avatar_*`, `scene_region_*`, `scene_object_*`,
  `graph_query`, `graph_query_changes`) takes `<C: Serialize>`
  `filter: Option<&Filter<C>>` (was the concrete `Option<&XFilter>`
  alias), matching jmap-mail-client and the other extension clients, so
  callers can send `WithMetadata` or vendor conditions. A bare `None`
  filter now needs its type named, e.g. `None::<&VtcCallFilter>`. The
  single-condition `graph_node_*`, `graph_edge_*` and `graph_node_op_*`
  queries are unchanged (bd JMAP-w9ox.3).
- **jmap-chat-client**: **BREAKING** `ChatPatch.receipt_sharing` is
  `Patch<bool>` (was `Option<bool>`), like `mute_until` and
  `message_expiry_seconds`. `Patch::Clear` sends `receiptSharing: null`,
  which removes the per-Chat override so the account-level
  `PresenceStatus.receiptSharing` applies again
  (draft-atwood-jmap-chat-00 §Chat "When absent, the account-level
  preference applies"); `Some(b)` becomes `Patch::Set(b)` and `None`
  becomes `Patch::Keep` (bd JMAP-w9ox.2).
- **jmap-contacts-types**: **BREAKING**
  `ContactsAccountCapability.max_address_books_per_card` is
  `Option<u64>` (was `Option<u32>`). RFC 9610 §1.4.1 types it
  `UnsignedInt|null` (up to 2^53-1), so a value above `u32::MAX` no
  longer fails to decode the whole capability (bd JMAP-w9ox.4).
- **jmap-calendars-server**: **BREAKING** the reference `MemoryBackend`
  answers `unsupportedFilter` (RFC 8620 §5.5) for a filter it cannot
  evaluate: a `CalendarEvent/query` condition naming anything but
  `inCalendar` (alone or inside an operator tree), and any non-empty
  filter on another type such as `CalendarEventNotification`. It used to
  count the unevaluated condition as a match, so `NOT` over one returned
  an empty list (bd JMAP-skk9.19).
- **BREAKING** The blocked-sender hooks of Scene, VTC and Graph have one
  shape (bd JMAP-skk9.4): the delivery function takes the recipient's
  own account, applies the block rule only when that account advertises
  `urn:ietf:params:jmap:chat` (`has_capability`), and asks a hook that
  takes only the caller and the sender, since the caller names "the
  recipient's contact list". **jmap-vtc-server**:
  `VtcBackend::is_user_blocked(caller, user_id)` drops `account_id`, and
  `should_deliver` no longer suppresses a blocked user's events when the
  recipient's account lacks Chat. **jmap-graph-server**:
  `GraphBackend::is_sender_blocked(caller, sender_id)` drops
  `account_id` (it was the node's hosting account, so a shared account
  looked up the wrong contact list); new required
  `GraphBackend::has_capability(caller, account_id, uri)`, declared like
  `SceneBackend::has_capability`; `should_deliver` takes
  `recipient_account_id` after `caller`. **jmap-chat-vtc-server**:
  `is_blocked_by_contact` keeps its `account_id`; pass the caller's own
  account from `is_user_blocked`. Scene keeps its shape.
- **BREAKING** `/query` and `/queryChanges` take the full RFC 8620 §5.5
  filter tree ("FilterOperator|FilterCondition|null") for every object
  whose `QueryObject::Filter` was a bare condition, as Email already
  did (bd JMAP-skk9.2). **jmap-mail-types**: `Mailbox` uses
  `MailboxFilter`. **jmap-calendars-types**: `CalendarEvent` and
  `CalendarEventNotification` use `CalendarEventFilter` and
  `NotificationFilter`.
  **jmap-contacts-types**: `ContactCard` uses `ContactCardFilter`.
  **jmap-tasks-types**: `Task` and `TaskNotification` use `TaskFilter`
  and `TaskNotificationFilter`. **jmap-filenode-types**: `FileNode` uses
  `FileNodeFilter`. **jmap-sharing-types**: `Principal` and
  `ShareNotification` use `PrincipalFilter` and
  `ShareNotificationFilter`. Backends' `query_objects::<O>`,
  `query_changes::<O>` and `CalendarsBackend::query_calendar_events`
  receive `Filter<…FilterCondition>`; wrap a single condition in
  `Filter::Condition`. The default `FileNodeBackend::query_subtree`
  passes `Filter::Condition`. An operator tree is no longer
  `unsupportedFilter`; the Mail, Calendars (`inCalendar`), Contacts and
  FileNode `MemoryBackend`s evaluate `AND`/`OR`/`NOT`, the Tasks one
  answers an operator tree with `unsupportedFilter` (not `serverFail`),
  and the Sharing one keeps its documented filter limits.
  `CalendarEvent/query` with `expandRecurrences: true` still requires a
  single FilterCondition with `before` and `after`
  (draft-ietf-jmap-calendars-33 §5.11) and answers an operator tree with
  `invalidArguments`. Graph's GraphNode, GraphEdge and GraphNodeOp
  filters stay single conditions by their drafts.
- **BREAKING** Filter conditions use the object's enum instead of
  `String`, as `MailboxFilterCondition.role` does (bd JMAP-skk9.1).
  **jmap-filenode-types**: `FileNodeFilterCondition.node_type` is
  `Option<NodeType>` and `.role` is `Option<NodeRole>`
  (draft-ietf-jmap-filenode-16 §3.2.5). **jmap-calendars-types**:
  `NotificationFilterCondition.notification_type` is
  `Option<NotificationType>` (draft-ietf-jmap-calendars-33 §7.4.1).
  **jmap-tasks-types**: `TaskNotificationFilterCondition.notification_type`
  is `Option<NotificationType>` (draft-ietf-jmap-tasks-06 §5.5.1) and
  `TaskFilterCondition.progress` is `Option<TaskProgress>`. Unknown
  values still round-trip through each enum's `Other(String)`; the wire
  format is unchanged.
- **BREAKING** Typed `/query` comparators for the remaining types whose
  spec names sort properties, each with an `Other(String)` catch-all
  property enum (bd JMAP-wf92.25). **jmap-mail-types**:
  `MailboxComparator` / `MailboxComparatorProperty` (RFC 8621 §2.3
  `sortOrder`, `name`) and `EmailSubmissionComparator` /
  `EmailSubmissionComparatorProperty` (§7.3 `emailId`, `threadId`,
  `sentAt`). **jmap-calendars-types**: `CalendarEventNotificationComparator`
  (draft-ietf-jmap-calendars-33 §7.4.2 `created`).
  **jmap-sharing-types**: `ShareNotificationComparator` (RFC 9670
  §3.4.2 `created`). **jmap-filenode-types**: `FileNodeComparator`
  (draft-ietf-jmap-filenode-16 §3.2.5 `tree`, `nodeType`, plus the §1
  `fileNodeQuerySortOptions` example names). Each is now that type's
  `QueryObject::Comparator`. Calendar, ParticipantIdentity and TaskList
  keep `serde_json::Value` (their specs define no `/query`), and so
  does Principal (RFC 9670 §2.4 names no sort property).
- **BREAKING** Every client `/query` and `/queryChanges` `sort` argument
  takes `Option<&[XxxComparator]>`, the `email_query` shape:
  **jmap-mail-client** `mailbox_query*` and `email_submission_query*`
  (were `Option<serde_json::Value>`), **jmap-filenode-client**
  `filenode_query*` and **jmap-sharing-client**
  `share_notification_query*` (were `Option<serde_json::Value>`),
  **jmap-calendars-client** `calendar_event_notification_query*` (was
  `Option<&[serde_json::Value]>`), and **jmap-sharing-client**
  `principal_query*`, which takes `Option<&[serde_json::Value]>`
  (bd JMAP-wf92.25).
- **jmap-mail-server**: `Mailbox/query` and `Mailbox/queryChanges`
  decode `sort` as `MailboxComparator`; a comparator with a non-boolean
  `isAscending` or other malformed field is now `invalidArguments`
  instead of being read as ascending (RFC 8620 §5.5). An unknown
  property is still `unsupportedSort` (bd JMAP-wf92.25).
- **jmap-sharing-server**, **jmap-filenode-server**,
  **jmap-calendars-server**: the generic `/query` and `/queryChanges`
  handlers for ShareNotification, FileNode and CalendarEventNotification
  decode
  `sort` as the typed comparator, so a malformed comparator (e.g.
  `"sort": [5]`) is now `invalidArguments`; it used to be accepted as
  opaque JSON (RFC 8620 §5.5; bd JMAP-wf92.25, JMAP-skk9.13).
- **BREAKING** **jmap-calendars-types**:
  `Calendar::default_alerts_with_time` and
  `Calendar::default_alerts_without_time` are
  `Option<BTreeMap<Id, serde_json::Value>>` (were keyed by `String`),
  matching `Id[Alert]|null` in draft-ietf-jmap-calendars-33 §4 and
  `TaskList` in jmap-tasks-types (bd JMAP-skk9.13).

- **jmap-metadata-server**: **BREAKING** `value_depth` returns `u64`
  and `exceeds_max_depth` takes `max_depth: u64`, matching
  `DataTypeMetadataInfo::max_depth: Option<u64>`
  (draft-ietf-jmap-metadata-04 §1.2.1 "maxDepth: UnsignedInt|null").
  The old `u32` made callers write `d as u32`, which turns an
  advertised `maxDepth` of 2^32 into 0 so that every write is rejected
  (bd JMAP-skk9.12).
- **jmap-calendars-client**: **BREAKING** `calendar_event_set` takes a
  trailing `params: Option<CalendarEventSetParams>` carrying
  `sendSchedulingMessages` (draft-ietf-jmap-calendars-33 §5.9), so a
  client can ask the server to send iTIP scheduling messages.
  `participant_identity_set` takes a trailing
  `params: Option<ParticipantIdentitySetParams>` carrying
  `onSuccessSetIsDefault` (§3.3). `CalendarSetParams` gains
  `on_success_set_is_default: Option<String>` (§4.3), which breaks
  struct literals without `..Default::default()`. Pass `None` to keep
  the old behaviour. `calendar_set` and `participant_identity_set` now
  send a call with no create, update or destroy when
  `on_success_set_is_default` is set (§8.4) (bd JMAP-skk9.9).
- **jmap-contacts-client**: **BREAKING**
  `AddressBookSetParams::on_success_set_is_default` is `Option<String>`
  (was `Option<serde_json::Value>`): RFC 9610 §2.3 defines it as
  `Id|null`, and a `String` also holds a `#creationId` reference.
  `address_book_set` now sends a call with only this argument set
  (RFC 9610 §4.2) (bd JMAP-skk9.9).
- **jmap-chat-client**: **BREAKING** `SessionClient::message_get` takes
  `ids: Option<&[Id]>` (was `&[Id]`), like `email_get` and every other
  client `*_get`; wrap existing arguments in `Some(..)`. The client no
  longer rejects empty `ids` with `InvalidArgument`: `None` omits `ids`
  so the server returns all Messages (RFC 8620 §5.1;
  draft-atwood-jmap-chat-00 §Message/get "Standard JMAP `/get`"), and
  the server enforces `maxObjectsInGet` (bd JMAP-skk9.10).
- **BREAKING** `#[non_exhaustive]` on `jmap_chat_client::RsvpStatus`
  (RFC 8984 §4.4.6 `participationStatus` is registry-extensible) and
  `jmap_filenode_client::FileNodeOnExists` (the `onExists` value set
  grew in draft-ietf-jmap-filenode-14 and -16). A `match` on either
  outside its crate needs a wildcard arm.
- **jmap-tasks-types**: **BREAKING** `Task.related_to`, `locations`,
  `virtual_locations`, `links`, `recurrence_rules`,
  `excluded_recurrence_rules`, `participants`, `alerts` and `time_zones`
  are `Option<serde_json::Value>` (were `Option<BTreeMap<..>>` or
  `Option<Vec<..>>`), matching `jmap-calendars-types` `CalendarEvent` and
  the workspace sloppy-Value rule. Decode entries with
  `serde_json::from_value` into the `jmap-jscalendar-types` sub-types.
  The wire format is unchanged (bd JMAP-skk9.7).
- **jmap-tasks-types**, **jmap-calendars-types**: **BREAKING**
  `TaskFilterCondition.after` / `before` (was `Option<UTCDate>`) and
  `CalendarEventFilterCondition.after` / `before` (was `Option<String>`) are
  `Option<LocalDateTime>`. draft-ietf-jmap-calendars-33 §5.11.1 types both as
  LocalDateTime in the `timeZone` argument, and Task/query is that method
  (draft-ietf-jmap-tasks-06 §4.13). `UTCDate` required a trailing `Z`, so a
  spec-conforming Task filter failed to deserialize (bd JMAP-skk9.6).

- **jmap-chat-types**, **jmap-chat-server**: **BREAKING**
  `ChatPushCapability.supported_urgency_values` is `Vec<jmap_types::Urgency>`
  and `BuiltPush.urgency` is `jmap_types::Urgency`, both previously the open
  `UrgencyLevel`. draft-atwood-jmap-chat-push-00 closes
  `supportedUrgencyValues` to the four RFC 8030 values, so a capability
  listing another string now fails to deserialize, and a built push always
  carries a sendable `Urgency` header. `UrgencyLevel` stays on the
  client-supplied `ChatPushConfig.urgency` / `mentionUrgency`, where an
  unknown value must be reported as `invalidProperties`; `validate_chat_push`
  now rejects such a value even if it were advertised (bd JMAP-skk9.5).
- **jmap-calendars-types**, **jmap-tasks-types**: **BREAKING**
  `CalendarsAccountCapability::max_expanded_query_duration`,
  `PrincipalsAvailabilityAccountCapability::max_availability_duration`
  and `TasksRecurrencesAccountCapability::max_expanded_query_duration`
  are `Duration`, not `String` (draft-ietf-jmap-calendars-33 §1.5.1,
  §1.5.2 and draft-ietf-jmap-tasks-06 §1.6.2 type them as Duration).
  The wire format is unchanged. Migrate with `Duration::from(s)` and
  `.as_ref()`. (bd JMAP-skk9.3)
- **jmap-mail-types**: `Keyword` is `#[non_exhaustive]`, matching
  `Id`, `State`, `UTCDate` and `LocalDateTime`. Not breaking: its field
  was already private, so callers construct it with `Keyword::new`,
  `Keyword::try_new` or `From<&str>` as before.
- **jmap-mail-server**, **jmap-chat-server**, **jmap-chat-mimi-server**,
  **jmap-calendars-server**, **jmap-contacts-server**,
  **jmap-tasks-server**, **jmap-sharing-server**,
  **jmap-filenode-server**, **jmap-graph-server**,
  **jmap-scene-server**, **jmap-vtc-server**: `MemoryBackend` is
  `#[non_exhaustive]`, stating that fields can be added. Not breaking:
  its fields were already private, so callers build it with
  `MemoryBackend::new()` (or `Default` where implemented) as before.
- **jmap-types**: **BREAKING** `SetResponse<T>` has no default type
  parameter (was `T = serde_json::Value`), so a signature can no longer
  depend on it silently; name the type, normally `PartialObject` for
  client `/set` results (RFC 8620 §5.3).
- **jmap-vtc-client**: **BREAKING** `vtc_call_set`,
  `vtc_participant_set`, `vtc_recording_set` and `vtc_livestream_set`
  return `SetResponse<PartialObject>` instead of the hidden default
  `SetResponse<serde_json::Value>` (RFC 8620 §5.3).
- **jmap-scene-client**: **BREAKING** `scene_asset_set`,
  `scene_avatar_set`, `scene_object_set` and `scene_region_set` return
  `SetResponse<PartialObject>` instead of the hidden default
  `SetResponse<serde_json::Value>` (RFC 8620 §5.3).
- **jmap-graph-client**: **BREAKING** `graph_set`, `graph_node_set`,
  `graph_edge_set` and `graph_node_op_set` return
  `SetResponse<PartialObject>` instead of the hidden default
  `SetResponse<serde_json::Value>` (RFC 8620 §5.3).
- **jmap-chat-client**: **BREAKING** every `/set` wrapper (`chat_*`,
  `message_*`, `space_*`, `space_ban_*`, `space_invite_*`,
  `custom_emoji_*`, `chat_contact_*`, `read_position_update`,
  `presence_status_update`, `push_subscription_update` /
  `_destroy`) returns `SetResponse<PartialObject>` instead of the hidden
  default `SetResponse<serde_json::Value>`; read a created id as
  `created[key].id` instead of `created[key]["id"]` (RFC 8620 §5.3).
- **jmap-sharing-client**: **BREAKING** `principal_set` and
  `share_notification_set` return `SetResponse<PartialObject>` (was
  `SetResponse<Principal>`, which rejected RFC 8620 §5.3 `created`
  values lacking the client-sent, type-required `type` / `name`, and the
  hidden `SetResponse<serde_json::Value>`).
- **jmap-filenode-client**: **BREAKING** `filenode_set` and
  `filenode_copy` return `SetResponse<PartialObject>` (was
  `SetResponse<FileNode>`, which rejected RFC 8620 §5.3 `created` values
  lacking the client-sent, type-required `name`). `filenode_copy` keeps
  the `SetResponse` shape rather than `CopyResponse` so the `destroyed`
  list draft-ietf-jmap-filenode-16 §3.2.3 / §3.2.4 requires stays typed.
- **jmap-tasks-client**: **BREAKING** `task_list_set`, `task_set` and
  `task_notification_set` return `SetResponse<PartialObject>` (was
  `SetResponse<TaskList>` / `SetResponse<Task>` / the hidden
  `SetResponse<serde_json::Value>`), and `task_copy` returns
  `CopyResponse<PartialObject>` (RFC 8620 §5.3 / §5.4 `created` values
  are partial).
- **jmap-contacts-client**: **BREAKING** `address_book_set` and
  `contact_card_set` return `SetResponse<PartialObject>` (was
  `SetResponse<AddressBook>` / `SetResponse<ContactCard>`, which
  rejected RFC 8620 §5.3 `created` values lacking client-sent required
  properties such as `name`); `contact_card_copy` returns
  `CopyResponse<PartialObject>`.
- **jmap-calendars-client**: **BREAKING** `calendar_set`,
  `calendar_event_set` and `participant_identity_set` take
  `create: Option<serde_json::Value>` (was a typed
  `BTreeMap<String, Calendar>` etc.) and `destroy: Option<Vec<Id>>` (was
  `Option<&[Id]>`), `calendar_event_notification_set` takes
  `destroy: Option<Vec<Id>>`, and `calendar_event_copy` takes
  `create: serde_json::Value`, matching the canonical `jmap-mail-client`
  shape. Every `/set` returns `SetResponse<PartialObject>` and
  `calendar_event_copy` returns `CopyResponse<PartialObject>` (RFC 8620
  §5.3 / §5.4 `created` values are partial).
- **jmap-mail-client**: **BREAKING** `email_set`, `mailbox_set`,
  `identity_set`, `email_submission_set` and `vacation_response_set`
  return `SetResponse<PartialObject>` instead of `SetResponse<Email>`
  etc., and `email_copy` returns `CopyResponse<PartialObject>`. RFC 8620
  §5.3 / §5.4 `created` values carry only the properties the client did
  not send, so spec-conformant responses (e.g. the RFC 8621 §7 Email/set
  example) failed to parse as full objects. `search_snippet_get` returns
  `SearchSnippetGetResponse` instead of `serde_json::Value`.
  `CopyResponse` and `PartialObject` are re-exported.
- **BREAKING** The calendars crates move from draft-ietf-jmap-calendars-26
  to draft-ietf-jmap-calendars-33. **jmap-calendars-types**:
  `PrincipalCalendarsCapability::calendar_address` is `Option<String>`,
  required and nullable (§2.1 `String|null`); the
  `CalendarEventFilterCondition::attendee` condition is now
  `participant`, matching any participant (§5.11.1), and `attendee` no
  longer parses; `CalendarEventComparatorProperty::RecurrenceId` is
  replaced by `Id`, so the MUST sorts are `id`, `uid`, `start` (§5.11.2)
  and `recurrenceId` decodes as `Other`; `CalendarEvent::ical_component`
  (`iCalComponent`, a string) is replaced by `CalendarEvent::icalendar`
  (`iCalendar`, an ICalComponent object as `serde_json::Value`, §5.7,
  draft-ietf-calext-jscalendar-icalendar-28 §5.1.1); `CalendarEvent`
  gains a typed `version`. **jmap-calendars-server**: `CalendarEvent/set`
  create and `CalendarEvent/copy` fill an omitted `@type` with `"Event"`
  and an omitted `version` with `"2.0"` (§5.9), and the backend trait
  docs carry the -33 duties for `hideAttendees` filtering, private
  events, the `updated` rule, floating-time query and sort, parse
  `method`, availability and notifications.
- **BREAKING** `jmap-peer-auth` and `jmap-did-types` move to
  `ed25519-dalek` 3 (from 2). `jmap_peer_auth::ed25519_dalek` re-exports
  the new major, so `SigningKey`, `VerifyingKey` and `Signature` values
  built with `ed25519-dalek` 2 no longer type-check; depend on
  `ed25519-dalek = "3"` (bd:JMAP-wf92.14). `reqwest` stays at 0.12; see
  `RELEASING.md` §Third-party majors in public APIs.
- **BREAKING** `jmap_mail_server::MailBackend::copy_email` and
  `jmap_contacts_server::ContactsBackend::copy_contact_card` are replaced
  by the generic `copy_object<O: SetObject>(caller, from_account_id,
  to_account_id, obj) -> (Id, O)`, matching `create_object`. `Email/copy`
  now reads the source Email itself, applies the overrides, assigns
  `threadId` in the destination and hands the record to `copy_object`; an
  unknown source id is `notFound` from the handler (bd:JMAP-a4zg).
- **BREAKING** `jmap_chat_server::ChatBackend::generate_invite_code` and
  `jmap_chat_federation_server::FederationBackend::new_message_id` take
  `caller` and `account_id` and are async and fallible
  (`impl Future<Output = Result<_, Self::Error>> + Send`). An `Err` fails
  that `SpaceInvite/set` create, or the `Peer/deliver` call, with
  `serverFail`. `MemoryBackend` returns `Err` instead of panicking when the
  OS CSPRNG is unavailable (bd:JMAP-x2gd.36.1).
- **BREAKING** `jmap_scene_server::SceneBackend::region_binding` takes
  the new `BindingProperty` enum (`ChatId`, `SpaceId`, `ChannelId`,
  `ActiveCallId`, with `as_str`) instead of a `&str` property name, and
  `MemoryBackend::set_region_binding` takes it too.
- **BREAKING** `jmap_tasks_types::jscalendar` and
  `jmap_contacts_types::jscontact` are modules re-exporting the same named
  sub-types as each crate's top level, no longer aliases of the whole
  `jmap-jscalendar-types` / `jmap-jscontact-types` crates. Paths such as
  `jmap_tasks_types::jscalendar::Location` keep working; names outside the
  list must be imported from the source crate (bd:JMAP-y6a8).
- **BREAKING** Capability URI constants are declared only in the types
  crates; server crates re-export them. jmap-mail-server,
  jmap-calendars-server and jmap-contacts-server drop their duplicate
  local `pub const` declarations, and
  `jmap_mail_server::JMAP_VACATION_RESPONSE_URI` is renamed
  `JMAP_VACATIONRESPONSE_URI` to match jmap-mail-types. Values are
  unchanged.
- **BREAKING** `#[non_exhaustive]` on `jmap_server::BackendQueryError`,
  `jmap_mime::JmapBodyFields`,
  `jmap_chat_filenode_server::{RootCheckError, TreeDestroyError}`,
  `jmap_chat_federation_server::DeliveryFailure`,
  `jmap_peer_auth::{IdentityBinding, SignatureHeaders}`,
  `jmap_peer_auth_io::PeerKeySet` and
  `jmap_chat_mimi_server::HubUnavailable`, so later fields and variants
  are not breaking. Matches need a wildcard arm; backends build
  `HubUnavailable` with its new `Default` impl.
- **BREAKING** `jmap_chat_federation_server::PEER_METHODS`,
  `jmap_chat_types::MANDATORY_DID_METHODS` and
  `jmap_scene_server::SCENE_DATA_TYPES` are `&[&str]`, not fixed-size
  arrays, so adding a value is not breaking.
- **BREAKING** **jmap-mail-server**: `MdnBackend::parse_mdns` takes
  `blob_ids: &[Id]`, not `Vec<Id>`, like the calendars parse methods.
- **BREAKING** **jmap-base-client**:
  `SubscribeEventsSessionParams::close_after` is `Option<CloseAfter>`
  (`State`, `No`), not `Option<&str>`: RFC 8620 §7.3 `closeafter` "MUST
  be one of" `"state"` or `"no"`.
- **BREAKING** **jmap-scene-server**: `Urgency` is now the shared
  `jmap_types::Urgency` (RFC 8030 §5.3) instead of a private copy, and
  `jmap_web_push::Urgency` re-exports the same type, so a
  `push_urgency` result goes straight into `PushOptions::urgency`. The
  shared type also derives `PartialOrd` and `Ord` (increasing urgency).
- **BREAKING** **jmap-graph-types**: `GraphEditStreamEnable::data_types`
  is `Vec<GraphEditDataType>` (`Ops`, `Cursors`, `Other(String)`), not
  `Vec<String>`, and `GraphEditStreamEnable::new` takes that type
  (draft-atwood-jmap-graph-edit-00 §GraphEditStreamEnable, jmap-chat-spec
  29e9a41 L301-307). Unrecognized values still decode, as `Other`, since
  they "are ignored" rather than rejected.
- **BREAKING** **jmap-scene-types**: `SceneStreamEnable::event_types` is
  `Option<Vec<SceneEventType>>` (`Avatar`, `Object`, `Interaction`,
  `Other(String)`), not `Option<Vec<String>>`, matching
  `VtcStreamEnable::event_types`. Unrecognized values still decode, as
  `Other`, since they "MUST be silently ignored"
  (draft-atwood-jmap-scene-wss-00 §SceneStreamEnable, jmap-chat-spec
  29e9a41 L444-447, L559-561).
- **BREAKING** **jmap-vtc-types**: `VtcStreamEnable::event_types` is
  `Option<Vec<VtcEventType>>`, one variant per `VTC_EVENT_TYPES` value
  plus `Other(String)` for the unrecognized values the server "MUST
  silently ignore" (draft-atwood-jmap-vtc-wss-00 §VTCStreamEnable,
  jmap-chat-spec 29e9a41 L221).
- **BREAKING** **jmap-types**: `GetResponse::not_found` is `Vec<Id>`, not
  `Option<Vec<Id>>` (RFC 8620 §5.1 `notFound: Id[]`). A `null` or absent
  `notFound` still decodes, as empty; it always encodes as an array.
- **BREAKING** **jmap-chat-types**: `Message::body_type` and
  `MessageRevision::body_type` are `BodyType`, not `String`
  (draft-atwood-jmap-chat-00 §Message, §MessageRevision, jmap-chat-spec
  29e9a41 L698, L465). `Message::new` takes `impl Into<BodyType>`, so
  `&str` and `String` arguments still compile. `BodyType` now also
  derives `Eq` and `Hash`.
- **BREAKING** **jmap-chat-federation-types**: `PeerMessage::body_type`
  and `PeerEdit::body_type` are `BodyType`, not `String`, matching
  `Message::body_type` (draft-atwood-jmap-chat-federation-00
  §Peer/deliver, jmap-chat-spec 29e9a41 L362, L382). `PeerMessage::new`
  and `PeerEdit::new` take `impl Into<BodyType>`, so `&str` and `String`
  arguments still compile.
- **BREAKING** **jmap-calendars-types**: `BusyPeriod::busy_status` is the
  new closed `BusyStatus` enum (`Confirmed`, `Tentative`, `Unavailable`),
  not `Option<String>`. An absent `busyStatus` decodes as `Unavailable`
  and any other string is rejected: "This MUST be one of" the three
  values, default `"unavailable"` (draft-ietf-jmap-calendars-33 §2.2,
  L512).
- **BREAKING** **jmap-mail-types**: `MailboxFilterCondition::parent_id`
  is `Option<Option<Id>>` (was `Option<serde_json::Value>`) and
  `MailboxFilterCondition::role` is `Option<Option<MailboxRole>>` (was
  `Option<String>`): `None` is absent, `Some(None)` is `null`. A filter
  can now send `"role": null` (RFC 8621 §2.3 `role: String|null`).
  **jmap-mail-server**'s `MemoryBackend` matches `role: null` against
  role-less mailboxes; it previously treated `role: null` as absent.
- **BREAKING** **jmap-chat-server**, **jmap-chat-federation-server**,
  **jmap-chat-filenode-server**, **jmap-chat-vtc-server**: the implicit
  `@everyone` role's permissions are read per Space through one seam,
  the new required `ChatBackend::everyone_permissions(caller,
  account_id, space)`, async and returning `Result<HashSet<ChatPermission>,
  Self::Error>` (draft-atwood-jmap-chat-00 §SpaceRole, §Space Permission
  Resolution step 1, jmap-chat-spec 29e9a41 L812, L1705).
  `FederationBackend::everyone_permissions` is removed; implement the
  `ChatBackend` method instead, and a backend error in `Peer/deliver`
  step 5 is `serverFail`. The `everyone: &HashSet<ChatPermission>`
  parameter is removed from `check_call_create`, `check_root_create`,
  `resolve_filenode_attachment`, `space_files_rights`,
  `check_space_create`, `check_space_update` and `check_space_destroy`,
  which now ask the Chat backend for the floor of the Space they check.
  `MemoryBackend` implements it with the set from
  `set_everyone_permissions` for every Space.
- **BREAKING** **jmap-chat-client**: `message_query` and
  `Batch::message_query_get` with `sort: None` omit `sort` instead of
  sending `receivedAt` descending, so the server applies the draft
  default: `receivedAt` ascending when `chatId` is present, descending
  when absent (draft-atwood-jmap-chat-00 §Message/query, jmap-chat-spec
  29e9a41 L1295). This matches `message_query_changes`. Callers that
  relied on newest-first must pass a descending `receivedAt` comparator
  (bd JMAP-v4g4).
- **BREAKING** **jmap-chat-client**: `ChatWsFrame` takes the
  `VtcWsFrame` / `SceneWsFrame` / `GraphEditWsFrame` shape:
  `ChatTyping`, `ChatPresence` and `Other(WsFrame)`. The `StateChange`,
  `Response` and `Unknown { type_name }` variants are removed; those
  frames arrive in `Other` unchanged. A `RequestError` (the failure
  report for `ChatStreamEnable`, draft-atwood-jmap-chat-wss-00 L192)
  now reaches the caller as `Other(WsFrame::RequestError)` with its
  `requestId` and Problem Details, instead of `Unknown { type_name:
  "<unknown>" }`, and a malformed Chat event keeps its raw JSON in
  `Other(WsFrame::Unknown)` (bd JMAP-wf92.12, JMAP-yt1w.44).
- **BREAKING** Every typed `/query` comparator `property` is a typed enum
  with an `Other(String)` catch-all, the `jmap-mail-types`
  `ComparatorProperty` shape (bd JMAP-wf92.11). Comparators that are
  still untyped `serde_json::Value` are out of scope: Principal
  (jmap-sharing-types); bd JMAP-wf92.25 typed the others, and bd
  JMAP-w9ox.1 removed the Calendar, ParticipantIdentity and TaskList
  `QueryObject` impls. **jmap-calendars-types**:
  `CalendarEventComparatorProperty` (draft-ietf-jmap-calendars-26
  §5.11.2). **jmap-tasks-types**: `TaskComparatorProperty`
  (draft-ietf-jmap-tasks-06 §4.13) and `TaskNotificationComparatorProperty`
  (§5.5.2). **jmap-contacts-types**: `ContactCardComparatorProperty`
  (RFC 9610 §3.3.2); the `card::prop` consts and the `comparator_prop`
  re-export are removed. These four comparators lose `Default` and gain
  `new(property)`. **jmap-chat-types**, **jmap-graph-types**,
  **jmap-scene-types**, **jmap-vtc-types**: the per-object sort property
  enums gain `Other(String)` and are no longer `Copy`; an unlisted name
  now decodes to `Other` instead of failing. **jmap-chat-server**,
  **jmap-graph-server**, **jmap-scene-server**, **jmap-vtc-server** still
  answer an unlisted sort property with `unsupportedSort` (RFC 8620
  §5.5), as does GraphNodeOp/query for any property other than `seq`.
- **BREAKING** **jmap-mail-client**: `mailbox_set`, `identity_set` and
  `vacation_response_set` take `if_in_state: Option<&State>`, sent as
  the RFC 8620 §5.3 `ifInState` optimistic-concurrency guard, matching
  `email_set` and `email_submission_set`. It follows `destroy` (or
  `update`) and precedes `params`. Pass `None` for the old behaviour.
- **BREAKING** **jmap-tasks-client** (`task_set`, `task_list_set`,
  `task_notification_set`), **jmap-contacts-client** (`contact_card_set`,
  `address_book_set`), **jmap-filenode-client** (`filenode_set`) and
  **jmap-sharing-client** (`principal_set`, `share_notification_set`):
  the same `if_in_state: Option<&State>` argument, placed as on
  jmap-mail-client.
- **BREAKING** **jmap-vtc-client**, **jmap-scene-client** and
  **jmap-graph-client**: every `*_set` method (including
  `graph_node_op_set`, a standard `/set` per draft-atwood-jmap-graph-edit-00
  L206) takes a trailing `if_in_state: Option<&State>`.
- **BREAKING** **jmap-chat-client**: every per-operation `/set` method
  (`chat_*`, `message_*`, `space_*`, `space_ban_*`, `space_invite_*`,
  `chat_contact_*`, `custom_emoji_*` `create` / `update` / `destroy`,
  `read_position_update`, `presence_status_update` and
  `push_subscription_*`) and **jmap-chat-mimi-client**
  `key_package_set` take a trailing `if_in_state: Option<&State>`.
- **BREAKING** **jmap-base-client** and every extension client: each
  `*_query` method (and chat `Batch::chat_query_get` /
  `message_query_get`) takes `params: &QueryParams` in place of the
  `position` / `limit` (and, on `chat_query` / `message_query`,
  `anchor` / `anchor_offset` / `calculate_total`) arguments.
  `jmap_base_client::QueryParams` carries the RFC 8620 §5.5 window
  arguments `position` (now `i64`: the RFC type is `Int`, negative
  values count from the end), `anchor`, `anchorOffset`, `limit` and
  `calculateTotal`, plus an `extra` map for vendor arguments, so every
  `/query` method can page by anchor and request a total. Type-specific
  arguments (`collapseThreads`, `expandRecurrences`, `depth`) stay
  separate parameters. Migrate `x_query(f, s, Some(0), Some(50))` to
  `x_query(f, s, &QueryParams { position: Some(0), limit: Some(50),
  ..Default::default() })`.
- **BREAKING** **jmap-base-client**: `ClientError::MethodError` is now
  the tuple variant `MethodError(jmap_types::JmapError)`, replacing
  `MethodError { error_type: String, description }`. The typed
  `MethodErrorType` and the `existingId` and `limit` properties (such as
  a `tooManyChanges` limit) are no longer dropped. An `"error"` response
  without a string `type` is now `ClientError::Parse` instead of an
  invented `"serverError"` type (RFC 8620 §3.6.2: it "MUST have a type
  property"), and so is one whose `type`, `description`, `existingId` or
  `limit` is malformed (such as a string or negative `limit`). Migrate `MethodError { error_type, .. }` matches to
  `MethodError(e)` and read `e.error_type`.
- **BREAKING** **jmap-vtc-server**: `VtcBackend` gains the required
  `update_object_if`, a compare-and-set `update_object` that applies a
  patch only while the stored `state` is one of `expected`, checked
  atomically with the write, and returns `Ok(Err(state))` otherwise. The
  new `StatefulObject` trait (`VtcCall`, `VtcRecording`,
  `VtcLivestream`) gives the compared state. Every handler `state` change
  now goes through it. The VTCCall end and the participant-driven
  activations are covered, and so are the recording pause, resume and
  stop and the livestream stop. Before this, a check against an earlier
  read let two concurrent updates both pass (draft-atwood-jmap-vtc-00,
  jmap-chat-spec 29e9a41: L506 "The server MUST NOT transition a call
  out of `"ended"`", L1165, L1266). Implement it in each backend; the
  `MemoryBackend` one is the reference.
- **BREAKING** **jmap-vtc-server**: `end_call` takes `from`, the call
  states its `end_reason` applies to, and ends the call only while the
  stored state is one of them (for example `&[CallState::Active]` for
  `CallEndReason::Timeout`). Before this it compared against its own
  fresh read, so a reason chosen from an earlier read could land on a
  call that had moved on. `expire_ring` now ends only a still-ringing
  call, and an answered one continues (draft-atwood-jmap-vtc-00,
  jmap-chat-spec 29e9a41 L1615 "If the call is already `"active"` when
  the period expires, the call continues"). The all-declined end applies
  only while the call is still `"ringing"` (L930). `after_leave` ends a
  call answered since its read `"completed"`, not `"cancelled"` (L966).
- **BREAKING** **jmap-scene-vtc-server**, **jmap-chat-vtc-server**:
  `clear_active_call_id` and `sync_active_call_id` must also run after a
  `VtcBackend::update_object_if::<VtcCall>` write that applied, since
  every VTCCall end now arrives there.
- **BREAKING** **jmap-peer-auth**: `SignedRequest::verify` drops its
  `seen` replay closure and runs checks 2, 3, 4 and 6 only. A sync lookup
  followed by a separate record let two concurrent requests with the same
  nonce both pass check 5 ("The receiver has not accepted a request
  carrying the same peer domain and `nonce` before",
  draft-atwood-jmap-chat-federation-00 §Request Signing, jmap-chat-spec
  29e9a41 L212). Check 5 is now the caller's atomic test-and-insert of the
  returned `Accepted` pair, as `jmap_peer_auth_io::authenticate` does with
  `ReplayStore::insert`. Migrate `verify(keys, now, seen)` to
  `verify(keys, now)` plus that insert.
- **BREAKING** **jmap-chat-server**: `ChatBackend::attachment_blob` has
  no default. The removed default returned `Ok(Ok(None))`, so a backend
  that did not override it never checked an Attachment `size`, which
  "Servers MUST verify against actual content" (draft-atwood-jmap-chat-00
  §Attachment, jmap-chat-spec 29e9a41 L368). A backend without a blob
  store returns `Ok(Ok(None))` explicitly.
- **jmap-chat-server**: the `ChatBackend::is_contact_blocked` and
  `validate_pinned_message_ids` defaults read the stored ChatContact and
  Messages through `get_objects` instead of returning "not blocked" and
  "valid". Events from a blocked contact are now dropped (§Blocked
  Contacts, L1780), and a `Chat/set` adding a `pinnedMessageIds` entry
  that is not a Message in that Chat is rejected with
  `invalidProperties` (§Chat, L632). The `MemoryBackend` overrides are
  removed in favour of the defaults.
- **BREAKING** **jmap-graph-server**: `GraphBackend::edit_log` has no
  default. The removed default returned an empty, uncompacted log, so an
  edit-capable backend that did not override it rejected every non-zero
  `baseSeq` and `snapshotSeq` and never raised `opsCompacted`, a MUST of
  draft-atwood-jmap-graph-edit-00 §GraphNodeOp/query. Backends without
  edit support return `Ok(EditLog::default())` explicitly.
- **BREAKING** **jmap-mail-server**: `MailBackend::create_object` takes
  a builder `build: F` with `F: FnOnce(&Id) -> O + Send` in place of a
  prebuilt object whose `id` was the sentinel `"placeholder"`. The
  backend mints the id, calls `build(&id)` and stores the result in the
  same atomic write; it no longer rewrites `id`. `MemoryBackend` mints
  `"singleton"` for `VacationResponse` (RFC 8621 §8) and a fresh UUID
  otherwise, and no longer keeps a caller-supplied id. Email `blobId`
  and `size` remain backend-assigned on `Email/set` create.
- **BREAKING** **jmap-chat-server**: `ChatBackend::create_object` takes
  the same builder. The `senderMsgId` sentinel is gone: `Message/set`
  builds an owner-composed Message with `senderMsgId` equal to the
  minted id (draft-atwood-jmap-chat-00 §Message `senderMsgId`), and the
  backend stores `senderMsgId` as built, so a server-originated Message
  whose `senderMsgId` is literally `"placeholder"` is no longer
  rewritten. `store_server_message(backend, caller, account_id, chat_id,
  sender_msg_id, build)` takes the Chat id, the `senderMsgId` and a
  builder in place of a prebuilt `Message`. **jmap-chat-federation-server**
  `Peer/deliver` and placeholder ChatContact creation use the builders.
- **BREAKING** **jmap-tasks-server**: `TasksBackend::create_object`
  takes the same builder; the `"placeholder"` id sentinel is gone.
  `Task/set` and `Task/copy` parse the create without an id and the
  builder sets it. **jmap-chat-tasks-server** `create_space_task_list`
  builds its TaskList in the builder.
- **BREAKING** **jmap-contacts-server**: `ContactsBackend::create_object`
  takes the same builder; the `"placeholder"` id sentinel is gone.
- **BREAKING** **jmap-calendars-server**: `CalendarsBackend::create_object`
  and `create_calendar_event` take the same builder (`build: F` in place
  of `event`); the `"placeholder"` id sentinel is gone.
  **jmap-chat-calendars-server** `create_space_calendar` sets the id in
  the builder.
- **BREAKING** **jmap-sharing-server**: `SharingBackend::create_object`
  takes the same builder; the `"placeholder"` id sentinel is gone, and
  the returned object carrying the minted id holds by construction.
- **BREAKING** **jmap-filenode-server**: `FileNodeBackend::create_object`
  takes the same builder; the `"placeholder"` id sentinel of
  `FileNode/set` and `FileNode/copy` is gone.
  **jmap-chat-filenode-server** `create_space_file_tree` builds the root
  for the minted id (it previously passed the Space id for the backend
  to replace).
- **BREAKING** **jmap-graph-server**: `GraphBackend::create_object`
  takes the same builder; the `"placeholder"` id sentinel is gone. The
  GraphNodeOp `seq` and Graph `myRights` the backend fills are
  unchanged.
- **BREAKING** **jmap-scene-server**: `SceneBackend::create_object`
  takes the same builder; the `"placeholder"` id sentinel is gone. The
  SceneObject `ownerId` the backend fills is unchanged.
- **BREAKING** **jmap-vtc-server**: `VtcBackend::create_object` and
  `create_call` take the same builder (`build: F` in place of `call`);
  the `"placeholder"` id sentinel is gone. The `joinUri` the backend
  fills is unchanged.
- **BREAKING** **jmap-server** and every extension server backend:
  `JmapBackend::query_objects` takes `params: QueryParams<'_, O>` in place
  of its six positional `/query` arguments (`filter`, `sort`, `limit`,
  `position`, `anchor`, `anchor_offset`), and `query_changes` takes
  `params: QueryChangesParams<'_, O>` in place of `since_query_state`,
  `filter`, `sort`, `max_changes`, `up_to_id` and `collapse_threads`.
  Both structs are `#[non_exhaustive]` with public fields, so a new RFC
  argument becomes a field instead of a break to every backend. Backends
  destructure them with `..`. Callers build them with
  `QueryParams::new()` (the RFC 8620 §5.5 defaults) or
  `QueryChangesParams::new(since_query_state)` plus setters. Both are
  re-exported from `jmap_server` and from each extension server's
  `backend` module.
- **BREAKING** **jmap-calendars-server**:
  `CalendarsBackend::query_calendar_events` takes
  `params: QueryParams<'_, CalendarEvent>` in place of the same six
  positional `/query` arguments; `args: &CalendarEventQueryArgs` still
  carries the §5.11 `expandRecurrences` / `timeZone` extras. The default
  impl forwards `params` to `query_objects` unchanged.
- **BREAKING** **jmap-server**: `JmapHandler::call` takes one
  `#[non_exhaustive]` `CallContext<CallerCtx>` (`method`, `call_id`,
  `args`, `caller`) instead of four positional arguments, and
  `ClosureHandler` closures (`BackendCallFn`) take
  `(Arc<B>, CallContext<C>)` instead of `(Arc<B>, call_id, args, caller)`.
  Closures now see the method name, which `ClosureHandler` used to drop.
  More per-call context can be added later without another break. Code
  that calls a handler directly builds the context with
  `CallContext::new`. The `register_*_handlers` functions in every
  extension server are unchanged for callers.
- **BREAKING** **jmap-types**, **jmap-server**: one `SetError` type for
  servers and clients. `SetErrorType` and the builder-style `SetError`
  (`with_description`, `with_properties`, `with_extra`,
  `validate_extras`, ...), `ReservedExtrasKey` and
  `RESERVED_SET_ERROR_WIRE_NAMES` moved from `jmap_server::backend` to
  `jmap_types` (`jmap_types::set_error`); `jmap_server` re-exports them.
  `jmap_types::SetError::error_type` is now `SetErrorType` instead of
  `String`, so clients match `SetErrorType::RateLimit` instead of
  comparing `"rateLimit"`. `SetError::new` takes `impl Into<SetErrorType>`.
  `SetErrorType::Custom(String)` is renamed `Other(String)`, and
  `SetErrorType::custom(s)` is replaced by `SetErrorType::from(s)`, which
  still builds the typed variant for a known wire string. The 13 RFC 8621
  mail variants stay typed in the shared enum (bd:JMAP-wlip.35).
- **BREAKING** **jmap-types**: `JmapError::error_type` is now
  `MethodErrorType` instead of `String`. Its known variants are the RFC
  8620 §3.6.1 request-level, §3.6.2 method-level and §5.1-§5.6
  method-specific error types; anything else, including extension types,
  is `MethodErrorType::Other`. `JmapError::custom` takes
  `impl Into<MethodErrorType>`. Constructors for types outside that set,
  such as `not_found` and `too_large`, build `Other`.
- **BREAKING** **jmap-types**, **jmap-server**: the request-level `limit`
  error's limit name moved from `JmapError::description` to the new
  `JmapError::limit_name` field (RFC 8620 §3.6.1: "the "limit" property
  MUST contain the name of the limit being applied"). `RequestError`
  writes it as the problem-details `limit` property and now also sends a
  `description` as `detail`. `limit_name` is not serialized into
  `methodResponses`.
- **jmap-types**: `impl_string_enum!` also emits `From<&str>` and
  `From<String>`, using the same mapping as `Deserialize`, so a known wire
  string always builds its typed variant.
- **BREAKING** **jmap-tasks-server**, **jmap-calendars-server**:
  `TasksBackend::update_task_per_user` and
  `CalendarsBackend::update_per_user_properties` no longer have default
  bodies. The old default delegated to `update_object`, which is wrong
  for a multi-user backend: draft-ietf-jmap-tasks-06 §4.5.1 and
  draft-ietf-jmap-calendars-26 §5.4 store `updated` per user on a
  per-user-only write. A single-user backend can still delegate to
  `update_object` explicitly, as `MemoryBackend` does.
- **jmap-chat-server**: `ChatBackend::expire_message` keeps its no-op
  default; its rustdoc now says backends whose messages can expire MUST
  override it.
- **jmap-peer-auth-io**: `PeerKeys` treats a fetched key set as fresh
  for at least 60 seconds (`REFETCH_INTERVAL`), so a known `kid` keeps
  verifying until the refetch interval ends even when the peer's
  `/.well-known/jmap` is `no-store` or has no `Cache-Control`
  (draft-atwood-jmap-chat-federation-00 L225 at jmap-chat-spec 29e9a41).
  Previously such a peer was usable on one request per minute. A DNS
  TTL under 60 seconds still ends the entry early.
- **BREAKING** **jmap-chat-server**: `resolve_channel_permissions` takes
  the Space id after `space_permissions`. A ChannelPermission with
  `targetType: "role"` whose `targetId` is that Space id addresses the
  implicit `@everyone` role and applies to every member in steps 2 and 3
  (draft-atwood-jmap-chat-00 §ChannelPermission `targetId`, §Space
  Permission Resolution step 2). Previously no record could address
  `@everyone`.
- **BREAKING** **jmap-chat-server**: the reference `MemoryBackend` gives
  the implicit `@everyone` role `view` and `send` in every Space by
  default, so a member with no SpaceRole can read and post in channels;
  `MemoryBackend::set_everyone_permissions` changes the set. Previously
  the floor was empty.
- **BREAKING** **jmap-calendars-server**:
  `CalendarsBackend::query_calendar_events` takes `anchor: Option<&Id>`
  and `anchor_offset: i64` after `position`. `CalendarEvent/query` now
  parses `anchor` / `anchorOffset` (RFC 8620 §5.5) and forwards them; the
  default impl passes them to `query_objects`, so an anchor absent from the
  results is `anchorNotFound`. Previously both were silently ignored.
- **BREAKING** **jmap-chat-types**, **jmap-chat-server**:
  `SpacePatchOp::AddCategory` now carries the new `CategoryCreate`
  (`name`, optional `position`, `channelIds` defaulting to empty) instead of
  `Category`, matching draft §Space/set `addCategories` ("optional
  `position` (UnsignedInt) and `channelIds` (String[])"). Backends no longer
  receive a placeholder `id`.
- **BREAKING** **jmap-chat-types**, **jmap-chat-server**:
  `SpacePatchOp::AddRole` now carries the new `RoleCreate` (`name`,
  `permissions`, `position`, optional `color`) instead of `SpaceRole`,
  matching draft §Space/set `addRoles` ("Server assigns ULIDs"). Backends
  no longer receive a placeholder `id`; they mint the role id (bd
  JMAP-liqs).

- **BREAKING** **jmap-tasks-server**: `memory::MemoryError` is now
  `#[non_exhaustive]` and matches the canonical `jmap-mail-server` shape:
  `MemoryError::message()` is renamed `description()`, and `Display`
  prints `MemoryBackend error: <description>` like its siblings.
- **BREAKING** **jmap-vtc-server**: new required
  `VtcBackend::get_objects_unscoped`, a `get_objects` read without
  §Per-call access. Ending a call reads its breakout rooms and the
  recordings and livestreams of each ended call through it, so a room the
  ending moderator (or the server context of `end_call`) cannot access is
  still ended, with its recordings and livestreams stopped
  (jmap-vtc-implementer-guide §Breakout room cleanup). Before, such a room
  stayed active after its parent ended.
- **BREAKING** **jmap-scene-server**: `SceneBackend::is_user_blocked`
  drops its `account_id` parameter. It now answers whether `user_id` is
  blocked on the caller's own contact list, which the implementation
  resolves from the caller. On a shared region the request's `accountId`
  is the hosting account, holding the host's contacts rather than the
  requester's, and the draft filters on "the requesting user's contact
  list" (draft-atwood-jmap-scene-00 §Blocked-Contact Spatial Presence).
  `should_deliver` already passes the recipient as the caller, so
  `recipient_account_id` now feeds only the Chat capability check. This
  replaces the `is_user_blocked` account argument described in the
  `should_deliver` entry below.
- **BREAKING** **jmap-vtc-server**: `expire_ring` drops its `timeout`
  parameter and reads the period from the new `VtcBackend::ring_timeout`
  (default `RING_TIMEOUT`, 30 seconds). `VTCParticipant/set` now rejects a
  still-`"pending"` target's answer (`joinMethod`) or decline
  (`callResponse`) once `createdAt` plus that period has passed, with
  `invalidProperties` naming the patched property:
  draft-atwood-jmap-vtc-00 §Multi-Device Forking, each target "may still
  answer (and join) or decline until the ring timeout expires".
- **jmap-scene-types**, **jmap-scene-server**, **jmap-scene-client**,
  **jmap-chat-scene-server**, **jmap-scene-vtc-server**, **jmap-vtc-types**,
  **jmap-vtc-server**, **jmap-vtc-client**, **jmap-chat-vtc-server**,
  **jmap-graph-types**, **jmap-graph-server**, **jmap-graph-client** and
  **jmap-graph-projection-server** are publishable: `publish = false` and
  the README draft notes are gone, and `RELEASING.md` places them in
  layers 2a, 6, 7 and 8 of the publish order.

- **BREAKING** **jmap-chat-server**, **jmap-chat-federation-server**,
  **jmap-chat-vtc-server**, **jmap-chat-filenode-server**: Space
  permissions are typed as `jmap_chat_types::ChatPermission` instead of
  wire strings. The `&str` constants `MANAGE_SPACE`, `MANAGE_ROLES`,
  `MANAGE_MEMBERS`, `MANAGE_CHANNELS`, `PIN`, `BAN`, `MENTION_BROADCAST`,
  `VIEW` and `SEND` (`jmap_chat_server::permissions` and the crate root)
  and `jmap_chat_vtc_server::START_CALL` are removed; use the matching
  `ChatPermission` variant. `RequiredPermissions::Known` carries
  `&'static [ChatPermission]`; `resolve_channel_permissions` takes and
  returns `HashSet<ChatPermission>`; `FederationBackend::everyone_permissions`
  returns `HashSet<ChatPermission>`; and the `everyone` parameter of
  `check_call_create`, `check_root_create`, `resolve_filenode_attachment`
  and the other filenode gates is `&HashSet<ChatPermission>`. A channel
  `deny` entry removes every permission with its wire string, so a
  `ChatPermission::Other` spelling of a known name can deny but never
  grant it.

- **BREAKING** **jmap-chat-types**: broadcast scopes are typed as the new
  `BroadcastScope` enum (`Everyone`, `Here`, `Admins`, `Other(String)`):
  `BroadcastMention.scope: BroadcastScope`, `Span.scope:
  Option<BroadcastScope>`, `ChatMessageEntry.mention_scopes:
  Option<Vec<BroadcastScope>>`, and `BroadcastMention::new` and
  `ChatMessageEntry::plaintext` take `BroadcastScope` instead of strings.
  Unknown scopes still round-trip through `Other`. The spec closes the set
  (draft-atwood-jmap-chat-push-00 L188: "Each element is one of
  `"everyone"`, `"here"`, or `"admins"`").
- **BREAKING** **jmap-chat-server**: `ChatBackend::may_broadcast_mention`
  takes a `chat_id: &Id` after `space_id`, so implementations can apply
  the target channel's `permissionOverrides`. `MemoryBackend` now resolves
  `mention_broadcast` and the channel `pin` gate of
  `may_set_pinned_messages` per draft-atwood-jmap-chat-00 §Space
  Permission Resolution ("When determining whether a member may perform
  an action in a channel Chat, servers MUST evaluate permissions in this
  order"); before, a channel override denying or allowing either
  permission was ignored.

- **BREAKING** **jmap-chat-server**: Space/set last-administrator
  protection now covers `updateMembers` role changes, `removeRoles` and
  `updateRoles` permission changes as well as `removeMembers`, and the
  `invalidProperties` refusal names the patch key(s) concerned (was always
  `removeMembers`), per draft-atwood-jmap-chat-00 §Space/set
  `removeMembers`: "a refusal names the patch key concerned". It fires only
  when the patch takes the Space from at least one `manage_members` or
  `manage_space` holder to none. The `ChatBackend::apply_space_patch`
  contract table now records the role-position hierarchy (a SHOULD in the
  draft) for `addMembers` and `updateMembers` as well as `addRoles` and
  `updateRoles`; `MemoryBackend` enforces it for all four.

- **BREAKING** **jmap-scene-server**: `SceneAsset/set` create whose
  `blobId` names no uploaded blob now fails with `invalidProperties`
  naming `blobId` (was `notFound`), per draft-atwood-jmap-scene-00
  §SceneAsset/set Create Errors `invalidProperties`: "`blobId` does not
  reference a valid blob ... This applies whether the blobId is malformed
  or well-formed but absent."

- **BREAKING** **jmap-scene-server**: `SceneObject/set` create or update
  whose `visualRef` names no uploaded blob now fails with
  `invalidProperties` naming `visualRef` (was `notFound`), per
  draft-atwood-jmap-scene-00 §SceneObject/set Create Errors
  `invalidProperties`: "`visualRef` does not reference a valid blob", and
  Update Errors: "Same field-level validations as create".

- **BREAKING** **jmap-scene-server**: `SceneObject/set` create or update
  whose `parentId` names no existing SceneObject now fails with
  `invalidProperties` naming `parentId` (was `notFound`), per
  draft-atwood-jmap-scene-00 §SceneObject/set Create Errors
  `invalidProperties`: "`parentId` does not reference an existing
  SceneObject", and Update Errors: "Same field-level validations as
  create".

- **BREAKING** **jmap-scene-server**: `SceneObject/set` create naming a
  missing region, or one the caller cannot access, now fails with
  `invalidProperties` naming `regionId` (was `notFound`), per
  draft-atwood-jmap-scene-00 §SceneObject/set: "The server MUST return
  `invalidProperties` naming `regionId` when `regionId` does not exist,
  and SHOULD return the same error (not `forbidden`) when the caller does
  not have access to the region".

- **BREAKING** **jmap-scene-server**: `SceneAvatar/set` create now refuses a
  client-supplied `position` or `orientation` with `invalidProperties`
  and always sets them from the region's `spawnPosition` and
  `spawnOrientation` (draft-atwood-jmap-scene-00 §Entering a Region: "The
  server sets ... `position` (to the region's `spawnPosition`),
  `orientation` (to the region's `spawnOrientation`)"; the
  teleport-on-entry exception was removed). Previously a supplied value
  was kept.

- **BREAKING** **jmap-scene-server**: `SceneRegion/set` names `spaceId`,
  not `accessPolicy`, in `invalidProperties` when `accessPolicy` is
  `"space"` and `spaceId` is null (draft-atwood-jmap-scene-00 §Updating a
  Region: "an update that sets `spaceId` to `null` while `accessPolicy` is
  `"space"` fails with `invalidProperties` naming `spaceId`"). A missing
  `urn:ietf:params:jmap:chat` capability still names `accessPolicy`.

- **jmap-scene-server**: `MemoryBackend::may_destroy_region` now lets the
  users registered with `add_admin` destroy a region they do not own, as
  `may_update_region` already did, per draft-atwood-jmap-scene-00
  §SceneRegion/set ("SHOULD return `forbidden` when the caller is not the
  region owner and does not have deployment-defined administrative
  privileges"). The `forbidden` description now reads "only the region
  owner or an admin may destroy this region".
- **BREAKING** **jmap-scene-server**: a `SceneAvatar/set` create whose
  `regionId` names no SceneRegion, or a region the caller has no access
  to under `SceneBackend::may_access_region`, is now `invalidProperties`
  naming `regionId` (was `notFound` and `forbidden`), per
  draft-atwood-jmap-scene-00 §SceneAvatar/set ("The server MUST return
  `invalidProperties` naming `regionId` ... SHOULD return the same error
  when the caller has no access to the region under its
  `accessPolicy`"). `SceneBackend::may_enter_region` now covers only a
  ban or eject cooldown (`forbidden`); the `memory` backend moves its
  `"invite"` rule to `may_access_region` alone and gains
  `ban_from_region`.

- **BREAKING** **jmap-scene-server**: `SceneAvatar/set` stores the
  server's current time as `leftAt` when an update sets it to a non-null
  value, and echoes it in `updated`, instead of the client-supplied value
  (draft-atwood-jmap-scene-00 §Leaving a Region: "The server records its
  own current time as `leftAt`"). An eject by a region owner or
  administrator may patch only `leftAt`; one that also patches
  `worldState` is `forbidden` ("An eject update patches only `leftAt`").
- **BREAKING** **jmap-scene-server**: a departed user re-enters a region
  by updating their SceneAvatar to `leftAt: null`
  (draft-atwood-jmap-scene-00 §Re-entering a Region), not by a create. A
  create for a user with any record in the region, present or departed,
  is now `alreadyExists` with `existingId`. Re-entry sets `joinedAt`,
  `position` and `orientation` (the region's spawn point) and echoes them
  in `updated`; a `position` or `orientation` in the same patch is
  `invalidProperties`. Lost region access is `notFound`,
  `SceneBackend::may_enter_region` `false` is `forbidden`, and
  `maxAvatarsPerRegion` is `overQuota`; the user's present avatar in
  another region is ejected. `SceneBackend::update_object` implementors
  must apply the capacity, auto-eject and `activeAvatarCount` rules to a
  `SceneAvatar` patch that sets `leftAt` to `null`.

- **BREAKING** **jmap-scene-types**: `SceneAsset` gains `owner_id`
  (`ownerId`, required, "The userId of the user who created this asset")
  (draft-atwood-jmap-scene-00 §SceneAsset). An asset without `ownerId` no
  longer deserializes.

- **BREAKING** **jmap-scene-server**: `SceneAsset/set` create sets
  `ownerId` to `SceneBackend::caller_user_id` and echoes it in `created`;
  `ownerId` is refused on create. An update that patches `ownerId` or
  `mediaType`, now immutable, is `invalidProperties`.
  `MemoryBackend::may_modify_asset` takes the asset owner from `ownerId`,
  and the hidden test hook `MemoryBackend::insert_object_for_test` drops
  its `owner` argument.

- **BREAKING** **jmap-scene-types**: `SceneRegion` gains `owner_id`
  (`ownerId`, required) and `invited_user_ids` (`invitedUserIds`, default
  `[]`) (draft-atwood-jmap-scene-00 §SceneRegion). A region without
  `ownerId` no longer deserializes.

- **BREAKING** **jmap-scene-server**: `SceneRegion/set` create sets
  `ownerId` to `SceneBackend::caller_user_id` and echoes it in `created`;
  `ownerId` is refused on create and update like the other server-set
  properties, and an `invitedUserIds` that is not a String array is
  `invalidProperties`. `SceneRegion/get` returns `invitedUserIds` as `[]`
  to callers for whom `SceneBackend::may_update_region` is `false`.
  `MemoryBackend` takes the region owner from `ownerId` and admits users
  to an `"invite"` region by `invitedUserIds` and by an active avatar in
  it, besides `grant_region_access`.

- **BREAKING** **jmap-scene-server**: `validate_interaction` returns
  `Option<SceneInteractionEvent>`, the event to relay with `userId` set to
  the caller's, instead of `bool`. A frame whose `userId` names another
  user is no longer discarded: "The server sets `userId` on the relayed
  event to the authenticated user's userId; a `userId` supplied in the
  frame is ignored" (draft-atwood-jmap-scene-wss-00 §Client-to-Server
  Submission). **jmap-scene-types**: `SceneInteractionEvent` deserializes
  without `userId` (it defaults to empty).

- **BREAKING** **jmap-scene-types**, **jmap-scene-server**,
  **jmap-chat-scene-server**: `SceneAvatarEvent`, `SceneObjectEvent` and
  `SceneInteractionEvent` have a required `account_id`, the region's
  hosting account (draft-atwood-jmap-scene-wss-00 §Ephemeral Event Types:
  "Every event carries an `accountId` (String) property"), and
  `SceneInteractionEvent::new` takes it as its first argument. Frames
  without `accountId` no longer decode. `should_deliver`,
  `should_deliver_scene` and `validate_interaction` return `false` for an
  event whose `accountId` is not the account they are given, and
  `SceneObject/interact` sets it on the events it hands to
  `SceneBackend::accept_interaction`.
- **BREAKING** **jmap-chat-vtc-server**: `check_call_create` returns the
  `invalidProperties` SetError naming `chatId` when the owner is not a
  member of the bound Chat, the same error as for a Chat that does not
  resolve, instead of `forbidden` (draft-atwood-jmap-vtc-00 §VTCCall/set:
  "The server SHOULD return the same error when the caller is not a
  member of that Chat").
- **BREAKING** **jmap-scene-server**, **jmap-chat-scene-server**:
  `should_deliver` and `should_deliver_scene` take the region's hosting
  account (the event's `accountId`) and the recipient's own account as
  separate arguments. The region, avatars and object are read from the
  hosting account; the Chat capability check and
  `SceneBackend::is_user_blocked` use the recipient's account, since
  suppression applies when Chat "is available for the receiving account"
  and consults "the recipient's ChatContact records"
  (draft-atwood-jmap-scene-wss-00 §Blocked-Sender Suppression).
- **BREAKING** **jmap-vtc-server**, **jmap-chat-vtc-server**:
  `should_deliver` and `should_deliver_vtc` take the call's hosting
  account (the event's `accountId`) and the recipient's own account as
  separate arguments. The call and its participants are read from the
  hosting account, and `VtcBackend::is_user_blocked` is asked with the
  recipient's account, so a block is looked up on "the recipient's
  contact list" rather than the hosting account's
  (draft-atwood-jmap-vtc-wss-00 §Event Types and §Blocked-Sender
  Suppression).
- **BREAKING** **jmap-vtc-server**: `VTCCall/set` rejects an update of
  `e2eeEnabled` from `true` to `false`, including a `null` patch that
  resets it to its default `false`, with `invalidProperties` naming
  `e2eeEnabled` (draft-atwood-jmap-vtc-00 §VTCCall `e2eeEnabled`).
- **BREAKING** **jmap-vtc-server**: `VtcBackend` has a new required
  method, `active_call_count`, the account's number of `"active"` calls
  regardless of the caller's access. `VTCParticipant/set` uses it for
  `maxConcurrentCalls` instead of the caller-scoped `get_objects`, which
  under §Per-call access left out calls the caller cannot access and let
  the account exceed the limit.
- **BREAKING** **jmap-vtc-server**: the `VtcBackend` contract gains
  §Per-call access. `get_objects`, `query_objects` and `query_changes`
  MUST treat a call the caller cannot access, and that call's
  participants, recordings and livestreams, as absent. For a call with
  `e2eeEnabled: true` they MUST also hide its participants from a user
  with no record in it, so every VTC method answers `notFound`
  (§Access Control, §Participant Visibility). `MemoryBackend` implements
  this, and its `deny_call_access` makes a call inaccessible to a user.
- **BREAKING** **jmap-vtc-types**: all eight VTC ephemeral event structs
  (`VtcRingEvent`, `VtcCallEndEvent`, `VtcGatewaySignal`,
  `VtcParticipantEvent`, `VtcMediaStateEvent`, `VtcActiveSpeakerEvent`,
  `VtcUnmuteRequestEvent`, `VtcRecordingStateEvent`) have a required
  `account_id: Id`, the call's hosting account
  (draft-atwood-jmap-vtc-wss-00 §Event Types). A frame without
  `accountId` no longer decodes. **jmap-vtc-server** fills it with the
  hosting account on every event it builds.
- **BREAKING** **jmap-vtc-server**: `VtcBackend` has a new required
  method, `deliver_event`, for the `VTCUnmuteRequestEvent` and outbound
  `VTCGatewaySignal` that `VTCParticipant/set` produces. The
  `create_object` and `update_object` contracts now cover
  `VTCParticipant` (`alreadyExists` with `existingId` for a second record
  of a user, `activeParticipantCount` upkeep, `overQuota` above
  `maxParticipantsPerCall`, placing a dial-out call), and ending or
  destroying a breakout room also resets the parent call's participants'
  `breakoutCallId` naming it.
- **BREAKING** **jmap-vtc-types**: `VtcParticipant` has a typed
  `breakout_call_id: Option<Id>` (draft-atwood-jmap-vtc-00 §VTCParticipant
  `breakoutCallId`, "Default: `null`"), which always serializes, as
  `null` when unset; it no longer lands in `extra`.

- **BREAKING** (MSRV): the workspace minimum supported Rust version is
  now 1.91 (was 1.88) for every crate. `openmls` 0.9.1, which
  `jmap-chat-mimi-server` needs to parse and verify MLS KeyPackages
  (draft-atwood-jmap-chat-mimi-00 §KeyPackage/set), declares
  `rust-version = "1.91.0"`, as do `openmls_traits` 0.6.0 and
  `openmls_serialization_helpers` 0.1.0. The CI `msrv` job now runs on
  1.91.0.
- Docs: metadata citations across the workspace, including the
  `jmap-metadata-server` and `jmap-metadata-client` package
  descriptions, now name draft-ietf-jmap-metadata-04 and its renumbered
  sections (`/query` §3.6, `/queryChanges` §3.5). No API change.
- **BREAKING** **jmap-types**: `ChangesResponse::updated_properties` is
  now `Option<Option<Vec<String>>>`: `None` when absent, `Some(None)` for
  `null`, `Some(Some(names))` for a list. An explicit `null` used to
  decode as `None` and was dropped on re-encode. draft-ietf-jmap-metadata-04
  §3.3 says that for a request using the metadata capability the argument
  "is always present, and is null when updated is empty", so `Some(None)`
  now serializes as `"updatedProperties": null`. `None` is still omitted.
- **BREAKING** **jmap-metadata-server**: `metadata_text_contains` matches
  under the `i;unicode-casemap` collation (RFC 5051), per
  draft-ietf-jmap-metadata-04 §3.6, instead of comparing
  `str::to_lowercase` forms. Both strings are titlecased and then
  compatibility-decomposed, so `"\u{e9}t\u{e9}"` now contains
  `"E\u{301}T"` and fullwidth `"\u{ff21}"` contains `"a"`.
  `jmap-metadata-server` now depends on `icu_casemap` and
  `icu_normalizer`.
- **BREAKING** **jmap-metadata-types**: `is_registered_namespace`,
  `is_vendor_namespace` and `is_valid_namespace` reject uppercase ASCII
  letters, per draft-ietf-jmap-metadata-04 §2.1 (registered names are
  lowercase; domain names "MUST be in lowercase"). `"A1-b2_c3"` and
  `"Acme.example.com"` were accepted before and are now invalid;
  `jmap-metadata-server`'s `is_namespace_supported` inherits the check.
- **BREAKING** **jmap-metadata-server**: `value_depth` follows the
  recursive depth definition of draft-ietf-jmap-metadata-04 §2.1: a
  scalar or an array of scalars (including `[]`) now has depth 0 instead
  of 1. Object values keep their depth. A non-object value therefore
  never fails `exceeds_max_depth`; the caller rejects it, since -04 §2.1
  says a namespace value "MUST be a JSON object".
- **BREAKING** **jmap-metadata-server**: `metadata_path_exists`,
  `metadata_text_contains` and `metadata_text_equals` resolve a path of
  any depth, per draft-ietf-jmap-metadata-04 §3.6: `"ns/a/b"` now walks
  `ns` -> `a` -> `b` instead of looking up the single key `"a/b"`
  (spelt `"ns/a~1b"`). A path that would traverse an array or a scalar,
  or that has an invalid `~` escape, matches nothing.
- **BREAKING** **jmap-chat-types**: the stringly vocabulary fields are now
  `impl_string_enum!` enums whose `Other(String)` arm round-trips
  unrecognized values, so the wire format is unchanged.
  `SpaceRole.permissions`, `ChannelPermission.allow` and `.deny` are
  `Vec<ChatPermission>` and `RolePatch.permissions` is
  `Option<Vec<ChatPermission>>` (the ten names of draft-atwood-jmap-chat-00
  §SpaceRole). `ChannelPermission.target_type` is
  `ChannelPermissionTargetType` (`"role"`, `"member"`; §ChannelPermission).
  `ChatStreamEnable.data_types` is `Vec<EphemeralDataType>` (`"typing"`,
  `"presence"`, `"vtc"`, `"scene"`; draft-atwood-jmap-chat-wss-00
  §ChatStreamEnable). Each enum has `as_str()` for wire-string comparison.
  The `vocabulary` const slices stay as the wire-string lists.
- **jmap-chat-federation-server**: the `FederationBackend::fetch_attachment`
  and `fetch_avatar` contracts now require a GET signed per
  draft-atwood-jmap-chat-federation-00 §Request Signing that follows no
  cross-origin redirect (§Blob Fetch), and `fetch_attachment` documents the
  step 11 failure mapping: connection failure, timeout or 5xx to
  `serverFail`, 429 to `rateLimited`, 403 or 404 to `invalidArguments`.
- **BREAKING** **jmap-chat-federation-types**: `PeerGroupUpdateArgs.avatar_blob_id`
  is replaced by `avatar: Option<Clearable<PeerAvatar>>`, the wire
  `avatar` object (`contentType`, `size`, `sha256`, `fetchUrl`) of
  draft-atwood-jmap-chat-federation-00 §Peer/groupUpdate; `null` removes
  the group avatar and absent leaves it unchanged. A wire `avatarBlobId`
  now lands in `extra`.
- **BREAKING** **jmap-chat-federation-server**: `FederationBackend` gains
  the required `fetch_avatar`. `handle_peer_group_update` fetches an
  `avatar` object after the authorization and `closedAt` checks, checks
  `contentType`, `size` and `sha256`, and stores the local blob id as
  `avatarBlobId`; a failed avatar leaves the Chat's avatar as it was and
  never rejects the request. `avatar: null` on `"updateMetadata"` removes
  `avatarBlobId`. The sender's blob id is no longer stored verbatim.
- **BREAKING** **jmap-chat-federation-server**: `peer_group_update_args`
  takes an `avatar: Option<PeerAvatar>` argument, the object the
  deployment built (minting `fetchUrl` and recording the grant) for the
  chat's `avatarBlobId`. `"updateMetadata"` for a chat without an avatar
  sends `avatar: null`; a chat with one and no object given is the new
  `OutboundError::MissingAvatar`.
- **BREAKING** **jmap-chat-federation-types**: `owner_endpoints` removes
  every Endpoint whose `uri` is a JMAP object URI and returns `None`,
  not `Some(vec![])`, when no entry remains
  (draft-atwood-jmap-chat-federation-00 §User References in Peer Methods).
- **jmap-chat-federation-server**: `Peer/deliver` removes every
  MessageAction whose `uri` is a JMAP object URI before storing the
  message, keeping the order of the rest; a removed task action's
  `assigneeIds` are no longer validated (same section).

- **BREAKING** **jmap-chat-server**: `chat_state_change_for` takes
  `carried: &HashSet<Id>` in place of the `ChatPushCapability` map and
  drops `Message` only for accounts whose every Message change in the
  batch was carried by a delivered `ChatMessagePush`. It used to drop
  `Message` for every account with a live `chatPush` entry, so edits,
  destroys, the owner's own messages and filtered messages were never
  announced (draft-atwood-jmap-chat-push-00 §PushSubscription Extension:
  "Every other change in the account is still announced by a standard
  `StateChange` [...] this includes Message changes that the payload does
  not carry").
- **BREAKING** **jmap-chat-types**: `unread_count` moves from
  `ChatMessageEntry` to `ChatMessagePush`, which the spec defines it on
  (draft-atwood-jmap-chat-push-00 §ChatMessagePush: "It is a property of
  the push as a whole, not of any one entry"). A top-level wire
  `unreadCount` used to land in `ChatMessagePush.extra`; an entry-level one
  now lands in `ChatMessageEntry.extra`.
- **BREAKING** **jmap-peer-auth**, **jmap-peer-auth-io**: `Accepted` and
  `Authenticated` gain a `key: PeerSigningKey` field, the key whose `kid`
  the signature's `keyid` names and that verified the signature, and are
  now `#[non_exhaustive]`, so downstream code can no longer build them
  with a struct literal or match them exhaustively. A caller needs the key
  for draft-atwood-jmap-chat-did-00 §Federation Authentication step 2.
- **BREAKING** **jmap-chat-server**: `EphemeralSubscription::from_enable`
  and `replace_with` take a new `CompanionStreams` argument naming the
  companion WebSocket capabilities the server advertises. `"vtc"` and
  `"scene"` in `dataTypes` now count as recognized when their capability
  is advertised, so a companion-only `ChatStreamEnable` is accepted, and
  the new `vtc()` / `scene()` accessors report them
  (draft-atwood-jmap-chat-wss-00 §Enabling Ephemeral Push: an error only
  when there is "none of `"typing"`, `"presence"`, or a companion value
  whose capability the server advertises"). Pass `vtc()` / `scene()` as
  the `chat_vtc` / `chat_scene` flags of `jmap-chat-vtc-server` and
  `jmap-chat-scene-server`.
- **BREAKING** **jmap-chat-types**: `ChatTypingEvent` and
  `ChatPresenceEvent` gain a required `account_id` (`accountId`), the
  account that contains the Chat or ChatContact
  (draft-atwood-jmap-chat-wss-00 §ChatTypingEvent, §ChatPresenceEvent).
  A frame without `accountId` no longer deserializes.
- **jmap-chat-federation-server**: `Peer/typing` and `Peer/presence` set
  the forwarded event's `accountId` to the receiving account.
- **jmap-chat-server**: `should_deliver_typing` and `project_presence`
  drop an event whose `account_id` is not the recipient account.
- **BREAKING** **jmap-chat-types**: `TaskMetadata.status` is renamed to
  `progress` and serializes as the `progress` key
  (draft-atwood-jmap-chat-tasks-00 §MessageAction format, which replaced
  `status`). The key carries the Task's `progress` value, or its
  `workflowStatus` when `progress` is not set. A `status` key now lands
  in `extra`.
- **BREAKING** **jmap-chat-types**: `CHAT_CAP_VTC`,
  `CHAT_CAP_CALENDAR_EVENT`, `CHAT_CAP_AVAILABILITY` and `CHAT_CAP_TASK`
  are now `urn:ietf:params:jmap:chat:cap:{vtc,calendar-event,availability,task}`
  (draft-atwood-jmap-chat-00 §Endpoint), replacing the
  `urn:jmap:chat:cap:*` values. Clean break with no alias: actions and
  endpoints that carry the old type names are no longer recognized, and
  the federation server maps `assigneeIds` only for
  `urn:ietf:params:jmap:chat:cap:task` actions.
- **BREAKING** **jmap-filenode-types**: new `NodeRole::ChatSpace` variant
  for the role `chatspace` (draft-atwood-jmap-chat-filenode-00
  §JMAP FileNode Roles Registry). That string used to parse as
  `NodeRole::Other("chatspace")`, and `spaceId`, which used
  to land in `FileNode.extra`, now goes in `FileNode.space_id`.
- **BREAKING** **jmap-filenode-types**, **jmap-chat-types**: the Space file
  tree root role on the wire is `chatspace` (`NodeRole::ChatSpace`,
  `CHAT_FILENODE_SPACE_ROLE`), replacing `urn:jmap:chat:filenode:space`
  (draft-atwood-jmap-chat-filenode-00 §Space File Tree: "identified by the
  well-known `role` value `"chatspace"`"). Clean break with no alias: a
  node stored with the old string parses as `NodeRole::Other` and is no
  longer a Space root.
- **BREAKING** **jmap-chat-client**: the method-input structs are built
  like the other extension clients' `*Params` types, with struct literals
  and, for the all-optional patch structs, `..Default::default()`. The
  `new` constructors and `with_*` builders are removed from
  `MessageCreateInput`, `CustomEmojiCreateInput`, `SpaceInviteCreateInput`,
  `SpaceBanCreateInput`, `SpaceCreateInput`, `AddMemberInput`,
  `UpdateMemberRoleInput`, `SpaceAddMemberInput`, `SpaceUpdateMemberInput`,
  `SpaceAddChannelInput`, `SpaceAddRoleInput`, `SpaceUpdateRoleInput`,
  `SpaceUpdateChannelInput`, `SpaceAddCategoryInput`,
  `SpaceUpdateCategoryInput` and `PushSubscriptionCreateInput`. These
  structs and `MessagePatch`, `PresenceStatusPatch`, `CustomEmojiPatch`,
  `SpaceBanPatch`, `ChatContactPatch`, `ChatPatch`, `SpacePatch` and
  `PushSubscriptionPatch` are no longer `#[non_exhaustive]`, so a new
  field is a breaking change, as it already is for the sibling clients'
  `*Params` types.
- **BREAKING** **jmap-chat-client**: `chat_query`, `message_query`,
  `space_query`, `chat_contact_query`, `custom_emoji_query`,
  `Batch::chat_query_get` and `Batch::message_query_get` take positional
  `(filter: Option<&Filter<C>>, sort: Option<&[XxxComparator]>,
  position, limit)` like the other extension clients' `*_query` methods,
  so they can send RFC 8620 §5.5 `AND`/`OR`/`NOT` trees and
  draft-ietf-jmap-metadata-02 §3.5 `WithMetadata` conditions, the same
  filter `*_query_changes` accepts (RFC 8620 §5.6). `chat_query` and
  `message_query` (and their batch adders) also take trailing `anchor`,
  `anchor_offset` and `calculate_total`. `ChatQueryInput`,
  `MessageQueryInput` (with its deprecated `after` / `before` fields),
  `SpaceQueryInput`, `ChatContactQueryInput`, `CustomEmojiQueryInput` and
  `ContactSortProperty` are removed; build filters from the
  `jmap_chat_types::query` condition and comparator types.
  `message_query` still rejects `filter: None` client-side and still
  sends `receivedAt` descending when `sort` is `None`.

- **BREAKING** **jmap-base-client**: `connect_ws`, `connect_ws_with_limit`
  and `JmapClient::connect_ws_session` now accept only `wss://` URLs and
  return `ClientError::InvalidArgument` for `ws://` (jmap-chat-wss-guide:
  "clients MUST reject any URL that does not begin with wss:// rather than
  downgrading the connection"). Plaintext test rigs switch to
  `connect_ws_allow_plaintext`.

- **BREAKING** **jmap-chat-types**: a `ChatContact` whose JSON carries a
  `did` that is not a valid DID now fails to deserialize; before, any
  `did` value was kept verbatim in `extra`.

- **BREAKING** **jmap-chat-types**: `Chat` has a typed
  `active_call_id: Option<Id>` for the server-set `activeCallId`
  property (draft-atwood-jmap-chat-00 §Chat, `String|null`). The value
  no longer lands in `Chat.extra["activeCallId"]`, and a `Chat` with no
  active call now serializes `"activeCallId": null`. `Chat/set` still
  rejects it as a patch key.

- **BREAKING** **jmap-chat-server**: `OwnerFlags`, `PushItem`, `BuiltPush`
  and `StoreOutcome` are `#[non_exhaustive]`, so a field or variant can be
  added after the 0.4 freeze without a breaking change. Code outside the
  crate can no longer build `OwnerFlags` or `PushItem` with a struct literal:
  use the new `OwnerFlags::new(active, admin)` and
  `PushItem::new(message, chat, sender)`, then set `sender_member` and
  `space` on the result. A `match` on `StoreOutcome` needs a wildcard arm.

- **BREAKING** **jmap-chat-types**, **jmap-chat-client**: `ChatMemberRole`
  moves from jmap-chat-client into jmap-chat-types, and `ChatMember.role` and
  `ChatMember::new` now take `ChatMemberRole` instead of `String`, so server
  and client share one model of the draft-atwood-jmap-chat-00 §ChatMember
  `role` ("Either `"admin"` or `"member"`"). Unknown wire values still
  round-trip through `ChatMemberRole::Other`. jmap-chat-client re-exports the
  type, so `jmap_chat_client::ChatMemberRole` and
  `jmap_chat_client::types::ChatMemberRole` keep working.

- **BREAKING** **jmap-chat-types**, **jmap-chat-client**: new
  `AttachmentCreate` type for `Message/set` create entries, with `blobId`,
  `filename`, `contentType`, `size` and `sha256` optional, because
  draft-atwood-jmap-chat-filenode-00 §filenodeId makes them "OPTIONAL for
  that Attachment entry" when `filenodeId` is present. The stored and
  returned `Attachment` keeps them required, since the server "MUST
  populate any absent fields ... before storage".
  `MessageCreateInput::attachments` and `MessageCreateInput::with_attachments`
  now take `&[AttachmentCreate]` instead of `&[Attachment]`.

- **BREAKING** **jmap-chat-server**: `ChatLimits::max_attachments_per_message`
  and `ChatLimits::with_max_attachments_per_message` take `u64` instead of
  `u32`, matching `ChatCapability::max_attachments_per_message` and the
  `UnsignedInt` type of `maxAttachmentsPerMessage`
  (draft-atwood-jmap-chat-00 §Account-Level Capability Object).

- **BREAKING** **jmap-base-client**: `WsSession::next_frame` /
  `WsReceiver::next_frame` return `Some(Err(ClientError::WsClosed { code,
  reason }))` instead of `None` when the server closes with status code 1008
  (Policy Violation), so callers can tell credential expiry from a clean
  close (draft-atwood-jmap-chat-wss-00 §Authentication and Identity). Other
  Close frames still end the stream with `None`. `ClientError` gains the
  `WsClosed` variant.

- **BREAKING** **jmap-chat-server**: `ChatBackend::is_contact_blocked` is
  now documented as a plain lookup: whether `contact_id` has
  `blocked: true` on `account_id`'s own contact list, with `account_id`
  the recipient and `contact_id` the sender's ChatContact id in that list
  (draft-atwood-jmap-chat-00 §Blocked Contacts, §ChatContact `id`). It no
  longer claims `caller` identifies the sender. `handle_chat_typing` no
  longer calls it: it passed the sender's `accountId` and the direct
  Chat's `contactId`, which asks whether the sender blocked the recipient.
  The transport that fans typing events out calls it per recipient.
- **BREAKING** **jmap-chat-server**: a `/query` without `sort` now uses
  the draft-atwood-jmap-chat-00 default order: `Chat/query` by
  `lastMessageAt` descending (chats without messages last),
  `Message/query` by `receivedAt` ascending with `chatId` and descending
  without it, and `Space/query` and `CustomEmoji/query` by `name`
  ascending. The handlers pass this sort to `query_objects`, which
  previously received `None`.
- **BREAKING** **jmap-chat-server**: `MemoryBackend` `Space/query` lists
  only Spaces the caller is a member of, plus, for an `isPublic: true`
  condition, non-member Spaces with `isPublic` and
  `isPubliclyPreviewable` both `true` (§Space/query). The `isPublic`
  condition now filters results.
- **BREAKING** **jmap-chat-server**: CustomEmoji `name` is unique per
  scope (`spaceId`, or server-global when absent; §CustomEmoji). This is a
  new `ChatBackend::create_object` uniqueness contract: a duplicate create
  is `alreadyExists` with `existingId`, and a rename onto a held name is
  `invalidProperties` naming `name`. `MemoryBackend` enforces it.
- **BREAKING** **jmap-chat-server**: `Message/set` create rejects a
  `body` longer than the account's `ChatLimits::max_body_bytes` (it was
  a fixed 100,000), more than `max_attachments_per_message` attachments,
  or an Attachment whose `size` exceeds `max_attachment_bytes`, each with
  a `tooLarge` SetError (draft-atwood-jmap-chat-00 §Account-Level
  Capability Object, §Denial of Service). The attachment limits were
  previously not enforced.

- **BREAKING** **jmap-chat-server**: the reference `MemoryBackend` now
  enforces draft-atwood-jmap-chat-00 §Message/set and §Message/query. A
  Message create is `forbidden` when the owner is not a member of the
  group Chat or of the channel's Space, or lacks the resolved `"send"`
  permission in the channel. Messages in channels where the owner lacks
  the resolved `"view"` permission are left out of `Message/get` (a
  requested id goes to `notFound`) and `Message/query`. The `@everyone`
  floor stays empty, so a member who holds no roles can neither send nor
  view in channels unless a member-targeted override allows it.
- **BREAKING** **jmap-chat-server**: `Chat/set` enforces the per-kind
  rules of draft-atwood-jmap-chat-00 §Chat/set. An update key that
  "Updating a Chat" does not define (for example `contactId`, `spaceId`,
  `members`, `kind` or a channel's `topic`) is `invalidProperties`; it
  was previously stored. `name`, `description` and `avatarBlobId` updates
  are `forbidden` unless the Chat is a group and the caller is one of its
  admins. Destroying a channel Chat is `forbidden`; channels are removed
  via `Space/set` `removeChannels`. Group create stores `avatarBlobId` and
  `messageExpirySeconds`, and is `overQuota` when the membership exceeds
  `ChatLimits::max_group_chat_members`.
- **BREAKING** **jmap-chat-server**: `Message/set` create rejects a
  `replyTo` that is not a Message in the same Chat, and a `threadRootId`
  that breaks the draft-atwood-jmap-chat-00 §Thread Model assignment
  rules, with `invalidProperties`; both were previously stored unchecked.
  An omitted `threadRootId` on a reply is now set by the server.
- **BREAKING** **jmap-chat-server**: `ReadPosition/set` update sets
  `lastReadAt` to the current time, and rejects a client-supplied
  `lastReadAt` (server-set per draft-atwood-jmap-chat-00 §ReadPosition)
  or a non-String `lastReadMessageId` with `invalidProperties`.
- **BREAKING** **jmap-chat-server**: the `ChatBackend::destroy_object`
  and `apply_space_patch` contracts now require every Chat destroy,
  including the Space-destroy and `removeChannels` cascades, to destroy
  that Chat's ReadPosition and advance the `ReadPosition` state
  (draft-atwood-jmap-chat-00 §ReadPosition: the server "destroys it when
  the Chat is destroyed"). The reference `MemoryBackend` does so.

### Removed

- **BREAKING** **jmap-mail-server**: `Email/copy` no longer honours the
  non-standard `onSuccessUpdateOriginal` argument; it is ignored like any
  other unknown argument. RFC 8621 §4.7 defines Email/copy as the standard
  RFC 8620 §5.4 `/copy`, whose only side-effect arguments are
  `onSuccessDestroyOriginal` and `destroyFromIfInState`. The implicit
  `Email/set` response is now emitted only for `onSuccessDestroyOriginal`
  (bd JMAP-sj4m).
- **BREAKING** **jmap-types**: the `JmapError` constructors `not_found`,
  `over_quota`, `invalid_patch`, `will_destroy`, `invalid_properties`,
  `singleton` and `already_exists`, and the `JmapError.existing_id` field.
  RFC 8620 §5.3 and §5.4 define these types as SetErrors, not method-level
  errors; build them with `SetError` instead. `too_large` and `rate_limit`
  stay, because draft-ietf-jmap-calendars §2.2 defines them as
  `Principal/getAvailability` method errors. A method-level `notFound`
  (same draft) is `JmapError::custom("notFound")` (bd JMAP-q5n7).
- **BREAKING** **jmap-calendars-types**, **jmap-tasks-types**: the
  `QueryObject` impls for `Calendar`, `ParticipantIdentity` and
  `TaskList`, and `CalendarFilterCondition` with its `CalendarFilter`
  alias. draft-ietf-jmap-calendars-33 lists only `/get`, `/changes` and
  `/set` for Calendar and ParticipantIdentity (§3, §4), and
  draft-ietf-jmap-tasks-06 the same for TaskList (§3), so no
  `QueryParams<'_, Calendar>` call has a method to reach, and the
  `name`/`isSubscribed` filter was a wire shape no spec defines. These
  types are now only `JmapObject`/`GetObject`/`SetObject`, like the
  query-less jmap-mail-types objects (bd JMAP-w9ox.1).
- **BREAKING** Every remaining public module path of the extension
  types crates (jmap-calendars-types, -chat-federation-types,
  -chat-mimi-types, -chat-types, -cid-types, -contacts-types,
  -did-types, -filenode-types, -graph-types, -mail-types,
  -metadata-types, -scene-types, -sharing-types, -tasks-types,
  -vtc-types): for example `jmap_mail_types::email`,
  `jmap_chat_types::query`, `jmap_graph_types::search`,
  `jmap_mail_types::keyword`. The modules are private and the crate
  root is the single public path, so name `jmap_mail_types::Email`, not
  `jmap_mail_types::email::Email`, and `jmap_mail_types::SEEN`, not
  `jmap_mail_types::keyword::SEEN` (bd JMAP-wf92.23).
- **BREAKING** **jmap-types**: the `backend`, `error`, `id`, `jmap_uri`,
  `methods`, `patch`, `push`, `query`, `resultref`, `set_error` and
  `wire` module paths. The modules are private and every item keeps its
  crate-root path, so name `jmap_types::Filter`, not
  `jmap_types::query::Filter` (bd JMAP-wf92.23).
- **BREAKING** **jmap-server**: the `backend` and `handlers` module
  paths. The modules are private and every item keeps its crate-root
  path, so name `jmap_server::JmapBackend` and `jmap_server::handle_get`,
  not `jmap_server::backend::JmapBackend` or
  `jmap_server::handlers::handle_get` (bd JMAP-wf92.23).
- **BREAKING** The public module paths of the client crates:
  jmap-base-client `auth`, `blob`, `client`, `error`, `push`, `query`,
  `request`, `sse`, `ws`; jmap-chat-client `calendars`, `methods`
  (including `methods::batch`, `methods::blob`, `methods::quota`),
  `session`, `sse`, `types`, `utils`, `ws`; the `methods` module of
  jmap-calendars-, -chat-mimi-, -contacts-, -filenode-, -graph-, -mail-,
  -scene-, -sharing-, -tasks- and -vtc-client; and the `ws` module of
  jmap-graph-, -scene- and -vtc-client. The modules are private and the
  crate root is the single public path, so name
  `jmap_base_client::BearerAuth`, not `jmap_base_client::auth::BearerAuth`,
  and `jmap_chat_client::Batch`, not `jmap_chat_client::methods::batch::Batch`
  (bd JMAP-wf92.23).
- **BREAKING** The `jmap_calendars_types::jscalendar`,
  `jmap_tasks_types::jscalendar` and `jmap_contacts_types::jscontact`
  alias modules. They re-exported exactly the names already at each
  crate root; use the root path (`jmap_calendars_types::Location`) or
  the source crate (`jmap_jscalendar_types::Location`)
  (bd JMAP-wf92.23).
- **BREAKING** The `backend` module path in jmap-mail-types,
  -calendars-types, -chat-types, -contacts-types, -filenode-types,
  -sharing-types and -tasks-types. The module is private; its items
  (the `*Property` enums, plus the per-user property lists and
  predicates in -calendars-types and -tasks-types) keep their
  crate-root path, so name `jmap_mail_types::EmailProperty`, not
  `jmap_mail_types::backend::EmailProperty` (bd JMAP-wf92.26).
- **BREAKING** The `capability` module path in jmap-mail-types,
  -calendars-types, -chat-types, -chat-mimi-types, -cid-types,
  -contacts-types, -did-types, -filenode-types, -graph-types,
  -metadata-types, -scene-types, -sharing-types, -tasks-types and
  -vtc-types. The module is private; its items (capability URI constants
  and capability objects) keep their crate-root path, so name
  `jmap_mail_types::JMAP_MAIL_URI`, not
  `jmap_mail_types::capability::JMAP_MAIL_URI`.
- **BREAKING** Second public paths to server handler items. Where the
  crate root re-exports every public item of a handler module, the module
  is private: jmap-mail-server `email`, `identity`, `mailbox`, `snippet`,
  `submission`, `thread`, `vacation` and `sieve`, so name
  `jmap_mail_server::handle_email_get`, not
  `jmap_mail_server::email::handle_email_get`. The same holds for every
  root-covered module of jmap-calendars-server, -chat-server,
  -contacts-server, -filenode-server, -graph-server (not `query` and
  `search`, which have no root re-exports), -metadata-server,
  -scene-server (not `query`), -sharing-server, -tasks-server and
  -vtc-server (not `query`). Where the module has items the root does
  not re-export, the root re-export goes instead: jmap-mail-server's
  `MdnBackend`, `handle_mdn_send` and `handle_mdn_parse` are reached only
  as `jmap_mail_server::mdn::*`, and jmap-chat-server's
  `handle_message_*`, `store_server_message`,
  `find_message_by_sender_msg_id` and `StoreOutcome` only as
  `jmap_chat_server::message::*`.
- **BREAKING** The jmap-jscalendar-types constructors (`new` on `NDay`,
  `RecurrenceRule`, `Location`, `VirtualLocation`, `Link`, `Relation`,
  `Participant`, `OffsetTrigger`, `AbsoluteTrigger`, `Alert`,
  `TimeZoneRule` and `TimeZone`; `Link::with_href` and
  `Link::with_blob_id`; `Default` for `Location`, `Link` and `Relation`),
  matching jmap-jscontact-types and the workspace rule that values are
  built by deserializing JSON (bd:JMAP-sgrr.27, bd:JMAP-ops7.34).
- **BREAKING** The jmap-jscalendar-types validators: the
  `TypeDiscriminator` trait and its impls, `TypeTagMismatch`,
  `Link::validate_source` / `LinkSourceError`,
  `Participant::validate_roles` / `ParticipantRolesError` and
  `TimeZoneRule::validate_recurrence_overrides_empty` /
  `RecurrenceOverridesError`. Neither RFC sub-type crate ships validators;
  consumers check RFC 8984 invariants themselves (bd:JMAP-ops7.38).
- **BREAKING** `jmap_tasks_server::TasksBackend::enforce_is_draft_atomically`.
  `Task/set` no longer pre-fetches the Task to reject an `isDraft: true`
  patch; every backend's `update_object` MUST reject a patch that sets
  `isDraft: true` on a Task whose stored value is not `true` with
  `invalidProperties` naming `isDraft` (draft-tasks-06 §4), as
  `MemoryBackend` already does (bd:JMAP-h47t.29).
- **BREAKING** The `ClosureHandler` re-export from jmap-mail-server,
  -calendars-server, -chat-server, -chat-mimi-server, -contacts-server,
  -filenode-server, -graph-server, -scene-server, -sharing-server,
  -tasks-server and -vtc-server. The `register_*_handlers` functions
  build it internally; name it as `jmap_server::ClosureHandler`.
- **BREAKING** The public `backend` module of the same eleven server
  crates. Each item was reachable both as `crate::X` and
  `crate::backend::X`; the crate root is now the only path. Replace
  `jmap_<x>_server::backend::X` with `jmap_<x>_server::X`. The root
  gains the items that were only under `backend`:
  `jmap_filenode_server::CaseFolding` and the `jmap_server` backend types
  in jmap-chat-mimi-server. `jmap_tasks_server::backend::PatchObject` has
  no replacement; import `jmap_types::PatchObject`, as for mail and
  calendars.
- **BREAKING** **jmap-contacts-types**: the public `collision` module;
  `CollisionError` stays at the crate root.
- **BREAKING** **jmap-server**: the deprecated `helpers::ser` alias; use
  `serialize_value`.
- **BREAKING** **jmap-types**: the deprecated `JmapError::too_many_changes`
  and `JmapError::unknown_capability`, which omitted the limit and the
  failing URI; use `too_many_changes_with_limit` and
  `unknown_capability_with_detail`.
- **BREAKING** **jmap-server**: the `BackendChangesError::TooManyChanges
  { limit: 0 }` legacy alias for `CannotCalculate`. A zero `limit` now
  maps to `tooManyChanges` like any other value; a backend that cannot
  calculate changes returns `CannotCalculate` (RFC 8620 §5.2, §5.6
  `cannotCalculateChanges`).
- **BREAKING** **jmap-scene-types**: `RETRY_AFTER`, `retry_after` and
  `RetryAfterError`. draft-atwood-jmap-scene-00 (jmap-chat-spec 6bcd9d8,
  L2690) no longer defines a `retryAfter` property on `rateLimit`: "When a
  rate limit is exceeded, the server SHOULD return a `SetError` of type
  `rateLimit` ({{RFC8620}} Section 5.3)." `CONTENT_POLICY` stays.
- **BREAKING** **jmap-vtc-types**: `RingEndReason::Answered`.
  draft-atwood-jmap-vtc-00 (jmap-chat-spec 6bcd9d8, L1431) no longer
  lists `"answered"`, which now decodes as `RingEndReason::Other`.
- **BREAKING** **jmap-chat-client**: `ContactPresenceFilter` and its
  `TryFrom<Presence>` impl. Its only consumer, `ChatContactQueryInput`,
  was removed when `chat_contact_query` moved to `Filter`, whose
  `presence` condition is `jmap_chat_types::Presence`. Its `Invisible`
  variant also contradicted draft-atwood-jmap-chat-00 §ChatContact: "A
  contact whose own presence is `"invisible"` appears as `"offline"`."

### Fixed

- **jmap-chat-server**: **BREAKING** `Space/set` create rejects a key
  that is neither a client-settable Space property nor a registered
  `space_extension_properties` key with an `invalidProperties` SetError
  naming it (RFC 8620 §5.3), as update already did, instead of silently
  dropping it. A create carrying `calendarId` or `taskListId` without
  `mayBindCalendar`/`mayBindTaskList`, or a structural key such as
  `addRoles`, is now refused (bd JMAP-zmqp).
- **jmap-chat-server**: the nine `/set` handlers (`Chat`, `Message`,
  `Space`, `ChatContact`, `PresenceStatus`, `SpaceInvite`, `CustomEmoji`,
  `ReadPosition`, `SpaceBan`) now call `JmapBackend::account_exists` and
  return the RFC 8620 §3.6.2 `accountNotFound` method error for an
  unknown `accountId`, as the canonical jmap-mail-server handlers do (bd
  JMAP-fo38). Before, they wrote to the unknown account.
- **jmap-chat-server**, **jmap-tasks-server**, **jmap-sharing-server**:
  `MemoryBackend::get_changes` now omits an id created and destroyed in
  the same window, the RFC 8620 §5.2 SHOULD path, instead of listing it
  in `destroyed` (bd JMAP-cqy7). The three backends now use the
  canonical jmap-mail-server `IdFate` rules.
- **jmap-mail-server, jmap-chat-server, jmap-calendars-server,
  jmap-tasks-server, jmap-filenode-server, jmap-sharing-server,
  jmap-contacts-server**: README trait blocks match the source again:
  every method shows `caller: &Self::CallerCtx`, and `create_object`
  takes the `build` closure. The "How to use" blocks are marked
  `rust,ignore`, use a typed `Dispatcher<MyCallerCtx>`, and point to the
  verified `lib.rs` doctest (bd JMAP-50yd).

- **jmap-mail-server**: narrowing and sign-changing `as` casts are gone
  and the clippy gate now rejects them (bd JMAP-uqhn.1).
  `maxBodyValueBytes` saturates instead of truncating on 32-bit targets,
  and an `Email/query` `before`/`after` date with month `00` no longer
  wraps to a bogus instant; it is treated as unparseable (RFC 3339 §5.6).

- **jmap-mail-server**: `MemoryBackend::create_object` returns the stored
  object when one already exists under the key (the VacationResponse
  `singleton`) instead of overwriting it, as the `MailBackend::create_object`
  "Singleton types" contract requires. A racing VacationResponse/set no
  longer resets a concurrent request's update to the default (bd JMAP-9n21).
- **mail, calendars, chat, chat-mimi, contacts, filenode, graph, scene,
  sharing, tasks, vtc servers**: the self-dev-dependency that enables
  `memory` (and `mdn`, `sieve`) for tests is now path-only, so
  `cargo publish` drops it instead of resolving it against crates.io. A
  release that adds a server feature no longer fails with "depends on
  `<crate>` with feature `<f>` but `<crate>` does not have that feature",
  and no temporary `Cargo.toml` edit is needed (bd JMAP-31o4).
- **mail, calendars, chat, chat-mimi, contacts, filenode, graph, scene,
  sharing, tasks, vtc servers**: `MemoryError` no longer embeds a
  `serde_json` error message. serde data errors quote the offending value
  (`invalid type: string "alice@example.com", expected u64`), which put
  object content and email-shaped PII into `Display` output against the
  `JmapBackend::Error` contract. The description now carries only the
  operation and the serde error category, e.g.
  `deserialize Email: serde_json Data error` (bd JMAP-510h.37).

- **jmap-chat-server**: the reference `MemoryBackend` sorts UTCDate
  properties such as `receivedAt` chronologically. Before this fix it
  compared them byte by byte, so an instant on a whole second (written
  `...:05Z`, because RFC 8620 §1.4 omits a zero fraction) sorted after
  `...:05.001Z` in the same second. That made `Message/query` order
  flaky (bd JMAP-rvp2).
- **all extension servers, jmap-server**: `invalidArguments`
  descriptions for a wrongly typed `/set` `destroy` element or `/query`
  `filter`, `limit`, `position`, `anchor` or `anchorOffset` (plus
  SieveScript/set `onSuccessActivateScript` and SceneRegion filter
  `accessPolicy`) name only the JSON type of the offending value instead
  of re-serialising it, so a request can no longer copy arbitrary JSON
  into its own error response (RFC 8620 §3.6.1; bd JMAP-zegc).
  jmap-chat-server's private `json_value_kind` is replaced by the shared
  helper.
- **jmap-server**: `now_utc_string` and `now_utc_string_checked` give the
  right time of day on a pre-epoch clock. Hours and minutes came from
  truncating division, so one second before the epoch formatted as
  `1969-12-31T00:00:59Z` instead of `1969-12-31T23:59:59Z`
  (bd JMAP-yt1w.40).
- **jmap-scene-server**: `SceneBackend` rustdoc no longer requires
  `query_changes` to omit records hidden from the caller, and states the
  `query_objects` omission as SHOULD. draft-atwood-jmap-scene-00
  §Visibility Contract: the server "SHOULD NOT include a SceneObject in a
  ... `SceneObject/query` result", and "`/changes` and `/queryChanges`
  report changes for the account".
- **BREAKING** **jmap-types**, **jmap-server**: `UTCDate::new_validated` and
  `UTCDate::to_epoch_seconds` now accept a non-zero RFC 3339 fractional
  second (`2014-10-30T06:12:00.123Z`) and still reject a zero one
  (`.000Z`), per RFC 8620 §1.4 ("the "time-secfrac" MUST always be omitted
  if zero"); `to_epoch_seconds` truncates the fraction. The short-input
  error text changed from "exactly 20 characters" to "at least 20
  characters". `now_utc_string` / `now_utc_string_checked` now omit the
  fraction when the milliseconds are zero, instead of emitting `.000Z`, so
  every value they return passes `UTCDate::new_validated` (bd JMAP-jfia.23).

- **jmap-chat-server**: `build_chat_message_push` now sends `bodySnippet`
  for `application/jmap-chat-rich` messages, rendered from the span `text`
  values joined in order, per the draft-atwood-jmap-chat-push-00
  §ChatMessageEntry SHOULD for a plaintext approximation. It used to omit
  the snippet. A body that does not parse as a rich body still gets no
  snippet.
- **jmap-contacts-server**: the `BackendSetError` catch-all arms in
  `AddressBook/set` and `ContactCard/set`/`copy` no longer Debug-format the
  backend error into the `serverFail` description; they now send only
  "unhandled backend error variant", matching jmap-mail-server and keeping
  backend error content off the wire.
- **jmap-chat-mimi-server, jmap-chat-mimi-client**: pin `openmls =0.9.2`
  (was `=0.9.1`) for GHSA-94v4-rg25-x625 (removed leaves reappear when an
  external commit truncates and regrows the ratchet tree) and
  GHSA-p276-g3f3-wwgh (rejected private messages advance the sender
  ratchet). The companion crates stay at 0.6.0 and MSRV stays 1.91. The
  GREASE KeyPackage fix (openmls#2162) is not in 0.9.2, so that limitation
  remains (bd:JMAP-6k5d.9).
- **jmap-mail-server**: the reference `MemoryBackend` sorts a stored
  `EmailSubmission` that fails to deserialize after every valid one in
  `EmailSubmission/query`, so the sort comparator is a total order
  (`slice::sort_by` may panic otherwise) (bd JMAP-skk9.19).
- **jmap-mail-server**: `SieveScript/query` now accepts an RFC 8620 §5.5
  `FilterOperator` tree (`AND`/`OR`/`NOT` over `name`/`isActive`
  conditions) and evaluates it with `Filter::matches`, instead of
  refusing every operator tree with `unsupportedFilter` (bd JMAP-skk9.17).
- **BREAKING** **jmap-mail-server**: `EmailSubmission/query` and
  `EmailSubmission/queryChanges` now decode `sort` as
  `EmailSubmissionComparator[]` and pass it to the backend instead of
  ignoring it. A malformed `sort` (e.g. `[5]`) is `invalidArguments` and
  a property outside RFC 8621 §7.3's `emailId`/`threadId`/`sentAt` is
  `unsupportedSort` (RFC 8620 §5.5); `MemoryBackend` orders
  EmailSubmission query results by those comparators (bd JMAP-skk9.15).
- **BREAKING** **jmap-mail-server**: `EmailSubmission/queryChanges` now
  decodes `filter` as `EmailSubmissionFilter` and passes it to the backend
  instead of ignoring it, so the delta covers only the filtered results
  and a malformed or unknown filter is `invalidArguments` /
  `unsupportedFilter` (RFC 8620 §5.6: "The filter argument that was used
  with Foo/query") (bd JMAP-skk9.18).
- **jmap-calendars-server**: `CalendarEvent/copy` creates now go through
  `CalendarsBackend::create_calendar_event` (with default
  `CalendarEventSetArgs`) instead of the generic `create_object`, so a
  backend's §5.9 create rules and the draft-ietf-jmap-calendars-33 §5.10
  rule (non-`public` privacy copied into an account that does not belong
  to the user → `invalidProperties`) apply to copies (bd JMAP-wf92.24).
- **jmap-vtc-server**: a `VTCCall/set` `state: "ended"` on a call that
  changed state after the handler read it (a ring answered, a scheduled
  call joined) now ends the call with the `endReason` of its new state
  (draft-atwood-jmap-vtc-00 §State Machine "`"active"` → `"ended"`: ...
  a moderator ends the call") instead of failing with
  `invalidProperties` after its breakout rooms had already ended
  (bd JMAP-ace5, JMAP-wf92.22).
- **jmap-chat-server**: `store_server_message` stores the Message with
  `chatId` and `senderMsgId` set to its `chat_id` and `sender_msg_id`
  arguments, so the stored keys are always the ones the duplicate check
  searched (draft-atwood-jmap-chat-mimi-00: "The server MUST NOT store a
  FanoutMessage whose `senderMsgId` it has already stored in the Chat";
  bd JMAP-dpbp, JMAP-wf92.22).
- **jmap-calendars-server**: `CalendarEvent/set` rejects a `destroy`
  array with a non-string element (`invalidArguments`) before applying
  any create or update; it used to apply them first.
- **BREAKING** (wire) **jmap-mail-server**: `Email/copy` without
  `keywords` keeps the source Email's keywords instead of clearing them,
  and the copy carries every source property rather than a fixed subset
  (RFC 8620 §5.4: "any other properties included are used instead of the
  current value").
- **jmap-vtc-server**: a participant join or answer no longer moves a
  call that a moderator ended concurrently back to `"active"`. A failed
  activation in `VTCParticipant/set` no longer skips the decline and
  leave transitions that follow it. A breakout room whose state changed
  during a parent end gets the `endReason` for its new state.
- **jmap-server**: the dispatcher keeps the creation id map for every
  request, not only when the client sent `createdIds`; RFC 8620 §5.3
  says the server "MUST keep a simple map for the duration of the
  request". The response still echoes `createdIds` only when the client
  sent it (RFC 8620 §3.4).
- **jmap-chat-server**: `MemoryBackend` `Space/query` reveals non-member
  public, previewable Spaces for an `isPublic: true` condition nested in a
  FilterOperator, not only a top-level one (draft-atwood-jmap-chat-00
  §Space/query, jmap-chat-spec 29e9a41 L1394).
- **jmap-contacts-server**: `AddressBook/set` with
  `onDestroyRemoveContents` no longer copies backend error text into the
  `serverFail` description when the contents cascade fails; the
  description is the fixed `"internal error"`, as everywhere else.
- **jmap-chat-server**: `MemoryBackend` `Message/query` `hasMention` no
  longer treats every `"admins"` broadcast mention as targeting the
  owner. The owner is an admin when its group ChatMember `role` is
  `"admin"`, or when its Space permissions in a channel's Space include
  `manage_members` or `manage_space`; direct Chats have no admins
  (draft-atwood-jmap-chat-00 §BroadcastMention).
- **jmap-chat-server**: `Space/set` `addCategories` accepts entries that
  omit `position` or `channelIds` instead of rejecting them with
  `invalidProperties`. A missing `channelIds` means no channels; the draft
  does not define a default `position`, so the memory backend appends the
  category at one past the highest existing `position` (`0` when the Space
  has none).

- **jmap-contacts-server**: a `ContactCard/copy` whose `destroyFromIfInState`
  does not match the source state now answers the implicit `ContactCard/set`
  with a method-level `error` of type `stateMismatch` (RFC 8620 §5.4, §5.3
  `ifInState`) instead of a `ContactCard/set` listing each source id under
  `notDestroyed`. Nothing is destroyed, as before.
- **BREAKING** **jmap-chat-server**: an owner-composed `Message/set` create
  now has `senderMsgId` equal to its `id` (draft-atwood-jmap-chat-00
  §Message: "Equals `id` for messages composed by the owner."). It
  previously stored the client creation id, so creation ids reused across
  requests collided in `find_message_by_sender_msg_id`. The handler passes
  `senderMsgId` as the `"placeholder"` sentinel, and
  `ChatBackend::create_object` MUST replace it with the assigned id in the
  same write, as `MemoryBackend` now does; a custom backend must add this.
- **jmap-chat-server**: `MemoryBackend` applies every draft-atwood-jmap-chat-00
  `/query` filter clause: Chat `kind` / `muted`, ChatContact `blocked` /
  `presence`, Message `topicId` / `hasAttachment` / `hasMention: false`,
  Space `name` (substring), and CustomEmoji `spaceId`, as well as RFC 8620
  §5.5 `AND` / `OR` / `NOT` FilterOperators. It previously accepted these
  and ignored them, and ignored any FilterOperator entirely. `/queryChanges`
  re-runs the same query, so its deltas follow the filter too.
- **BREAKING** **jmap-chat-server**: `Message/queryChanges` returns
  `unsupportedFilter` for the filters `Message/query` refuses (no `chatId`
  without `hasMention: true`, any `text`, `threadRootId` without
  `supportsThreads`), since RFC 8620 §5.6 takes the filter of the original
  query. It previously passed them through unchecked.
- **jmap-mail-server**: `Mailbox/queryChanges` decodes and forwards
  `filter` and `sort` to `query_changes`, as `Mailbox/query` does (RFC 8620
  §5.6: they are the arguments "that was used with Foo/query"). It
  previously ignored both, returning the unfiltered delta and never
  answering `unsupportedFilter` or `unsupportedSort`.
- **jmap-mail-server**, **jmap-chat-server**, **jmap-tasks-server**:
  `MemoryBackend::query_changes` lists every updated id in `removed`, and
  `added` reinserts those still in the results (RFC 8620 §5.6: "the server
  MUST include all Foos in the current results for which this property may
  have changed"). An updated id that still matched was previously only in
  `added`, so a client applying `removed` then `added` duplicated it.
- **jmap-contacts-client**: `address_book_set` now sends
  `AddressBookSetParams::extra` on the wire; it was silently dropped.
  Typed fields win over a colliding `extra` key, as in `mailbox_set`.
- **jmap-vtc-server**: a decline and a cancelled ring now deliver a
  `VTCCallEndEvent` through `VtcBackend::deliver_event`, per
  draft-atwood-jmap-vtc-00 §VTCCallEndEvent: "When a ringing call is
  answered, declined, cancelled, or times out, the server delivers a
  `VTCCallEndEvent` to all devices that received the ring." Declining
  delivers `endReason` `"declined"` for the decliner's other devices. A
  ringing call ending `"cancelled"` (the initiator's `VTCCall/set`, the
  initiator leaving, or `end_call`) delivers `"cancelled"`. Only
  `"answered_elsewhere"` and `"timeout"` were delivered before.
- **jmap-filenode-server**: `FileNode/set` now answers `accountNotFound`
  for an unknown `accountId` (RFC 8620 §3.6.2), like the other
  `FileNode/*` handlers. It previously returned an empty `/set` response.
- **BREAKING** **jmap-chat-server**: `Space/set` destroy now requires
  `manage_space`. `MemoryBackend::destroy_object::<Space>` returns a
  `forbidden` SetError when the caller lacks it, and the
  `ChatBackend::destroy_object` rustdoc makes that check a backend duty,
  per draft-atwood-jmap-chat-00 §Destroying a Space: "The caller MUST
  hold `"manage_space"`". Any caller could previously destroy a Space and
  its channels.
- **jmap-chat-server**: a `Message/set` update that sets `readAt` on the
  owner's own sent `burnOnRead` message no longer hard-deletes it; only a
  received copy burns, per draft-atwood-jmap-chat-00 §Message
  `burnOnRead`: "Applies only to the receiving mailbox; the sender's own
  copy is not affected."
- **jmap-chat-filenode-server**: the Space file tree gates compare
  Space permissions by `ChatPermission` variant, as `jmap-chat-server`,
  `jmap-chat-vtc-server` and `jmap-chat-federation-server` do. A
  hand-built `ChatPermission::Other("manage_space")` (or another `Other`
  spelling of a known name) in a SpaceRole or the `everyone` set used to
  grant that permission's file tree rights; it now grants nothing, per
  draft-atwood-jmap-chat-00 §SpaceRole: "Servers MUST ignore
  unrecognized permission names."
- **jmap-filenode-server**: `FileNode/copy` sets `sha256` as
  `FileNode/set` create does, per draft-atwood-jmap-cid-00 §FileNode
  Extension: a copied file gets the digest of its blob in the destination
  account (the source's or an override's value is dropped), and with the
  destination advertising `urn:ietf:params:jmap:cid` a copy without a
  digest returns `"sha256": null` in `created`. A `cid_advertised` `Err`
  is now a top-level `serverFail` before any copy is applied.
- **jmap-chat-mimi-server**: `MemoryBackend::store_fanout` no longer
  rebinds an unbound Chat for a Welcome FanoutMessage it already stores.
  A hub retrying the same `/notify` after `Room/unbind` used to bind the
  Chat again, so later FanoutMessages of a group the account left were
  stored; draft-atwood-jmap-chat-mimi-00 §Chat.roomId binds it again only
  for "A later Welcome for the account".
- **jmap-chat-federation-server**: `handle_peer_group_update` parses an
  `avatar` object only after the authorization and `closedAt` checks, as
  draft-atwood-jmap-chat-federation-00 §Peer/groupUpdate Receiver
  Behavior requires. A malformed one (negative `size`, no `fetchUrl`) used
  to reject the whole request with `invalidArguments`, before an
  unauthorized peer's `forbidden`; it is now a failed avatar, and the rest
  of the update applies.
- **jmap-chat-client**: `server_retry_after` (and so `message_create`)
  validates `serverRetryAfter` as an RFC 8620 §1.4 `UTCDate`. Any string
  used to be accepted, so `"in five seconds"` became the `RateLimited`
  deadline; it is now `ServerRetryAfterError::Malformed`, which
  `message_create` reports as `ClientError::UnexpectedResponse` as its
  docs already said.
- **jmap-graph-types**: `Expression::to_value` leaves out a field test
  with no operators (`FieldTest::default()`, always true) instead of
  writing `{}`, which draft-atwood-jmap-graph-search-00 §Field Tests reads
  as `{"$eq": {}}`, a narrower filter than the one built.
- **jmap-chat-server**: a Direct `Chat/set` create whose duplicate
  re-check read fails, or that loses the duplicate race, now counts the
  committed create as a mutation, so `newState` is re-read (RFC 8620 §5.3)
  even when the rollback destroy also fails and the Chat stays live.
- **jmap-vtc-server**: the `"pending"` to `"active"` transition after a
  `VTCParticipant/set` join is best-effort, as for update effects; a
  backend failure there no longer reports the stored join as `notCreated`
  with a stale `newState`.

- **jmap-scene-server**: `SceneRegion/set` normalizes an accepted
  `spawnOrientation` to unit length before storage and reports the
  normalized value in `created`/`updated` (draft-atwood-jmap-scene-00
  §Coordinate System: "If the server does not reject the value, it MUST
  normalize the quaternion to unit length before storage"). Its
  magnitude tolerance is now 1e-3, the one SceneObject and SceneAvatar
  `orientation` use, so the draft's three-decimal quaternions such as
  `[0, 0.707, 0, 0.707]` are accepted. Before, the tolerance was 1e-6
  and accepted values were stored unnormalized.
- **jmap-vtc-server**: `VTCRecording/set` destroy of a recording in state
  `"recording"` or `"paused"` first stops it, setting `stoppedAt`, and
  delivers a `VTCRecordingStateEvent` with `state: "stopped"`
  (draft-atwood-jmap-vtc-00 §Destroying a Recording: "the server first
  stops it and delivers the `VTCRecordingStateEvent` with
  `state: "stopped"`"). Before, the recording was removed with no stop
  and no consent event.
- **jmap-vtc-server**: ending a call, by `VTCCall/set` or `end_call`,
  also stops its livestreams and those of its breakout rooms in state
  `"starting"` or `"live"`, setting `stoppedAt`
  (draft-atwood-jmap-vtc-00 §Ending a Call: "the server stops ... every
  VTCLivestream in state `"starting"` or `"live"`, as if a moderator had
  set each to `"stopped"`"). Before, they stayed active.
- **jmap-vtc-server**: `should_deliver` delivers a
  `VTCRecordingStateEvent` even when the subscription's `eventTypes`
  does not list it; the `callIds` and current-participant checks still
  apply (draft-atwood-jmap-vtc-wss-00 §VTCStreamEnable: "the server
  delivers it for every call the subscription covers whether or not
  `eventTypes` lists it").
- **BREAKING** **jmap-vtc-server**: a ring `VTCCall/set` create now joins
  the initiator: its VTCParticipant record has `joinedAt` equal to the
  call's `createdAt` and the call's `activeParticipantCount` is 1
  (draft-atwood-jmap-vtc-00 §Creating a Ring Call, "with `joinedAt` set
  to the creation time, since the initiator is in the call from the
  start"). `VtcBackend::create_call` implementations must follow the
  revised contract; `MemoryBackend` does.
- **jmap-chat-server**: when the duplicate re-check read after a direct
  `Chat/set` create fails, the handler destroys the new Chat and reports
  that create in `notCreated` as `serverFail`, and the rest of the batch
  goes on. It used to fail the whole method with `serverFail` even though
  the Chat and earlier changes in the batch were already committed (RFC
  8620 §3.6.2: "The method call made no changes to the server's state").
- **BREAKING** **jmap-chat-server**: a `Message/set` create throttled by
  `ChatBackend::slow_mode_check` is rejected in `notCreated` with the RFC
  8620 §5.3 `rateLimit` SetError (`SetErrorType::RateLimit`), not the
  custom `rateLimited` (draft-atwood-jmap-chat-00 §Chat `slowModeSeconds`:
  "with a `rateLimit` SetError ({{RFC8620, Section 5.3}}) on the offending
  `Message/set` create"; `rateLimited` is the method-level error of the
  Peer/* methods). The `serverRetryAfter` extra field is unchanged.
- **BREAKING** **jmap-chat-client**: `message_create` maps a `rateLimit`
  SetError on its creation key to `ClientError::RateLimited`, not the
  custom `rateLimited` (draft-atwood-jmap-chat-00 §Chat `slowModeSeconds`:
  "Clients MAY simply attempt the send and react to `rateLimit`"). A
  `rateLimited` SetError now comes back in `not_created` on `Ok`. Since
  RFC 8620 defines no `serverRetryAfter`, a `rateLimit` without it yields
  `RateLimited` with `retry_after` set to the receive time, as for an HTTP
  429 without `Retry-After`, instead of `UnexpectedResponse`.
- **BREAKING** **jmap-filenode-server**: `FileNode/set` rejects an update
  that sets `role` to `chatspace` on a node that is not a Space file tree
  root, with `invalidProperties` naming `role`, as it already did for an
  update moving a root off that role (draft-atwood-jmap-chat-filenode-00
  §Creating the File Tree Root: "A server MUST reject a `FileNode/set`
  update that changes `role` to or from `"chatspace"`").
- **BREAKING** **jmap-chat-server**: `Message`, `Space`, `ChatContact` and
  `CustomEmoji` `/queryChanges` now pass the request's `filter` and `sort`
  to the backend, and every chat `/queryChanges` applies the draft's
  default sort when `sort` is absent, as the `/query` handlers do (RFC 8620
  §5.6). They now answer `accountNotFound` for an unknown account and
  `unsupportedSort` for an undefined sort property, and a `Message`
  `/queryChanges` scoped to a Chat the caller cannot view fails with
  `cannotCalculateChanges` (draft-atwood-jmap-chat-00 §Message/query).
- **jmap-chat-server**, **jmap-tasks-server**: the reference
  `MemoryBackend::query_changes` re-runs the query with its `filter` and
  `sort`, as `jmap-mail-server` does, so it no longer announces ids that
  `/query` hides (Messages in unviewable channels, non-member Spaces) or
  ignores a Task `taskListId` filter. Updated ids that left the results
  are now in `removed`, and those that entered them are in `added`.
- **BREAKING** **jmap-chat-server**: with `supportsThreads` on, a Message
  create or destroy now records the `replyTo` parent as an updated Message,
  and a ReadPosition update that moves `lastReadMessageId` records each
  affected thread root, so `Message/changes` reports `replyCount` and
  `unreadReplyCount` changes (draft-atwood-jmap-chat-00 §Message, RFC 8620
  §5.1: "If the data changes, this string MUST change"). `ChatBackend`
  implementors MUST do the same; see "State and change log" on
  `ChatBackend::create_object`. `MemoryBackend` does.
- **jmap-chat-server**: `Message/set` update now applies the checks that
  create already did (draft-atwood-jmap-chat-00 §Account-Level Capability
  Object, §Fields Inside the Ciphertext): an edited `body` over
  `maxBodyBytes` is a `tooLarge` SetError, and an edit leaving the Message
  with an encrypted `bodyType` and any of `mentions`, `broadcastMentions`,
  `attachments`, `actions`, `replyTo`, `threadRootId`, `topicId` or
  `subject` non-empty (stored or patched) is an `invalidProperties`
  SetError listing each such field.
- **jmap-tasks-server**: `TaskList/set` with `onDestroyRemoveTasks: true`
  now destroys the Tasks in each destroyed TaskList before the list
  (draft-ietf-jmap-tasks-06 §3.4); it used to destroy only the list and
  leave its Tasks behind with no `Task/changes` entry. If a Task destroy
  fails, the list is reported in `notDestroyed` and stays in place. The
  reference `MemoryBackend` now honors a `Task/query` filter of just
  `taskListId`, which the cascade uses; other filters still fail loud.
- **BREAKING** **jmap-chat-server**: a `Chat/set` create carrying a
  property draft-atwood-jmap-chat-00 §Chat Object Fields scopes to another
  kind is an `invalidProperties` SetError naming it, instead of being stored:
  `name`, `description` or `avatarBlobId` on a direct Chat, and `contactId`
  or `spaceId` on a group Chat.
- **jmap-chat-server**: a backend error while `Chat/set` reads the Chats to
  destroy is now a `serverFail` in `notDestroyed` for each id, not a method
  `serverFail` sent after the creates and updates had been committed (RFC
  8620 §3.6.2: "The method call made no changes to the server's state").
- **jmap-chat-server**: `store_server_message` hands `create_object` the
  `"placeholder"` id that `ChatBackend::create_object` documents, not the
  caller's `msg.id`.
- **jmap-chat-server**: `Space/set` create no longer copies a key listed by
  `ChatBackend::space_extension_properties` into `Space::extra` when it is a
  core Space property or a structural patch key, as that method documents.
- **jmap-chat-server**: `Space/set` update rejects a listed `closedAt` with
  `invalidProperties` instead of passing it to
  `ChatBackend::apply_space_metadata_patch`; `closedAt` is server-set
  (draft-atwood-jmap-chat-00 §Space).
- **jmap-chat-server**: `Chat/set` and `Message/set` updates read the stored
  Chat, or the stored Message and its Chat, once per target and reuse it for
  every check, instead of re-reading it per check.
- **BREAKING** **jmap-chat-server**: `Message/set` create whose `chatId`
  names no stored Chat is rejected with `invalidProperties` naming
  `chatId` instead of being stored and returned in `created`
  (draft-atwood-jmap-chat-00 §Input Validation).

## [0.3.0] - 2026-10-20

The JMAP Chat crates (`jmap-chat-types`, `jmap-chat-server`,
`jmap-chat-client`) are **pre-stable** in 0.3.x: their API, and the
`ChatBackend` trait in particular, will keep changing until the 0.4 API
freeze. The other crates follow the usual pre-1.0 rule above.

### Breaking: Rust API

- **all type and client crates, jmap-server, jmap-mail-server,
  jmap-calendars-server, jmap-chat-server**: every wire-format JSON object
  map is a `std::collections::BTreeMap` instead of a `HashMap` (e.g.
  `Email.mailbox_ids`/`keywords`, `ContactCard.localizations`/
  `address_book_ids`, `Task.localizations`, `TimeZoneRule.recurrence_overrides`,
  `SetResponse.created`/`not_updated`, `JmapRequest`/`JmapResponse.created_ids`,
  `StateChange.changed`, `/set` `update` parameters, `state_change_for`,
  `MdnSendResult`, `CalendarEventParseResult`). Serialized maps are now in
  sorted key order and byte-stable across runs. Replace `HashMap` with
  `BTreeMap` at call sites. `Id`, `State`, `Keyword` and `LocalDateTime`
  now derive `PartialOrd` and `Ord`.
- **jmap-types**: `Invocation` is a struct with public `name`, `arguments`
  and `call_id` fields instead of the alias `(String, serde_json::Value,
  String)`. The wire form is unchanged (RFC 8620 §3.2 3-element array).
  Replace `inv.0` / `inv.1` / `inv.2` with `inv.name` / `inv.arguments` /
  `inv.call_id`, and tuple literals with `Invocation::new(name, arguments,
  call_id)`; `From` conversions to and from the tuple are provided.
- **jmap-server**: `HandlerFuture` resolves to `Result<HandlerOutput,
  JmapError>` instead of `Result<(Value, Vec<Invocation>), JmapError>`, and
  every extension crate's `handle_*` function returns
  `Result<HandlerOutput, JmapError>`. `HandlerOutput` is `#[non_exhaustive]`
  with `primary` and `extra_invocations` fields; build it with
  `HandlerOutput::new(primary)` and `.with_extra(invocations)`.
- **jmap-server**: `JmapBackend::query_objects` gains `anchor: Option<&Id>`
  and `anchor_offset: i64`, and returns
  `Result<QueryResult, BackendQueryError<Self::Error>>`. Implementors must
  apply the RFC 8620 §5.5 anchor rules (`resolve_query_start` does this for an
  in-memory id list). `?` on `Self::Error` still compiles through
  `From<E> for BackendQueryError<E>`; a bare `Err(e)` must become
  `Err(e.into())`.
- **jmap-server**: `PushSubscriptionBackend` gains required methods
  `create_push_subscription`, `update_push_subscription` and
  `destroy_push_subscription`.
- **jmap-server**: `enforce_max_objects_in_set` now fails with
  `JmapError::request_too_large()` (description `maxObjectsInSet`) instead
  of `JmapError::limit("maxObjectsInSet")`. Code matching
  `error_type == "limit"` must match `"requestTooLarge"`.
- **jmap-types**: `StateChange` moved here from `jmap-base-client` (still
  re-exported there) and has hand-written serde: it always serializes
  `"@type":"StateChange"` and rejects any other `@type`.
- **jmap-mail-types**: the eight property enums (`MailboxProperty`,
  `ThreadProperty`, `EmailProperty`, `IdentityProperty`,
  `EmailSubmissionProperty`, `VacationResponseProperty`,
  `SearchSnippetProperty`, `SieveScriptProperty`) are `#[non_exhaustive]`.
  External matches need a wildcard arm.
- **jmap-tasks-types**: `TasksAccountCapability.min_date_time` and
  `max_date_time` are `LocalDateTime` instead of `String`. The wire format is
  unchanged.
- **jmap-mail-server, jmap-chat-server, jmap-calendars-server,
  jmap-tasks-server, jmap-contacts-server, jmap-filenode-server,
  jmap-sharing-server**: `supports_type` takes `(caller, account_id)` and has
  no default, because RFC 8620 §2 capabilities are per account.
- **jmap-filenode-server**: `FileNodeBackend::query_subtree` returns
  `Result<Vec<Id>, BackendQueryError<Self::Error>>`.
- **jmap-chat-server**: new required `ChatBackend::read_and_burn_message`
  (atomic burnOnRead) and `ChatBackend::join_space_with_invite` (atomic invite
  redemption, with `InviteJoinRefusal`). Neither has a default.
- **jmap-chat-server**: backends must look up the caller's Space membership
  and permissions by `account_id` (the owner's member entry), not by
  `principal_id`. The `apply_space_metadata_patch` contract no longer lets a
  backend skip the gate when `principal_id` returns `None`.
- **jmap-chat-server**: removed the `#[doc(hidden)]` `MemoryBackend` helpers
  `apply_space_patch_with_caller_id`,
  `apply_space_metadata_patch_with_caller_id`,
  `may_set_pinned_messages_with_caller_id` and
  `may_set_space_ban_with_caller_id`. Use the `ChatBackend` trait methods.
- **jmap-chat-types**: `SenderId` is removed. `Message.sender_id`,
  `Reaction.sender_id`, `ChatTypingEvent.sender_id`,
  `ChatMessageEntry.sender_id` and the `sender_id` parameters of
  `Message::new`, `ChatMessageEntry::plaintext` and
  `ChatMessageEntry::encrypted` are `jmap_types::Id`. Replace
  `SenderId::Owner` with the accountId.
- **jmap-chat-types**: `Mention` and `BroadcastMention` drop `offset` and
  `length`. If a peer sends them, they land in `extra`.
- **jmap-chat-types**: `ChatContact` has a new required `identity: String`
  (JSON without `identity` fails to deserialize), and `ChatContact::new` takes
  `identity` as its second parameter.
- **jmap-chat-types**: `SpaceRole.color` is `Option<serde_json::Value>` and
  `RolePatch.color` is `Option<Clearable<serde_json::Value>>`, covering all
  three draft Color Representation forms.
- **jmap-chat-types**: six `ChatMessageEntry` fields are now `Option`:
  `message_id`, `chat_id`, `chat_kind`, `sender_id`, `sent_at` and
  `encrypted`. Each is omitted when `None`.
- **jmap-chat-types**: chat `Filter` and `Comparator` are typed per object in
  the new `jmap_chat_types::query` module instead of `serde_json::Value`.
- **jmap-chat-types**: `vocabulary::SPEC_PERMISSION_NAMES` gains
  `"start_call"` (9 to 10 entries), matching the draft.
- **jmap-chat-client**: the `*_query_changes` methods take the typed
  `filter`, `sort`, `up_to_id` and `calculate_total` arguments.
- **jmap-chat-client**: `SpaceAddRoleInput.color` is a JSON value
  (`with_color` takes `impl Into<serde_json::Value>`). `MessageQueryInput`
  `after`/`before` are deprecated, and the client rejects them with
  `ClientError::InvalidArgument`.
- **jmap-chat-server**: `PushItem`, `OwnerFlags` and `BuiltPush` are not
  `#[non_exhaustive]`, so a field added to any of them later is a breaking
  change.
- **jmap-types and all type crates**: `FilterOperator` and every
  `*FilterCondition` struct carry `#[serde(deny_unknown_fields)]`, so
  deserializing a filter with a key the type does not define fails.
  `jmap_mail_types::MAILBOX_FILTER_CONDITION_KEYS` and the
  `jmap_chat_types::query::*_FILTER_CONDITION_KEYS` constants are removed;
  the typed decode now does their job.
- **jmap-mail-client, jmap-calendars-client, jmap-tasks-client,
  jmap-contacts-client, jmap-filenode-client, jmap-sharing-client,
  jmap-chat-client**: every `*_query` and `*_query_changes` method that
  takes a `filter` now takes `Option<&jmap_types::query::Filter<C>>`,
  generic over `C: Serialize`, so callers can send RFC 8620 §5.5
  `AND` / `OR` / `NOT` operators. `C` is normally the crate's
  `*FilterCondition`; `jmap_metadata_types::WithMetadata<C>` adds the
  draft-ietf-jmap-metadata-02 §3.5 conditions. Wrap a bare condition in
  `Filter::Condition`; a `None` filter needs its type named
  (`None::<&EmailFilter>`). `search_snippet_get` takes `&Filter<C>`. The
  chat client's `*_query` methods (which take `*QueryInput`) are unchanged.
  Methods affected: mail `email_query`, `email_query_changes`,
  `mailbox_query`, `mailbox_query_changes`, `email_submission_query`,
  `email_submission_query_changes`, `search_snippet_get`; calendars
  `calendar_event_query`, `calendar_event_query_changes`,
  `calendar_event_notification_query`,
  `calendar_event_notification_query_changes`; tasks `task_query`,
  `task_query_changes`, `task_notification_query`,
  `task_notification_query_changes`; contacts `contact_card_query`,
  `contact_card_query_changes`; filenode `filenode_query`,
  `filenode_query_changes`; sharing `principal_query`,
  `principal_query_changes`, `share_notification_query`,
  `share_notification_query_changes`; chat `chat_query_changes`,
  `message_query_changes`, `space_query_changes`,
  `chat_contact_query_changes`, `custom_emoji_query_changes`.
- **jmap-mail-client, jmap-calendars-client, jmap-tasks-client,
  jmap-contacts-client**: `sort` is a typed comparator slice where the
  types crate defines one: `&[EmailComparator]` (Email query and
  queryChanges), `&[CalendarEventComparator]`, `&[TaskComparator]`,
  `&[TaskNotificationComparator]`, `&[ContactCardComparator]`.
  `calendar_event_notification_query_changes` sort is
  `&[serde_json::Value]`, matching its `/query`. Mailbox, EmailSubmission,
  FileNode, Principal and ShareNotification sort stays
  `Option<serde_json::Value>`.
- **jmap-calendars-server**: `CalendarsBackend::get_availability` and
  `CalendarsBackend::parse_calendar_event_blobs` no longer have default
  implementations. The old defaults reported every principal as free
  (draft-ietf-jmap-calendars-26 §2.2) and every blob as `notParsable`
  (§5.13). Implement both; a backend without the feature says so in its
  own override, as `MemoryBackend` does.
- **jmap-calendars-server, jmap-chat-server, jmap-contacts-server,
  jmap-sharing-server, jmap-tasks-server**: the `realistic-demo-ids`
  cargo feature is removed. `MemoryBackend` mints every server-assigned id
  as a uuid v4, as `jmap-mail-server` does, instead of the
  `"<type><n:016x>"` counter form. Drop the feature from `Cargo.toml` and
  read ids from `/set` `created` responses instead of predicting them.
  Their `memory` feature now pulls in the optional `uuid` dependency (`v4`,
  `std`).
- **jmap-mail-client**: `EmailCopyParams` gains `if_from_in_state` and
  `if_in_state` (RFC 8620 §5.4), so struct literals must set them.
- **jmap-calendars-client, jmap-tasks-client, jmap-contacts-client**:
  `calendar_event_copy`, `task_copy` and `contact_card_copy` take the new
  `CalendarEventCopyParams`, `TaskCopyParams` and `ContactCardCopyParams`
  instead of a `&Id` from-account. **jmap-filenode-client**:
  `FileNodeCopyParams` gains `if_from_in_state`, `if_in_state`,
  `on_success_destroy_original` and `destroy_from_if_in_state`. Unset
  options are omitted from the wire.
- **jmap-tasks-server**: `TasksBackend::is_per_user_property` is removed.
  The draft-ietf-jmap-tasks-06 §4.5.1 per-user property list is a fixed rule,
  now `jmap_tasks_types::is_per_user_task_property`, and `Task/set` uses it.
  Delete any implementation of the old method.

### Breaking: wire behaviour

- **All servers**: a `/query`, `/queryChanges` or `SearchSnippet/get`
  `filter` with an unknown or misspelt key at any depth, an extra key on an
  operator node, or an operator tree sent to an object whose filter is a
  bare FilterCondition returns `unsupportedFilter` (RFC 8620 §5.5). Before,
  most such filters decoded as an empty condition and matched every record.
  `Email/query`, `Email/queryChanges`, `Mailbox/query`,
  `EmailSubmission/query`, `SieveScript/query`, `SearchSnippet/get` and
  `CalendarEvent/query` return `unsupportedFilter` instead of
  `invalidArguments` for a filter object that fails to decode; a non-object
  filter is still `invalidArguments` (except `SearchSnippet/get`, which
  returns `unsupportedFilter` for any bad filter, RFC 8621 §5.1).
- **All servers**: a `/set` over `maxObjectsInSet` returns method error
  `requestTooLarge` instead of `limit`.
- **All servers**: a `/get` (including `SearchSnippet/get` `emailIds`) whose
  ids exceed `JmapBackend::max_objects_in_get` (default 500) returns
  `requestTooLarge` with description `maxObjectsInGet`. Before, there was no
  cap.
- **All servers**: `/query` returns a backend's RFC 8620 §5.5 method error
  (`unsupportedFilter`, `unsupportedSort`, `anchorNotFound`, ...) unchanged
  instead of `serverFail`. `*/query` via `handle_query` and the chat
  `Message`, `Space`, `ChatContact` and `CustomEmoji/query` honour
  `anchor`/`anchorOffset`, return `anchorNotFound` for an absent anchor and
  `invalidArguments` for a mistyped one.
- **jmap-mail-server, jmap-calendars-server, jmap-filenode-server**: `/copy`
  honours `destroyFromIfInState`. On a mismatch it returns `stateMismatch` and
  destroys nothing.
- **jmap-base-client**: an SSE `state` event whose `@type` is present but not
  `StateChange` parses as `SseEvent::Unknown`.
- **jmap-chat-server**: owner-authored Messages and reactions carry the
  accountId as `senderId` (was `"self"`). Stored `"self"` values are treated as
  another sender. `Message/query` `hasMention` matches only a mention whose id
  is the accountId.
- **jmap-chat-server**: owner references (Chat creator and `invitedBy`,
  Space/join member id, `bannedBy`, `createdBy`, self-removal and self-nick
  keys) use the accountId instead of the principal.
- **jmap-chat-server**: `Space/get` decides membership by the accountId
  member entry whatever `principal_id` returns. Single-user deployments no
  longer see the full object of Spaces they are not a member of.
- **jmap-chat-server**: `Space/set` create returns the creator as sole admin
  member (one `admin` role, `memberCount` 1). The creator's `Space/join` on
  that Space now fails as already a member. `MemoryBackend` with
  `principal_id` `None` enforces Space, pin and ban permission gates against
  the owner's member entry.
- **jmap-chat-server**: in a channel, `Message/set` with non-empty
  `broadcastMentions` returns `forbidden` unless the owner holds
  `mention_broadcast`. Broadcast gating keys only on the
  `broadcastMentions` array. Rich-body spans are no longer parsed, and
  mention offsets are no longer bounds-checked.
- **jmap-chat-server**: `Message/set` update accepts only the draft's edit,
  reaction, delete and read properties; any other key returns
  `invalidProperties`. Edits of another sender's or a deleted Message return
  `forbidden`. A non-null `readDisposition` needs a non-null `readAt`, and
  `readAt: null` clears `readDisposition`.
- **jmap-chat-server**: `Message/set` create validates `bodyType` against
  `ChatLimits::supported_body_types` and refuses
  `server_reserved_body_types`. It rejects mention, attachment, reply and
  topic properties on `application/mls-ciphertext`, and a body over 100 000
  bytes, with the SetErrors listed below. It stores `broadcastMentions`, and it
  creates the owner's ReadPosition for the Chat.
- **jmap-chat-server**: `/set` create rejects a wrongly typed optional
  property with `invalidProperties` instead of ignoring it.
- **jmap-chat-server**: `ReadPosition/set` create and destroy return
  `forbidden`. `Chat/set` with kind `channel` returns `forbidden`. `Chat/set`
  update rejects `closedAt` and `activeCallId`. `Message/query` with
  non-null `text` or `threadRootId` returns `unsupportedFilter`.
- **jmap-chat-server**: `Chat/typing` for an unknown chat or a non-member
  group chat returns `invalidArguments`.
- **jmap-chat-server**: `Space/join` with a nonexistent or non-public Space
  returns `notPermitted`, and it returns `forbidden` for a banned caller or a
  closed Space.
- **jmap-chat-server**: the last admin leaving a group Chat or Space with
  members remaining sets `closedAt`. Closed groups and Spaces refuse
  messages, membership changes, joins and invites.
- **jmap-chat-server**: the `Space/set` last-admin refusal is
  `invalidProperties ["removeMembers"]` (was `forbidden`), and self-removal no
  longer needs `manage_members`.
- **jmap-chat-server**: `SpaceBan/set` requires the `ban` permission.
  `SpaceInvite/set` is gated by `ChatBackend::may_set_space_invite`.
  `SpaceInvite/get`, `SpaceInvite/changes` and `SpaceBan/get` show only the
  records the draft visibility rules allow.
- **jmap-chat-server**: `ChatContact/set` create of `{identity}` succeeds
  (was always `forbidden`). An update that patches `identity` is rejected
  with `invalidProperties`.
- **jmap-chat-server**: `Space/set` `addRoles` and `addCategories` entries
  without `id` are accepted, and the server assigns unique ids. Moving a
  channel between categories removes it from the old category.
- **jmap-chat-server**: `Space/set` destroy cascades to the Space's channel
  Chats and their Messages, and `/changes` reports those destroys.
- **jmap-chat-server**: `Space/set` `updateMembers` `roleIds` changes need
  only `manage_roles` (was `manage_roles` plus `manage_members`), and every
  role granted or removed must rank below the caller's highest role (was
  checked only for the roles in the new set).
  `required_permissions_for_op` returns `[manage_roles]` for a `roleIds`-only
  entry.
- **jmap-chat-server**: `Message/set` create rejections in `notCreated` use
  RFC 8620 SetErrors instead of `invalidArguments` (jmap-chat-spec 91672c9):
  `tooLarge` for a body over `maxBodyBytes`; `invalidProperties` with
  `properties: ["broadcastMentions"]` for an unrecognized broadcast scope;
  `invalidProperties` listing each non-empty ciphertext-only field for an
  encrypted `bodyType`; and `invalidProperties` with
  `properties: ["bodyType"]` for a server-reserved `bodyType`.

### Added

- **jmap-metadata-types**: `WithMetadata<C>`, a Serialize-only wrapper that
  flattens a per-type `FilterCondition` and adds the six
  draft-ietf-jmap-metadata-02 §3.5 condition fields, for use as the leaf of
  a client `Filter`. `MetadataTextMatch::new`.
- **jmap-server**: `filter_arg`, which decodes a hand-written handler's
  `filter` argument and maps failures to `unsupportedFilter` or
  `invalidArguments` (RFC 8620 §5.5).
- **jmap-types**: `push` module with `PushSubscription`, `PushKeys` and
  `PushVerification`, plus `StateChange::new`.
- **jmap-server**: `PushSubscription/get` and `PushSubscription/set` handlers
  (`register_push_subscription_handlers`), `StoredPushSubscription`,
  `MAX_PUSH_EXPIRY_SECS`, `is_deliverable`, `state_change_for`,
  `utc_date_from_epoch_seconds`, `BackendQueryError`, `query_anchor_args`,
  `resolve_query_start`, `enforce_max_objects_in_get` and the provided method
  `JmapBackend::max_objects_in_get` (default 500). A downstream type that
  already has a method with that name may hit method-resolution ambiguity.
- **jmap-server**: `BackendSetError::into_set_error_or_method_error`, which
  returns the per-object `SetError` or a `serverFail` that never carries the
  backend error's `Display` text.
- **jmap-chat-server**: chatPush support from draft-atwood-jmap-chat-push-00:
  `push::validate_chat_push`, `strip_unsupported_chat_push`,
  `chat_state_change_for`, `build_chat_message_push` and `should_push`, plus
  a `MemoryBackend` `PushSubscriptionBackend` capped at 50 subscriptions per
  user.
- **jmap-chat-server**: provided `ChatBackend` hooks `owner_user_id`,
  `may_set_space_ban`, `may_set_space_invite`, `may_get_space_invite` and
  `may_get_space_ban`. The permission hooks default to fail-open.
- **jmap-chat-server**: `ChatLimits` fields `max_group_chat_members`,
  `supported_body_types` and `server_reserved_body_types`, with setters, and
  the constants `message::SUPPORTED_BODY_TYPES`,
  `permissions::MENTION_BROADCAST` and `permissions::BAN`.
- **jmap-chat-types**: typed `Message.topic_id`, `subject` and
  `expires_after_read_seconds`, `Chat.closed_at` and `Space.closed_at`,
  `ChatMessageEntry.unread_count`, `ChatCapability.role`
  (`ChatCapabilityRole`), `JMAP_CHAT_WEBSOCKET_URI`, `ChatMember::new` and
  `ChatContactProperty::Identity`. Keys that used to land in `extra` now fill
  the typed fields.
- **jmap-chat-client**: every Message/set create property, Message/query
  sort, anchor and `calculateTotal`, CustomEmoji and SpaceBan update, Core/echo,
  the `chat:filenode` binding, typed batching with back-references,
  `chat_contact_create`, `ChatContactQueryInput.filter_identity`, and the
  `ChatSessionExt` accessors `owner_user_id`, `owner_login`,
  `owner_endpoints` and `owner_direct_address`.
- **jmap-base-client**: EventSource reconnect with `Last-Event-ID`, WebSocket
  keepalive, typed push enable and disable, a typed `WsFrame::RequestError`
  with `requestId`, HTTP 429 as `RateLimited`, `MethodError` extra
  properties, and streaming blob upload and download.
- **jmap-tasks-types**: `PER_USER_TASK_PROPERTIES` and
  `is_per_user_task_property`, mirroring
  `jmap_calendars_types::is_per_user_calendar_event_property`.
- **Clients**: every `/copy` method can send the RFC 8620 §5.4
  `ifFromInState`, `ifInState`, `onSuccessDestroyOriginal` and
  `destroyFromIfInState` arguments, so a client can state-guard a copy or
  move objects in one request.

### Fixed

- **Clients**: a colliding key in a params struct's `extra` map no longer
  overrides the typed field on the wire.
- **jmap-calendars-server**: `Calendar/set` destroy no longer leaks backend
  error text into `serverFail`.
- **jmap-filenode-server**: `FileNode/set` rejects a mistyped `parentId` with
  `invalidProperties ["parentId"]`. `MemoryBackend` honours `position`, and it
  reports `total` before `limit` is applied.
- **jmap-chat-server**: `MemoryBackend::query_objects` honours `sort`, and
  group `Chat/set` create honours `memberIds`.

## [0.2.0] - 2026-10-18

Every publishable crate moved from the 0.1 line to `0.2.0`.

### Breaking: Rust API

- **jmap-mail-types**: `MailboxFilterCondition.parent_id` changed from
  `Option<Id>` to `Option<serde_json::Value>` (869f7fb). Serde's default
  `Option` deserializer collapses JSON `null` to `None`, so the old type could
  not keep RFC 8621 §2.3's three-way absent / `null` / Id `parentId` filter.
  Code that read `cond.parent_id` as an `Id` must handle the JSON value.
- **jmap-types**: the response-side state fields are the `State` newtype
  instead of `String`: `GetResponse.state`, `ChangesResponse.old_state` and
  `new_state`, `SetResponse.old_state` and `new_state` (f084943), and
  `QueryResponse.query_state` and `QueryChangesResponse.old_query_state` and
  `new_query_state` (25e5021). jmap-mail-client's `EmailImportResponse`
  state fields changed the same way. The wire format is unchanged. `State`
  implements `PartialEq<str>`, `AsRef<str>` and `From<String>`, but bindings
  typed as `String` break.
- **jmap-types**: `GetResponse`, `ChangesResponse`, `SetResponse`,
  `QueryResponse`, `QueryChangesResponse`, `AddedItem`, `JmapRequest` and
  `JmapResponse` gain a flattened `extra` map that preserves unknown fields
  (bae3a90), and `ChangesResponse` gains `updated_properties` (52a37049).
  The structs are `#[non_exhaustive]`, but in-crate struct literals had to
  change.
- **jmap-types**: `SetError.extra` changed from `HashMap` to
  `serde_json::Map` (9c66783). Accessor methods are unchanged, and serialized
  output is now insertion-ordered.
- **jmap-server**: patch helpers and `PatchPathError` changed. `ChatBackend`
  and `CalendarsBackend` have new behaviour, and the `*-types` crates have new
  serde defaults. See `RELEASING.md` for the 0.2.x line.

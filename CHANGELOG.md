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
- **jmap-vtc-client** (new crate, `publish = false`): binds every
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
  `false`). When it returns `true`, `FileNode/get` emits `"sha256": null`
  for directories and for files without a stored digest, since with
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
  land in `extra`.
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
  `urn:jmap:chat:cap:task` MessageAction is mapped as a user reference
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
  `urn:jmap:chat:cap:task` MessageAction metadata shape `TaskMetadata`, and
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
  or failed. `VTCLivestream/get` returns `streamKey` and `streamUri` as
  `""` to non-moderators. `VtcBackend` gains the required
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
  name the `urn:jmap:chat:cap:*` Endpoint / MessageAction types.
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

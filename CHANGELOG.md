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

- **jmap-chat-types**: `JmapUri` parses and builds the private-use
  `jmap:<type>:<accountId>:<objectId>` URI
  (draft-atwood-jmap-chat-calendars-00 §MessageAction format), including the
  four-part `jmap:task:<accountId>:<taskListId>:<taskId>` form
  (draft-atwood-jmap-chat-tasks-00 §MessageAction format), validating every
  id segment per RFC 8620 §1.2. `vocabulary::CHAT_CAP_VTC`,
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
  `ChatBackend::expire_message`; clears `muted` and `muteUntil` once
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

### Fixed

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

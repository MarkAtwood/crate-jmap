# jmap-chat-server

> **Pre-stable (0.x).** The JMAP Chat crates (`jmap-chat-types`, `jmap-chat-server`,
> `jmap-chat-client`) do not yet have a stable API. Expect breaking changes,
> especially new required `ChatBackend` methods, in any minor release before 1.0.

## What it is

JMAP Chat extension method handlers and the `ChatBackend` trait
([draft-atwood-jmap-chat]). Backend-agnostic — plugs into
`jmap-server::Dispatcher`. Consumers implement `ChatBackend` for their
storage; the crate handles all wire protocol, RFC 8620 conformance, and
non-trivial concurrency logic (Direct chat dedup, `Space/join` TOCTOU,
invite-use accounting).

## What it's for

Implements the JMAP Chat extension (draft-atwood-jmap-chat-00) — Chat,
Message, Space, ChatContact, ReadPosition, SpaceInvite, SpaceBan, ChatBan,
CustomEmoji, and PresenceStatus objects — so consumers can wire chat method
dispatch into their HTTP transport. Sibling to `jmap-mail-server` (the
canonical extension-server template) and the other `jmap-*-server` crates;
all of them inter-depend through the workspace `JmapHandler` trait in
`jmap-server`. The consumer supplies a `ChatBackend` impl (storage), a
`CallerCtx` type (auth identity), and wires the dispatcher into HTTP / SSE
/ WebSocket transport themselves.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_chat_server::{ChatBackend, register_chat_handlers};
use jmap_server::Dispatcher;

// 1. Implement ChatBackend for your storage layer. The impl must also
//    satisfy the JmapBackend supertrait (account_exists, get_objects,
//    get_state, get_changes, query_objects, query_changes) and declare
//    `type Error` and `type CallerCtx`. See the ChatBackend trait
//    section below for the full surface.

// 2. Construct a dispatcher parameterised on your CallerCtx type, then
//    register every Chat method in one call.
let mut dispatcher: Dispatcher<MyCallerCtx> = Dispatcher::new();
register_chat_handlers(&mut dispatcher, Arc::new(my_backend));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, caller_ctx, session_state).await;
```

`Dispatcher<C>` requires `B::CallerCtx = C`; `register_chat_handlers`
enforces the match at the type level. For a single-user dev server use
`Dispatcher<()>` with a backend whose `CallerCtx = ()`. For
multi-tenant production, define a typed caller context (e.g. a
principal-id newtype) and parameterise both sides on that type.

For a runnable, type-checked variant of this example, see the
[crate-level documentation](https://docs.rs/jmap-chat-server) — the
doctest in `lib.rs` is verified by `cargo test --doc`.

After `register_chat_handlers` returns, the dispatcher handles every method
name listed in the [Registered methods](#registered-methods) section below.
Each request's `CallerCtx` value is forwarded into the handler closure (see
[CallerCtx](#callerctx) below).

## Registered methods

`register_chat_handlers` installs handlers for all of the following:

| Object | Methods |
|---|---|
| `Chat` | `get`, `changes`, `query`, `queryChanges`, `set`, `typing` |
| `Message` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `Space` | `get`, `changes`, `query`, `queryChanges`, `set`, `join` |
| `ChatContact` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `ReadPosition` | `get`, `changes`, `set` |
| `SpaceInvite` | `get`, `changes`, `set` |
| `SpaceBan` | `get`, `changes`, `set` |
| `ChatBan` | `get`, `changes`, `set` |
| `CustomEmoji` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `PresenceStatus` | `get`, `changes`, `set` |

## `ChatBackend` trait

```rust,ignore
pub trait ChatBackend: JmapBackend {
    /// Create a new object. `build` receives the backend-assigned id and
    /// returns the object to store. Returns `(assigned_id, created_object)`.
    /// `create_id` is the client-side creation id from the `/set` request.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send;

    /// Apply a partial update (patch) to an existing object.
    /// Returns `Some(updated_object)` if the backend modified any properties beyond
    /// what the client requested (RFC 8620 §5.3 server-set field echo), or `None`
    /// if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    /// Called by the session/capability builder — NOT called internally by handlers.
    fn supports_type<O: JmapObject>(&self, caller: &Self::CallerCtx, account_id: &Id) -> bool;

    /// Generate a cryptographically random invite code.
    /// MUST use a CSPRNG. Do NOT use timestamps or sequential counters.
    fn generate_invite_code(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
    ) -> impl Future<Output = Result<String, Self::Error>> + Send;

    /// Redeem the SpaceInvite `invite_id` for `Space/join`: add the owner to
    /// the invite's Space and increment the invite's `uses`.
    fn join_space_with_invite(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        invite_id: &Id,
        joined_at: &UTCDate,
    ) -> impl Future<Output = Result<Result<(), InviteJoinRefusal>, Self::Error>> + Send;

    /// Apply a `readAt`-setting `patch` to the `burnOnRead` Message
    /// `message_id` and hard-delete it, as one atomic operation.
    fn read_and_burn_message(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        message_id: &Id,
        patch: PatchObject,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Apply a sequence of structural mutations to a Space (§Space/set).
    fn apply_space_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        space_id: &Id,
        ops: Vec<SpacePatchOp>,
    ) -> impl Future<Output = Result<Vec<OpResult>, BackendSetError<Self::Error>>> + Send;

    /// Apply a JSON Merge Patch to a Space's top-level metadata fields.
    fn apply_space_metadata_patch(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        space_id: &Id,
        patch: SpaceMetadataPatch,
    ) -> impl Future<Output = Result<Option<Space>, BackendSetError<Self::Error>>> + Send;

    /// Permissions of the implicit `@everyone` role in `space`, the
    /// permission floor every member holds. No default.
    fn everyone_permissions(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        space: &Space,
    ) -> impl Future<Output = Result<HashSet<ChatPermission>, Self::Error>> + Send;

    /// Blob lookup consulted for each Attachment of a Message create.
    fn attachment_blob(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl Future<Output = Result<Result<Option<AttachmentBlob>, SetError>, Self::Error>> + Send;

    // Methods with defaults (all take `caller: &Self::CallerCtx` first
    // except `retains_edit_history` and `maintains_unread_count`):
    // on_chat_destroyed, on_space_destroyed, on_space_ban_created,
    // on_outbound_event, limits, space_extension_properties,
    // protect_last_admin, retains_edit_history, maintains_unread_count,
    // is_contact_blocked, owner_user_id, may_set_custom_emoji,
    // may_broadcast_mention, may_set_pinned_messages, may_set_space_ban,
    // may_set_space_invite, may_get_space_invite, may_get_space_ban,
    // may_send_message, may_create_message, may_update_message,
    // may_update_chat, filenode_attachment, may_view_chat,
    // validate_pinned_message_ids, slow_mode_check, expire_message.
}
```

This block lists the required methods; see `src/backend.rs` for the full
signatures and contracts of the defaulted hooks.

The read-side operations (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait from `jmap-server`.

## Testing a backend: the conformance suite

The `testkit` feature ships the backend-agnostic tests that check a
`ChatBackend` against draft-atwood-jmap-chat-00 and RFC 8620: state and
change logs, PatchObject paths and `null` defaults, `/query` filters,
sort and anchors, atomic `Space/join` and burn-on-read, `create_message`,
ChatContact identity, thread counts, expiry sweeps, `unreadCount` and the
ReadPosition lifecycle. Each test quotes its oracle in its rustdoc.

A backend crate writes one `ConformanceBackend` impl and one macro line:

```toml
[dev-dependencies]
jmap-chat-server = { version = "0.5", features = ["testkit"] }
```

```rust,ignore
// tests/backend_conformance.rs
use jmap_chat_server::testkit::ConformanceBackend;
use jmap_chat_server::OutboundEvent;
use jmap_types::Id;

impl ConformanceBackend for MyBackend {
    // The CallerCtx of a request made by the owner of `account_id`.
    fn caller(&self, account_id: &Id) -> MyCallerCtx { /* ... */ }
    // Provision an empty account.
    async fn add_account(&self, account_id: &Id, owner_user_id: &str) { /* ... */ }
    // Store a wire-JSON object exactly as given, under its `id`.
    async fn seed(&self, account_id: &Id, type_name: &'static str, object: serde_json::Value) { /* ... */ }
    // Drain enqueued OutboundEvents; `None` skips the event tests.
    fn take_outbound_events(&self) -> Option<Vec<OutboundEvent>> { None }
}

jmap_chat_server::chat_backend_conformance!(MyBackend::new_for_test);
```

The macro emits one `#[test]` per suite test, each on a fresh backend from
the constructor and on its own multi-thread tokio runtime, so the calling
crate needs no tokio macros. Rust's orphan rule needs the impl in the
crate that defines the backend (or a newtype around it), so a backend from
another crate is wrapped. Tests whose assertion depends on a capability
pass without running when the backend lacks it: the thread-count tests
check `limits().supports_threads` (invoke the macro once per setting, in
separate modules, to cover both), the exact `unreadCount` tests run only
when `maintains_unread_count()` is `false`, and the event test runs only
when `take_outbound_events` returns `Some`. `tests/backend_conformance.rs`
in this crate runs the suite on `MemoryBackend`.

## Permission enforcement responsibilities

The handlers in this crate do **no** caller-identity checking. Per the
workspace AGENTS.md "Caller identity (foundation seam)" rule, backends
are canonical for authorization. Caller identity is read via
`JmapBackend::principal_id` on the `caller: &Self::CallerCtx`
parameter every backend method receives. In JMAP Chat the caller
acts as the account's owner, whose Space member entry is the
accountId (draft-atwood-jmap-chat-00 §Terminology), so authorization
splits three ways:

- **Account access** — the backend MUST verify that `caller` may act
  as the owner of `account_id` in `create_object`, `update_object`,
  `destroy_object`, `read_and_burn_message` and the other write
  methods, and return a `forbidden` SetError otherwise. Handlers do
  not check this, so a backend that trusts them lets any caller write
  any account.
- **Space permissions** — the backend decides them in the `may_*`
  hooks, `apply_space_patch`, `apply_space_metadata_patch` and
  `destroy_object::<Space>`, resolving the owner's permissions per
  §Space Permission Resolution (`everyone_permissions` supplies the
  `@everyone` floor). The hooks default to permit (fail-open,
  bd:JMAP-bghk).
- **Owner-relative rules on stored data** — group-admin and Message
  authorship rules are checked by the handler against the stored
  object and the accountId. A backend with concurrent writers
  re-verifies them atomically with the write.

The reference `memory::MemoryBackend` implements the Space-permission
hooks against the owner's member entry (except `may_set_custom_emoji`,
which permits every op), but returns `None` from `principal_id` and
does **not** check account access. Production backends MUST implement
every check below or they will silently accept JMAP requests that
draft-atwood-jmap-chat-00 mandates the server to reject. Denials are
`forbidden` SetErrors.

### Space/set and Space destroy (§Space/set, §Destroying a Space)

1. **`apply_space_patch` gates every op on its permission**:
   `manage_space`, `manage_channels`, `manage_members` or
   `manage_roles`, as `required_permissions_for_op` maps them. Role
   adds and edits, and role assignment, are limited to positions
   strictly below the caller's highest role; "servers SHOULD enforce
   this".

2. **Last-administrator protection.** When `protect_last_admin`
   returns `true`, reject any patch that leaves no member holding
   `manage_members` or `manage_space`; self-removal is exempt.

3. **Destroy requires `manage_space`**, checked atomically in
   `destroy_object::<Space>`: "The caller SHOULD hold `"manage_space"`"
   (jmap-chat-spec 77c3474 L1391); this crate requires it by choice.

### SpaceInvite/set and SpaceBan/set (§SpaceInvite/set, §SpaceBan/set)

4. **SpaceInvite create "Requires `"manage_members"` permission";
   destroy "Requires `"manage_members"` permission or ownership of the
   invite"** — `may_set_space_invite`.

5. **SpaceBan create, update and destroy each "Requires `"ban"`
   permission in the Space"** — `may_set_space_ban`.

6. **Visibility.** "Only members of the Space may retrieve its
   invites. Members with `"manage_members"` permission see all
   invites; other members see only invites they created"
   (`may_get_space_invite`). Members with `ban` see all SpaceBans of
   the Space; a banned user sees only their own (`may_get_space_ban`).

### ChatBan/set and ChatBan/get (§ChatBan Methods)

6a. **ChatBan create "requires the `"admin"` role in the Chat, and the
   Chat MUST be a group Chat"**, else `forbidden`; update and destroy
   require the admin role too. The handler reads the role from the
   stored Chat's `members`, as the `Chat/set` member keys do. A `chatId`
   naming no Chat is `invalidProperties`, and the owner banning
   themselves is `forbidden` (the draft is silent; the owner leaves with
   `removeMembers`). Admins see all bans of their Chat; the banned user
   sees bans whose `userId` is the accountId.

6b. **Writes go through backend hooks.** `create_chat_ban` stores the ban
   and removes the user from `members` ("as `removeMembers` would",
   reporting the `removeMembers` group update); `update_chat_ban` and
   `destroy_chat_ban` take the update and the lift ("Lifting a ban does
   not add the user back"). The defaults use `create_object`,
   `update_object` and `destroy_object`; a backend that binds Chats to a
   MIMI room overrides them to emit the room commits.

6c. **`addMembers` naming a user with a ChatBan in force is `forbidden`**
   (§ChatBan/set: "while the ban lasts"). `sweep_expired` lifts bans whose
   `expiresAt` has passed through `destroy_chat_ban`.

6d. **`kickMembers` (draft-atwood-jmap-chat-mimi-00 §Moderation).** The
   `Chat/set` update key is accepted only as an array of ChatContact ids
   (else `invalidProperties`), taken out of the patch and passed to
   `ChatBackend::kick_chat_members` with the stored Chat; `members` is
   not changed. The draft defines the key only for Chats bound to a MIMI
   room, and this crate has no room model, so the default hook rejects
   every kick with `invalidProperties` naming `kickMembers`, as for any
   key outside §Chat/set "Updating a Chat". A backend with rooms
   overrides it. An empty list is a no-op.

### Message/set and Message reads (§Message/set, §Message/query)

7. **Sender membership and `send`.** "The server SHOULD verify the
   sender is a current member of the target Chat … Non-members SHOULD
   receive a SetError of type `forbidden`" (jmap-chat-spec 77c3474
   L1218), and channel senders need the `send` permission (L1220) —
   `may_send_message`. This crate performs both recommended checks by
   choice.

8. **Broadcast mentions** in a channel require `mention_broadcast`
   on create and update — `may_broadcast_mention`.

9. **Authorship (handler-checked).** Edits are allowed only "on a
   message where `senderId` is the accountId and `deletedAt` is
   absent"; `deletedForAll: true` and reaction changes for other
   senders are `forbidden`.

10. **Channel `view`.** "Messages in channels the caller cannot view
    SHOULD be omitted" (jmap-chat-spec 77c3474 L1301) from `Message/get`
    and `Message/query`, which this crate does by choice —
    `may_view_chat`. A query without `chatId` spans Chats, so
    `query_objects` MUST itself omit them.

11. **Extension-bound Chats.** `may_create_message`,
    `may_update_message` and `may_update_chat` let a binding such as
    draft-atwood-jmap-chat-mimi-00 refuse writes to a Chat it
    restricts.

### Chat/set (§Chat)

12. **Group member changes "all require admin role, except leaving
    the group"** (handler-checked).

13. **`pinnedMessageIds`.** "For group chats, only admins may modify
    this list … For channel chats, only members with the `"pin"`
    Space permission may modify this list" — `may_set_pinned_messages`.

### CustomEmoji/set

14. **Implementation-defined.** `may_set_custom_emoji` decides every
    create, update and destroy, for server-global and Space-scoped
    emoji alike.

### Caller-identity-less backends

The reference `MemoryBackend` returns `None` from `principal_id` and
does not check account access (fail-open, bd:JMAP-bghk) — it is
suitable for single-user dev servers, fixtures, and the workspace's
own integration tests, but NOT for any deployment where more than one
principal can reach the JMAP dispatcher. A deployment that needs
access control MUST override `JmapBackend::principal_id` and implement
each of the checks above; the workspace ships no production-grade
reference impl.

## How it works

### Generic handlers

`*/get`, `*/changes`, `*/query`, and `*/queryChanges` all delegate to generic
implementations from `jmap-server`, ensuring RFC 8620 conformance across every object type
without duplication.

### `Chat/set` — Direct chat deduplication

Creating a `Chat` with `kind: "direct"` must be idempotent: if a Direct chat with the same
member set already exists, the caller should get back the existing chat rather than a
duplicate.

The handler uses optimistic create-then-validate deduplication:

1. Before the create loop, fetch all existing Direct chats for the account once (hoisted to
   avoid N+1 queries).
2. Attempt the create normally.
3. After the create, re-fetch to catch concurrent races where two clients create the same
   Direct chat simultaneously.
4. The canonical winner is the object with the lexicographically smallest ID. The loser
   destroys itself and returns an `alreadyExists` SetError pointing at the winner's ID.

### `Space/join` — invite-code and TOCTOU handling

`Space/join` is a non-standard method (not `/set`) that adds the caller as a member. It
accepts either an `inviteCode` or a `spaceId` (for public spaces).

- After the join write succeeds at the storage layer, the handler re-reads the member list
  to detect concurrent joins. If two callers both succeed, the racer with the later
  `joinedAt` timestamp undoes its write and returns a retryable error.
- `SpaceInvite.uses` is incremented only on the success path (after the TOCTOU check), so
  race-losses do not silently exhaust invite capacity.

### `Chat/typing` — ephemeral push

`Chat/typing` stores no state. It fires a push event via `JmapBackend::push_event` if the
backend implements push, then returns immediately. The wire response carries no payload
beyond the standard invocation envelope.

### Conditional Foo/set (draft-ietf-jmap-conditional-02)

`register_chat_conditional(dispatcher, backend)`, called after
`register_chat_handlers`, attaches `SetSupport` to each registered chat `/set`
(Chat, Message, Space, ChatContact, ReadPosition, SpaceInvite, SpaceBan,
ChatBan, CustomEmoji, PresenceStatus) and skips the rest; it re-registers nothing, and a
deployment that never calls it is unchanged (docs/design/DECISIONS.md decisions 1
and 17). For `PushSubscription/set` call
`jmap_server::register_push_subscription_conditional` too. Advertise
`urn:ietf:params:jmap:conditional` only when `Dispatcher::conditional_ready()` is
true.

The dispatcher evaluates `ifUnchangedBy` through each type's `/get` and runs the
call inside the backend's `ConditionalBackend::begin_set` scope. `MemoryBackend`'s
store is a `jmap_server::ScopedState`: an atomic rollback restores objects, state
strings, change logs and push subscriptions, and drops the queued lifecycle and
outbound federation events, so nothing is announced for a change that never
happened (§4.3). Under an atomic scope it leaves the CustomEmoji name check and the
push subscription cap to the commit, which checks the final state: an emoji name
swap, or replacing a subscription at the cap, succeeds. Per-object caps (group
members, Space roles and categories) already see their own final state and are
unchanged.

## CallerCtx

`register_chat_handlers` registers each method as a `ClosureHandler`
that forwards the dispatcher's `CallerCtx` value into the closure as `ctx`,
which the closure body then passes by reference (`&ctx`) into the standard
`handle_*` handler. The handler in turn forwards `caller: &B::CallerCtx` into
every `ChatBackend` (and inherited `JmapBackend`) method call.

To use this:

1. Pick a `CallerCtx` type for your backend — e.g. `()` if no auth context is
   needed, or a struct like `AuthCtx { user_id: Id, scopes: Vec<Scope> }`.
2. Implement `JmapBackend` with `type CallerCtx = AuthCtx;` (the bound is
   `Clone + Send + Sync + 'static`).
3. Use the matching `Dispatcher<AuthCtx>` and pass the constructed auth
   context as the second argument to `Dispatcher::dispatch(request, auth, …)`.
4. Inside `ChatBackend` method bodies, read `caller: &AuthCtx` to apply
   per-user visibility rules and the `may_*` permission hooks (e.g., that a
   caller can only modify their own `PresenceStatus`).

Backends that don't need an auth identity use `type CallerCtx = ();` and a
`Dispatcher<()>`. Both shapes register the same way.

## Object history (draft-ietf-jmap-object-history-03)

`register_chat_handlers_with_history(dispatcher, backend)`, for a backend
that also implements `jmap_server::ObjectHistoryBackend`, calls
`register_chat_handlers` and re-registers history-aware `/get` and `/set`
of Chat, Message, Space, ChatContact, CustomEmoji, SpaceBan, ChatBan and
SpaceInvite and `/query`, `/queryChanges` of the first five (each wraps
the unchanged plain handler). ReadPosition and PresenceStatus keep no
history, so the dispatcher refuses history arguments for them
(docs/design/DECISIONS.md decision 22).

Two chat-00 rules (decision 22), which a backend follows and the memory
backend implements: a Message's versions are purged when it is deleted
(`deletedAt`) or hard-deleted (destroy, expiry, burn on read), so a
Message is never a destroyed object; and a Space destroy purges the
history of the Space and of everything in it, so a Space restore is
always `notFound`. `MemoryBackend::with_object_history(capability,
retention)` turns history on; a restore needs the referenced Space to
exist and a CustomEmoji name to be free; destroyed queries take no filter
or sort here.

## Gotchas

- **Direct chat deduplication is eventually consistent.** The `Chat/set` create handler uses optimistic create-then-validate to detect duplicate Direct chats. Under concurrent load two clients may both successfully create the same Direct chat; the loser's write is rolled back and it receives `alreadyExists`, but the duplicate briefly exists. Backends can eliminate this window by enforcing uniqueness atomically in `create_object`.
- **`Space/join` TOCTOU window.** After a successful join write, the handler re-reads the member list to detect concurrent joins. There is a brief window between the write and the re-read during which a second concurrent join can succeed. Both joiners will detect the race, but the loser's join is rolled back, not the winner's — so the final state is consistent.
- **Draft spec only.** `draft-atwood-jmap-chat` has not been submitted to the IETF; wire format and method semantics may change.

## Crate family

```
jmap-types
    ├── jmap-server          Dispatcher this crate plugs into
    └── jmap-chat-types      domain types (Chat, Message, Space, etc.)
            └── jmap-chat-server   ← this crate
```

## References

| Document | Covers |
|---|---|
| [draft-atwood-jmap-chat-00] | Core Chat objects and methods |
| [draft-atwood-jmap-chat-push-00] | Push notification payloads |
| [draft-atwood-jmap-chat-wss-00] | WebSocket ephemeral events |
| [RFC 8620] | JMAP Core (request format, SetError, ResultReference) |

[draft-atwood-jmap-chat]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-push-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

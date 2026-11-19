# Design: split `ChatBackend` into hook traits (bd:JMAP-it7g.6)

Status: final. Revised with the mimid session's answers; Mark answered the four open questions in §5 on 2026-11-19 (decision 39), each as recommended. Design only; no code in the repo was changed. Decision 37
(Mark, 2026-11-19) chose a split into smaller hook traits over a forwarding macro and over
leaving `ChatBackend` as it is. Requested by the mimid session (mimid-s2qc item 4).

## 1. Problem

`jmap_chat_server::ChatBackend` is one trait with 43 methods on top of `jmap_server::JmapBackend`
(12 more). The reference `memory::MemoryBackend` overrides 34 of the 43, so a backend that wraps
`MemoryBackend` and changes a few behaviours must still write every method by hand to keep the
rest. mimid's `AgentChat` (mimid main 24df72e, on crate-jmap fe13c651; inventory from the mimid
session, 2026-11-19) writes 36 `ChatBackend` methods, and only 16 of them have behaviour of its own:

- **Its own (16):**
  - Object writes and types: `create_object`, `create_message`, `update_object`,
    `destroy_object`, `supports_type`.
  - Moderation as MIMI commits: `create_chat_ban`, `update_chat_ban`, `destroy_chat_ban`,
    `kick_chat_members`.
  - Settings: `limits`, `retains_edit_history`, `owner_user_id`.
  - `on_outbound_event` (a deliberate no-op, which is also the trait default) and
    `attachment_blob`.
  - Two per-Chat restriction gates: `may_update_chat`, `may_update_message`.
- **Pure forwards to `MemoryBackend` (20):**
  - Spaces: `generate_invite_code`, `join_space_with_invite`, `protect_last_admin`,
    `space_extension_properties`, `apply_space_patch`, `apply_space_metadata_patch`,
    `everyone_permissions`.
  - `maintains_unread_count`.
  - Gates: `may_set_custom_emoji`, `may_broadcast_mention`, `may_send_message`, `may_view_chat`,
    `may_set_pinned_messages`, `may_set_space_ban`, `may_set_space_invite`,
    `may_get_space_invite`, `may_get_space_ban`, `slow_mode_check`.
  - Message lifecycle: `expire_message`, `read_and_burn_message`.
- **`JmapBackend`:** it forwards the five reads (`get_objects`, `get_state`, `get_changes`,
  `query_objects`, `query_changes`) and overrides `account_exists`.

Each new trait method (ChatBan added three, kickMembers one) is another forwarding stub in every
wrapper, or a default that silently replaces `MemoryBackend`'s behaviour.

## 2. Goals and non-goals

Goals:
- A wrapper writes only the behaviour it changes; reusing a group of the wrapped backend's
  behaviour costs one short impl, not one stub per method.
- Adding a method to a group does not touch wrappers that delegate that group.
- Handlers keep a single bound, `B: ChatBackend`, and call methods as today.

Non-goals:
- Changing any wire behaviour, method semantics or default.
- Splitting `JmapBackend` or the other family backends (§7 lists this as a possible follow-up).
- Runtime composition (trait objects): the crate stays generic and static.

## 3. Proposal

### 3.1 Hook traits

`ChatBackend`'s methods move, unchanged in signature, docs and defaults, into ten traits. Each
has `JmapBackend` as its supertrait, so `CallerCtx` and `Error` stay shared:

| Trait | Methods |
|---|---|
| `ChatStore` | `create_object`, `update_object`, `destroy_object`, `create_message`, `supports_type` |
| `ChatSpaces` | `generate_invite_code`, `join_space_with_invite`, `apply_space_patch`, `apply_space_metadata_patch`, `everyone_permissions`, `protect_last_admin`, `space_extension_properties` |
| `SpacePolicy` | `may_set_custom_emoji`, `may_set_space_ban`, `may_set_space_invite`, `may_get_space_invite`, `may_get_space_ban` |
| `ChatAccessPolicy` | `may_send_message`, `may_view_chat`, `may_set_pinned_messages`, `may_broadcast_mention`, `slow_mode_check`, `is_contact_blocked`, `validate_pinned_message_ids` |
| `ChatRestrictions` | `may_create_message`, `may_update_message`, `may_update_chat` |
| `ChatModeration` | `create_chat_ban`, `update_chat_ban`, `destroy_chat_ban`, `kick_chat_members` |
| `ChatMessageLifecycle` | `expire_message`, `read_and_burn_message` |
| `ChatEvents` | `on_chat_destroyed`, `on_space_destroyed`, `on_space_ban_created`, `on_outbound_event` |
| `ChatSettings` | `limits`, `retains_edit_history`, `maintains_unread_count`, `owner_user_id` |
| `ChatResolvers` | `attachment_blob`, `filenode_attachment` |

`ChatBackend` becomes the union, implemented automatically:

```rust
pub trait ChatBackend:
    ChatStore + ChatSpaces + SpacePolicy + ChatAccessPolicy + ChatRestrictions
    + ChatModeration + ChatMessageLifecycle + ChatEvents + ChatSettings + ChatResolvers {}

impl<T> ChatBackend for T where
    T: ChatStore + ChatSpaces + SpacePolicy + ChatAccessPolicy + ChatRestrictions
     + ChatModeration + ChatMessageLifecycle + ChatEvents + ChatSettings + ChatResolvers {}
```

Handlers keep `B: ChatBackend`. Methods of the supertraits are callable through that bound, so
handler bodies don't change (`backend.may_send_message(..)` still resolves). The crate root
re-exports all ten traits.

The table follows the group boundaries in mimid's wrapper, so each part is either reused or
replaced as a whole. The mimid session confirmed the groups (2026-11-19). The gates are split
three ways because the per-Chat restriction gates (`may_create_message`, `may_update_message`,
`may_update_chat`) are the ones a room-bound backend overrides. `MemoryBackend` keeps the trait
default for `may_create_message`, so a wrapper implementing `ChatRestrictions` writes only the
gates it changes and forwards none. `ChatModeration` is separate from `ChatStore` so that bans and
kicks can become MIMI commits while storage is delegated.

### 3.2 Delegation, per trait

Each hook trait gets a companion `Delegate*` trait, and a blanket impl that forwards every
method of the hook trait to a target:

```rust
pub trait DelegateChatAccessPolicy: JmapBackend {
    type Target: ChatAccessPolicy<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    fn chat_access_policy(&self) -> &Self::Target;
}

impl<T: DelegateChatAccessPolicy> ChatAccessPolicy for T {
    fn may_send_message(&self, caller: &Self::CallerCtx, /* .. */) -> impl Future<..> + Send {
        self.chat_access_policy().may_send_message(caller, /* .. */)
    }
    // .. every ChatAccessPolicy method, written once, here
}
```

A wrapper then picks for each part either "mine" (implement the hook trait) or "the inner
backend's" (implement `Delegate*` with one accessor). mimid's `AgentChat` would be:

```rust
impl ChatStore for AgentChat { /* its own writes and supports_type */ }
impl ChatModeration for AgentChat { /* bans and kicks as MIMI commits */ }
impl ChatRestrictions for AgentChat { /* may_update_chat, may_update_message */ }
impl ChatSettings for AgentChat { /* limits, retains_edit_history, owner_user_id, plus one
                                      forward: maintains_unread_count */ }
impl ChatResolvers for AgentChat { /* attachment_blob */ }
impl ChatEvents for AgentChat {} // every default, including the no-op on_outbound_event
impl DelegateChatSpaces for AgentChat {
    type Target = MemoryBackend;
    fn chat_spaces(&self) -> &MemoryBackend { &self.inner }
}
impl DelegateSpacePolicy for AgentChat { /* same, &self.inner */ }
impl DelegateChatAccessPolicy for AgentChat { /* same */ }
impl DelegateChatMessageLifecycle for AgentChat { /* same */ }
```

That leaves one forwarding stub, `maintains_unread_count`, instead of 20.

Coherence (checked with a two-crate prototype, Rust 2024 edition): the blanket
`impl<T: DelegateX> X for T` and `impl X for MemoryBackend` do not overlap, because `MemoryBackend` does not implement `DelegateX`, and no other crate can add that
impl (local trait, local type). A downstream type implements either `X` or `DelegateX`, never
both; implementing both is a compile error, which is the intent.

### 3.3 What a delegated part sees

A delegated method runs with the target as `self`. When a default in one part calls another
part (the `ChatModeration` defaults call `ChatStore::create_object` and update the Chat's
members), a delegated `ChatModeration` writes through the target's `ChatStore`, not the
wrapper's. The rustdoc of every `Delegate*` trait states this. For mimid it changes nothing:
today it forwards those methods to `self.inner` too, with the same effect. A wrapper that
needs its own store under a delegated part implements that part itself.

### 3.4 The other backend traits

`ConditionalBackend` (begin_set), `PushSubscriptionBackend` and `ObjectHistoryBackend` (defined
in `jmap-server` and `jmap-object-history-server`) are already separate traits. No current
wrapper implements them, so they get no delegates now. `JmapBackend` is the one a wrapper does
forward. Phase 2 (§6, open question 3) splits its reads out, so they can be delegated while
`account_exists` is overridden.

## 4. Compatibility

Breaking for implementors, under the no-production-users rule (decisions 34–36). A backend
moves its `impl ChatBackend for X { .. }` methods into the ten hook-trait impl blocks;
bodies are unchanged. Code that names a method by path (`ChatBackend::may_send_message`)
renames the trait. Callers that use the methods through a `B: ChatBackend` bound are
unaffected. Wire behaviour is unchanged; the test suite must pass with no test edits other than
moving test backends' impl blocks.

In-repo implementors to move: `memory::MemoryBackend`, and the test backends `FaultyBackend`,
`TrackingBackend`, `IdentityBackend` and `InjectableBackend` (chat-server tests/common),
and `Fed` (chat-federation-server tests); `Bound` in chat-vtc-server tests implements only
`JmapBackend` and is unaffected. The four test wrappers in chat-server tests/common are also the first users of
`Delegate*`, which shrinks them.

## 5. Questions, answered by Mark 2026-11-19 (decision 39)

Answers: (1) the Chat-prefixed names; (2) per-trait delegation, not per method; (3) phase 2, after phase 1; (4) the same error type.


1. **Trait names.** The ten names in §3.1, each with a `Delegate*` companion. Fine, or shorter (`Store`, `Policy`, ..) under a `backend` module?
2. **Per-method delegation** (mimid's suggestion): give each hook trait a fallback, so that an
   implementor overrides single methods and the rest go to the wrapped backend. In stable Rust
   this means `type Fallback: ChatX<..>; fn fallback(&self) -> &Self::Fallback;` on every hook
   trait, with each default body calling `self.fallback()`. The costs:
   - Associated type defaults are unstable, so every backend names a fallback for all ten
     traits, `MemoryBackend` and the test backends included.
   - The end of the chain needs a type that holds the spec defaults. For `ChatModeration`,
     whose defaults write through the store, no such store-free type exists.
   - A backend that points its fallback at itself recurses forever at runtime, with no
     compile error.
   - `Option<&dyn ChatX>` is not available: RPITIT async methods and generic methods are not
     object-safe.

   Recommended: per-trait delegation with the three-way gate split above. That brings mimid down
   to one stub, without the per-method machinery.
3. **Delegation for the shared traits** (§3.4). mimid would use a delegate for the five
   `JmapBackend` reads, but it overrides `account_exists`, so a single `DelegateJmapBackend`
   over all 12 methods would not fit. That needs the same kind of split in `jmap-server`: a
   reads trait (`get_objects`, `get_state`, `get_changes`, `query_objects`, `query_changes`)
   and an accounts and limits trait. mimid does not implement `ConditionalBackend`,
   `PushSubscriptionBackend` or `ObjectHistoryBackend`, so delegates for those have no user yet.
   Recommended: phase 2 splits only the `JmapBackend` reads, since that change reaches every
   family server. Delegates for the other three traits wait for a user.
4. **Error types.** `Delegate*::Target` must have the wrapper's `Error`. mimid uses
   `MemoryError` already. Should the target's error be allowed to differ, via
   `Self::Error: From<Target::Error>`? Recommended: not now. Equality keeps the blanket impl
   simple, and no current wrapper needs a different error.

## 6. Plan and beads (after review)

Phase 1, chat-server only:
1. Introduce the ten traits and the blanket `ChatBackend`; move the methods and their docs.
   Move `MemoryBackend`'s impl and the in-repo test backends.
2. Add the ten `Delegate*` traits with their blanket impls, plus a test that a wrapper using
   only `Delegate*` behaves exactly like the wrapped `MemoryBackend`: run the `testkit`
   conformance suite against it.
3. Convert the chat-server test wrappers to `Delegate*`.
4. Update README "`ChatBackend` trait", AGENTS.md, CHANGELOG (BREAKING) and the public-api
   snapshot, and write a migration note for mimid.

Phase 2, if open question 3 is yes: split `JmapBackend` into a reads trait and an accounts and
limits trait in `jmap-server`, with a `Delegate*` for the reads, across every family server.

## 7. Alternatives considered

- **Forwarding macro** (`delegate_chat_backend!(Wrapper, inner, { overrides })`). Least churn,
  but it has to be kept in step with the trait by hand. A macro_rules body cannot take a
  method's signature from the trait, so the macro has to repeat all 43 signatures. Rejected by
  decision 37.
- **One `DelegateChatBackend` for the whole trait.** It doesn't fit the need: every wrapper
  overrides something, and one override disables the whole blanket impl.
- **Runtime composition** (`Box<dyn ..>` parts). The traits use RPITIT async methods and generic
  methods (`create_object<O, F>`), which are not object-safe.

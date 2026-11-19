# Design: split `ChatBackend` into hook traits (bd:JMAP-it7g.6)

Status: draft for Mark's review. Design only; no code in the repo was changed. Decision 37
(Mark, 2026-11-19) chose a split into smaller hook traits over a forwarding macro and over
leaving `ChatBackend` as it is. Requested by the mimid session (mimid-s2qc item 4).

## 1. Problem

`jmap_chat_server::ChatBackend` is one trait with 43 methods on top of `jmap_server::JmapBackend`
(12 more). The reference `memory::MemoryBackend` overrides 34 of the 43, so a backend that wraps
`MemoryBackend` and changes a few behaviours must still write every method by hand to keep the
rest. mimid's `AgentChat` (crates/mimid-agent-jmap/src/backend.rs) implements 32 `ChatBackend`
methods and 6 `JmapBackend` methods; 21 of the 32 only forward to the wrapped `MemoryBackend`:

- **Forwarded unchanged:**
  - Space structure: `apply_space_patch`, `apply_space_metadata_patch`, `generate_invite_code`,
    `everyone_permissions`, `protect_last_admin`, `space_extension_properties`.
  - Every Space and Chat gate: `may_send_message`, `may_view_chat`, `may_set_*`, `may_get_*`,
    `slow_mode_check`, `may_broadcast_mention`.
  - Message lifecycle: `expire_message`, `read_and_burn_message`.
  - Also `destroy_object` and `maintains_unread_count`.
- **Its own:**
  - Object writes: `create_object`, `create_message`, `update_object`.
  - `on_outbound_event`, `supports_type`, `owner_user_id`, `limits`, `retains_edit_history`,
    `attachment_blob`, `join_space_with_invite`.
  - Two gates: `may_update_chat`, `may_update_message`.

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

`ChatBackend`'s methods move, unchanged in signature, docs and defaults, into eight traits. Each
has `JmapBackend` as its supertrait, so `CallerCtx` and `Error` stay shared:

| Trait | Methods |
|---|---|
| `ChatStore` | `create_object`, `update_object`, `destroy_object`, `create_message`, `supports_type` |
| `ChatSpaces` | `generate_invite_code`, `join_space_with_invite`, `apply_space_patch`, `apply_space_metadata_patch`, `everyone_permissions`, `protect_last_admin`, `space_extension_properties` |
| `ChatPolicy` | `may_set_custom_emoji`, `may_broadcast_mention`, `may_set_pinned_messages`, `may_set_space_ban`, `may_set_space_invite`, `may_get_space_invite`, `may_get_space_ban`, `may_send_message`, `may_create_message`, `may_update_message`, `may_update_chat`, `may_view_chat`, `slow_mode_check`, `is_contact_blocked`, `validate_pinned_message_ids` |
| `ChatModeration` | `create_chat_ban`, `update_chat_ban`, `destroy_chat_ban`, `kick_chat_members` |
| `ChatMessageLifecycle` | `expire_message`, `read_and_burn_message` |
| `ChatEvents` | `on_chat_destroyed`, `on_space_destroyed`, `on_space_ban_created`, `on_outbound_event` |
| `ChatSettings` | `limits`, `retains_edit_history`, `maintains_unread_count`, `owner_user_id` |
| `ChatResolvers` | `attachment_blob`, `filenode_attachment` |

`ChatBackend` becomes the union, implemented automatically:

```rust
pub trait ChatBackend:
    ChatStore + ChatSpaces + ChatPolicy + ChatModeration
    + ChatMessageLifecycle + ChatEvents + ChatSettings + ChatResolvers {}

impl<T> ChatBackend for T where
    T: ChatStore + ChatSpaces + ChatPolicy + ChatModeration
     + ChatMessageLifecycle + ChatEvents + ChatSettings + ChatResolvers {}
```

Handlers keep `B: ChatBackend`. Methods of the supertraits are callable through that bound, so
handler bodies don't change (`backend.may_send_message(..)` still resolves). The crate root
re-exports all eight traits.

The table follows the group boundaries visible in mimid's wrapper, so a part is either reused
or replaced as a whole. Two methods land where a wrapper will look for them, not where their
type would put them. `join_space_with_invite` is in `ChatSpaces`, although mimid implements it,
because it is a Space operation. `ChatModeration` is separate from `ChatStore` so that mimid can
turn bans and kicks into MIMI commits while still delegating storage.

### 3.2 Delegation, per trait

Each hook trait gets a companion `Delegate*` trait, and a blanket impl that forwards every
method of the hook trait to a target:

```rust
pub trait DelegateChatPolicy: JmapBackend {
    type Target: ChatPolicy<CallerCtx = Self::CallerCtx, Error = Self::Error>;
    fn chat_policy(&self) -> &Self::Target;
}

impl<T: DelegateChatPolicy> ChatPolicy for T {
    fn may_send_message(&self, caller: &Self::CallerCtx, /* .. */) -> impl Future<..> + Send {
        self.chat_policy().may_send_message(caller, /* .. */)
    }
    // .. every ChatPolicy method, written once, here
}
```

A wrapper then picks for each part either "mine" (implement the hook trait) or "the inner
backend's" (implement `Delegate*` with one accessor). mimid's `AgentChat` would be:

```rust
impl ChatStore for AgentChat { /* its own writes and supports_type */ }
impl ChatEvents for AgentChat { /* on_outbound_event */ }
impl ChatSettings for AgentChat { /* limits, owner_user_id, .. */ }
impl ChatResolvers for AgentChat { /* attachment_blob */ }
impl ChatModeration for AgentChat { /* bans and kicks as MIMI commits */ }
impl DelegateChatSpaces for AgentChat { type Target = MemoryBackend; fn chat_spaces(&self) -> &MemoryBackend { &self.inner } }
impl DelegateChatPolicy for AgentChat { /* same, &self.inner */ }
impl DelegateChatMessageLifecycle for AgentChat { /* same */ }
```

Its own `ChatSpaces`/`ChatPolicy` overrides (`join_space_with_invite`, `may_update_chat`,
`may_update_message`) mean it implements those two traits fully instead. That is the cost of a
partial override, at most one trait's methods (§5, open question 2).

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

### 3.4 The other chat-server backend traits

`ConditionalBackend` (begin_set), `PushSubscriptionBackend` and `ObjectHistoryBackend` (defined
in `jmap-server` and `jmap-object-history-server`) are separate traits already. Phase 2 (§6)
adds `DelegateConditionalBackend`, `DelegatePushSubscriptionBackend` and
`DelegateObjectHistoryBackend` in the crates that define them, with the same shape. That needs
decision 3 below, because those crates serve every family.

## 4. Compatibility

Breaking for implementors, under the no-production-users rule (decisions 34–36). A backend
moves its `impl ChatBackend for X { .. }` methods into the eight `impl ChatX for X` blocks;
bodies are unchanged. Code that names a method by path (`ChatBackend::may_send_message`)
renames the trait. Callers that use the methods through a `B: ChatBackend` bound are
unaffected. Wire behaviour is unchanged; the test suite must pass with no test edits other than
moving test backends' impl blocks.

In-repo implementors to move: `memory::MemoryBackend`, and the test backends `FaultyBackend`,
`TrackingBackend`, `IdentityBackend` and `InjectableBackend` (chat-server tests/common),
and `Fed` (chat-federation-server tests); `Bound` in chat-vtc-server tests implements only
`JmapBackend` and is unaffected. The four test wrappers in chat-server tests/common are also the first users of
`Delegate*`, which shrinks them.

## 5. Open questions for Mark

1. **Trait names.** `ChatStore`, `ChatSpaces`, `ChatPolicy`, `ChatModeration`,
   `ChatMessageLifecycle`, `ChatEvents`, `ChatSettings`, `ChatResolvers`, each with a
   `Delegate*` companion. Fine, or shorter (`Store`, `Policy`, ..) under a `backend` module?
2. **Granularity of `ChatPolicy`.** 15 gates in one trait means a wrapper that overrides two
   gates (as mimid does) writes 13 forwarding stubs. Options:
   (a) keep one trait;
   (b) split it into `SpacePolicy` (the Space and invite or ban gates) and `ChatMessagePolicy`
   (Chat and Message gates);
   (c) add a per-method escape hatch to the `Delegate*` pattern.
   Recommended: (b). It matches mimid's split: it overrides only Chat and Message gates.
3. **Delegation for the shared traits** (§3.4) in `jmap-server` and
   `jmap-object-history-server`, and optionally `DelegateJmapBackend` for the six read methods.
   These would help every family's wrappers, but they widen the change to crates outside chat.
   Recommended: yes, as phase 2, after phase 1 has been used in mimid.
4. **Error types.** `Delegate*::Target` must have the wrapper's `Error`. mimid uses
   `MemoryError` already. Should the target's error be allowed to differ, via
   `Self::Error: From<Target::Error>`? Recommended: not now. Equality keeps the blanket impl
   simple, and no current wrapper needs a different error.

## 6. Plan and beads (after review)

Phase 1, chat-server only:
1. Introduce the eight traits and the blanket `ChatBackend`; move the methods and their docs.
   Move `MemoryBackend`'s impl and the in-repo test backends.
2. Add the eight `Delegate*` traits with their blanket impls, plus a test that a wrapper using
   only `Delegate*` behaves exactly like the wrapped `MemoryBackend`: run the `testkit`
   conformance suite against it.
3. Convert the chat-server test wrappers to `Delegate*`.
4. Update README "`ChatBackend` trait", AGENTS.md, CHANGELOG (BREAKING) and the public-api
   snapshot, and write a migration note for mimid.

Phase 2, if open question 3 is yes: the delegate traits for `ConditionalBackend`,
`PushSubscriptionBackend`, `ObjectHistoryBackend` and `JmapBackend`.

## 7. Alternatives considered

- **Forwarding macro** (`delegate_chat_backend!(Wrapper, inner, { overrides })`). Least churn,
  but it has to be kept in step with the trait by hand. A macro_rules body cannot take a
  method's signature from the trait, so the macro has to repeat all 43 signatures. Rejected by
  decision 37.
- **One `DelegateChatBackend` for the whole trait.** It doesn't fit the need: every wrapper
  overrides something, and one override disables the whole blanket impl.
- **Runtime composition** (`Box<dyn ..>` parts). The traits use RPITIT async methods and generic
  methods (`create_object<O, F>`), which are not object-safe.

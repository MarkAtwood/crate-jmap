//! JMAP Chat extension method handlers.
//!
//! # Usage
//!
//! Implement [`ChatBackend`] for your storage layer, then call
//! [`register_chat_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_chat_server::{ChatBackend, register_chat_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: ChatBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_chat_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`ChatBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.
//!
//! # `realistic-demo-ids` feature (test-id format toggle)
//!
//! The `realistic-demo-ids` feature changes the id-minting format
//! that [`memory::MemoryBackend`] uses for server-assigned object
//! ids. It requires `memory` to be enabled (the feature only
//! affects the in-memory reference backend; no other code path
//! observes it).
//!
//! - **Default (feature OFF):** deterministic, lex-orderable
//!   per-(type, account) ids of the form `"<type><n:016x>"`. Easy
//!   to read in test debug output. Load-bearing for
//!   draft-atwood-jmap-chat-00 `Chat.unreadCount` semantics (count
//!   of `Message`s whose id is lex-greater than
//!   `lastReadMessageId`).
//! - **Feature ON:** mail-server-style timestamp+counter ids of
//!   the form `"{n:016x}"`. Lex-orderable globally within a
//!   process, not repeatable across runs.
//!
//! **Cargo feature unification hazard.** The output format of
//! every `MemoryBackend`-minted id depends on this feature, and
//! cargo unifies features across the dep graph. If crate A's
//! tests activate `realistic-demo-ids` for realism and crate B's
//! tests consume `MemoryBackend` expecting the deterministic
//! format, a unified workspace build silently switches B to the
//! realistic format and B's tests break. Crates that depend on
//! a specific id format MUST NOT rely on the *absence* of the
//! feature elsewhere in the dep graph — they MUST encode the
//! expected format in their own tests via fixture matching or
//! prefix assertions rather than literal id comparison.
//!
//! # Permission enforcement: backend canonical
//!
//! All Space/set permission gates live in the backend's
//! `apply_space_patch` implementation (see `memory::MemoryBackend`
//! under the `memory` feature for the reference impl). The handler
//! [`handle_space_set`] does NOT permission-check.
//!
//! The pure helper [`required_permissions_for_op`] in [`permissions`]
//! maps each [`SpacePatchOp`] variant to its required permission strings
//! per draft-atwood-jmap-chat-00 §Space/set. Backends consume this
//! helper inside `apply_space_patch` after resolving the caller's
//! effective permissions via [`jmap_server::JmapBackend::principal_id`].
//! Handlers MUST NOT consume the helper for gating — backends are the
//! single source of truth. This mirrors the workspace-wide rule
//! documented in workspace AGENTS.md "Caller identity (foundation
//! seam)":
//!
//! - Handlers do NO permission checking.
//! - Defense-in-depth handler pre-checks are allowed but the backend
//!   MUST re-verify atomically with the mutation.
//! - A `None` return from `JmapBackend::principal_id` means the
//!   deployment has not wired identity; chat permission semantics
//!   cannot be honored and the backend MUST reject the patch rather
//!   than fail open.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod ban;
pub mod chat;
pub mod contact;
pub mod emoji;
mod helpers;
pub mod invite;
/// In-memory reference implementation of [`ChatBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
pub mod message;
pub mod permissions;
pub mod position;
pub mod presence;
pub mod space;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, ChatBackend,
    ChatContactProperty, ChatLimits, ChatProperty, EmojiSetOp, GetObject, JmapBackend, JmapObject,
    MessageProperty, OpResult, QueryChangesResult, QueryObject, QueryResult, ReadPositionProperty,
    SetError, SetErrorType, SetObject, SlowModeError, SpaceMetadataPatch, SpacePatchOp,
    SpaceProperty,
};
pub use ban::{handle_ban_changes, handle_ban_get, handle_ban_set};
pub use chat::{
    handle_chat_changes, handle_chat_get, handle_chat_query, handle_chat_query_changes,
    handle_chat_set, handle_chat_typing,
};
pub use contact::{
    handle_contact_changes, handle_contact_get, handle_contact_query, handle_contact_query_changes,
    handle_contact_set,
};
pub use emoji::{
    handle_emoji_changes, handle_emoji_get, handle_emoji_query, handle_emoji_query_changes,
    handle_emoji_set,
};
pub use invite::{handle_invite_changes, handle_invite_get, handle_invite_set};
pub use message::{
    handle_message_changes, handle_message_get, handle_message_query, handle_message_query_changes,
    handle_message_set,
};
pub use permissions::{
    required_permissions_for_op, RequiredPermissions, MANAGE_CHANNELS, MANAGE_MEMBERS,
    MANAGE_ROLES, MANAGE_SPACE,
};
pub use position::{handle_position_changes, handle_position_get, handle_position_set};
pub use presence::{handle_presence_changes, handle_presence_get, handle_presence_set};
pub use space::{
    handle_space_changes, handle_space_get, handle_space_join, handle_space_query,
    handle_space_query_changes, handle_space_set,
};

// ---------------------------------------------------------------------------
// register_chat_handlers — the main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP Chat method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler.
/// You may pass any `Arc<B>` — the function clones it internally into each
/// registered handler closure. Sharing the same `Arc<B>` across this call
/// and other application-level uses of the backend is a memory
/// optimization, not a correctness requirement; separate `Arc<B>` instances
/// pointing at the same underlying backend would also work.
///
/// After this call, the dispatcher handles:
/// `Chat/*`, `Message/*`, `Space/*`, `SpaceBan/*`, `ChatContact/*`,
/// `ReadPosition/*`, `CustomEmoji/*`, `SpaceInvite/*`, and `PresenceStatus/*`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
///
/// # Re-registration semantics
///
/// This function calls [`Dispatcher::register`] once per
/// draft-atwood-jmap-chat-00 method name. `Dispatcher::register`
/// **silently overwrites** any pre-existing handler under the same
/// method name (the underlying primitive is `HashMap::insert`). Three
/// consequences callers MUST be aware of:
///
/// - **Double-call**: invoking this function twice on the same
///   dispatcher loses the first set's handlers. The second call wins.
/// - **Custom overrides go LAST**: to replace a single handler (e.g.
///   provide a custom `Chat/get`), call this function FIRST, then
///   `dispatcher.register("Chat/get", my_override)`. The inverse
///   order silently undoes the custom handler.
/// - **No collision diagnostic**: there is no error or log when a
///   handler is overwritten. The contract is "last register wins" and
///   the caller is responsible for ordering.
///
/// [`Dispatcher::register`]: jmap_server::Dispatcher::register
pub fn register_chat_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: ChatBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx).
    //
    // `$ci` is the call_id string (echoed back to the client). Most handlers
    // ignore it and use `_ci` as the identifier.
    //
    // `$ctx` is the per-request caller context (`B::CallerCtx`) forwarded
    // by the dispatcher. Closures pass `&ctx` to the inner `handle_*` fn.
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                move |$b: Arc<B>, $ci: String, $a: serde_json::Value, $ctx: B::CallerCtx| {
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    // Chat
    reg!("Chat/get", backend, |b, _ci, a, ctx| {
        handle_chat_get(&*b, &ctx, a).await
    });
    reg!("Chat/changes", backend, |b, _ci, a, ctx| {
        handle_chat_changes(&*b, &ctx, a).await
    });
    reg!("Chat/query", backend, |b, _ci, a, ctx| {
        handle_chat_query(&*b, &ctx, a).await
    });
    reg!("Chat/queryChanges", backend, |b, _ci, a, ctx| {
        handle_chat_query_changes(&*b, &ctx, a).await
    });
    reg!("Chat/set", backend, |b, _ci, a, ctx| {
        handle_chat_set(&*b, &ctx, a).await
    });
    reg!("Chat/typing", backend, |b, _ci, a, ctx| {
        handle_chat_typing(&*b, &ctx, a).await
    });

    // Message
    reg!("Message/get", backend, |b, _ci, a, ctx| {
        handle_message_get(&*b, &ctx, a).await
    });
    reg!("Message/changes", backend, |b, _ci, a, ctx| {
        handle_message_changes(&*b, &ctx, a).await
    });
    reg!("Message/query", backend, |b, _ci, a, ctx| {
        handle_message_query(&*b, &ctx, a).await
    });
    reg!("Message/queryChanges", backend, |b, _ci, a, ctx| {
        handle_message_query_changes(&*b, &ctx, a).await
    });
    reg!("Message/set", backend, |b, _ci, a, ctx| {
        handle_message_set(&*b, &ctx, a).await
    });

    // Space
    reg!("Space/get", backend, |b, _ci, a, ctx| {
        handle_space_get(&*b, &ctx, a).await
    });
    reg!("Space/changes", backend, |b, _ci, a, ctx| {
        handle_space_changes(&*b, &ctx, a).await
    });
    reg!("Space/query", backend, |b, _ci, a, ctx| {
        handle_space_query(&*b, &ctx, a).await
    });
    reg!("Space/queryChanges", backend, |b, _ci, a, ctx| {
        handle_space_query_changes(&*b, &ctx, a).await
    });
    reg!("Space/set", backend, |b, _ci, a, ctx| {
        handle_space_set(&*b, &ctx, a).await
    });
    reg!("Space/join", backend, |b, _ci, a, ctx| {
        handle_space_join(&*b, &ctx, a).await
    });

    // ChatContact
    reg!("ChatContact/get", backend, |b, _ci, a, ctx| {
        handle_contact_get(&*b, &ctx, a).await
    });
    reg!("ChatContact/changes", backend, |b, _ci, a, ctx| {
        handle_contact_changes(&*b, &ctx, a).await
    });
    reg!("ChatContact/query", backend, |b, _ci, a, ctx| {
        handle_contact_query(&*b, &ctx, a).await
    });
    reg!("ChatContact/queryChanges", backend, |b, _ci, a, ctx| {
        handle_contact_query_changes(&*b, &ctx, a).await
    });
    reg!("ChatContact/set", backend, |b, _ci, a, ctx| {
        handle_contact_set(&*b, &ctx, a).await
    });

    // ReadPosition
    reg!("ReadPosition/get", backend, |b, _ci, a, ctx| {
        handle_position_get(&*b, &ctx, a).await
    });
    reg!("ReadPosition/changes", backend, |b, _ci, a, ctx| {
        handle_position_changes(&*b, &ctx, a).await
    });
    reg!("ReadPosition/set", backend, |b, _ci, a, ctx| {
        handle_position_set(&*b, &ctx, a).await
    });

    // SpaceInvite
    reg!("SpaceInvite/get", backend, |b, _ci, a, ctx| {
        handle_invite_get(&*b, &ctx, a).await
    });
    reg!("SpaceInvite/changes", backend, |b, _ci, a, ctx| {
        handle_invite_changes(&*b, &ctx, a).await
    });
    reg!("SpaceInvite/set", backend, |b, _ci, a, ctx| {
        handle_invite_set(&*b, &ctx, a).await
    });

    // SpaceBan
    reg!("SpaceBan/get", backend, |b, _ci, a, ctx| {
        handle_ban_get(&*b, &ctx, a).await
    });
    reg!("SpaceBan/changes", backend, |b, _ci, a, ctx| {
        handle_ban_changes(&*b, &ctx, a).await
    });
    reg!("SpaceBan/set", backend, |b, _ci, a, ctx| {
        handle_ban_set(&*b, &ctx, a).await
    });

    // CustomEmoji
    reg!("CustomEmoji/get", backend, |b, _ci, a, ctx| {
        handle_emoji_get(&*b, &ctx, a).await
    });
    reg!("CustomEmoji/changes", backend, |b, _ci, a, ctx| {
        handle_emoji_changes(&*b, &ctx, a).await
    });
    reg!("CustomEmoji/query", backend, |b, _ci, a, ctx| {
        handle_emoji_query(&*b, &ctx, a).await
    });
    reg!("CustomEmoji/queryChanges", backend, |b, _ci, a, ctx| {
        handle_emoji_query_changes(&*b, &ctx, a).await
    });
    reg!("CustomEmoji/set", backend, |b, _ci, a, ctx| {
        handle_emoji_set(&*b, &ctx, a).await
    });

    // PresenceStatus
    reg!("PresenceStatus/get", backend, |b, _ci, a, ctx| {
        handle_presence_get(&*b, &ctx, a).await
    });
    reg!("PresenceStatus/changes", backend, |b, _ci, a, ctx| {
        handle_presence_changes(&*b, &ctx, a).await
    });
    reg!("PresenceStatus/set", backend, |b, _ci, a, ctx| {
        handle_presence_set(&*b, &ctx, a).await
    });
}

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`].
///
/// Re-exported so the [`register_chat_handlers`] macro body can name
/// `ClosureHandler` without a fully-qualified path. **Stability**: this
/// re-export pins the major-version contract of [`jmap_server::ClosureHandler`]
/// into this crate's public surface — a breaking change to that type
/// upstream is a breaking change here. Consumers needing a closure handler
/// adapter SHOULD prefer importing from [`jmap_server`] directly; the
/// re-export is retained primarily for the in-crate macro and for
/// backward-compatible spelling of the existing handler-registration
/// pattern.
pub use jmap_server::ClosureHandler;

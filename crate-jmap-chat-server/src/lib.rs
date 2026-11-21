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
//! # `websocket` feature (ephemeral events over RFC 8887)
//!
//! Enable `websocket` for the `websocket` module: the `jmap-websocket`
//! extension handler for `ChatStreamEnable` / `ChatStreamDisable`
//! (draft-atwood-jmap-chat-wss-00) and the server-wide hub the deployment
//! publishes events to. Off by default, so a deployment without the WebSocket
//! transport does not pull in `jmap-websocket`.
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
//! # Permission enforcement: backend canonical
//!
//! All Space/set permission gates live in the backend's
//! `apply_space_patch` implementation (see `memory::MemoryBackend`
//! under the `memory` feature for the reference impl). The handler
//! [`handle_space_set`] does NOT permission-check.
//!
//! The pure helper [`required_permissions_for_op`]
//! maps each [`SpacePatchOp`] variant to its required
//! [`ChatPermission`](jmap_chat_types::ChatPermission)s per draft-atwood-jmap-chat-00 §Space/set. Backends consume this
//! helper inside `apply_space_patch` after resolving the caller's
//! effective permissions from the Space `members` entry whose `id` is
//! the `accountId` (the owner, per draft §Terminology). The gates apply
//! whether or not [`jmap_server::JmapAccounts::principal_id`] resolves a
//! caller identity.
//! Handlers MUST NOT consume the helper for gating — backends are the
//! single source of truth. This mirrors the workspace-wide rule
//! documented in workspace AGENTS.md "Caller identity (foundation
//! seam)":
//!
//! - Handlers do NO permission checking.
//! - Defense-in-depth handler pre-checks are allowed but the backend
//!   MUST re-verify atomically with the mutation.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
// bd:JMAP-uqhn.2 — narrowing and sign-changing `as` casts are banned;
// use `try_from`. Widening `usize as u64` stays legal (these lints do
// not fire on it): it is lossless on every target Rust supports.
#![warn(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::sync::Arc;

use jmap_server::{
    CallContext, ClosureHandler, ConditionalBackend, Dispatcher, HandlerFuture, JmapHandler,
    RegistrationError, SetSupport,
};

mod backend;
mod ban;
mod chat;
mod chat_ban;
mod contact;
mod did_auth;
mod emoji;
mod ephemeral;
mod expiry;
mod helpers;
mod history;
mod invite;
/// In-memory reference implementation of [`ChatBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
pub mod message;
mod outbound;
mod permissions;
mod position;
mod presence;
mod push;
mod space;
#[cfg(feature = "testkit")]
pub mod testkit;
#[cfg(feature = "websocket")]
pub mod websocket;

pub use backend::{
    AttachmentBlob, ChatAccessPolicy, ChatBackend, ChatEvents, ChatLimits, ChatMessageLifecycle,
    ChatModeration, ChatResolvers, ChatRestrictions, ChatSettings, ChatSpaces, ChatStore,
    EmojiSetOp, InviteJoinRefusal, OpResult, SlowModeError, SpacePolicy,
};
pub use backend::{
    DelegateChatAccessPolicy, DelegateChatEvents, DelegateChatMessageLifecycle,
    DelegateChatModeration, DelegateChatResolvers, DelegateChatRestrictions, DelegateChatSettings,
    DelegateChatSpaces, DelegateChatStore, DelegateSpacePolicy,
};
pub use ban::{handle_ban_changes, handle_ban_get, handle_ban_set};
pub use chat::{
    handle_chat_changes, handle_chat_get, handle_chat_query, handle_chat_query_changes,
    handle_chat_set, handle_chat_typing,
};
pub use chat_ban::{handle_chat_ban_changes, handle_chat_ban_get, handle_chat_ban_set};
pub use contact::{
    handle_contact_changes, handle_contact_get, handle_contact_query, handle_contact_query_changes,
    handle_contact_set,
};
pub use did_auth::{DidAuthRejection, DidDocument, verify_did_binding};
pub use emoji::{
    handle_emoji_changes, handle_emoji_get, handle_emoji_query, handle_emoji_query_changes,
    handle_emoji_set,
};
pub use ephemeral::{
    CompanionStreams, EphemeralSubscription, IdScope, project_presence, should_deliver_typing,
};
pub use expiry::sweep_expired;
pub use history::register_chat_handlers_with_history;
pub use invite::{handle_invite_changes, handle_invite_get, handle_invite_set};
pub use jmap_chat_types::{
    ChatContactProperty, ChatProperty, MessageProperty, ReadPositionProperty, SpaceProperty,
};
pub use jmap_chat_types::{SpaceMetadataPatch, SpacePatchOp};
pub use outbound::{GroupUpdateAction, OutboundEvent, ReactionAction};
pub use permissions::{
    RequiredPermissions, required_permissions_for_op, resolve_channel_permissions,
};
pub use position::{handle_position_changes, handle_position_get, handle_position_set};
pub use presence::{handle_presence_changes, handle_presence_get, handle_presence_set};
pub use push::{
    BuiltPush, OwnerFlags, PushItem, build_chat_message_push, chat_state_change_for,
    fit_chat_message_push, owner_unread_count, should_push, strip_unsupported_chat_push,
    validate_chat_push,
};
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
/// `Chat/*`, `Message/*`, `Space/*`, `SpaceBan/*`, `ChatBan/*`, `ChatContact/*`,
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
    register_chat_message_handlers(dispatcher, &backend);
    register_space_contact_handlers(dispatcher, &backend);
    register_space_object_handlers(dispatcher, &backend);
}

// Helper: register one method on the dispatcher `$d` with a closure that
// takes (Arc<B>, call_id, args, ctx), where `B` is the backend type of the
// calling function.
//
// `$ci` is the call_id string (echoed back to the client). Most handlers
// ignore it and use `_ci` as the identifier.
//
// `$ctx` is the per-request caller context (`B::CallerCtx`) forwarded
// by the dispatcher. Closures pass `&ctx` to the inner `handle_*` fn.
// Each optional trailing ident binds the `CallContext` field of that name:
// `created_ids`, the request-wide creation id map (RFC 8620 §5.3) that
// `/set` handlers use to resolve `#creationId` references, or `using`,
// the request's capability list (draft-ietf-jmap-metadata-05 §3).
macro_rules! reg {
    ($d:ident, $method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident $(, $field:ident)*| $body:expr) => {{
        let backend_arc: Arc<B> = Arc::clone(&$backend);
        let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
            backend_arc,
            move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                let CallContext {
                    call_id: $ci,
                    args: $a,
                    caller: $ctx,
                    $($field,)*
                    ..
                } = cx;
                Box::pin(async move { $body }) as HandlerFuture
            },
        ));
        $d.register($method, h);
    }};
}

/// The `Chat/*` and `Message/*` part of [`register_chat_handlers`], on the
/// dispatcher `d`.
fn register_chat_message_handlers<B>(d: &mut Dispatcher<B::CallerCtx>, backend: &Arc<B>)
where
    B: ChatBackend + 'static,
{
    // Chat
    reg!(d, "Chat/get", backend, |b, _ci, a, ctx, using| {
        handle_chat_get(&*b, &ctx, a, &using).await
    });
    reg!(d, "Chat/changes", backend, |b, _ci, a, ctx, using| {
        handle_chat_changes(&*b, &ctx, a, &using).await
    });
    reg!(d, "Chat/query", backend, |b, _ci, a, ctx, using| {
        handle_chat_query(&*b, &ctx, a, &using).await
    });
    reg!(d, "Chat/queryChanges", backend, |b, _ci, a, ctx, using| {
        handle_chat_query_changes(&*b, &ctx, a, &using).await
    });
    reg!(
        d,
        "Chat/set",
        backend,
        |b, _ci, a, ctx, created_ids, using| {
            handle_chat_set(&*b, &ctx, &created_ids, a, &using).await
        }
    );
    reg!(d, "Chat/typing", backend, |b, _ci, a, ctx| {
        handle_chat_typing(&*b, &ctx, a).await
    });

    // Message
    reg!(d, "Message/get", backend, |b, _ci, a, ctx, using| {
        message::handle_message_get(&*b, &ctx, a, &using).await
    });
    reg!(d, "Message/changes", backend, |b, _ci, a, ctx, using| {
        message::handle_message_changes(&*b, &ctx, a, &using).await
    });
    reg!(d, "Message/query", backend, |b, _ci, a, ctx, using| {
        message::handle_message_query(&*b, &ctx, a, &using).await
    });
    reg!(
        d,
        "Message/queryChanges",
        backend,
        |b, _ci, a, ctx, using| {
            message::handle_message_query_changes(&*b, &ctx, a, &using).await
        }
    );
    reg!(
        d,
        "Message/set",
        backend,
        |b, _ci, a, ctx, created_ids, using| {
            message::handle_message_set(&*b, &ctx, &created_ids, a, &using).await
        }
    );
}

/// The `Space/*`, `ChatContact/*` and `ReadPosition/*` part of
/// [`register_chat_handlers`], on the dispatcher `d`.
fn register_space_contact_handlers<B>(d: &mut Dispatcher<B::CallerCtx>, backend: &Arc<B>)
where
    B: ChatBackend + 'static,
{
    // Space
    reg!(d, "Space/get", backend, |b, _ci, a, ctx, using| {
        handle_space_get(&*b, &ctx, a, &using).await
    });
    reg!(d, "Space/changes", backend, |b, _ci, a, ctx, using| {
        handle_space_changes(&*b, &ctx, a, &using).await
    });
    reg!(d, "Space/query", backend, |b, _ci, a, ctx, using| {
        handle_space_query(&*b, &ctx, a, &using).await
    });
    reg!(d, "Space/queryChanges", backend, |b, _ci, a, ctx, using| {
        handle_space_query_changes(&*b, &ctx, a, &using).await
    });
    reg!(
        d,
        "Space/set",
        backend,
        |b, _ci, a, ctx, created_ids, using| {
            handle_space_set(&*b, &ctx, &created_ids, a, &using).await
        }
    );
    reg!(d, "Space/join", backend, |b, _ci, a, ctx| {
        handle_space_join(&*b, &ctx, a).await
    });

    // ChatContact
    reg!(d, "ChatContact/get", backend, |b, _ci, a, ctx| {
        handle_contact_get(&*b, &ctx, a).await
    });
    reg!(d, "ChatContact/changes", backend, |b, _ci, a, ctx| {
        handle_contact_changes(&*b, &ctx, a).await
    });
    reg!(d, "ChatContact/query", backend, |b, _ci, a, ctx| {
        handle_contact_query(&*b, &ctx, a).await
    });
    reg!(d, "ChatContact/queryChanges", backend, |b, _ci, a, ctx| {
        handle_contact_query_changes(&*b, &ctx, a).await
    });
    reg!(d, "ChatContact/set", backend, |b, _ci, a, ctx| {
        handle_contact_set(&*b, &ctx, a).await
    });

    // ReadPosition
    reg!(d, "ReadPosition/get", backend, |b, _ci, a, ctx| {
        handle_position_get(&*b, &ctx, a).await
    });
    reg!(d, "ReadPosition/changes", backend, |b, _ci, a, ctx| {
        handle_position_changes(&*b, &ctx, a).await
    });
    reg!(
        d,
        "ReadPosition/set",
        backend,
        |b, _ci, a, ctx, created_ids| handle_position_set(&*b, &ctx, &created_ids, a).await
    );
}

/// The `SpaceInvite/*`, `SpaceBan/*`, `ChatBan/*`, `CustomEmoji/*` and
/// `PresenceStatus/*` part of [`register_chat_handlers`], on the
/// dispatcher `d`.
fn register_space_object_handlers<B>(d: &mut Dispatcher<B::CallerCtx>, backend: &Arc<B>)
where
    B: ChatBackend + 'static,
{
    // SpaceInvite
    reg!(d, "SpaceInvite/get", backend, |b, _ci, a, ctx| {
        handle_invite_get(&*b, &ctx, a).await
    });
    reg!(d, "SpaceInvite/changes", backend, |b, _ci, a, ctx| {
        handle_invite_changes(&*b, &ctx, a).await
    });
    reg!(
        d,
        "SpaceInvite/set",
        backend,
        |b, _ci, a, ctx, created_ids| handle_invite_set(&*b, &ctx, &created_ids, a).await
    );

    // SpaceBan
    reg!(d, "SpaceBan/get", backend, |b, _ci, a, ctx| {
        handle_ban_get(&*b, &ctx, a).await
    });
    reg!(d, "SpaceBan/changes", backend, |b, _ci, a, ctx| {
        handle_ban_changes(&*b, &ctx, a).await
    });
    reg!(d, "SpaceBan/set", backend, |b, _ci, a, ctx, created_ids| {
        handle_ban_set(&*b, &ctx, &created_ids, a).await
    });

    // ChatBan
    reg!(d, "ChatBan/get", backend, |b, _ci, a, ctx| {
        handle_chat_ban_get(&*b, &ctx, a).await
    });
    reg!(d, "ChatBan/changes", backend, |b, _ci, a, ctx| {
        handle_chat_ban_changes(&*b, &ctx, a).await
    });
    reg!(d, "ChatBan/set", backend, |b, _ci, a, ctx, created_ids| {
        handle_chat_ban_set(&*b, &ctx, &created_ids, a).await
    });

    // CustomEmoji
    reg!(d, "CustomEmoji/get", backend, |b, _ci, a, ctx| {
        handle_emoji_get(&*b, &ctx, a).await
    });
    reg!(d, "CustomEmoji/changes", backend, |b, _ci, a, ctx| {
        handle_emoji_changes(&*b, &ctx, a).await
    });
    reg!(d, "CustomEmoji/query", backend, |b, _ci, a, ctx| {
        handle_emoji_query(&*b, &ctx, a).await
    });
    reg!(d, "CustomEmoji/queryChanges", backend, |b, _ci, a, ctx| {
        handle_emoji_query_changes(&*b, &ctx, a).await
    });
    reg!(
        d,
        "CustomEmoji/set",
        backend,
        |b, _ci, a, ctx, created_ids| handle_emoji_set(&*b, &ctx, &created_ids, a).await
    );

    // PresenceStatus
    reg!(d, "PresenceStatus/get", backend, |b, _ci, a, ctx| {
        handle_presence_get(&*b, &ctx, a).await
    });
    reg!(d, "PresenceStatus/changes", backend, |b, _ci, a, ctx| {
        handle_presence_changes(&*b, &ctx, a).await
    });
    reg!(d, "PresenceStatus/set", backend, |b, _ci, a, ctx| {
        handle_presence_set(&*b, &ctx, a).await
    });
}

/// Opt this crate's registered Foo/set methods in to
/// draft-ietf-jmap-conditional-02 (`ifUnchangedBy` and `atomic`;
/// docs/design/DECISIONS.md decisions 1 and 17).
///
/// Call after [`register_chat_handlers`]. Attaches [`SetSupport`] to each
/// of `Chat/set`, `Message/set`, `Space/set`, `ChatContact/set`,
/// `ReadPosition/set`, `SpaceInvite/set`, `SpaceBan/set`, `ChatBan/set`,
/// `CustomEmoji/set` and `PresenceStatus/set` that is registered, and skips
/// the rest; no handler is re-registered. For `PushSubscription/set`, call
/// [`jmap_server::register_push_subscription_conditional`] too.
///
/// The dispatcher reads preconditions through each type's `/get` and runs
/// each call in the scope `backend` opens
/// ([`ConditionalBackend::begin_set`]). Under `atomic` that scope must hold
/// back outbound federation and lifecycle effects until commit and check
/// cross-object constraints (`CustomEmoji` names, the push subscription cap)
/// against the final state; the memory backend does. Advertise
/// `urn:ietf:params:jmap:conditional` only once
/// [`Dispatcher::conditional_ready`] is true for the whole dispatcher.
pub fn register_chat_conditional<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: ChatBackend + ConditionalBackend + 'static,
{
    fn attach<C: Clone + Send + 'static>(
        dispatcher: &mut Dispatcher<C>,
        method: &str,
        support: SetSupport<C>,
    ) {
        match dispatcher.attach_set_support(method, support) {
            Ok(()) | Err(RegistrationError::NotRegistered(_)) => {}
            Err(e) => unreachable!("{method} pairs with its own data type: {e}"),
        }
    }
    macro_rules! attach {
        ($($t:ident),*) => {$(
            attach(
                dispatcher,
                concat!(stringify!($t), "/set"),
                SetSupport::new::<jmap_chat_types::$t, B>(Arc::clone(&backend)),
            );
        )*};
    }
    attach!(
        Chat,
        Message,
        Space,
        ChatContact,
        ReadPosition,
        SpaceInvite,
        SpaceBan,
        ChatBan,
        CustomEmoji,
        PresenceStatus
    );
}

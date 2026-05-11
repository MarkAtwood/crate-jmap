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
//! # fn example<B: ChatBackend + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_chat_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```

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
pub mod message;
pub mod position;
pub mod presence;
pub mod space;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, ChatBackend, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
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
/// Pass the same `Arc<B>` to both this function and any application-level code
/// that needs to access the backend.
///
/// After this call, the dispatcher handles:
/// `Chat/*`, `Message/*`, `Space/*`, `SpaceBan/*`, `ChatContact/*`,
/// `ReadPosition/*`, `CustomEmoji/*`, `SpaceInvite/*`, and `PresenceStatus/*`.
///
/// The dispatcher's `CallerCtx` (`C`) is forwarded into each registered
/// handler. Backends that need to read it from inside a method body can
/// register a custom [`ClosureHandler`] directly on the dispatcher
/// instead of using this convenience function.
pub fn register_chat_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: ChatBackend + 'static,
    C: Clone + Send + 'static,
{
    // Helper: register one method with a closure that takes (Arc<B>, call_id, args).
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<C>> = Arc::new(ClosureHandler {
                backend: backend_arc,
                call_fn: Box::new(
                    move |$b: Arc<B>, $ci: String, $a: serde_json::Value, _ctx: C| {
                        Box::pin(async move { $body }) as HandlerFuture
                    },
                ),
            });
            dispatcher.register($method, h);
        }};
    }

    // Chat
    reg!("Chat/get", backend, |b, _ci, a| handle_chat_get(&*b, a)
        .await);
    reg!("Chat/changes", backend, |b, _ci, a| handle_chat_changes(
        &*b, a
    )
    .await);
    reg!("Chat/query", backend, |b, _ci, a| handle_chat_query(&*b, a)
        .await);
    reg!("Chat/queryChanges", backend, |b, _ci, a| {
        handle_chat_query_changes(&*b, a).await
    });
    reg!("Chat/set", backend, |b, _ci, a| handle_chat_set(&*b, a)
        .await);
    reg!("Chat/typing", backend, |b, _ci, a| handle_chat_typing(
        &*b, a
    )
    .await);

    // Message
    reg!("Message/get", backend, |b, _ci, a| handle_message_get(
        &*b, a
    )
    .await);
    reg!("Message/changes", backend, |b, _ci, a| {
        handle_message_changes(&*b, a).await
    });
    reg!("Message/query", backend, |b, _ci, a| handle_message_query(
        &*b, a
    )
    .await);
    reg!("Message/queryChanges", backend, |b, _ci, a| {
        handle_message_query_changes(&*b, a).await
    });
    reg!("Message/set", backend, |b, _ci, a| handle_message_set(
        &*b, a
    )
    .await);

    // Space
    reg!("Space/get", backend, |b, _ci, a| handle_space_get(&*b, a)
        .await);
    reg!("Space/changes", backend, |b, _ci, a| handle_space_changes(
        &*b, a
    )
    .await);
    reg!("Space/query", backend, |b, _ci, a| handle_space_query(
        &*b, a
    )
    .await);
    reg!("Space/queryChanges", backend, |b, _ci, a| {
        handle_space_query_changes(&*b, a).await
    });
    reg!("Space/set", backend, |b, _ci, a| handle_space_set(&*b, a)
        .await);
    reg!("Space/join", backend, |b, _ci, a| handle_space_join(&*b, a)
        .await);

    // ChatContact
    reg!("ChatContact/get", backend, |b, _ci, a| {
        handle_contact_get(&*b, a).await
    });
    reg!("ChatContact/changes", backend, |b, _ci, a| {
        handle_contact_changes(&*b, a).await
    });
    reg!("ChatContact/query", backend, |b, _ci, a| {
        handle_contact_query(&*b, a).await
    });
    reg!("ChatContact/queryChanges", backend, |b, _ci, a| {
        handle_contact_query_changes(&*b, a).await
    });
    reg!("ChatContact/set", backend, |b, _ci, a| {
        handle_contact_set(&*b, a).await
    });

    // ReadPosition
    reg!("ReadPosition/get", backend, |b, _ci, a| {
        handle_position_get(&*b, a).await
    });
    reg!("ReadPosition/changes", backend, |b, _ci, a| {
        handle_position_changes(&*b, a).await
    });
    reg!("ReadPosition/set", backend, |b, _ci, a| {
        handle_position_set(&*b, a).await
    });

    // SpaceInvite
    reg!("SpaceInvite/get", backend, |b, _ci, a| {
        handle_invite_get(&*b, a).await
    });
    reg!("SpaceInvite/changes", backend, |b, _ci, a| {
        handle_invite_changes(&*b, a).await
    });
    reg!("SpaceInvite/set", backend, |b, _ci, a| {
        handle_invite_set(&*b, a).await
    });

    // SpaceBan
    reg!("SpaceBan/get", backend, |b, _ci, a| {
        handle_ban_get(&*b, a).await
    });
    reg!("SpaceBan/changes", backend, |b, _ci, a| {
        handle_ban_changes(&*b, a).await
    });
    reg!("SpaceBan/set", backend, |b, _ci, a| {
        handle_ban_set(&*b, a).await
    });

    // CustomEmoji
    reg!("CustomEmoji/get", backend, |b, _ci, a| {
        handle_emoji_get(&*b, a).await
    });
    reg!("CustomEmoji/changes", backend, |b, _ci, a| {
        handle_emoji_changes(&*b, a).await
    });
    reg!("CustomEmoji/query", backend, |b, _ci, a| {
        handle_emoji_query(&*b, a).await
    });
    reg!("CustomEmoji/queryChanges", backend, |b, _ci, a| {
        handle_emoji_query_changes(&*b, a).await
    });
    reg!("CustomEmoji/set", backend, |b, _ci, a| {
        handle_emoji_set(&*b, a).await
    });

    // PresenceStatus
    reg!("PresenceStatus/get", backend, |b, _ci, a| {
        handle_presence_get(&*b, a).await
    });
    reg!("PresenceStatus/changes", backend, |b, _ci, a| {
        handle_presence_changes(&*b, a).await
    });
    reg!("PresenceStatus/set", backend, |b, _ci, a| {
        handle_presence_set(&*b, a).await
    });
}

pub use jmap_server::ClosureHandler;

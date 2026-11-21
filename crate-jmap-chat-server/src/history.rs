//! Object history for the chat types (draft-ietf-jmap-object-history-03
//! with the -04 editor's copy changes; docs/design/object-history.md;
//! docs/design/DECISIONS.md decision 22): Chat, Message, Space,
//! [`ChatContact`], [`CustomEmoji`], [`SpaceBan`], [`ChatBan`] and `SpaceInvite`. `ReadPosition` and
//! `PresenceStatus` have none.
//!
//! In the shape of the canonical `jmap-mail-server` opt-in: each
//! registered handler wraps the type's plain handler with
//! `jmap_server::history_get`, `history_query`, `history_query_changes` or
//! `history_set`, so the plain handler runs unchanged when no history
//! argument is set.

use std::sync::Arc;

use jmap_chat_types::{
    Chat, ChatBan, ChatContact, CustomEmoji, Message, Space, SpaceBan, SpaceInvite,
};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, JmapHandler, NoRestoreHooks,
    ObjectHistoryBackend, history_get, history_query, history_query_changes, history_set,
};

use crate::{
    ChatBackend, handle_ban_get, handle_ban_set, handle_chat_ban_get, handle_chat_ban_set,
    handle_chat_get, handle_chat_query, handle_chat_query_changes, handle_chat_set,
    handle_contact_get, handle_contact_query, handle_contact_query_changes, handle_contact_set,
    handle_emoji_get, handle_emoji_query, handle_emoji_query_changes, handle_emoji_set,
    handle_invite_get, handle_invite_set, handle_space_get, handle_space_query,
    handle_space_query_changes, handle_space_set, message, register_chat_handlers,
};

/// [`register_chat_handlers`], then history-aware `/get` and `/set` of the
/// eight chat history types and `/query`, `/queryChanges` of those that
/// have them (Chat, Message, Space, [`ChatContact`], [`CustomEmoji`]), through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). Handlers that take no `using` in the plain registration are bound to
/// the request's here.
///
/// Two chat-00 rules shape what the backend returns (decision 22): a
/// Message's versions go when it is deleted or hard-deleted, so a Message
/// is never restored; a Space destroy purges the history of the Space and
/// everything in it, so a Space restore is always `notFound` and Space/set's
/// structural patch operations never meet a restore. Call
/// `register_chat_conditional` after this, if used.
pub fn register_chat_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: ChatBackend + ObjectHistoryBackend + 'static,
{
    register_chat_handlers(dispatcher, Arc::clone(&backend));
    macro_rules! reg {
        ($method:expr, |$b:ident, $a:ident, $ctx:ident, $using:ident $(, $field:ident)*| $body:expr) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: $a,
                        caller: $ctx,
                        using: $using,
                        $($field,)*
                        ..
                    } = cx;
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register_history_aware($method, h);
        }};
    }

    // Chat
    reg!("Chat/get", |b, a, ctx, using| {
        history_get::<Chat, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_chat_get(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Chat/query", |b, a, ctx, using| {
        history_query::<Chat, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_chat_query(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Chat/queryChanges", |b, a, ctx, using| {
        history_query_changes::<Chat, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_chat_query_changes(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Chat/set", |b, a, ctx, using, created_ids| {
        history_set::<Chat, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_chat_set(&*b, &ctx, &created_ids, a, &using)
        })
        .await
    });

    // Message
    reg!("Message/get", |b, a, ctx, using| {
        history_get::<Message, B, _, _>(&*b, &ctx, a, &using, |a| {
            message::handle_message_get(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Message/query", |b, a, ctx, using| {
        history_query::<Message, B, _, _>(&*b, &ctx, a, &using, |a| {
            message::handle_message_query(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Message/queryChanges", |b, a, ctx, using| {
        history_query_changes::<Message, B, _, _>(&*b, &ctx, a, &using, |a| {
            message::handle_message_query_changes(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Message/set", |b, a, ctx, using, created_ids| {
        history_set::<Message, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            message::handle_message_set(&*b, &ctx, &created_ids, a, &using)
        })
        .await
    });

    // Space
    reg!("Space/get", |b, a, ctx, using| {
        history_get::<Space, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_space_get(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Space/query", |b, a, ctx, using| {
        history_query::<Space, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_space_query(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Space/queryChanges", |b, a, ctx, using| {
        history_query_changes::<Space, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_space_query_changes(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!("Space/set", |b, a, ctx, using, created_ids| {
        history_set::<Space, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_space_set(&*b, &ctx, &created_ids, a, &using)
        })
        .await
    });

    // ChatContact
    reg!("ChatContact/get", |b, a, ctx, using| {
        history_get::<ChatContact, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_contact_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("ChatContact/query", |b, a, ctx, using| {
        history_query::<ChatContact, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_contact_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("ChatContact/queryChanges", |b, a, ctx, using| {
        history_query_changes::<ChatContact, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_contact_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("ChatContact/set", |b, a, ctx, using| {
        history_set::<ChatContact, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_contact_set(&*b, &ctx, a)
        })
        .await
    });

    // CustomEmoji
    reg!("CustomEmoji/get", |b, a, ctx, using| {
        history_get::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_emoji_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("CustomEmoji/query", |b, a, ctx, using| {
        history_query::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_emoji_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("CustomEmoji/queryChanges", |b, a, ctx, using| {
        history_query_changes::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_emoji_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("CustomEmoji/set", |b, a, ctx, using, created_ids| {
        history_set::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_emoji_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });

    // SpaceInvite (restored with its server-set `code` and `uses`)
    reg!("SpaceInvite/get", |b, a, ctx, using| {
        history_get::<SpaceInvite, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_invite_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("SpaceInvite/set", |b, a, ctx, using, created_ids| {
        history_set::<SpaceInvite, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_invite_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });

    // SpaceBan
    reg!("SpaceBan/get", |b, a, ctx, using| {
        history_get::<SpaceBan, B, _, _>(&*b, &ctx, a, &using, |a| handle_ban_get(&*b, &ctx, a))
            .await
    });
    reg!("SpaceBan/set", |b, a, ctx, using, created_ids| {
        history_set::<SpaceBan, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_ban_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });

    // ChatBan
    reg!("ChatBan/get", |b, a, ctx, using| {
        history_get::<ChatBan, B, _, _>(&*b, &ctx, a, &using, |a| handle_chat_ban_get(&*b, &ctx, a))
            .await
    });
    reg!("ChatBan/set", |b, a, ctx, using, created_ids| {
        history_set::<ChatBan, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_chat_ban_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
}

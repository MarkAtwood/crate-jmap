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
    register_chat_message_history(dispatcher, &backend);
    register_space_contact_history(dispatcher, &backend);
    register_space_object_history(dispatcher, &backend);
}

// Register one history-aware method on the dispatcher `$d` with a closure
// over a clone of `$backend`, where `B` is the backend type of the calling
// function.
macro_rules! reg {
    ($d:ident, $backend:ident, $method:expr, |$b:ident, $a:ident, $ctx:ident, $using:ident $(, $field:ident)*| $body:expr) => {{
        let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
            Arc::clone($backend),
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
        $d.register_history_aware($method, h);
    }};
}

/// The Chat and Message part of [`register_chat_handlers_with_history`],
/// on the dispatcher `d`.
fn register_chat_message_history<B>(d: &mut Dispatcher<B::CallerCtx>, backend: &Arc<B>)
where
    B: ChatBackend + ObjectHistoryBackend + 'static,
{
    // Chat
    reg!(d, backend, "Chat/get", |b, a, ctx, using| {
        history_get::<Chat, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_chat_get(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Chat/query", |b, a, ctx, using| {
        history_query::<Chat, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_chat_query(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Chat/queryChanges", |b, a, ctx, using| {
        history_query_changes::<Chat, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_chat_query_changes(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Chat/set", |b, a, ctx, using, created_ids| {
        history_set::<Chat, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_chat_set(&*b, &ctx, &created_ids, a, &using)
        })
        .await
    });

    // Message
    reg!(d, backend, "Message/get", |b, a, ctx, using| {
        history_get::<Message, B, _, _>(&*b, &ctx, a, &using, |a| {
            message::handle_message_get(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Message/query", |b, a, ctx, using| {
        history_query::<Message, B, _, _>(&*b, &ctx, a, &using, |a| {
            message::handle_message_query(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Message/queryChanges", |b, a, ctx, using| {
        history_query_changes::<Message, B, _, _>(&*b, &ctx, a, &using, |a| {
            message::handle_message_query_changes(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(
        d,
        backend,
        "Message/set",
        |b, a, ctx, using, created_ids| {
            history_set::<Message, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
                message::handle_message_set(&*b, &ctx, &created_ids, a, &using)
            })
            .await
        }
    );
}

/// The Space and [`ChatContact`] part of
/// [`register_chat_handlers_with_history`], on the dispatcher `d`.
fn register_space_contact_history<B>(d: &mut Dispatcher<B::CallerCtx>, backend: &Arc<B>)
where
    B: ChatBackend + ObjectHistoryBackend + 'static,
{
    // Space
    reg!(d, backend, "Space/get", |b, a, ctx, using| {
        history_get::<Space, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_space_get(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Space/query", |b, a, ctx, using| {
        history_query::<Space, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_space_query(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Space/queryChanges", |b, a, ctx, using| {
        history_query_changes::<Space, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_space_query_changes(&*b, &ctx, a, &using)
        })
        .await
    });
    reg!(d, backend, "Space/set", |b, a, ctx, using, created_ids| {
        history_set::<Space, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_space_set(&*b, &ctx, &created_ids, a, &using)
        })
        .await
    });

    // ChatContact
    reg!(d, backend, "ChatContact/get", |b, a, ctx, using| {
        history_get::<ChatContact, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_contact_get(&*b, &ctx, a)
        })
        .await
    });
    reg!(d, backend, "ChatContact/query", |b, a, ctx, using| {
        history_query::<ChatContact, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_contact_query(&*b, &ctx, a)
        })
        .await
    });
    reg!(
        d,
        backend,
        "ChatContact/queryChanges",
        |b, a, ctx, using| {
            history_query_changes::<ChatContact, B, _, _>(&*b, &ctx, a, &using, |a| {
                handle_contact_query_changes(&*b, &ctx, a)
            })
            .await
        }
    );
    reg!(d, backend, "ChatContact/set", |b, a, ctx, using| {
        history_set::<ChatContact, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_contact_set(&*b, &ctx, a)
        })
        .await
    });
}

/// The [`CustomEmoji`], `SpaceInvite`, [`SpaceBan`] and [`ChatBan`] part of
/// [`register_chat_handlers_with_history`], on the dispatcher `d`.
fn register_space_object_history<B>(d: &mut Dispatcher<B::CallerCtx>, backend: &Arc<B>)
where
    B: ChatBackend + ObjectHistoryBackend + 'static,
{
    // CustomEmoji
    reg!(d, backend, "CustomEmoji/get", |b, a, ctx, using| {
        history_get::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_emoji_get(&*b, &ctx, a)
        })
        .await
    });
    reg!(d, backend, "CustomEmoji/query", |b, a, ctx, using| {
        history_query::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_emoji_query(&*b, &ctx, a)
        })
        .await
    });
    reg!(
        d,
        backend,
        "CustomEmoji/queryChanges",
        |b, a, ctx, using| {
            history_query_changes::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, |a| {
                handle_emoji_query_changes(&*b, &ctx, a)
            })
            .await
        }
    );
    reg!(
        d,
        backend,
        "CustomEmoji/set",
        |b, a, ctx, using, created_ids| {
            history_set::<CustomEmoji, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
                handle_emoji_set(&*b, &ctx, &created_ids, a)
            })
            .await
        }
    );

    // SpaceInvite (restored with its server-set `code` and `uses`)
    reg!(d, backend, "SpaceInvite/get", |b, a, ctx, using| {
        history_get::<SpaceInvite, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_invite_get(&*b, &ctx, a)
        })
        .await
    });
    reg!(
        d,
        backend,
        "SpaceInvite/set",
        |b, a, ctx, using, created_ids| {
            history_set::<SpaceInvite, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
                handle_invite_set(&*b, &ctx, &created_ids, a)
            })
            .await
        }
    );

    // SpaceBan
    reg!(d, backend, "SpaceBan/get", |b, a, ctx, using| {
        history_get::<SpaceBan, B, _, _>(&*b, &ctx, a, &using, |a| handle_ban_get(&*b, &ctx, a))
            .await
    });
    reg!(
        d,
        backend,
        "SpaceBan/set",
        |b, a, ctx, using, created_ids| {
            history_set::<SpaceBan, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
                handle_ban_set(&*b, &ctx, &created_ids, a)
            })
            .await
        }
    );

    // ChatBan
    reg!(d, backend, "ChatBan/get", |b, a, ctx, using| {
        history_get::<ChatBan, B, _, _>(&*b, &ctx, a, &using, |a| handle_chat_ban_get(&*b, &ctx, a))
            .await
    });
    reg!(
        d,
        backend,
        "ChatBan/set",
        |b, a, ctx, using, created_ids| {
            history_set::<ChatBan, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
                handle_chat_ban_set(&*b, &ctx, &created_ids, a)
            })
            .await
        }
    );
}

//! Object history for VTCCall, VTCRecording and VTCLivestream
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md; docs/design/DECISIONS.md decision 22).
//! VTCParticipant keeps none.

use std::sync::Arc;

use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, JmapHandler, NoRestoreHooks,
    ObjectHistoryBackend, history_get, history_query, history_query_changes, history_set,
};
use jmap_vtc_types::{VtcCall, VtcLivestream, VtcRecording};

use crate::media::livestream_get_with;
use crate::{
    VtcBackend, handle_vtc_call_get, handle_vtc_call_query, handle_vtc_call_query_changes,
    handle_vtc_call_set, handle_vtc_livestream_query, handle_vtc_livestream_query_changes,
    handle_vtc_livestream_set, handle_vtc_recording_get, handle_vtc_recording_query,
    handle_vtc_recording_query_changes, handle_vtc_recording_set, register_vtc_handlers,
};

/// [`register_vtc_handlers`], then history-aware `/get`, `/set`, `/query`
/// and `/queryChanges` of VTCCall, VTCRecording and VTCLivestream, each
/// wrapping the unchanged plain handler, through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). Every VTCLivestream version gets the same `streamKey`/`streamUri`
/// blanking as the current one (§8.2). History reads emit no VTC events;
/// a restore emits what an update would. A restored recording or
/// livestream needs a current call.
pub fn register_vtc_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: VtcBackend + ObjectHistoryBackend + 'static,
{
    register_vtc_handlers(dispatcher, Arc::clone(&backend));
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
    reg!("VTCCall/get", |b, a, ctx, using| {
        history_get::<VtcCall, B, _, _>(&*b, &ctx, a, &using, |a| handle_vtc_call_get(&*b, &ctx, a))
            .await
    });
    reg!("VTCCall/query", |b, a, ctx, using| {
        history_query::<VtcCall, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_call_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCCall/queryChanges", |b, a, ctx, using| {
        history_query_changes::<VtcCall, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_call_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCCall/set", |b, a, ctx, using, created_ids| {
        history_set::<VtcCall, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_vtc_call_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
    reg!("VTCRecording/get", |b, a, ctx, using| {
        history_get::<VtcRecording, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_recording_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCRecording/query", |b, a, ctx, using| {
        history_query::<VtcRecording, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_recording_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCRecording/queryChanges", |b, a, ctx, using| {
        history_query_changes::<VtcRecording, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_recording_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCRecording/set", |b, a, ctx, using, created_ids| {
        history_set::<VtcRecording, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_vtc_recording_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
    reg!("VTCLivestream/get", |b, a, ctx, using| {
        livestream_get_with(&*b, &ctx, a, |a| {
            history_get::<VtcLivestream, B, _, _>(&*b, &ctx, a, &using, |a| {
                jmap_server::handle_get::<VtcLivestream, B>(&*b, &ctx, a)
            })
        })
        .await
    });
    reg!("VTCLivestream/query", |b, a, ctx, using| {
        history_query::<VtcLivestream, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_livestream_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCLivestream/queryChanges", |b, a, ctx, using| {
        history_query_changes::<VtcLivestream, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_vtc_livestream_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("VTCLivestream/set", |b, a, ctx, using, created_ids| {
        history_set::<VtcLivestream, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_vtc_livestream_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
}

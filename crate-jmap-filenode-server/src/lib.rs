//! JMAP FileNode extension method handlers (draft-ietf-jmap-filenode-13).
//!
//! # Usage
//!
//! Implement [`FileNodeBackend`] for your storage layer, then call
//! [`register_filenode_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_filenode_server::{FileNodeBackend, register_filenode_handlers};
//! # use jmap_server::{Dispatcher, JmapBackend};
//! # fn example<B: FileNodeBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_filenode_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`FileNodeBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod filenode;
mod helpers;
/// In-memory reference implementation of [`FileNodeBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, FileNodeBackend,
    FileNodeProperty, GetObject, JmapBackend, JmapObject, QueryChangesResult, QueryObject,
    QueryResult, SetError, SetErrorType, SetObject,
};
pub use filenode::{
    handle_filenode_changes, handle_filenode_copy, handle_filenode_get, handle_filenode_query,
    handle_filenode_query_changes, handle_filenode_set,
};

/// Capability URI for `urn:ietf:params:jmap:filenode`.
pub use jmap_filenode_types::JMAP_FILENODE_URI;

// ---------------------------------------------------------------------------
// register_filenode_handlers — main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP FileNode method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler
/// closure.  Pass the same `Arc<B>` to both this function and any
/// application-level code that needs the backend directly.
///
/// After this call the dispatcher handles:
/// `FileNode/get`, `FileNode/changes`, `FileNode/set`,
/// `FileNode/copy`, `FileNode/query`, `FileNode/queryChanges`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
pub fn register_filenode_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: FileNodeBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx). `$ctx` is the per-request caller context
    // (`B::CallerCtx`) forwarded by the dispatcher; closures pass `&ctx` to the
    // inner `handle_*` fn.
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

    reg!("FileNode/get", backend, |b, _ci, a, ctx| {
        handle_filenode_get(&*b, &ctx, a).await
    });
    reg!("FileNode/changes", backend, |b, _ci, a, ctx| {
        handle_filenode_changes(&*b, &ctx, a).await
    });
    reg!("FileNode/set", backend, |b, _ci, a, ctx| {
        handle_filenode_set(&*b, &ctx, a).await
    });
    reg!("FileNode/copy", backend, |b, ci, a, ctx| {
        handle_filenode_copy(&*b, &ctx, a, &ci).await
    });
    reg!("FileNode/query", backend, |b, _ci, a, ctx| {
        handle_filenode_query(&*b, &ctx, a).await
    });
    reg!("FileNode/queryChanges", backend, |b, _ci, a, ctx| {
        handle_filenode_query_changes(&*b, &ctx, a).await
    });
}

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`].
///
/// Re-exported so the [`register_filenode_handlers`] macro body can name
/// `ClosureHandler` without a fully-qualified path. **Stability**: this
/// re-export pins the major-version contract of [`jmap_server::ClosureHandler`]
/// into this crate's public surface — a breaking change to that type
/// upstream is a breaking change here. Consumers needing a closure handler
/// adapter SHOULD prefer importing from [`jmap_server`] directly; the
/// re-export is retained primarily for the in-crate macro and for
/// backward-compatible spelling of the existing handler-registration
/// pattern.
pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jmap_server::{Dispatcher, JmapRequest, State};
    use serde_json::json;

    use super::*;
    use crate::memory::MemoryBackend;

    fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
        JmapRequest::new(
            vec![jmap_filenode_types::JMAP_FILENODE_URI.into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    /// Oracle: register_filenode_handlers registers all 6 JMAP FileNode methods.
    ///
    /// Verification: each method returns a non-`unknownMethod` response when
    /// dispatched with a valid account.
    #[tokio::test]
    async fn registers_all_6_methods() {
        let backend = Arc::new(MemoryBackend::new().with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_filenode_handlers(&mut dispatcher, Arc::clone(&backend));

        let methods = [
            ("FileNode/get", json!({"accountId": "acc1", "ids": null})),
            (
                "FileNode/changes",
                json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            ("FileNode/set", json!({"accountId": "acc1", "destroy": []})),
            (
                "FileNode/copy",
                json!({
                    "fromAccountId": "acc1",
                    "accountId": "acc1",
                    "create": {}
                }),
            ),
            (
                "FileNode/query",
                json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "FileNode/queryChanges",
                json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
        ];

        for (method, args) in methods {
            let req = single_call(method, args, "c0");
            let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
            assert_eq!(
                resp.method_responses.len(),
                1,
                "{method}: expected 1 response"
            );
            let (_, resp_args, _) = &resp.method_responses[0];
            assert_ne!(
                resp_args["type"], "unknownMethod",
                "{method}: must not be unknownMethod — is it registered?"
            );
        }
    }

    /// Oracle: FileNode/set empty destroy returns valid empty set response.
    #[tokio::test]
    async fn set_empty_destroy_is_valid() {
        let backend = Arc::new(MemoryBackend::new().with_account("acc1"));
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_filenode_handlers(&mut dispatcher, Arc::clone(&backend));

        let req = single_call(
            "FileNode/set",
            json!({"accountId": "acc1", "destroy": []}),
            "c0",
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_none(),
            "must not be an error response: {args}"
        );
        assert_eq!(args["accountId"], "acc1");
    }
}

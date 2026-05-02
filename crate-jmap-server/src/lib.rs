//! Backend-agnostic JMAP server framework (RFC 8620).
//!
//! Provides request parsing, ResultReference resolution, HTTP response helpers,
//! the [`Dispatcher`] machinery, shared backend infrastructure, and generic
//! JMAP method handlers.

#![forbid(unsafe_code)]

pub use jmap_types::{
    Argument, Id, Invocation, JmapError, JmapRequest, JmapResponse, ResultReference, State, UTCDate,
};

pub mod backend;
pub mod handlers;
pub(crate) mod helpers;

pub use backend::{
    AddedItem, BackendChangesError, ChangesResult, GetObject, JmapBackend, JmapObject,
    QueryChangesResult, QueryObject, QueryResult, SetObject,
};
pub use handlers::{handle_changes, handle_get, handle_query, handle_query_changes};
pub use helpers::{extract_account_id, not_found_json, now_utc_string, ser};

mod parse;
mod response;

pub use parse::{parse_request, resolve_args};
pub use response::{error_invocation, error_status, request_error, RequestError};

use std::{collections::HashMap, fmt, future::Future, pin::Pin, sync::Arc};

use serde_json::Value;
use tokio::task;

/// The return type for all [`JmapHandler`] implementations.
///
/// Handlers must return a `Send` future.  The concrete type is a heap-allocated
/// trait object so the trait itself remains object-safe.
///
/// The `Vec<Invocation>` holds zero or more additional entries to append to
/// `methodResponses` immediately after the primary response (in order).  Most
/// handlers return an empty `Vec`.  RFC 8621 §7.5 `EmailSubmission/set` uses
/// this to append the implicit `Email/set` invocation for `onSuccessUpdateEmail`.
pub type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<(Value, Vec<Invocation>), JmapError>> + Send>>;

/// Implement this for each JMAP method handler.
///
/// `CallerCtx` is whatever your auth layer produces — an `Identity`, a session
/// token, `()`, etc. The dispatcher passes it through unchanged.
///
/// # /set response contract
///
/// Handlers for `/set` methods (RFC 8620 §5.3) that create objects MUST include
/// an `"id"` field (type string) in each entry of the `"created"` map.  The
/// dispatcher reads this field to accumulate `createdIds` in the response.
/// Entries without an `"id"` field are silently skipped — the dispatcher cannot
/// retroactively error a method call that already returned success.
pub trait JmapHandler<CallerCtx>: Send + Sync {
    /// `method` is the registered method name for this call.  A single handler
    /// instance may be registered under multiple names (e.g. both `"Foo/get"` and
    /// `"Bar/get"`); this parameter lets the handler distinguish between them.
    ///
    /// `call_id` is the client-supplied identifier for this invocation (RFC 8620 §3.3).
    /// Handlers may use it for logging or correlation but need not echo it —
    /// the dispatcher echoes it in the response automatically.
    ///
    /// Both parameters are `String` (not `&str`) because the returned future is
    /// `'static` — it must own all data it captures.  Handlers that do not need
    /// `method`/`call_id` can ignore them; handlers that do (e.g. echo) simply
    /// capture the owned value.
    fn call(
        &self,
        method: String,
        call_id: String,
        args: Value,
        caller: CallerCtx,
    ) -> HandlerFuture;
}

/// Dispatches a [`JmapRequest`] to registered method handlers.
///
/// Register handlers with [`Dispatcher::register`], then call
/// [`Dispatcher::dispatch`] per request.  `CallerCtx` is cloned for each
/// method call in the batch, so it must be `Clone`.
///
/// `CallerCtx` must also be `'static` because each handler call is spawned as
/// a [`tokio::task`].  To share non-static data (e.g. a database connection),
/// wrap it in `Arc<T>` — `Arc` is `Clone + Send + 'static` when `T: Send + Sync`.
///
/// # Thread safety
///
/// `Dispatcher` is both `Send` and `Sync`.  Register handlers on one thread,
/// then wrap in `Arc` and share across tasks — `dispatch` takes `&self` and is
/// safe to call concurrently.
pub struct Dispatcher<CallerCtx> {
    handlers: HashMap<String, Arc<dyn JmapHandler<CallerCtx>>>,
}

impl<CallerCtx: Clone + Send + 'static> Dispatcher<CallerCtx> {
    /// Create an empty dispatcher with no registered handlers.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for the given method name.
    ///
    /// Registering the same name twice replaces the earlier handler.
    ///
    /// Using `Arc` rather than `Box` allows the same handler instance to be
    /// shared across multiple method name registrations (via `Arc::clone`).
    pub fn register(
        &mut self,
        method: impl Into<String>,
        handler: Arc<dyn JmapHandler<CallerCtx>>,
    ) {
        self.handlers.insert(method.into(), handler);
    }

    /// Process a validated [`JmapRequest`] and return a [`JmapResponse`].
    ///
    /// Method calls are processed sequentially per RFC 8620 §3.3.  Each
    /// handler runs in a `tokio::task::spawn` for panic isolation: a panicking
    /// handler returns a `serverFail` invocation rather than crashing the
    /// connection task.
    ///
    /// `CallerCtx` must be `Clone + Send + 'static`; see the struct-level doc.
    ///
    /// # Cancellation
    ///
    /// If this future is dropped while a handler task is running (e.g., the
    /// HTTP connection closes), the spawned task runs to completion — tokio
    /// does not cancel tasks when their `JoinHandle` is dropped.  The handler
    /// result is discarded.  Callers that need strict cancellation should
    /// implement it at the handler level (e.g., `tokio::select!` with a
    /// shutdown signal).
    pub async fn dispatch(
        &self,
        request: JmapRequest,
        caller: CallerCtx,
        session_state: State,
    ) -> JmapResponse {
        let mut method_responses: Vec<Invocation> = Vec::with_capacity(request.method_calls.len());
        let client_sent_created_ids = request.created_ids.is_some();
        let mut created_ids: HashMap<Id, Id> = request.created_ids.unwrap_or_default();

        // Invocation layout: (method_name, args, call_id) — RFC 8620 §3.3.
        for (method, mut args, call_id) in request.method_calls {
            // Resolve ResultReferences from prior responses.
            if let Err(e) = resolve_args(&mut args, &method_responses) {
                method_responses.push(error_invocation(&call_id, e));
                continue;
            }

            // Look up the handler.
            let handler = match self.handlers.get(&method) {
                Some(h) => Arc::clone(h),
                None => {
                    method_responses.push(error_invocation(&call_id, JmapError::unknown_method()));
                    continue;
                }
            };

            let caller_clone = caller.clone();
            let method_clone = method.clone();
            let call_id_clone = call_id.clone();

            // Run in a spawned task for panic isolation.
            let result: Result<
                Result<(Value, Vec<Invocation>), JmapError>,
                tokio::task::JoinError,
            > = task::spawn(async move {
                handler
                    .call(method_clone, call_id_clone, args, caller_clone)
                    .await
            })
            .await;

            match result {
                Ok(Ok((primary_value, extra_invocations))) => {
                    // Accumulate createdIds from /set responses (RFC 8620 §3.4).
                    // Only when the client sent createdIds; otherwise the field
                    // is omitted from the response.
                    if client_sent_created_ids {
                        if let Some(map) = primary_value.get("created").and_then(|v| v.as_object())
                        {
                            for (client_id, created_obj) in map {
                                // RFC 8620 §5.3 requires each created object to contain
                                // an "id" field.  If the handler violates this contract
                                // (no "id" key or non-string value), the entry is silently
                                // skipped — the dispatcher cannot produce an error for a
                                // method call that already succeeded.
                                if let Some(id_val) = created_obj.get("id").and_then(|v| v.as_str())
                                {
                                    created_ids.insert(client_id.as_str().into(), id_val.into());
                                }
                            }
                        }
                    }
                    // Push the primary response first, then any extra invocations
                    // appended by the handler (e.g. onSuccessUpdateEmail from
                    // EmailSubmission/set, RFC 8621 §7.5).  Order is preserved.
                    method_responses.push((method, primary_value, call_id));
                    method_responses.extend(extra_invocations);
                }
                Ok(Err(e)) => {
                    method_responses.push(error_invocation(&call_id, e));
                }
                Err(join_err) => {
                    // Panics and cancellations both map to serverFail, but with
                    // distinct descriptions to aid server-side diagnostics.
                    let desc = if join_err.is_cancelled() {
                        "task cancelled"
                    } else {
                        "internal error"
                    };
                    method_responses.push(error_invocation(&call_id, JmapError::server_fail(desc)));
                }
            }
        }

        let created_ids = client_sent_created_ids.then_some(created_ids);

        JmapResponse::new(method_responses, session_state, created_ids)
    }
}

impl<CallerCtx: Clone + Send + 'static> Default for Dispatcher<CallerCtx> {
    fn default() -> Self {
        Self::new()
    }
}

impl<CallerCtx> fmt::Debug for Dispatcher<CallerCtx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dispatcher")
            .field("methods", &self.handlers.keys())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    // Compile-time: Dispatcher must be Send + Sync so it can be wrapped in Arc
    // and shared across tokio tasks.  This assertion catches future regressions
    // that would silently break thread-safety (e.g., adding a Cell or Rc field).
    #[allow(dead_code)]
    fn assert_dispatcher_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Dispatcher<String>>();
        check::<Dispatcher<()>>();
    }

    // -----------------------------------------------------------------------
    // Test handler implementations
    // -----------------------------------------------------------------------

    /// Returns a fixed Value regardless of inputs.
    struct EchoHandler(Value);

    impl<C: Clone + Send + 'static> JmapHandler<C> for EchoHandler {
        fn call(
            &self,
            _method: String,
            _call_id: String,
            _args: Value,
            _caller: C,
        ) -> HandlerFuture {
            let v = self.0.clone();
            Box::pin(async move { Ok((v, vec![])) })
        }
    }

    /// Returns a fixed error.
    struct ErrorHandler(JmapError);

    impl JmapHandler<String> for ErrorHandler {
        fn call(
            &self,
            _method: String,
            _call_id: String,
            _args: Value,
            _caller: String,
        ) -> HandlerFuture {
            let e = self.0.clone();
            Box::pin(async move { Err(e) })
        }
    }

    /// Captures the resolved args it was called with.
    struct CaptureArgsHandler(Arc<Mutex<Option<Value>>>);

    impl JmapHandler<String> for CaptureArgsHandler {
        fn call(
            &self,
            _method: String,
            _call_id: String,
            args: Value,
            _caller: String,
        ) -> HandlerFuture {
            let slot = self.0.clone();
            Box::pin(async move {
                *slot.lock().expect("test: mutex poisoned") = Some(args);
                Ok((json!({}), vec![]))
            })
        }
    }

    /// Captures the caller value it was called with.
    struct CaptureCallerHandler(Arc<Mutex<Option<String>>>);

    impl JmapHandler<String> for CaptureCallerHandler {
        fn call(
            &self,
            _method: String,
            _call_id: String,
            _args: Value,
            caller: String,
        ) -> HandlerFuture {
            let slot = self.0.clone();
            Box::pin(async move {
                *slot.lock().expect("test: mutex poisoned") = Some(caller);
                Ok((json!({}), vec![]))
            })
        }
    }

    /// Panics unconditionally.
    struct PanicHandler;

    impl JmapHandler<String> for PanicHandler {
        fn call(
            &self,
            _method: String,
            _call_id: String,
            _args: Value,
            _caller: String,
        ) -> HandlerFuture {
            Box::pin(async move { panic!("deliberate test panic") })
        }
    }

    // -----------------------------------------------------------------------
    // Helper: build a minimal JmapRequest with a single method call.
    // -----------------------------------------------------------------------

    fn single_call(method: &str, args: Value, call_id: &str) -> JmapRequest {
        JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![(method.into(), args, call_id.into())],
            None,
        )
    }

    // -----------------------------------------------------------------------
    // Basic dispatch
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §7.1 — unknownMethod when no handler is registered.
    #[tokio::test]
    async fn unknown_method_returns_error_invocation() {
        let d: Dispatcher<String> = Dispatcher::new();
        let req = single_call("Foo/get", json!({}), "c0");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 1);
        let (_, args, call_id) = &resp.method_responses[0];
        assert_eq!(call_id, "c0");
        assert_eq!(args["type"], "unknownMethod");
    }

    /// Oracle: RFC 8620 §3.5 — successful call appears in methodResponses.
    #[tokio::test]
    async fn known_method_success() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("Foo/get", Arc::new(EchoHandler(json!({"list": []}))));
        let req = single_call("Foo/get", json!({}), "c1");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 1);
        let (method, args, call_id) = &resp.method_responses[0];
        assert_eq!(method, "Foo/get");
        assert_eq!(call_id, "c1");
        assert_eq!(args["list"], json!([]));
    }

    /// Oracle: RFC 8620 §3.6.2 — method-level errors appear in methodResponses.
    #[tokio::test]
    async fn handler_returns_error() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("Foo/get", Arc::new(ErrorHandler(JmapError::not_found())));
        let req = single_call("Foo/get", json!({}), "c2");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 1);
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(args["type"], "notFound");
    }

    /// Oracle: RFC 8620 §3.4 — sessionState in response matches what dispatcher was given.
    #[tokio::test]
    async fn session_state_echoed() {
        let d: Dispatcher<String> = Dispatcher::new();
        let req = JmapRequest::new(vec!["urn:ietf:params:jmap:core".into()], vec![], None);
        let resp = d.dispatch(req, "alice".into(), "my-state-123".into()).await;
        assert_eq!(resp.session_state.as_ref(), "my-state-123");
    }

    // -----------------------------------------------------------------------
    // Batch
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.3 — methodCalls processed in order, all responses present.
    /// Also covers: error in one method does not abort the batch (RFC 8620 §3.6.2).
    #[tokio::test]
    async fn mixed_batch_all_responses_in_order() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("M/a", Arc::new(EchoHandler(json!({"ok": true}))));
        // "M/b" is NOT registered → unknownMethod
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![
                ("M/a".into(), json!({}), "c0".into()),
                ("M/b".into(), json!({}), "c1".into()),
                ("M/a".into(), json!({}), "c2".into()),
            ],
            None,
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(
            resp.method_responses.len(),
            3,
            "all three calls must produce a response"
        );
        // responses[0]: M/a success
        assert_eq!(resp.method_responses[0].2, "c0");
        assert!(
            resp.method_responses[0].1.get("type").is_none(),
            "c0 must not be an error"
        );
        // responses[1]: M/b unknownMethod
        assert_eq!(resp.method_responses[1].2, "c1");
        assert_eq!(resp.method_responses[1].1["type"], "unknownMethod");
        // responses[2]: M/a success (error in [1] did not abort the batch)
        assert_eq!(resp.method_responses[2].2, "c2");
        assert!(
            resp.method_responses[2].1.get("type").is_none(),
            "c2 must not be an error"
        );
    }

    /// Oracle: RFC 8620 §3.6.2 — error in one method does not abort subsequent calls.
    #[tokio::test]
    async fn error_does_not_abort_subsequent_calls() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("M/ok", Arc::new(EchoHandler(json!({"ok": true}))));
        d.register("M/err", Arc::new(ErrorHandler(JmapError::forbidden())));
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![
                ("M/err".into(), json!({}), "c0".into()),
                ("M/ok".into(), json!({}), "c1".into()),
            ],
            None,
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 2);
        assert_eq!(resp.method_responses[0].1["type"], "forbidden");
        assert!(
            resp.method_responses[1].1.get("type").is_none(),
            "second call must succeed"
        );
    }

    // -----------------------------------------------------------------------
    // Panic isolation
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §7.1 serverFail; PLAN.md panic isolation design decision.
    #[tokio::test]
    async fn panicking_handler_returns_server_fail() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("Panic/now", Arc::new(PanicHandler));
        let req = single_call("Panic/now", json!({}), "c0");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 1);
        let (_, args, _) = &resp.method_responses[0];
        assert_eq!(
            args["type"], "serverFail",
            "panicking handler must produce serverFail"
        );
    }

    /// Oracle: security invariant — panic payloads may contain secrets, must not be leaked.
    #[tokio::test]
    async fn panic_message_not_in_response() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("Panic/now", Arc::new(PanicHandler));
        let req = single_call("Panic/now", json!({}), "c0");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        let (_, args, _) = &resp.method_responses[0];
        if let Some(desc) = args["description"].as_str() {
            assert!(
                !desc.contains("deliberate test panic"),
                "panic message must not leak into response description"
            );
        }
    }

    // -----------------------------------------------------------------------
    // ResultReference end-to-end
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.7 — #-prefixed args resolved from prior responses before handler call.
    #[tokio::test]
    async fn result_reference_resolved_before_dispatch() {
        let captured = Arc::new(Mutex::new(None::<Value>));
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register(
            "Foo/get",
            Arc::new(EchoHandler(json!({"list": [{"id": "item-1"}]}))),
        );
        d.register(
            "Bar/query",
            Arc::new(CaptureArgsHandler(Arc::clone(&captured))),
        );
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![
                ("Foo/get".into(), json!({}), "c0".into()),
                (
                    "Bar/query".into(),
                    json!({"#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/list/0/id"}}),
                    "c1".into(),
                ),
            ],
            None,
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 2);
        // c1 must succeed, not be an error
        assert!(
            resp.method_responses[1].1.get("type").is_none(),
            "Bar/query must succeed after ResultReference resolution"
        );
        // Handler must have received the resolved value, not the original #ids object
        let got = captured
            .lock()
            .unwrap()
            .clone()
            .expect("CaptureArgsHandler was not called");
        assert_eq!(
            got["ids"],
            json!("item-1"),
            "resolved value must be the string item-1"
        );
        assert!(
            got.get("#ids").is_none(),
            "#ids key must have been replaced"
        );
    }

    /// Oracle: RFC 8620 §3.7 — resolution failure → error for that call, batch continues.
    #[tokio::test]
    async fn result_reference_failure_stops_that_call() {
        let d: Dispatcher<String> = Dispatcher::new();
        let req = single_call(
            "Foo/get",
            json!({"#ids": {"resultOf": "nonexistent", "name": "Foo/get", "path": "/x"}}),
            "c0",
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 1);
        let (_, args, _) = &resp.method_responses[0];
        assert!(
            args.get("type").is_some(),
            "failed ResultReference must produce an error invocation"
        );
    }

    // -----------------------------------------------------------------------
    // createdIds
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.3 createdIds — server-assigned IDs returned from /set
    /// responses are accumulated into resp.created_ids when client sent createdIds.
    #[tokio::test]
    async fn created_ids_accumulated_from_set_response() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register(
            "Foo/set",
            Arc::new(EchoHandler(
                json!({"created": {"client-1": {"id": "server-abc"}}}),
            )),
        );
        // Client sends createdIds (empty map) to signal it wants the response field.
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![("Foo/set".into(), json!({}), "c0".into())],
            Some(std::collections::HashMap::new()),
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        let ids = resp
            .created_ids
            .as_ref()
            .expect("created_ids must be Some when client sent createdIds");
        assert_eq!(
            ids.get(&Id::from("client-1")),
            Some(&Id::from("server-abc")),
            "client-1 must map to server-abc"
        );
    }

    /// Oracle: RFC 8620 §3.4 — createdIds omitted when no objects were created.
    #[tokio::test]
    async fn created_ids_absent_when_no_set() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("Foo/get", Arc::new(EchoHandler(json!({"list": []}))));
        let req = single_call("Foo/get", json!({}), "c0");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert!(
            resp.created_ids.is_none(),
            "created_ids must be None when no /set call created objects"
        );
    }

    /// Oracle: RFC 8620 §3.3 — createdIds accumulates across ALL /set calls in the batch.
    #[tokio::test]
    async fn created_ids_accumulated_across_multiple_set_calls() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register(
            "A/set",
            Arc::new(EchoHandler(json!({"created": {"cA": {"id": "sA"}}}))),
        );
        d.register(
            "B/set",
            Arc::new(EchoHandler(json!({"created": {"cB": {"id": "sB"}}}))),
        );
        // Client sends createdIds to signal it wants the response field.
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![
                ("A/set".into(), json!({}), "c0".into()),
                ("B/set".into(), json!({}), "c1".into()),
            ],
            Some(std::collections::HashMap::new()),
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        let ids = resp
            .created_ids
            .as_ref()
            .expect("created_ids must be Some when client sent createdIds");
        assert_eq!(
            ids.get(&Id::from("cA")),
            Some(&Id::from("sA")),
            "cA must be present"
        );
        assert_eq!(
            ids.get(&Id::from("cB")),
            Some(&Id::from("sB")),
            "cB must be present"
        );
    }

    /// Oracle: RFC 8620 §3.4 — pre-populated client createdIds are preserved and
    /// new /set entries are merged in alongside them.
    #[tokio::test]
    async fn created_ids_merges_with_pre_populated_map() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register(
            "Foo/set",
            Arc::new(EchoHandler(
                json!({"created": {"client-new": {"id": "server-new"}}}),
            )),
        );
        // Client sends a pre-populated createdIds map.
        let mut initial = std::collections::HashMap::new();
        initial.insert(Id::from("client-old"), Id::from("server-old"));
        let req = JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".into()],
            vec![("Foo/set".into(), json!({}), "c0".into())],
            Some(initial),
        );
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        let ids = resp
            .created_ids
            .as_ref()
            .expect("created_ids must be Some when client sent createdIds");
        assert_eq!(
            ids.get(&Id::from("client-old")),
            Some(&Id::from("server-old")),
            "pre-populated entry must be preserved"
        );
        assert_eq!(
            ids.get(&Id::from("client-new")),
            Some(&Id::from("server-new")),
            "new /set entry must be merged in"
        );
    }

    // -----------------------------------------------------------------------
    // CallerCtx
    // -----------------------------------------------------------------------

    /// Oracle: PLAN.md CallerCtx design — caller value passed through to handler unchanged.
    #[tokio::test]
    async fn caller_ctx_passed_to_handler() {
        let captured = Arc::new(Mutex::new(None::<String>));
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register(
            "Foo/get",
            Arc::new(CaptureCallerHandler(Arc::clone(&captured))),
        );
        let req = single_call("Foo/get", json!({}), "c0");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
        assert!(
            resp.method_responses[0].1.get("type").is_none(),
            "must succeed"
        );
        let got = captured
            .lock()
            .unwrap()
            .clone()
            .expect("handler was not called");
        assert_eq!(got, "alice", "caller must be passed through unchanged");
    }

    /// Oracle: PLAN.md — CallerCtx = () must work (unit type as auth context).
    #[tokio::test]
    async fn unit_caller_ctx_works() {
        let mut d: Dispatcher<()> = Dispatcher::new();
        d.register("Foo/get", Arc::new(EchoHandler(json!({"ok": true}))));
        let req = single_call("Foo/get", json!({}), "c0");
        let resp = d.dispatch(req, (), "s0".into()).await;
        assert_eq!(resp.method_responses.len(), 1);
        assert!(
            resp.method_responses[0].1.get("type").is_none(),
            "must succeed with () caller"
        );
    }

    // -----------------------------------------------------------------------
    // Extra invocations
    // -----------------------------------------------------------------------

    /// A handler that returns both a primary response and one extra invocation.
    ///
    /// Models RFC 8621 §7.5 EmailSubmission/set with onSuccessUpdateEmail: the
    /// submission response is primary; the implied Email/set call is extra.
    struct ExtraInvocationHandler;

    impl JmapHandler<String> for ExtraInvocationHandler {
        fn call(
            &self,
            _method: String,
            _call_id: String,
            _args: Value,
            _caller: String,
        ) -> HandlerFuture {
            Box::pin(async move {
                let primary = json!({"type": "primary"});
                let extra: Vec<Invocation> = vec![(
                    "Extra/call".to_owned(),
                    json!({"type": "extra"}),
                    "x0".to_owned(),
                )];
                Ok((primary, extra))
            })
        }
    }

    /// Oracle: handler returning extra invocations → both primary and extra appear in
    /// methodResponses in order (primary first, then extra).
    #[tokio::test]
    async fn extra_invocations_appended_after_primary() {
        let mut d: Dispatcher<String> = Dispatcher::new();
        d.register("Sub/set", Arc::new(ExtraInvocationHandler));
        let req = single_call("Sub/set", json!({}), "c0");
        let resp = d.dispatch(req, "alice".into(), "s0".into()).await;

        assert_eq!(
            resp.method_responses.len(),
            2,
            "primary + 1 extra = 2 total invocations"
        );
        // First: the primary Sub/set response.
        assert_eq!(resp.method_responses[0].0, "Sub/set");
        assert_eq!(resp.method_responses[0].2, "c0");
        assert_eq!(resp.method_responses[0].1["type"], "primary");
        // Second: the appended extra invocation.
        assert_eq!(resp.method_responses[1].0, "Extra/call");
        assert_eq!(resp.method_responses[1].2, "x0");
        assert_eq!(resp.method_responses[1].1["type"], "extra");
    }
}

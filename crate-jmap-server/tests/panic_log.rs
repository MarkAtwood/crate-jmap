//! Log-capture redaction canary for the dispatcher's handler-panic
//! event (bd:JMAP-jfia.29; workspace AGENTS.md "Security testing",
//! pattern 2).
//!
//! Oracle: the bead decision (Mark, 2026-10-22): on a handler panic,
//! `Dispatcher::dispatch` emits one `tracing::error!` with the method
//! name and call id only, never the panic payload or the arguments.
//! The expected method, call id and canaries are written here by hand.
//!
//! A field-recording `Layer` is used instead of formatted text so the
//! test can check that the event carries exactly the fields `message`,
//! `method` and `call_id`, not just that some text appears.

use std::fmt;
use std::sync::{Arc, Mutex};

use jmap_server::{CallContext, Dispatcher, HandlerFuture, Invocation, JmapHandler, JmapRequest};
use serde_json::json;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

const PAYLOAD_CANARY: &str = "PANIC-PAYLOAD-CANARY-7f3a";
const ARG_CANARY: &str = "ARGUMENT-CANARY-91c2";

#[derive(Debug)]
struct Captured {
    level: Level,
    fields: Vec<(String, String)>,
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Captured>>>);

struct FieldVisitor<'a>(&'a mut Vec<(String, String)>);

impl Visit for FieldVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

impl<S: Subscriber> Layer<S> for Recorder {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Vec::new();
        event.record(&mut FieldVisitor(&mut fields));
        self.0.lock().expect("test: mutex poisoned").push(Captured {
            level: *event.metadata().level(),
            fields,
        });
    }
}

struct PanicHandler;

impl JmapHandler<String> for PanicHandler {
    fn call(&self, _cx: CallContext<String>) -> HandlerFuture {
        Box::pin(async move { panic!("{PAYLOAD_CANARY}") })
    }
}

/// A caught handler panic emits exactly one ERROR event whose fields are
/// `message`, `method` and `call_id`, and no field carries the panic
/// payload or an argument value.
#[tokio::test]
async fn handler_panic_emits_one_redacted_error_event() {
    let recorder = Recorder::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(recorder.clone()));

    let mut d: Dispatcher<String> = Dispatcher::new();
    d.register("Panic/now", Arc::new(PanicHandler));
    let req = JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".into()],
        vec![Invocation::new(
            "Panic/now",
            json!({ "secret": ARG_CANARY }),
            "c-panic-1",
        )],
        None,
    );
    let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
    assert_eq!(resp.method_responses[0].arguments["type"], "serverFail");

    let events = recorder.0.lock().expect("test: mutex poisoned");
    assert_eq!(events.len(), 1, "expected one event, got {events:?}");
    let event = &events[0];
    assert_eq!(event.level, Level::ERROR);

    let mut names: Vec<&str> = event.fields.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["call_id", "message", "method"]);

    let value = |name: &str| {
        event
            .fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(value("method"), Some("Panic/now"));
    assert_eq!(value("call_id"), Some("c-panic-1"));

    for (name, v) in &event.fields {
        assert!(!v.contains(PAYLOAD_CANARY), "{name} leaks panic payload");
        assert!(!v.contains(ARG_CANARY), "{name} leaks an argument");
    }
}

/// A handler's own error result is not a panic and emits no event.
#[tokio::test]
async fn handler_error_emits_no_event() {
    struct FailHandler;
    impl JmapHandler<String> for FailHandler {
        fn call(&self, _cx: CallContext<String>) -> HandlerFuture {
            Box::pin(async move { Err(jmap_server::JmapError::server_fail("legit")) })
        }
    }

    let recorder = Recorder::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(recorder.clone()));

    let mut d: Dispatcher<String> = Dispatcher::new();
    d.register("Fail/now", Arc::new(FailHandler));
    let req = JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".into()],
        vec![Invocation::new("Fail/now", json!({}), "c0")],
        None,
    );
    let resp = d.dispatch(req, "alice".into(), "s0".into()).await;
    assert_eq!(resp.method_responses[0].arguments["type"], "serverFail");
    assert!(recorder.0.lock().expect("test: mutex poisoned").is_empty());
}

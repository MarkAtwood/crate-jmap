//! Demonstrate the jmap-server [`Dispatcher`] hot path without a real backend.
//!
//! Registers one stub handler under `"Core/echo"`, builds a synthetic
//! [`JmapRequest`] with a single method call, dispatches it, and prints
//! the resulting [`JmapResponse`]. This closes the loop on RFC 8620 §3.3
//! (request envelope) → §3.4 (response envelope) with no HTTP transport,
//! no auth layer, and no storage backend in the way.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example dispatcher_minimal -p jmap-server
//! ```

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, Invocation, JmapHandler, JmapRequest, JmapResponse};
use serde_json::{json, Value};

/// Echoes the method-call arguments back unchanged. The simplest possible
/// `JmapHandler` — no state, no errors, no extra invocations.
struct EchoHandler;

impl<C: Clone + Send + 'static> JmapHandler<C> for EchoHandler {
    fn call(&self, _method: String, _call_id: String, args: Value, _caller: C) -> HandlerFuture {
        Box::pin(async move { Ok((args, Vec::<Invocation>::new())) })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the dispatcher with one registered handler. `CallerCtx` here
    //    is `String` — a real server would pass an auth identity instead.
    let mut dispatcher: Dispatcher<String> = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(EchoHandler));

    // 2. Synthesize a request: one method call carrying a small JSON payload.
    let request = JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".to_owned()],
        vec![(
            "Core/echo".to_owned(),
            json!({ "hello": "world", "n": 42 }),
            "c1".to_owned(),
        )],
        None,
    );

    // 3. Dispatch. Second arg is `CallerCtx` (cloned per method call);
    //    third is the session_state echoed into the response per §3.4.
    let response: JmapResponse = dispatcher
        .dispatch(request, "alice".to_owned(), "s-001".into())
        .await;

    // 4. Pretty-print the wire-format response, then the typed access paths
    //    a real server would consume.
    println!("--- JSON response ---");
    println!("{}", serde_json::to_string_pretty(&response)?);

    println!("\n--- typed access ---");
    println!(
        "method_responses ({} entries):",
        response.method_responses.len()
    );
    for (method, _args, call_id) in &response.method_responses {
        println!("  - {method}  (call_id={call_id})");
    }
    println!("session_state: {}", response.session_state);

    Ok(())
}

# jmap-server

Backend-agnostic JMAP server framework for Rust. Implements the [RFC 8620] wire
protocol: request parsing, ResultReference resolution, and the `Dispatcher`
machinery. No opinion on authentication, method sets, capability URIs, or storage.

## Usage

```rust
use std::sync::Arc;
use jmap_server::{Dispatcher, JmapHandler, HandlerFuture, JmapError};
use serde_json::Value;

// 1. Implement JmapHandler for each method.
struct FooGetHandler;

impl JmapHandler<String> for FooGetHandler {        // String is the CallerCtx type
    fn call(
        &self,
        _method: String,
        _call_id: String,
        _args: Value,
        _caller: String,                             // auth identity passed through
    ) -> HandlerFuture {
        Box::pin(async move { Ok(serde_json::json!({"list": [], "state": "s0"})) })
    }
}

// 2. Register handlers and dispatch requests.
let mut dispatcher: Dispatcher<String> = Dispatcher::new();
dispatcher.register("Foo/get", Arc::new(FooGetHandler));

// In your handler:
// let request = jmap_server::parse_request(body_json, 16)?;
// let response = dispatcher.dispatch(request, caller_identity, session_state).await;
```

## CallerCtx

`CallerCtx` is whatever your authentication layer produces — an identity struct,
a session token, `()`, etc. The dispatcher clones it for each method call and
passes it through to the handler unchanged. It must be `Clone + Send + 'static`;
use `Arc<T>` to share non-static data (e.g. a database connection pool).

## Request parsing

```rust
use jmap_server::{parse_request, request_error};

// Parse and validate a JMAP request body.
let req = parse_request(body_json, /* max_calls */ 16)
    .map_err(request_error)?;   // returns RequestError on failure
```

`parse_request` validates:
- The body is a valid JMAP Request object.
- `using` is non-empty.
- The number of method calls does not exceed `max_calls`.

Capability URI checking is NOT performed here — that is the caller's responsibility.

## Error responses

```rust
use jmap_server::{request_error, JmapError};

// Request-level errors (HTTP 400/403/500 with RFC 7807 body):
let resp = request_error(JmapError::not_request());          // → HTTP 400
let resp = request_error(JmapError::limit("maxCallsInRequest"));  // → HTTP 400

// Method-level errors (HTTP 200, inside methodResponses):
// Return Err(JmapError::...) from your JmapHandler::call implementation.
```

## Spec references

- **[RFC 8620]** — JMAP base protocol (request format, ResultReference, error types)
- **[RFC 6901]** — JSON Pointer (used by ResultReference path resolution)

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6901]: https://www.rfc-editor.org/rfc/rfc6901

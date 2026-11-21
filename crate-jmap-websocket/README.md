# jmap-websocket

JMAP over WebSocket (RFC 8887, `urn:ietf:params:jmap:websocket`) for the
`jmap-*` crate family: the capability object, the wire messages, and a
server session driver that runs a `jmap_server::Dispatcher` behind any
WebSocket library.

## What it is

A support crate. Always available, with no async runtime:

| Item | RFC 8887 |
|---|---|
| `CAPABILITY`, `WebSocketCapability` (`url`, `supportsPush`) | §3 |
| `SUBPROTOCOL` (`"jmap"`), `offers_jmap` | §4.2, §6.1 |
| `WsRequest` (`@type: "Request"`, optional `id`) | §4.3.2 |
| `WsResponse` (`@type: "Response"`, `requestId`) | §4.3.3 |
| `WsRequestError` (problem details, `@type: "RequestError"`, `requestId`) | §4.3.4 |
| `WsStateChange` (StateChange plus `pushState`) | §4.3.5.1 |
| `WebSocketPushEnable` (`dataTypes`, `pushState`), `WebSocketPushDisable` | §4.3.5.2, §4.3.5.3 |
| `ClientMessage`, `ServerMessage` (pick the type by `@type`) | §4.3 |

Feature `server` (module `server`):

- `Connection`: one connection's driver. `run(incoming, outgoing)` takes
  any `Stream` of `Inbound` (text, binary, close) and `Sink` of
  `Outbound` (text, close).
- `parse_client_message`: one text message to a `ClientMessage` or the
  `WsRequestError` to answer with (`notJSON`, `notRequest`, `limit`).
- `RequestHandler`: what answers a Request; `DispatcherHandler` wraps a
  `Dispatcher` and a caller, with an optional `unknownCapability` check.
- `PushSource`: where push notifications come from; `PushHub` is an
  in-memory fan-out with `pushState` tokens and replay; `NoPush` sends
  nothing. `filter_state_change` applies `dataTypes`.
- `ExtensionHandler`: takes the client messages of extension `@type`s
  (`ChatStreamEnable`, `VTCStreamEnable`, `SceneStreamEnable`, ...) and
  sends the extension's events through an `ExtensionSender`; register
  it with `Connection::with_extension`.
- `ConnectionConfig`: `maxCallsInRequest`, `maxSizeRequest`,
  `maxConcurrentRequests`, binary-message policy, extension queue size.

Feature `tungstenite`: `tungstenite::serve` runs a `Connection` over a
tokio-tungstenite `WebSocketStream`.

## What it's for

Servers built from the kit that want to offer RFC 8887. The `*-server`
crates stay transport-free (workspace AGENTS.md); a deployment adds this
crate next to its HTTP stack. `jmap-testjig` serves its `/ws` endpoint
with it over axum. Clients use `jmap-base-client`'s `connect_ws`, which
has its own frame types today.

## How to use

```toml
[dependencies]
jmap-websocket = { version = "0.6", features = ["tungstenite"] }
```

```rust,no_run
use std::sync::Arc;
use jmap_websocket::server::{Connection, DispatcherHandler, PushHub};
use jmap_websocket::jmap_server::Dispatcher;
use jmap_websocket::jmap_types::State;

# async fn run(tcp: tokio::net::TcpStream, dispatcher: Arc<Dispatcher<()>>, hub: PushHub)
#     -> Result<(), Box<dyn std::error::Error>> {
// Authenticate and check `offers_jmap` in the handshake callback, and
// answer with `Sec-WebSocket-Protocol: jmap` (RFC 8887 §4.1, §4.2).
let ws = jmap_websocket::tokio_tungstenite::accept_async(tcp).await?;
let handler = Arc::new(DispatcherHandler::new(dispatcher, (), || State::from("s0")));
let connection = Connection::new(handler).with_push(hub);
jmap_websocket::tungstenite::serve(connection, ws).await?;
# Ok(()) }
```

After each write, publish the changed states to the principal's
`PushHub` (`hub.publish(state_change)`); every connection of that
principal with push enabled gets them.

### Extension message types

The JMAP Chat, VTC and Scene WebSocket drafts update RFC 8887: a
capability in the Session "MAY define additional WebSocket message
types, each identified by its `@type` value", and the server sends them
only after the client enabled them with that capability's control
message (`ChatStreamEnable`, `VTCStreamEnable`, `SceneStreamEnable`).
Implement `ExtensionHandler` for those types and register one value per
connection, next to advertising the capability:

```rust,no_run
use jmap_websocket::jmap_types::JmapError;
use jmap_websocket::server::{ExtensionHandler, ExtensionMessage, ExtensionSender};

struct ToyStream {
    caller: String,
    events: Option<ExtensionSender>,
}

impl ExtensionHandler for ToyStream {
    fn message_types(&self) -> &[&str] {
        &["ToyStreamEnable", "ToyStreamDisable"]
    }

    fn start(&mut self, events: ExtensionSender) {
        // Keep it, or hand a clone to a task that forwards events.
        self.events = Some(events);
    }

    async fn handle(&mut self, message: ExtensionMessage) -> Result<(), JmapError> {
        // Update the subscription; Err becomes a RequestError whose
        // requestId is message.id.
        Ok(())
    }

    async fn close(&mut self) {
        // The connection ended: drop the subscription.
    }
}

# fn wire(connection: jmap_websocket::server::Connection<jmap_websocket::server::DispatcherHandler<()>>) {
let connection = connection.with_extension(ToyStream { caller: "u1".into(), events: None });
# let _ = connection; }
```

- `handle` runs one message at a time, in order; the driver reads the
  next client message only after it returns, while Responses,
  StateChanges and queued events keep going out.
- Events go through a bounded queue
  (`ConnectionConfig::extension_queue_capacity`, default 64):
  `try_send` drops on `Full`, `send` waits. An event must be a JSON
  object with an `@type` that is not an RFC 8887 one.
- `close` runs once when `run` ends, however it ends, after the queue
  closed and a running `handle` finished.
- A `@type` no handler registered is still answered with `notRequest`.

### Closing on credential expiry

RFC 8887 §4.2 lets the server close the connection when the handshake's
credentials expire, with a Close frame of status 1008 (Policy
Violation); the JMAP Chat, VTC and Scene WebSocket drafts make 1008 a
SHOULD. Take a `CloseHandle` before running the connection and close
through it from the task that tracks the expiry:

```rust,no_run
use jmap_websocket::server::{CloseCode, Connection, DispatcherHandler};

# async fn serve(
#     connection: Connection<DispatcherHandler<()>>,
#     ws: jmap_websocket::tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
#     expires_in: std::time::Duration,
# ) -> Result<(), jmap_websocket::server::ConnectionError> {
let close = connection.close_handle();
tokio::spawn(async move {
    tokio::time::sleep(expires_in).await;
    close.close(CloseCode::POLICY_VIOLATION, "credentials expired");
});
jmap_websocket::tungstenite::serve(connection, ws).await
# }
```

`run` then sends the Close, drops the push subscription, runs every
`ExtensionHandler::close`, and returns `Ok(())`.

## How it works

- Every message type serializes with its `@type` first and checks it on
  deserialize (absent accepted, wrong value rejected); the enums need
  it. Unknown members are kept (`extra`, or the inner RFC 8620 object's
  `extra`).
- `run` loops over three sources with `tokio::select!`: incoming
  messages, finished Requests (a `FuturesUnordered`, so Requests run
  concurrently and answer in finishing order, as §4.3.2 allows), and the
  push subscription. Writes go out one at a time; a slow client
  back-pressures the loop.
- A text message is read in order: size, JSON, object with `@type`,
  then the message's own shape. Every RequestError carries the
  message's string `id` as `requestId`, or `null`.
- `RequestHandler::prepare_problem` runs on every RequestError, so a
  deployment can localize `title`.
- `PushHub` numbers each published change. A token it issued replays the
  changes since then (merged into one notification), or the latest state
  of everything when they were already dropped; a foreign token replays
  nothing.

## Gotchas

- The handshake is not done here: authentication (§4.1), TLS (§5.1) and
  the `jmap` subprotocol echo (§4.2) are the deployment's.
- A binary message closes the connection with 1003 by default
  (`BinaryFrames::Ignore` to drop it instead); RFC 8887 does not answer
  it with a RequestError.
- `WsRequestError` always writes `requestId` (`null` when unknown), as
  the §4.4 `notJSON` example does; `WsResponse` omits it when the request
  had no `id`.
- A request over `maxConcurrentRequests` is answered with a `limit`
  RequestError, not queued. To count HTTP requests too, share a
  semaphore with `Connection::with_shared_limit`.
- `dataTypes: []` matches no type (the RFC's literal reading); use
  `null` for all.
- A `WebSocketPushEnable` with members of the wrong type is answered
  with `notRequest` instead of being ignored.
- `PushHub` does no access filtering: keep one per principal. Merging
  drops `StateChange.extra`.
- Two §4.4 examples are not valid as printed: the Response omits the
  `sessionState` RFC 8620 §3.4 requires, and the second StateChange
  misses a comma. The tests say so.
- `Dispatcher::dispatch` needs a Tokio runtime, so `run` does too.
- Extension `@type`s are answered with `notRequest` unless an
  `ExtensionHandler` is registered for them. The driver does not check
  `using` for them: the drafts tie them to the Session capability, so
  register a handler only where the capability is advertised.
- A handler error's `extra` members become problem-details members,
  except names the driver writes itself (`type`, `status`, `title`,
  `detail`, `instance`, `limit`, `requestId`, `@type`).
- `ExtensionHandler::close` does not run if the `run` future is dropped
  (a cancelled task); put cleanup that must happen then in `Drop`.
- Ending `incoming` ends the connection without a Close frame. To close
  with a status code (1008 for expired credentials), use
  `Connection::close_handle`. The first `close` wins; later ones, and
  any after `run` returned, do nothing and return `false`. Nothing
  queued is sent ahead of the Close: Responses still running, queued
  extension events and pending StateChanges are dropped. A reason over
  123 bytes (the RFC 6455 control-frame limit) is cut on a character
  boundary. `CloseCode::new` refuses codes an endpoint may not send.

## References

- [RFC 8887] — A JSON Meta Application Protocol (JMAP) Subprotocol for
  WebSocket
- [RFC 8620] §3.3, §3.4, §3.6.1, §7.1 — JMAP Core requests, responses,
  request-level errors, StateChange
- [RFC 6455] — The WebSocket Protocol
- [RFC 9457] — Problem Details for HTTP APIs
- draft-atwood-jmap-chat-wss-00, draft-atwood-jmap-vtc-wss-00,
  draft-atwood-jmap-scene-wss-00 — the extension message types
  (jmap-chat-spec)

[RFC 8887]: https://www.rfc-editor.org/rfc/rfc8887
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6455]: https://www.rfc-editor.org/rfc/rfc6455
[RFC 9457]: https://www.rfc-editor.org/rfc/rfc9457

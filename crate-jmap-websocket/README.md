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
- `ConnectionConfig`: `maxCallsInRequest`, `maxSizeRequest`,
  `maxConcurrentRequests`, binary-message policy.

Feature `tungstenite`: `tungstenite::serve` runs a `Connection` over a
tokio-tungstenite `WebSocketStream`.

## What it's for

Servers built from the kit that want to offer RFC 8887. The `*-server`
crates stay transport-free (workspace AGENTS.md); a deployment adds this
crate next to its HTTP stack. Clients use `jmap-base-client`'s `connect_ws`, which
has its own frame types today.

## How to use

```toml
[dependencies]
jmap-websocket = { version = "0.5", features = ["tungstenite"] }
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
- Requests from client-side extensions (e.g. the chat-wss `@type`s) are
  answered with `notRequest`; there is no hook for them yet.

## References

- [RFC 8887] — A JSON Meta Application Protocol (JMAP) Subprotocol for
  WebSocket
- [RFC 8620] §3.3, §3.4, §3.6.1, §7.1 — JMAP Core requests, responses,
  request-level errors, StateChange
- [RFC 6455] — The WebSocket Protocol
- [RFC 9457] — Problem Details for HTTP APIs

[RFC 8887]: https://www.rfc-editor.org/rfc/rfc8887
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6455]: https://www.rfc-editor.org/rfc/rfc6455
[RFC 9457]: https://www.rfc-editor.org/rfc/rfc9457

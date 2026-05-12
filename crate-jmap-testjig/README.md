# jmap-testjig

> **NOT FOR PRODUCTION.** This crate is internal scaffolding for the
> workspace's own integration testing and demonstration. It is
> `publish = false` and will never appear on crates.io. Production
> JMAP servers MUST be built on the workspace's library kit
> (`jmap-server` foundation, the 8 `jmap-*-server` extension crates,
> a consumer-supplied transport, auth integration, persistence, and
> multi-tenancy story). See the workspace `AGENTS.md` "What this
> workspace builds" section for the kit-vs-jig posture.

`jmap-testjig` wires the workspace's library kit (`jmap-server`
dispatcher + 8 extension method handlers + 8 reference
`MemoryBackend` implementations) into a running HTTP/SSE/WebSocket
process that the workspace's own integration tests and contributor
smoke-testing can hit.

## What is wired up

| Endpoint | RFC | Purpose |
|----------|-----|---------|
| `GET /.well-known/jmap` | 8620 §2 | Session resource discovery |
| `POST /jmap` | 8620 §3 | API endpoint (method dispatch) |
| `GET /events` | 8620 §7.3 | Server-Sent Events push |
| `GET /ws` | 8887 | JMAP-over-WebSocket subprotocol |

All routes are gated behind a single hardcoded bearer token (default
`test-token`). Browsers cannot set arbitrary headers on EventSource
or WebSocket handshakes, so the `?token=<token>` query-string
fallback is also accepted on those transports.

## Why "NOT FOR PRODUCTION"

The testjig is loudly, deliberately, demonstration-grade:

- **In-memory only.** Every backend stores state in `HashMap`s
  behind a `std::sync::Mutex`. Restarting the process loses every
  email, every Space, every CalendarEvent, every blob. No
  persistence, no recovery, no backups.
- **Single-user, single-account.** One hardcoded account-id
  (`testjig-account`) under one hardcoded principal
  (`testuser@testjig.local`). No multi-tenancy, no per-request
  identity, no real authorization.
- **No real auth.** A hardcoded bearer token gates every endpoint.
  No OAuth, no JWT, no mTLS, no upstream IdP, no audit log.
- **No TLS termination.** Binds plaintext HTTP on `127.0.0.1`.
  Operators terminate TLS in front of the jig if remote access is
  needed (there is no good reason for remote access here).
- **No quotas, no rate-limiting, no abuse mitigation.**
- **No CORS, no CSRF mitigation.** The testjig assumes the only
  caller is curl or another test on the same machine.

If you find yourself wishing the testjig had any of the above, the
right move is to build a real JMAP server on top of the library kit
(see the workspace `AGENTS.md` posture) — **not** to extend the
testjig.

## Quick start

```sh
# In one terminal:
cargo run -p jmap-testjig

# In another:
curl -s -H "Authorization: Bearer test-token" \
    http://127.0.0.1:8080/.well-known/jmap | jq .

curl -s -H "Authorization: Bearer test-token" \
    -H "Content-Type: application/json" \
    -X POST http://127.0.0.1:8080/jmap \
    -d '{
      "using": ["urn:ietf:params:jmap:core"],
      "methodCalls": [["Core/echo", {"hello": "world"}, "c1"]]
    }' | jq .
```

### CLI flags

The binary accepts two optional flags:

| Flag | Default | Notes |
|------|---------|-------|
| `--port <port>` | `8080` | TCP port on `127.0.0.1`. Use `0` for an OS-assigned port. |
| `--token <token>` | `test-token` | Bearer token clients must supply. |

```sh
cargo run -p jmap-testjig -- --port 9090 --token secret
```

There are no config files, no environment variables, no other
flags. Anything more complicated than this belongs in a downstream
consumer (kith, stoa, or your own JMAP server).

### Subscribe to the SSE event-source

```sh
curl -N -H "Authorization: Bearer test-token" \
    "http://127.0.0.1:8080/events?types=*&closeafter=no&ping=30"
```

The connection stays open; the server emits a `state` event each
time any tracked JMAP object type's `/get` state advances, and a
`ping` event every 30 seconds. Set `closeafter=state` to receive
only the first state event and have the server close the response.

### Use over WebSocket

```sh
# Any RFC 8887-conformant JMAP-over-WebSocket client works.
# The subprotocol name is "jmap"; auth via Authorization header or
# ?token=... query parameter.
wscat -c "ws://127.0.0.1:8080/ws?token=test-token" --subprotocol jmap
```

## Use from integration tests

The library API exposes `spawn_in_process` for per-client-crate
integration tests:

```rust,no_run
use jmap_testjig::{spawn_in_process, TestjigConfig};

# async fn example() -> std::io::Result<()> {
let jig = spawn_in_process(TestjigConfig::default()).await?;
let session_url = format!("http://{}/.well-known/jmap", jig.addr);
let response = reqwest::Client::new()
    .get(&session_url)
    .bearer_auth(&jig.token)
    .send()
    .await
    .expect("session fetch");
assert!(response.status().is_success());
// Drop drops the JoinHandle → aborts the task → closes the socket.
# Ok(())
# }
```

`TestjigConfig::default()` requests an OS-assigned ephemeral port
and the default `test-token` bearer token. Test runs in parallel
because each `spawn_in_process` call binds its own port.

The returned `TestjigHandle` aborts the server task on `Drop`. Call
`shutdown().await` explicitly when the test needs to free the port
synchronously (e.g. before spawning a second jig on the same port).

## Architectural posture

The testjig is **the workspace's only crate** that wires axum +
tokio-tungstenite into a running HTTP process. Every other
`jmap-*-server` crate stays transport-less — they define handler
libraries plus backend traits, with reference `MemoryBackend`
implementations gated behind a `memory` feature flag. Consumers
combine them into their own server.

This separation is deliberate. **Do not propose growing transport,
persistence, auth, or multi-tenancy into the `jmap-*-server` crates
"for symmetry" or "for completeness"**; the transport-less posture
is intentional and the consumer-brings-everything posture is
intentional. See the workspace `AGENTS.md` "What this workspace
builds" section.

## Bead epic

This crate's design is tracked at workspace bead `JMAP-cf7p`
(closed when the 8 child slices land). Open follow-up beads for
future polish:

- `JMAP-cf7p.9`: replace the SSE/WS polling loop with a
  signal-driven push when one of the MemoryBackends grows the
  plumbing.
- `JMAP-cf7p.10`: honor SSE `Last-Event-ID` on reconnect per
  RFC 8620 §7.3.
- `JMAP-cf7p.12`: honor RFC 8887 §4.3.5.2 `pushState` on
  `WebSocketPushEnable`.

## License

MIT OR Apache-2.0 — workspace-wide inheritance, see the workspace
`Cargo.toml`. The license metadata is sufficient for `cargo deny`
and crates.io; no `LICENSE-*` files are committed to this repo per
the workspace convention.

# jmap-base-client

RFC 8620 base JMAP HTTP client. Handles authentication, session fetch, blob
upload/download, SSE event streams, and WebSocket connections.

Extension-specific clients (`jmap-mail-client`, `jmap-chat-client`) depend on
this crate and add their method implementations as extension traits on
`JmapClient`.

---

## What it is

- **Session fetch** — GET `/.well-known/jmap`, parse and validate the Session object
- **API calls** — POST typed `JmapRequest` to `session.api_url`, receive `JmapResponse`
- **Blob upload/download** — RFC 8620 §6.1/§6.2 with optional SHA-256 integrity check
- **SSE event stream** — async `Stream` of `SseFrame` values from the server push channel
- **WebSocket transport** — RFC 8887 request/response and `StateChange` push frames
- **Auth** — `BearerAuth`, `BasicAuth`, `NoneAuth`; pluggable via `AuthProvider` trait
- **Transport** — `DefaultTransport` (webpki roots) and `CustomCaTransport` (private CA)

---

## What it's for

The foundation client crate for the `jmap-*` family. Every extension client
in the workspace (mail, chat, contacts, calendars, tasks, filenode, sharing,
metadata) builds on this crate by adding its own `Jmap*Ext` extension trait
on `JmapClient` plus a per-extension `SessionClient`. The private transport
dependencies (`reqwest` for HTTP, `tokio-tungstenite` for WebSocket) are
wrapped in opaque `HttpError`, `WebSocketError`, and
`InvalidHeaderValueError` types so they stay SemVer-isolated — those crates
can be bumped or swapped without breaking downstream extensions.

---

## Cargo.toml

```toml
[dependencies]
jmap-base-client = { path = "../crate-jmap-base-client" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

---

## How to use

### Basic setup: construct client, fetch session, make a call

```rust
use jmap_base_client::{
    BearerAuth, ClientConfig, JmapClient, JmapRequestBuilder, extract_response,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auth = BearerAuth::new("my-token")?;
    let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

    // Fetch the session object (GET /.well-known/jmap)
    let session = client.fetch_session().await?;

    // Find the primary account for JMAP Mail
    let account_id = session
        .primary_account_id("urn:ietf:params:jmap:mail")
        .expect("server must have a primary mail account");

    // Build a multi-method request
    let mut builder = JmapRequestBuilder::new(&[
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:mail",
    ]);
    builder.add_call(
        "Mailbox/get",
        json!({ "accountId": account_id, "ids": null }),
        "r1",
    )?;
    let request = builder.build()?;

    // POST to session.api_url
    let response = client.call(&session.api_url, &request).await?;

    // Extract the typed result for call ID "r1"
    let mailboxes: serde_json::Value = extract_response(&response, "r1")?;
    println!("{mailboxes:#}");

    Ok(())
}
```

### Auth variants

```rust
use jmap_base_client::{BearerAuth, BasicAuth, NoneAuth, JmapClient, ClientConfig};

// Bearer token (most common for modern JMAP servers)
let auth = BearerAuth::new("my-oauth-token")?;
let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

// HTTP Basic (username + password)
let auth = BasicAuth::new("alice@example.com", "s3cr3t")?;
let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

// No auth header (public server or pre-authenticated proxy)
let client = JmapClient::new_plain(NoneAuth, "https://jmap.example.com", ClientConfig::default())?;
```

`BearerAuth::new` is fallible: it rejects empty tokens, tokens containing
whitespace (RFC 6750 §2.1), and tokens with non-visible-ASCII bytes.
`BasicAuth::new` is fallible: it rejects usernames containing `:` (RFC 7617 §2).
Both pre-validate at construction time so errors surface early rather than on
the first request.

### Custom CA: private or self-hosted servers

```rust
use jmap_base_client::{BearerAuth, CustomCaTransport, JmapClient, ClientConfig};

let ca_der = std::fs::read("/etc/jmap/private-ca.der")?;
let transport = CustomCaTransport::new(ca_der);
let auth = BearerAuth::new("my-token")?;
let client = JmapClient::new(transport, auth, "https://100.64.1.1:8008", ClientConfig::default())?;
```

`CustomCaTransport` takes a DER-encoded CA certificate. It can be combined with
any `AuthProvider`.

### Blob upload

```rust
use jmap_base_client::expand_url_template;

let data = bytes::Bytes::from(std::fs::read("photo.jpg")?);
let upload_resp = client
    .upload_blob(&session.upload_url, account_id, data, "image/jpeg")
    .await?;

println!("blob ID: {}", upload_resp.blob_id);
println!("size:    {} bytes", upload_resp.size);
```

The server-returned SHA-256 digest (if present, from the JMAP-CID extension) is
verified against the locally-computed digest. A mismatch returns
`ClientError::BlobIntegrityMismatch`.

### Blob download

```rust
use jmap_base_client::DownloadBlobParams;

let bytes = client
    .download_blob(DownloadBlobParams {
        download_url_template: &session.download_url,
        account_id,
        blob_id: "Gbc4c377-...",
        name: "photo.jpg",
        accept_type: Some("image/jpeg"),
        expected_sha256: None, // pass Some("hex...") to verify integrity
    })
    .await?;
```

Use `DownloadBlobParams` as a struct literal to avoid confusion between the
string-typed fields.

### SSE event stream

```rust
use futures::StreamExt;
use jmap_base_client::{SseEvent, expand_url_template};

// expand_url_template expands RFC 6570 Level-1 templates from the Session object
let url = expand_url_template(
    &session.event_source_url,
    &[
        ("types", "*"),
        ("closeafter", "no"),
        ("ping", "0"),
    ],
)?;

let mut stream = client.subscribe_events(&url, None).await?;

while let Some(frame) = stream.next().await {
    let frame = frame?;
    match frame.event {
        SseEvent::StateChange(sc) => {
            for (account_id, changes) in &sc.changed {
                for (type_name, new_state) in changes {
                    println!("{account_id}: {type_name} changed to {new_state}");
                }
            }
        }
        SseEvent::Unknown { event_type } => {
            // keepalive ping or unrecognized event type — safe to ignore
            let _ = event_type;
        }
    }
    // Track frame.id for Last-Event-ID on reconnect
}
```

`subscribe_events` expands to an `async Stream` of `SseFrame` values. No
timeout is applied to the stream itself; wrap in `tokio::time::timeout` if you
need a deadline. The caller is responsible for reconnecting with exponential
backoff and passing the last `frame.id` as the `last_event_id` argument.

### WebSocket (RFC 8887)

```rust
use jmap_base_client::{connect_ws, WsFrame, JmapRequestBuilder};

// Get the WebSocket URL from the session capability
let ws_cap = session
    .websocket_capability()?
    .expect("server must support JMAP WebSocket");

let auth_header = client_auth.auth_header(); // from your AuthProvider
let mut ws = connect_ws(&ws_cap.url, auth_header).await?;

// Send a JMAP request over the WebSocket
let mut builder = JmapRequestBuilder::new(&["urn:ietf:params:jmap:core"]);
builder.add_call("Core/echo", serde_json::json!({}), "c1")?;
let req = builder.build()?;
ws.send_request(&req, Some("c1")).await?;

// Receive frames in a loop
while let Some(frame) = ws.next_frame().await {
    match frame? {
        WsFrame::StateChange(sc) => { /* handle push */ }
        WsFrame::Response(resp) => { /* handle method response */ }
        WsFrame::Unknown { .. } => { /* forward-compat: ignore */ }
    }
}
```

---

## Examples

A runnable end-to-end demo lives in [`examples/session_fetch.rs`](examples/session_fetch.rs):

```sh
cargo run --example session_fetch -p jmap-base-client
```

It starts an in-process `wiremock` server with a hand-written RFC 8620 §2
Session fixture and prints the parsed `username`, URL fields, capabilities,
and accounts. Set `JMAP_TEST_URL` (and optionally `JMAP_TEST_TOKEN`) to point
at a real JMAP endpoint instead of the offline fixture.

---

## API reference

### `JmapClient`

```rust
// For public internet servers (webpki trust roots)
pub fn new_plain(
    auth: impl AuthProvider + 'static,
    base_url: &str,
    config: ClientConfig,
) -> Result<Self, ClientError>;

// For custom transports (private CA, client certs, etc.)
pub fn new(
    transport: impl TransportConfig,
    auth: impl AuthProvider + 'static,
    base_url: &str,
    config: ClientConfig,
) -> Result<Self, ClientError>;

pub async fn fetch_session(&self) -> Result<Session, ClientError>;

pub async fn call(
    &self,
    api_url: &str,
    req: &JmapRequest,
) -> Result<JmapResponse, ClientError>;

pub async fn upload_blob(
    &self,
    upload_url_template: &str,
    account_id: &str,
    data: bytes::Bytes,
    content_type: &str,
) -> Result<BlobUploadResponse, ClientError>;

pub async fn download_blob(
    &self,
    params: DownloadBlobParams<'_>,
) -> Result<bytes::Bytes, ClientError>;

pub async fn subscribe_events(
    &self,
    event_source_url: &str,      // expand template first with expand_url_template()
    last_event_id: Option<&str>,
) -> Result<BoxStream<'static, Result<SseFrame, ClientError>>, ClientError>;
```

`base_url` must be the server origin (scheme + host + optional port) with no
path component — e.g. `"https://jmap.example.com"` or
`"https://100.64.1.1:8008"`. Trailing slashes are accepted.

### `extract_response<T>`

```rust
pub fn extract_response<T: DeserializeOwned>(
    resp: &JmapResponse,
    call_id: &str,
) -> Result<T, ClientError>;
```

Finds the invocation matching `call_id` in `resp.method_responses` and
deserializes its arguments into `T`. Returns `ClientError::MethodNotFound` if
the call ID is absent. Returns `ClientError::MethodError` if the server
returned a JMAP `"error"` response for that call (RFC 8620 §3.6.1).

Extension crates use this function to extract typed results without depending
on internal crate details.

### `JmapRequestBuilder`

```rust
let mut builder = JmapRequestBuilder::new(&[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
]);
builder.add_call("Email/get", json!({ "accountId": id, "ids": ids }), "c1")?;
builder.add_call("Mailbox/get", json!({ "accountId": id, "ids": null }), "c2")?;
let request = builder.build()?;
```

- `new(using)` — pass every capability URI required by the methods in this request
- `add_call(method, args, call_id)` — returns `Err` on duplicate `call_id`
- `build()` — returns `Err` if no calls were added

### `ClientConfig`

```rust
#[non_exhaustive]
pub struct ClientConfig {
    pub request_timeout: Duration,   // default: 30 s
    pub max_session_body: u64,        // default: 1 MiB
    pub max_call_body: u64,           // default: 8 MiB
    pub max_download_body: u64,       // default: 64 MiB
    pub max_upload_response_body: u64, // default: 1 MiB (response only)
    pub max_sse_frame: usize,         // default: 1 MiB
}
```

`ClientConfig` is `#[non_exhaustive]`. Construct it with:

```rust
ClientConfig::default()

// or override individual fields:
ClientConfig {
    request_timeout: Duration::from_secs(60),
    max_download_body: 256 * 1024 * 1024,
    ..ClientConfig::default()
}
```

All fields must be `> 0`; `JmapClient::new` validates them and returns
`ClientError::InvalidArgument` on violation.

`request_timeout` applies to `fetch_session`, `call`, `upload_blob`, and
`download_blob`. It does not apply to SSE or WebSocket streams, which are
indefinitely long by nature.

### `Session`

Returned by `fetch_session`. Key fields:

```rust
pub struct Session {
    pub capabilities: HashMap<String, serde_json::Value>,
    pub accounts: HashMap<String, AccountInfo>,
    pub primary_accounts: HashMap<String, String>,
    pub username: String,
    pub api_url: String,
    pub download_url: String,   // RFC 6570 Level-1 template
    pub upload_url: String,     // RFC 6570 Level-1 template
    pub event_source_url: String, // RFC 6570 Level-1 template
    pub state: State,
}
```

Helper methods:

```rust
// Primary account ID for a capability
session.primary_account_id("urn:ietf:params:jmap:mail") -> Option<&str>

// WebSocket capability object (RFC 8887)
session.websocket_capability() -> Result<Option<WebSocketCapability>, ClientError>
```

Extension crates extract their own capability objects from
`session.capabilities` (values are raw `serde_json::Value`).

### `expand_url_template`

Session URL fields (`upload_url`, `download_url`, `event_source_url`) are RFC
6570 Level-1 URI templates. Expand them before use:

```rust
let url = expand_url_template(
    &session.upload_url,
    &[("accountId", "A13824")],
)?;
```

Variables not present in the template are silently ignored. A variable present
in the template but not supplied returns `ClientError::InvalidSession`.

### Auth providers

| Type | Header produced |
|------|----------------|
| `BearerAuth::new(token)?` | `Authorization: Bearer <token>` |
| `BasicAuth::new(user, password)?` | `Authorization: Basic <base64(user:password)>` |
| `NoneAuth` | (no header) |

All three implement `AuthProvider`. Implement the trait yourself for custom
schemes:

```rust
pub trait AuthProvider: Send + Sync {
    fn auth_header(&self) -> Option<(&str, &str)>; // (header-name, header-value) or None
}
```

`Box<dyn AuthProvider>` and `Arc<dyn AuthProvider>` both implement `AuthProvider`.

### Transport configs

| Type | When to use |
|------|-------------|
| `DefaultTransport` | Public internet servers with publicly-trusted TLS |
| `CustomCaTransport::new(der_bytes)` | Private CA, self-hosted, or Tailscale servers |

`DefaultTransport` uses the webpki root certificate store. Both set a 10-second
TCP connect timeout.

`Box<dyn TransportConfig>` implements `TransportConfig`, enabling factory
functions to return a boxed transport.

---

## How it works

**Session fetch** — `fetch_session` GETs `{base_url}/.well-known/jmap`. The
response body is capped at `max_session_body` (default: 1 MiB). All URL fields
(`api_url`, `upload_url`, `download_url`, `event_source_url`) are validated to
have `http` or `https` scheme before the `Session` is returned.

**API calls** — `call` POSTs to `api_url`. The response body is capped at
`max_call_body` (default: 8 MiB). Auth is injected per-request via
`AuthProvider::auth_header()`. Both `fetch_session` and `call` return
`ClientError::AuthFailed` on HTTP 401 or 403 before reading the body.

**`extract_response<T>`** — scans `resp.method_responses` (a `Vec` of
`(method_name, args, call_id)` triples) for the entry whose `call_id` matches,
then deserializes `args` into `T`. If `method_name` is `"error"`, returns
`ClientError::MethodError` with the server-supplied `type` and optional
`description`.

**Blob upload** — `upload_blob` expands the `upload_url` template, POSTs the
raw bytes with the given `Content-Type`, and parses the `BlobUploadResponse`.
If the server returns a `sha256` field (JMAP-CID extension), it is compared
against a locally-computed SHA-256. A mismatch returns
`ClientError::BlobIntegrityMismatch`.

**Blob download** — `download_blob` expands the `download_url` template and
GETs the blob. The body is streamed chunk-by-chunk and capped at
`max_download_body` (default: 64 MiB) without buffering the entire response
first. An optional `expected_sha256` field enables integrity verification.

**SSE stream** — `subscribe_events` GETs the (already-expanded)
`event_source_url`. The response `Content-Type` is verified to be
`text/event-stream`. The chunked body is streamed and accumulated into a string
buffer. Multi-byte UTF-8 codepoints split across HTTP chunks are handled
correctly: the incomplete head bytes are retained between chunks. Each
double-newline-delimited SSE block is parsed into an `SseFrame`. Buffer growth
is capped at `max_sse_frame` (default: 1 MiB) per frame.

**WebSocket** — `connect_ws` validates the URL scheme (`ws://` or `wss://`),
applies a 10-second connect timeout, and returns a `WsSession`. Outgoing
requests are wrapped in a `WsRequestFrame` that injects `"@type": "Request"`
(RFC 8887 §4.3.2) in a single serialization pass. Incoming text frames are
dispatched on `"@type"`: `"StateChange"` and `"Response"` are deserialized into
typed variants; malformed frames and unknown types degrade to `WsFrame::Unknown`
rather than closing the connection. Incoming messages are capped at 1 MiB.

---

## Extension trait pattern

`jmap-mail-client` and `jmap-chat-client` add typed JMAP methods to
`JmapClient` via extension traits (the Rust orphan rule prevents adding
inherent methods to a type from another crate):

```rust
// In jmap-mail-client:
pub trait JmapMailExt {
    async fn email_get(
        &self,
        session: &Session,
        account_id: &str,
        ids: &[&str],
    ) -> Result<EmailGetResponse, ClientError>;
}

impl JmapMailExt for JmapClient {
    async fn email_get(&self, session: &Session, account_id: &str, ids: &[&str])
        -> Result<EmailGetResponse, ClientError>
    {
        let mut builder = JmapRequestBuilder::new(&[
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail",
        ]);
        builder.add_call(
            "Email/get",
            serde_json::json!({ "accountId": account_id, "ids": ids }),
            "c1",
        )?;
        let resp = self.call(&session.api_url, &builder.build()?).await?;
        extract_response(&resp, "c1")
    }
}

// Caller:
use jmap_mail_client::JmapMailExt;
let emails = client.email_get(&session, account_id, &[]).await?;
```

---

## Error types

`ClientError` covers all failure modes:

| Variant | Meaning |
|---------|---------|
| `Http(reqwest::Error)` | Network or TLS error; may be retriable |
| `AuthFailed(u16)` | HTTP 401 or 403; fix credentials before retrying |
| `Parse(serde_json::Error)` | Malformed server response |
| `InvalidArgument(String)` | Caller bug (empty token, bad URL, duplicate call ID, etc.) |
| `InvalidSession(String)` | Server returned a bad Session document |
| `MethodNotFound(String)` | `extract_response` call ID not in response |
| `MethodError { error_type, description }` | Server returned a JMAP error for a method call |
| `BlobIntegrityMismatch { expected, actual }` | SHA-256 mismatch on upload or download |
| `ResponseTooLarge { actual, limit }` | Server response exceeded configured size cap |
| `SseFrameTooLarge { limit }` | Single SSE frame exceeded `max_sse_frame`; stream terminated |
| `WebSocket(tungstenite::Error)` | WebSocket transport error; may be retriable |
| `UnexpectedResponse(String)` | Server violated the JMAP protocol (wrong Content-Type, etc.) |
| `Serialize(serde_json::Error)` | Serialization failure in `WsSession::send_request` |
| `InvalidHeaderValue` | Non-printable-ASCII bytes in a credential string |

---

## Crate family

```
jmap-types
    └── jmap-base-client  (this crate)
            ├── jmap-mail-client   RFC 8621 Email, Mailbox, Thread methods
            └── jmap-chat-client   JMAP Chat extension methods
```

Dependencies flow downward only. Type crates (`jmap-types`, `jmap-mail-types`,
`jmap-chat-types`) have no async dependencies. This crate brings in `tokio` and
`reqwest`.

---

## Gotchas

- **No automatic SSE reconnect.** `subscribe_events` returns an `async Stream` that terminates when the server closes the connection. Reconnect logic (with exponential backoff and `Last-Event-ID` header) is the caller's responsibility.
- **No WebSocket ping/pong keepalive.** `WsSession` does not send RFC 6455 ping frames. If your server closes idle WebSocket connections, implement keepalive in the caller.
- **`fetch_session` is not cached.** Call `fetch_session` once at startup or after receiving a `StateChange` that indicates the session state has changed. Calling it on every request adds unnecessary latency.
- **`request_timeout` applies per-call.** The timeout covers the entire request-response cycle for `fetch_session`, `call`, `upload_blob`, and `download_blob`. It does not apply to SSE or WebSocket streams, which are indefinitely long by design.
- **SSE frame size cap terminates the stream.** If a single SSE frame (between double newlines) exceeds `ClientConfig::max_sse_frame` (default 1 MiB), the stream is terminated with `ClientError::SseFrameTooLarge`. Increase the cap if your server sends large push events.

---

## References

- [RFC 8620](https://www.rfc-editor.org/rfc/rfc8620) — JMAP Core (session,
  blob, SSE, request/response format)
- [RFC 8621](https://www.rfc-editor.org/rfc/rfc8621) — JMAP for Mail
- [RFC 8887](https://www.rfc-editor.org/rfc/rfc8887) — JMAP over WebSocket
- [RFC 6750](https://www.rfc-editor.org/rfc/rfc6750) — Bearer Token Usage
- [RFC 7617](https://www.rfc-editor.org/rfc/rfc7617) — HTTP Basic Authentication
- [RFC 6570](https://www.rfc-editor.org/rfc/rfc6570) — URI Template

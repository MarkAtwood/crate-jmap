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
on `JmapClient` plus a per-extension `SessionClient`. Dependencies flow
downward only: `jmap-types` is the wire-format foundation with no async
deps; this crate brings in `tokio`, `reqwest`, and `tokio-tungstenite`;
extension client crates depend on this crate and add typed methods.

The private transport dependencies (`reqwest` for HTTP, `tokio-tungstenite`
for WebSocket) are wrapped in opaque `HttpError`, `WebSocketError`, and
`InvalidHeaderValueError` types so they stay SemVer-isolated — those crates
can be bumped or swapped without breaking downstream extensions.

---

## How to use

Add to `Cargo.toml`:

```toml
[dependencies]
jmap-base-client = { path = "../crate-jmap-base-client" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

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

### Streaming blob transfer

`upload_blob_stream` and `download_blob_stream` move a blob without holding it
in memory. They run the same checks as the whole-buffer calls.

```rust
use futures::StreamExt;
use jmap_base_client::{upload_body_from_reader, DownloadBlobParams, UploadBlobStreamParams};

let file = tokio::fs::File::open("video.mp4").await?;
let len = file.metadata().await?.len();
let uploaded = client
    .upload_blob_stream(UploadBlobStreamParams {
        upload_url_template: &session.upload_url,
        account_id,
        content_type: "video/mp4",
        body: upload_body_from_reader(file), // or Box::pin(any byte stream)
        content_length: Some(len),           // None: chunked transfer coding
    })
    .await?;

let mut stream = client
    .download_blob_stream(DownloadBlobParams {
        download_url_template: &session.download_url,
        account_id,
        blob_id: uploaded.blob_id.as_ref(),
        name: "video.mp4",
        accept_type: None,
        expected_sha256: uploaded.sha256.as_ref(),
    })
    .await?;
while let Some(chunk) = stream.next().await {
    let chunk = chunk?; // unverified until the stream ends without an error
    // write chunk somewhere
}
```

On upload, the SHA-256 and length are computed as chunks are sent and checked
against the server's response. A body that does not match a declared
`content_length` fails instead of being truncated. On download, the
`max_download_body` cap applies to the running total, and `expected_sha256` is
checked after the last chunk: a mismatch is the stream's final item. In both
directions, `request_timeout` limits the whole transfer, so large transfers
need a client configured with a longer timeout.

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
        SseEvent::Unknown { event_type, .. } => {
            // keepalive ping or unrecognized event type — safe to ignore
            let _ = event_type;
        }
    }
    // Track frame.id for Last-Event-ID on reconnect
}
```

`subscribe_events` expands to an `async Stream` of `SseFrame` values. No
timeout is applied to the stream itself; wrap in `tokio::time::timeout` if you
need a deadline. The stream ends when the server closes the connection.

For automatic reconnects use `subscribe_events_reconnecting` instead. It takes
the same URL and initial `last_event_id` plus an `SseReconnectPolicy`. It
reconnects with exponential backoff and equal jitter (1 s doubling to 60 s by
default, retrying forever unless `max_retries` is set), and sends the last seen
`frame.id` as `Last-Event-ID` (RFC 8620 §7.3). Transport errors, HTTP 408, 429
and 5xx are retried. Auth failures, other 4xx responses, a wrong Content-Type
and oversized frames end the stream.

```rust
use jmap_base_client::SseReconnectPolicy;

let mut stream = client.subscribe_events_reconnecting(&url, None, SseReconnectPolicy::default());
```

### WebSocket (RFC 8887)

```rust
use jmap_base_client::{connect_ws, WsFrame, JmapRequestBuilder};

// Get the WebSocket URL from the session capability
let ws_cap = session
    .websocket_capability()?
    .expect("server must support JMAP WebSocket");

let auth_header = client_auth.auth_header(); // from your AuthProvider
let mut ws = connect_ws(&ws_cap.url, auth_header).await?;

// Optional: Ping after 30 s of silence; treat the peer as dead if nothing
// arrives within 10 s of the Ping (next_frame then returns an I/O error).
ws.enable_keepalive(Duration::from_secs(30), Duration::from_secs(10))?;

// RFC 8887 §4.3.5.2: ask for StateChange pushes (None = all data types),
// resuming from a pushState saved on an earlier connection.
ws.send_push_enable(Some(&["Mailbox", "Email"]), saved_push_state.as_deref()).await?;

// Send a JMAP request over the WebSocket
let mut builder = JmapRequestBuilder::new(&["urn:ietf:params:jmap:core"]);
builder.add_call("Core/echo", serde_json::json!({}), "c1")?;
let req = builder.build()?;
ws.send_request(&req, Some("c1")).await?;

// Receive frames in a loop
while let Some(frame) = ws.next_frame().await {
    match frame? {
        WsFrame::StateChange(sc) => { /* handle push; save sc.extra["pushState"] */ }
        WsFrame::Response(resp) => { /* frame.request_id() == Some("c1") */ }
        WsFrame::RequestError(err) => { /* request-level Problem Details */ }
        _ => { /* WsFrame::Unknown and future variants: ignore */ }
    }
}
```

Match on a borrowed frame (`match &frame`) if you need `frame.request_id()`
inside the arms. It returns the RFC 8887 `requestId` of a `Response` or
`RequestError`, which is the `id` passed to `send_request`.

### Runnable example

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

pub fn subscribe_events_reconnecting(
    &self,
    event_source_url: &str,
    last_event_id: Option<&str>,
    policy: SseReconnectPolicy,
) -> BoxStream<'static, Result<SseFrame, ClientError>>;
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
returned a JMAP `"error"` response for that call (RFC 8620 §3.6.2), or
`ClientError::Parse` if that error object has no `type` (§3.6.2: it "MUST
have a type property"). `MethodError`
carries a `jmap_types::JmapError` (`type`, `description`, `existingId`,
`limit`); to read any other properties of the error object, call
`method_error_details(&resp, call_id)`, which returns the full error
object as a `serde_json::Map`.

**Multiple invocations sharing a call_id.** Per RFC 8620 §3.2, a single
method call may produce multiple invocations in the response — for
example, `Foo/copy` with `onSuccessDestroyOriginal: true` produces both
a `Foo/copy` and an implicit `Foo/set` invocation, both stamped with
the same `call_id` (RFC 8620 §5.8). `extract_response` handles this
case by giving errors precedence: if any invocation matching `call_id`
is a JMAP `"error"` response, that error is returned even when a
sibling invocation with the same `call_id` succeeded. A success cannot
mask a sibling error — silently returning success while the server
reported failure would be data loss. Otherwise the first non-error
invocation matching `call_id` is deserialized into `T`. See the
function-level rustdoc for the full contract, including the "method
name is not checked against `T`" caveat.

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
    pub max_ws_message: usize,        // default: 1 MiB (per WebSocket frame)
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
    pub capabilities: BTreeMap<String, serde_json::Value>,
    pub accounts: BTreeMap<String, AccountInfo>,
    pub primary_accounts: BTreeMap<String, String>,
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
`Invocation { name, arguments, call_id }`) for the entry whose `call_id`
matches, then deserializes `arguments` into `T`. If `name` is `"error"`, returns
`ClientError::MethodError` carrying the server's error object as a
`jmap_types::JmapError`. `method_error_details` returns the full error object,
including any extra properties.

**Blob upload** — `upload_blob` expands the `upload_url` template, POSTs the
raw bytes with the given `Content-Type`, and parses the `BlobUploadResponse`.
If the server returns a `sha256` field (JMAP-CID extension), it is compared
against a locally-computed SHA-256. A mismatch returns
`ClientError::BlobIntegrityMismatch`.

**Blob download** — `download_blob` expands the `download_url` template and
GETs the blob. The body is streamed chunk-by-chunk and capped at
`max_download_body` (default: 64 MiB) without buffering the entire response
first. An optional `expected_sha256` field enables integrity verification.

**Streaming blobs**: `upload_blob_stream` sends a `BlobUploadBody` (a byte
stream, or an `AsyncRead` via `upload_body_from_reader`) with a known or
unknown length. `download_blob_stream` returns a `BlobDownloadStream` of
chunks. Both apply the same caps and SHA-256 checks as `upload_blob` and
`download_blob`; the download digest is checked at end of stream.

**SSE stream** — `subscribe_events` GETs the (already-expanded)
`event_source_url`. The response `Content-Type` is verified to be
`text/event-stream`. The chunked body is streamed and accumulated into a string
buffer. Multi-byte UTF-8 codepoints split across HTTP chunks are handled
correctly: the incomplete head bytes are retained between chunks. Each
double-newline-delimited SSE block is parsed into an `SseFrame`. Buffer growth
is capped at `max_sse_frame` (default: 1 MiB) per frame.

**WebSocket** — `connect_ws` accepts only `wss://` URLs (plaintext `ws://`
needs the explicit opt-in `connect_ws_allow_plaintext`),
applies a 10-second connect timeout, and returns a `WsSession`. Outgoing
requests are wrapped in a `WsRequestFrame` that injects `"@type": "Request"`
(RFC 8887 §4.3.2) in a single serialization pass. Incoming text frames are
dispatched on `"@type"`: `"StateChange"`, `"Response"` and `"RequestError"`
are deserialized into typed variants; malformed frames and unknown types degrade to `WsFrame::Unknown`
rather than closing the connection. Incoming messages are capped at
`ClientConfig.max_ws_message` (default: 1 MiB) per frame.

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
| `Http(HttpError)` | Network or TLS error from the HTTP layer; may be retriable. The payload is an opaque wrapper — use `HttpError::is_timeout`, `HttpError::status`, etc. to diagnose. |
| `AuthFailed(u16)` | HTTP 401 or 403; fix credentials before retrying |
| `Parse(serde_json::Error)` | Malformed server response |
| `InvalidArgument(String)` | Caller bug (empty token, bad URL, duplicate call ID, etc.) |
| `InvalidSession(String)` | Server returned a bad Session document |
| `MethodNotFound(String)` | `extract_response` call ID not in response |
| `MethodError(JmapError)` | Server returned a JMAP error for a method call |
| `BlobIntegrityMismatch { expected, actual }` | SHA-256 mismatch on upload or download |
| `ResponseTooLarge { actual, limit }` | Server response exceeded configured size cap |
| `SseFrameTooLarge { limit }` | Single SSE frame exceeded `max_sse_frame`; stream terminated |
| `WebSocket(WebSocketError)` | WebSocket transport error; may be retriable. The payload is an opaque wrapper — use `WebSocketError::is_io`, `WebSocketError::is_protocol`, etc. to diagnose. |
| `UnexpectedResponse(String)` | Server violated the JMAP protocol (wrong Content-Type, etc.) |
| `Serialize(serde_json::Error)` | Serialization failure in `WsSession::send_request` |
| `InvalidHeaderValue(InvalidHeaderValueError)` | A header value contained characters that are not valid HTTP header-value bytes (typically a credential string with non-printable or non-ASCII characters). |
| `RateLimited { retry_after }` | Server rate-limited the request. Produced for HTTP 429 from `fetch_session`, `call`, `upload_blob` and `download_blob` (and their `_session` wrappers). `retry_after` is the absolute UTC instant from the RFC 9110 §10.2.3 `Retry-After` header: delay-seconds are added to the receive time, and an HTTP-date (any RFC 9110 §5.6.7 format) is used as given. When the header is absent or unparseable, `retry_after` is the receive time, so apply your own backoff. A 429 on the event-source connection still surfaces as `ClientError::Http` so `subscribe_events_reconnecting` keeps retrying it. |

`HttpError`, `WebSocketError`, and `InvalidHeaderValueError` are opaque
wrapper types around the underlying transport-crate errors. They keep
`reqwest` and `tokio-tungstenite` as private dependencies of this crate
so the transport can be swapped or its major version bumped without
breaking downstream callers (SemVer-isolation, bd:JMAP-6lsm.22). Use the
wrappers' accessor methods rather than trying to extract the inner
transport error type.

---

## Gotchas

- **SSE reconnect is opt-in.** `subscribe_events` returns an `async Stream` that terminates when the server closes the connection. Use `subscribe_events_reconnecting` for backoff and `Last-Event-ID` resumption. The SSE `retry:` field is not honoured; the policy alone sets the delays.
- **WebSocket keepalive is opt-in and needs a receive loop.** Call `WsSession::enable_keepalive` before `split`. Pings are sent from inside `next_frame`, so keepalive only works while something is awaiting `next_frame`. WebSocket reconnect is still the caller's job: reconnect, then `send_push_enable` with the last saved `pushState`.
- **`fetch_session` is not cached.** Call `fetch_session` once at startup or after receiving a `StateChange` that indicates the session state has changed. Calling it on every request adds unnecessary latency.
- **`request_timeout` applies per-call.** The timeout covers the entire request-response cycle for `fetch_session`, `call`, `upload_blob`, and `download_blob`. It does not apply to SSE or WebSocket streams, which are indefinitely long by design.
- **SSE frame size cap terminates the stream.** If a single SSE frame (between double newlines) exceeds `ClientConfig::max_sse_frame` (default 1 MiB), the stream is terminated with `ClientError::SseFrameTooLarge`. Increase the cap if your server sends large push events.
- **Cancellation is via drop-future; there is no explicit cancel token.** All four async APIs (`fetch_session`, `call`, `subscribe_events`, `connect_ws_session`) follow the idiomatic Rust async cancellation model: drop the returned future and the underlying transport is torn down by reqwest's / tungstenite's own drop handling. There is no `CancellationToken`-style argument, no `abort_handle()` accessor, and no graceful-shutdown signal. If you need graceful shutdown (release the connection-pool slot, send a WebSocket close frame, etc.), wrap the future in `tokio::select!` against your shutdown signal and let the drop happen on the losing branch — the connection teardown is synchronous. Long-running streams (`subscribe_events`, `WsSession::next_frame` loop) are cancel-safe in this sense: dropping the stream / session releases the underlying HTTP / WebSocket connection without corrupting subsequent operations on unrelated streams. See `JmapClient::subscribe_events` and `WsSession` rustdoc for the per-API contract.

---

## References

- [RFC 8620](https://www.rfc-editor.org/rfc/rfc8620) — JMAP Core (session,
  blob, SSE, request/response format)
- [RFC 8621](https://www.rfc-editor.org/rfc/rfc8621) — JMAP for Mail
- [RFC 8887](https://www.rfc-editor.org/rfc/rfc8887) — JMAP over WebSocket
- [RFC 6750](https://www.rfc-editor.org/rfc/rfc6750) — Bearer Token Usage
- [RFC 7617](https://www.rfc-editor.org/rfc/rfc7617) — HTTP Basic Authentication
- [RFC 6570](https://www.rfc-editor.org/rfc/rfc6570) — URI Template

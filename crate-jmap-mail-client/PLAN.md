# jmap-mail-client — Implementation Plan

RFC 8621 (JMAP for Mail) method implementations on top of `jmap-client`.

## Crate Family Position

```
jmap-types
    ├── jmap-mail-types
    │       └── (types used here)
    └── jmap-client
            └── jmap-mail-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-client` that adds typed methods for every RFC 8621
operation: `Email/get`, `Email/set`, `Email/query`, `Email/changes`, `Mailbox/get`,
`Mailbox/set`, `Thread/get`, `Identity/get`, `EmailSubmission/set`, `SearchSnippet/get`.

Consumers call `jmap-client::JmapClient::call()` directly or use the typed helpers
defined here. No new HTTP machinery — all network operations go through `jmap-client`.

Known potential consumers: a future CLI mail client, or `stoa` if it ever grows a
client-side sync path.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that's `jmap-client`)
- Not handling IMAP, SMTP, or other non-JMAP mail protocols

## Source Material

This is **greenfield** — no existing Rust implementation to extract from.

Design pattern to follow:
- `~/PROJECT/crate-jmapchat-client/src/methods/` — how method inputs/outputs are
  structured and how `JmapRequestBuilder` is used to issue calls
- `~/PROJECT/jmap-chat-spec/references/rfc8621.txt` — normative spec

## Dependencies

```toml
jmap-types      = { path = "../crate-jmap-types" }
jmap-mail-types = { path = "../crate-jmap-mail-types" }
jmap-client     = { path = "../crate-jmap-client" }
serde_json      = "1"
thiserror       = "2"
```

No direct reqwest/tokio dependency — all I/O goes through `jmap-client`.

## Planned Public API

```rust
/// Extension methods on JmapClient for RFC 8621.
impl JmapClient {
    // Email
    pub async fn email_get(&self, account_id: &Id, ids: &[Id], props: &[&str])
        -> Result<GetResponse<Email>, ClientError>;
    pub async fn email_set(&self, account_id: &Id, req: SetRequest<Email>)
        -> Result<SetResponse<Email>, ClientError>;
    pub async fn email_query(&self, account_id: &Id, req: EmailQueryRequest)
        -> Result<QueryResponse, ClientError>;
    pub async fn email_changes(&self, account_id: &Id, since_state: &State, max: Option<u64>)
        -> Result<ChangesResponse, ClientError>;

    // Mailbox
    pub async fn mailbox_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<Mailbox>, ClientError>;
    pub async fn mailbox_set(&self, account_id: &Id, req: SetRequest<Mailbox>)
        -> Result<SetResponse<Mailbox>, ClientError>;

    // Thread
    pub async fn thread_get(&self, account_id: &Id, ids: &[Id])
        -> Result<GetResponse<Thread>, ClientError>;

    // Identity
    pub async fn identity_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<Identity>, ClientError>;

    // EmailSubmission
    pub async fn email_submission_set(&self, account_id: &Id, req: SetRequest<EmailSubmission>)
        -> Result<SetResponse<EmailSubmission>, ClientError>;

    // SearchSnippet
    pub async fn search_snippet_get(&self, account_id: &Id, filter: serde_json::Value, email_ids: &[Id])
        -> Result<Vec<SearchSnippet>, ClientError>;
}
```

## Module Layout

```
src/
  lib.rs        re-exports; impl JmapClient extension methods
  email.rs      Email/get, Email/set, Email/query, Email/changes request/response types
  mailbox.rs    Mailbox/get, Mailbox/set, Mailbox/query request/response types
  thread.rs     Thread/get, Thread/changes request/response types
  identity.rs   Identity/get request/response types
  submission.rs EmailSubmission/set request/response types
  snippet.rs    SearchSnippet/get request/response types
```

## Test Strategy

- All tests use `wiremock` via `jmap-client`'s HTTP layer — no live network
- Request serialization tests: construct a typed request, verify JSON matches RFC 8621 examples
- Response deserialization tests: feed RFC 8621 example JSON, verify typed structs
- RFC 8621 §1.5 example exchange used as the primary oracle

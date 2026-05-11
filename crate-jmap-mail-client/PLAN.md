# jmap-mail-client — Implementation Plan

RFC 8621 (JMAP for Mail) method implementations on top of `jmap-base-client`.

## Crate Family Position

```
jmap-types
    ├── jmap-mail-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-mail-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for every RFC 8621
operation: `Email/get`, `Email/set`, `Email/query`, `Email/changes`, `Email/copy`,
`Mailbox/get`, `Mailbox/set`, `Mailbox/query`, `Mailbox/changes`, `Thread/get`,
`Thread/changes`, `Identity/get`, `Identity/set`, `Identity/changes`,
`EmailSubmission/get`, `EmailSubmission/changes`, `EmailSubmission/query`,
`EmailSubmission/queryChanges`, `EmailSubmission/set`,
`SearchSnippet/get`, `VacationResponse/get`, `VacationResponse/set`.

Consumers call `jmap-base-client::JmapClient::call()` directly or use the typed helpers
defined here. No new HTTP machinery — all network operations go through `jmap-base-client`.

Known potential consumers: a future CLI mail client, or `stoa` if it ever grows a
client-side sync path.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that's `jmap-base-client`)
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
jmap-base-client     = { path = "../crate-jmap-base-client" }
serde_json      = "1"
thiserror       = "2"
```

No direct reqwest/tokio dependency — all I/O goes through `jmap-base-client`.

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule: only the crate that defines
a type may add inherent methods to it). To add methods to `JmapClient` from this crate,
we use an **extension trait**:

```rust
pub trait JmapMailExt {
    async fn email_get(...) -> Result<...>;
}

impl JmapMailExt for JmapClient {
    async fn email_get(...) -> Result<...> { ... }
}
```

Callers must bring the trait into scope: `use jmap_mail_client::JmapMailExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used — no `async-trait` crate needed.
This works because we do not need `dyn JmapMailExt`. If dyn dispatch is ever required,
wrap with `async-trait 0.1` at that time.

## Planned Public API

The shipped API uses the `SessionClient` shape (extension trait returns a
session-bound client; methods are inherent on `SessionClient`). Id-shaped
parameters are `&Id` / `&[Id]` / `Option<&[Id]>` and state-shaped parameters
are `&State` since bd:JMAP-6by7.2 (2026-05-09); the brief drift to
`&str` / `&[&str]` from earlier 0.1.x preview revisions was reverted in
that bead. The sketch below uses the trait-method shape from the original
plan; the actual shipped API on `SessionClient` is documented in the
README "Registered methods" table.

```rust
use jmap_base_client::{ClientError, JmapClient};
use jmap_mail_types::{Email, Mailbox, Thread, Identity, EmailSubmission, SearchSnippet};
use jmap_types::{Id, State};

/// Extension trait adding RFC 8621 (JMAP for Mail) methods to [`JmapClient`].
///
/// Import this trait to use: `use jmap_mail_client::JmapMailExt;`
pub trait JmapMailExt {
    // Email
    async fn email_get(&self, account_id: &Id, ids: &[Id], props: &[&str])
        -> Result<GetResponse<Email>, ClientError>;
    async fn email_set(&self, account_id: &Id, req: SetRequest<Email>)
        -> Result<SetResponse<Email>, ClientError>;
    async fn email_query(&self, account_id: &Id, req: EmailQueryRequest)
        -> Result<QueryResponse, ClientError>;
    async fn email_changes(&self, account_id: &Id, since_state: &State, max: Option<u64>)
        -> Result<ChangesResponse, ClientError>;

    // Mailbox
    async fn mailbox_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<Mailbox>, ClientError>;
    async fn mailbox_set(&self, account_id: &Id, req: SetRequest<Mailbox>)
        -> Result<SetResponse<Mailbox>, ClientError>;

    // Thread
    async fn thread_get(&self, account_id: &Id, ids: &[Id])
        -> Result<GetResponse<Thread>, ClientError>;

    // Identity
    async fn identity_get(&self, account_id: &Id, ids: Option<&[Id]>)
        -> Result<GetResponse<Identity>, ClientError>;

    // EmailSubmission
    async fn email_submission_set(&self, account_id: &Id, req: SetRequest<EmailSubmission>)
        -> Result<SetResponse<EmailSubmission>, ClientError>;

    // SearchSnippet
    async fn search_snippet_get(&self, account_id: &Id, filter: serde_json::Value, email_ids: &[Id])
        -> Result<Vec<SearchSnippet>, ClientError>;
}

impl JmapMailExt for JmapClient {
    // implementations in email.rs, mailbox.rs, thread.rs, identity.rs, submission.rs, snippet.rs
}
```

## Module Layout

```
src/
  lib.rs        pub trait JmapMailExt; impl JmapMailExt for JmapClient; re-exports
  email.rs      Email/get, Email/set, Email/query, Email/changes request/response types
  mailbox.rs    Mailbox/get, Mailbox/set, Mailbox/query request/response types
  thread.rs     Thread/get, Thread/changes request/response types
  identity.rs   Identity/get request/response types
  submission.rs EmailSubmission/set request/response types
  snippet.rs    SearchSnippet/get request/response types
```

## Extras-preservation policy (JMAP-lbdy)

Every public method-argument and method-response struct defined in this
crate that appears on the JMAP wire carries an `extra` field per the
workspace extras-preservation policy (see workspace `AGENTS.md`):

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

This preserves vendor / site / private-extension fields across
deserialize/serialize round-trip. Wire format is byte-identical when extras
are empty. The `default` attribute is inert for Serialize-only method-arg
structs and active for Deserialize method-response structs; the attribute
set is kept uniform across both directions of wire flow.

In scope in this crate (each has at least one round-trip preservation
test in `methods/mod.rs`):

- Method-argument structs (Serialize-only): `EmailGetParams`,
  `EmailCopyParams`, `MailboxSetParams`, `EmailSubmissionSetParams`,
  `EmailImportInput`, `EmailParseParams`.
- Method-response structs (Deserialize): `EmailImportCreated`,
  `EmailImportResponse`, `EmailParseResponse`.

The crate also re-exports standard response wrappers (`GetResponse<T>`,
`SetResponse<T>`, `ChangesResponse`, `QueryResponse`,
`QueryChangesResponse`) from `jmap-types`; those carry their own `extra`
field per JMAP-lbdy.1 and are not re-documented here.

Out of scope (explicitly excluded by the workspace policy):

- Filter / comparator algebra types and control enums — see workspace
  AGENTS.md "Filter algebra and control enums are explicitly EXCLUDED"
  for the full rationale.
- Internal Rust state types (`SessionClient`) — not wire-format.

### New-type rule

Any new public method-argument or method-response struct added to this
crate that appears on the JMAP wire MUST include the `extra` field from
day one with the documented serde attributes and at least one round-trip
preservation test. Per the canonical-template propagation rule (workspace
AGENTS.md), changes to this crate's surface propagate to the six sibling
extension-client crates in lock-step.

## Test Strategy

- All tests use `wiremock` via `jmap-base-client`'s HTTP layer — no live network
- Request serialization tests: construct a typed request, verify JSON matches RFC 8621 examples
- Response deserialization tests: feed RFC 8621 example JSON, verify typed structs
- RFC 8621 §1.5 example exchange used as the primary oracle

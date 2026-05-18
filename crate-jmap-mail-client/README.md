# jmap-mail-client

## What it is

RFC 8621 JMAP for Mail client methods — typed bindings on top of `jmap-base-client`.

Typed JMAP client methods for JMAP Mail ([RFC 8621]). An extension trait on
`jmap-base-client::JmapClient` that adds all 26 RFC 8621 method calls as typed
async methods.

## What it's for

Implements RFC 8621 method bindings (`Email/get`, `Mailbox/get`/`set`/`changes`,
`Thread/get`, `EmailSubmission/set`, `Identity/get`, `SearchSnippet/get`,
`MDN/send`, etc.) on top of `jmap-base-client`. This crate is the canonical
template for every extension `*-client` crate in the workspace — chat, calendars,
tasks, contacts, filenode, sharing, and metadata all mirror its module layout
and method-shape conventions. Depends on `jmap-base-client` for transport and
session, and on `jmap-mail-types` for the wire types.

## How to use

```rust
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_mail_client::{JmapMailExt, EmailGetParams};
use jmap_types::Id;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auth = BearerAuth::new("my-token")?;
    let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

    // Fetch the session and bind it to a mail session client.
    let session = client.fetch_session().await?;
    let mail = client.with_mail_session(session);

    // Fetch two emails with body values inlined.
    let params = EmailGetParams {
        fetch_text_body_values: Some(true),
        ..Default::default()
    };
    let ids = [Id::new_validated("e1")?, Id::new_validated("e2")?];
    let resp: jmap_mail_client::GetResponse<jmap_mail_types::Email> =
        mail.email_get(Some(&ids), None, Some(params)).await?;

    for email in &resp.list {
        println!("{}: {:?}", email.id, email.subject);
    }
    Ok(())
}
```

Id parameters are typed `&jmap_types::Id` (or `&[jmap_types::Id]` for slices)
to make invalid Ids unrepresentable. State tokens use `&jmap_types::State`.
Construct Ids with `Id::new_validated(s)` to enforce RFC 8620 §1.2 syntax at
the boundary, or with `Id::from(s)` when the value is known-valid (e.g.
already came back from a server response).

After calling `JmapMailExt::with_mail_session(session)` the returned
[`SessionClient`] carries the session and makes it available to all methods
without requiring the caller to pass it again. Construct a new `SessionClient`
after each `fetch_session` call — do not reuse a stale one across session
state changes.

## Registered methods

All 26 RFC 8621 method names are available as typed async methods on
[`SessionClient`]:

| Method | Parameters | Returns |
|---|---|---|
| `email_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>, params: Option<EmailGetParams>` | `GetResponse<Email>` |
| `email_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `email_set` | `create: Option<Value>, update: Option<HashMap<Id, PatchObject>>, destroy: Option<Vec<Id>>, if_in_state: Option<&State>` | `SetResponse<Email>` |
| `email_query` | `filter: Option<Value>, sort: Option<Value>, position: Option<u64>, limit: Option<u64>, collapse_threads: Option<bool>` | `QueryResponse` |
| `email_query_changes` | `since_query_state: &State, max_changes: Option<u64>, collapse_threads: Option<bool>, filter: Option<Value>, sort: Option<Value>, up_to_id: Option<&Id>, calculate_total: Option<bool>` | `QueryChangesResponse` |
| `email_copy` | `params: EmailCopyParams, create: Value` | `SetResponse<Email>` |
| `email_import` | `emails: &HashMap<String, EmailImportInput>, if_in_state: Option<&State>` | `EmailImportResponse` |
| `email_parse` | `blob_ids: &[Id], params: Option<EmailParseParams>` | `EmailParseResponse` |
| `mailbox_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<Mailbox>` |
| `mailbox_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `mailbox_set` | `create: Option<Value>, update: Option<HashMap<Id, PatchObject>>, destroy: Option<Vec<Id>>, params: Option<MailboxSetParams>` | `SetResponse<Mailbox>` |
| `mailbox_query` | `filter: Option<Value>, sort: Option<Value>, position: Option<u64>, limit: Option<u64>` | `QueryResponse` |
| `mailbox_query_changes` | `since_query_state: &State, max_changes: Option<u64>, filter: Option<Value>, sort: Option<Value>, up_to_id: Option<&Id>, calculate_total: Option<bool>` | `QueryChangesResponse` |
| `thread_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<Thread>` |
| `thread_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `identity_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<Identity>` |
| `identity_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `identity_set` | `create: Option<Value>, update: Option<HashMap<Id, PatchObject>>, destroy: Option<Vec<Id>>` | `SetResponse<Identity>` |
| `search_snippet_get` | `filter: Value, email_ids: Option<&[Id]>` | `Value` |
| `email_submission_get` | `ids: Option<&[Id]>, properties: Option<&[&str]>` | `GetResponse<EmailSubmission>` |
| `email_submission_changes` | `since_state: &State, max_changes: Option<u64>` | `ChangesResponse` |
| `email_submission_query` | `filter: Option<Value>, sort: Option<Value>, position: Option<u64>, limit: Option<u64>` | `QueryResponse` |
| `email_submission_query_changes` | `since_query_state: &State, max_changes: Option<u64>, filter: Option<Value>, sort: Option<Value>, up_to_id: Option<&Id>, calculate_total: Option<bool>` | `QueryChangesResponse` |
| `email_submission_set` | `create: Option<Value>, update: Option<HashMap<Id, PatchObject>>, destroy: Option<Vec<Id>>, if_in_state: Option<&State>, params: Option<EmailSubmissionSetParams>` | `SetResponse<EmailSubmission>` |
| `vacation_response_get` | _(none)_ | `VacationResponse` (unwraps the spec-mandated singleton from the /get envelope) |
| `vacation_response_get_envelope` | _(none)_ | `GetResponse<VacationResponse>` (envelope including `state` token for `ifInState` guards) |
| `vacation_response_set` | `update: Option<HashMap<Id, PatchObject>>` | `SetResponse<VacationResponse>` |

`Id` and `State` here are `jmap_types::Id` and `jmap_types::State`.

## EmailSubmissionSetParams

`EmailSubmissionSetParams` carries two method-level fields that are sent at the
top level of the `EmailSubmission/set` request body, not inside a create or
update object:

```rust
pub struct EmailSubmissionSetParams {
    /// Map of creation key → PatchObject to apply to the related Email
    /// if the submission is created successfully (RFC 8621 §7.5).
    ///
    /// Keys prefixed with "#" are result references to creation keys in the
    /// same `create` map.  Typical use: remove `$draft` keyword on success.
    pub on_success_update_email: Option<HashMap<String, jmap_types::PatchObject>>,

    /// Email IDs (or "#"-prefixed creation keys) to destroy if the submission
    /// succeeds (RFC 8621 §7.5).
    ///
    /// Typical use: destroy the draft email after successful delivery.
    pub on_success_destroy_email: Option<Vec<String>>,

    /// Vendor / site / private extension fields preserved across the wire
    /// per the workspace extras-preservation policy.
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

Both fields are omitted from the JSON payload when `None`.

## EmailGetParams

`EmailGetParams` controls which body content the server includes in an
`Email/get` response (RFC 8621 §4.1.8). All fields default to `None` (server
default):

```rust
pub struct EmailGetParams {
    /// Which body-part properties to return (overrides server default list).
    pub body_properties: Option<Vec<String>>,

    /// If true, inline decoded values for text/plain body parts.
    pub fetch_text_body_values: Option<bool>,

    /// If true, inline decoded values for text/html body parts.
    /// Wire name is `fetchHTMLBodyValues` (HTML uppercase) per RFC 8621 §4.2.
    pub fetch_html_body_values: Option<bool>,

    /// If true, inline decoded values for all body parts.
    pub fetch_all_body_values: Option<bool>,

    /// Maximum bytes of body value to return per part (0 or absent = no limit).
    pub max_body_value_bytes: Option<u64>,

    /// Vendor / site / private extension fields preserved across the wire
    /// per the workspace extras-preservation policy.
    pub extra: serde_json::Map<String, serde_json::Value>,
}
```

Fields set to `None` are omitted from the request; the server uses its own
defaults for omitted fields.

## Response types

| Type | RFC section | Description |
|---|---|---|
| `GetResponse<T>` | RFC 8620 §5.1 | `/get` response: `account_id`, `state`, `list`, `not_found` |
| `ChangesResponse` | RFC 8620 §5.2 | `/changes` response: `old_state`, `new_state`, `has_more_changes`, `created`, `updated`, `destroyed` |
| `SetResponse<T>` | RFC 8620 §5.3 | `/set` response: `created`, `updated`, `destroyed`, `not_created`, `not_updated`, `not_destroyed` |
| `QueryResponse` | RFC 8620 §5.5 | `/query` response: `query_state`, `can_calculate_changes`, `position`, `ids`, `total`, `limit` |
| `QueryChangesResponse` | RFC 8620 §5.6 | `/queryChanges` response: `old_query_state`, `new_query_state`, `removed`, `added` |
| `AddedItem` | RFC 8620 §5.6 | Entry in `QueryChangesResponse::added`: `id` and `index` |

`SetResponse<T>` defaults to `SetResponse<serde_json::Value>` when no type
parameter is given. Use `SetResponse<Email>` to get typed created/updated maps.

## How it works

Every method on `SessionClient` follows the same six-step pipeline:

1. **Validate arguments** — defence-in-depth empty-state guards fire before any
   I/O, returning `ClientError::InvalidArgument` immediately. Id-shaped
   parameters are typed `&Id` / `&[Id]` and validated by construction
   (`Id::new_validated`); the production code does not re-validate Id syntax.
2. **`session_parts()`** — extracts `(api_url, account_id)` from the bound
   session; returns `ClientError::InvalidSession` if there is no primary account
   for `urn:ietf:params:jmap:mail`.
3. **Build args JSON** — constructs the `serde_json::Value` argument object,
   merging in any extra params structs by iterating their key-value pairs.
4. **`build_request(method, args, USING_*)`** — wraps the single invocation
   into a `JmapRequest` with the appropriate `using` array per RFC 8621 §1.3:
   `USING_MAIL` for Email/Mailbox/Thread/SearchSnippet methods,
   `USING_SUBMISSION` for Identity/EmailSubmission methods (includes both
   `urn:ietf:params:jmap:mail` and `urn:ietf:params:jmap:submission`), and
   `USING_VACATION` for VacationResponse methods. Call ID is `"r1"`.
5. **`call_internal(api_url, &req)`** — delegates to
   `jmap_base_client::JmapClient::call`, which POSTs the request and returns a
   `JmapResponse`.
6. **`extract_response(&resp, CALL_ID)`** — finds the invocation for call ID
   `"r1"` in the response and deserializes it into the typed return value.

## Examples

The crate ships one runnable example under `examples/` demonstrating the
end-to-end client flow:

- [`examples/mailbox_list.rs`](examples/mailbox_list.rs) — fetch and
  print all mailboxes from a synthetic JMAP server (wiremock mock with a
  hand-written `Mailbox/get` response covering the common RFC 8621 §2.1
  roles). Run: `cargo run --example mailbox_list -p jmap-mail-client`.

NOT FOR PRODUCTION — synthetic mock-server fixtures only, no auth, no TLS.
Demonstrates the consume-side API.

## Gotchas

- **`email_import` requires a separately uploaded blob.** RFC 8621 §4.8
  `Email/import` operates on a previously uploaded raw RFC 5322 message;
  callers must upload the raw bytes via `jmap_base_client::JmapClient`'s
  blob-upload API and pass the resulting `blob_id` to `EmailImportInput`.
  Likewise, `email_parse` operates on blobs already in the store.
- **Partial `Email/get` via `properties` filtering breaks deserialization.**
  `Email` has six required metadata fields (`id`, `blob_id`, `thread_id`,
  `mailbox_ids`, `keywords`, `size`, `received_at`). If the server omits any of
  these because of a `properties` filter, `GetResponse<Email>` will fail to
  deserialize. Use `GetResponse<serde_json::Value>` for partial-field responses.
- **No streaming API for large email bodies.** All body values returned by
  `Email/get` with `fetch_*_body_values` options are buffered in memory as part
  of the JMAP response. Very large messages should be downloaded via
  `JmapClient::download_blob` instead.
- **Implicit `Email/set` from `EmailSubmission/set`.** When
  `EmailSubmissionSetParams::on_success_update_email` or
  `on_success_destroy_email` is set, the server generates an implicit `Email/set`
  invocation and includes it in the response. `extract_response` extracts only
  the `EmailSubmission/set` result identified by call ID `"r1"`. Callers that
  need to inspect the implicit `Email/set` result must call
  `jmap_base_client::JmapClient::call` directly and iterate
  `JmapResponse::method_responses` themselves.

## Crate family

```
jmap-types
    ├── jmap-mail-types      Email, Mailbox, Thread, Identity, etc.
    │       └── jmap-mail-client  ← this crate
    └── jmap-base-client     transport, session, auth
            └── (also a dep of jmap-mail-client)
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[RFC 8621]** — JMAP for Mail (method names, argument shapes, error conditions)
- **[RFC 8620]** — JMAP Core (request/response envelope, `/set` and `/query`
  shapes, ResultReference, error types)
- **[RFC 5322]** — Internet Message Format (message structure referenced by
  `Email/import` and `Email/parse`)

[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 5322]: https://www.rfc-editor.org/rfc/rfc5322

# jmap-mail-client

RFC 8620 typed client methods for JMAP Mail ([RFC 8621]). An extension trait on
`jmap-base-client::JmapClient` that adds all 26 RFC 8621 method calls as typed
async methods.

## Usage

```rust
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_mail_client::{JmapMailExt, EmailGetParams};

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
    let resp: jmap_mail_client::GetResponse<jmap_mail_types::Email> =
        mail.email_get(Some(&["e1", "e2"]), None, Some(params)).await?;

    for email in &resp.list {
        println!("{}: {:?}", email.id, email.subject);
    }
    Ok(())
}
```

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
| `email_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>, params: Option<EmailGetParams>` | `GetResponse<Email>` |
| `email_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `email_set` | `create: Option<Value>, update: Option<Value>, destroy: Option<Vec<&str>>, if_in_state: Option<&str>` | `SetResponse<Email>` |
| `email_query` | `filter: Option<Value>, sort: Option<Value>, position: Option<u64>, limit: Option<u64>, collapse_threads: Option<bool>` | `QueryResponse` |
| `email_query_changes` | `since_query_state: &str, max_changes: Option<u64>, collapse_threads: Option<bool>` | `QueryChangesResponse` |
| `email_copy` | `params: EmailCopyParams, create: Value` | `SetResponse<Email>` |
| `mailbox_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<Mailbox>` |
| `mailbox_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `mailbox_set` | `create: Option<Value>, update: Option<Value>, destroy: Option<Vec<&str>>, params: Option<MailboxSetParams>` | `SetResponse<Mailbox>` |
| `mailbox_query` | `filter: Option<Value>, sort: Option<Value>, position: Option<u64>, limit: Option<u64>` | `QueryResponse` |
| `mailbox_query_changes` | `since_query_state: &str, max_changes: Option<u64>` | `QueryChangesResponse` |
| `thread_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<Thread>` |
| `thread_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `identity_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<Identity>` |
| `identity_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `identity_set` | `create: Option<Value>, update: Option<Value>, destroy: Option<Vec<&str>>` | `SetResponse<Identity>` |
| `search_snippet_get` | `account_id: Option<&str>, filter: Value, thread_ids: Option<&[&str]>, email_ids: Option<&[&str]>` | `Value` |
| `email_submission_get` | `ids: Option<&[&str]>, properties: Option<&[&str]>` | `GetResponse<EmailSubmission>` |
| `email_submission_changes` | `since_state: &str, max_changes: Option<u64>` | `ChangesResponse` |
| `email_submission_query` | `filter: Option<Value>, sort: Option<Value>, position: Option<u64>, limit: Option<u64>` | `QueryResponse` |
| `email_submission_query_changes` | `since_query_state: &str, max_changes: Option<u64>, filter: Option<Value>` | `QueryChangesResponse` |
| `email_submission_set` | `create: Option<Value>, update: Option<Value>, destroy: Option<Vec<&str>>, if_in_state: Option<&str>, params: Option<EmailSubmissionSetParams>` | `SetResponse<EmailSubmission>` |
| `vacation_response_get` | _(none)_ | `GetResponse<VacationResponse>` |
| `vacation_response_set` | `update: Option<Value>` | `SetResponse<VacationResponse>` |

`Email/import` and `Email/parse` are not yet implemented as typed methods; use
`jmap_base_client::JmapClient::call` directly with a `JmapRequestBuilder` for
those methods.

## EmailSubmissionSetParams

`EmailSubmissionSetParams` carries two method-level fields that are sent at the
top level of the `EmailSubmission/set` request body, not inside a create or
update object:

```rust
pub struct EmailSubmissionSetParams {
    /// Map of creation key → JSON Merge Patch to apply to the related Email
    /// if the submission is created successfully (RFC 8621 §7.5).
    ///
    /// Keys prefixed with "#" are result references to creation keys in the
    /// same `create` map.  Typical use: remove `$draft` keyword on success.
    pub on_success_update_email: Option<serde_json::Value>,

    /// Email IDs (or "#"-prefixed creation keys) to destroy if the submission
    /// succeeds (RFC 8621 §7.5).
    ///
    /// Typical use: destroy the draft email after successful delivery.
    pub on_success_destroy_email: Option<Vec<String>>,
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
    pub fetch_html_body_values: Option<bool>,

    /// If true, inline decoded values for all body parts.
    pub fetch_all_body_values: Option<bool>,

    /// Maximum bytes of body value to return per part (0 or absent = no limit).
    pub max_body_value_bytes: Option<u64>,
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

1. **Validate arguments** — empty-string guards fire before any I/O, returning
   `ClientError::InvalidArgument` immediately.
2. **`session_parts()`** — extracts `(api_url, account_id)` from the bound
   session; returns `ClientError::InvalidSession` if there is no primary account
   for `urn:ietf:params:jmap:mail`.
3. **Build args JSON** — constructs the `serde_json::Value` argument object,
   merging in any extra params structs by iterating their key-value pairs.
4. **`build_request(method, args, USING_MAIL)`** — wraps the single invocation
   into a `JmapRequest` with `using = ["urn:ietf:params:jmap:core",
   "urn:ietf:params:jmap:mail"]` and call ID `"r1"`.
5. **`call_internal(api_url, &req)`** — delegates to
   `jmap_base_client::JmapClient::call`, which POSTs the request and returns a
   `JmapResponse`.
6. **`extract_response(&resp, CALL_ID)`** — finds the invocation for call ID
   `"r1"` in the response and deserializes it into the typed return value.

## Known Limitations

- **`email_import` and `email_parse` not implemented as typed methods.** RFC
  8621 §5.7 `Email/import` requires the caller to upload the raw message blob
  separately using `jmap_base_client::JmapClient::upload_blob` first and then
  pass a `blob_id` string to the method. RFC 8621 §5.8 `Email/parse` similarly
  operates on a blob already in the store. Both methods can be called via
  `JmapRequestBuilder` / `JmapClient::call` directly today; typed wrappers are
  deferred to a future version.
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

## License

MIT OR Apache-2.0

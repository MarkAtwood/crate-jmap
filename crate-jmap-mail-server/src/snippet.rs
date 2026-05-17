//! SearchSnippet/get method handler (RFC 8621 §5.1).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1), with the type-specific arguments defined by RFC 8621
//! §5.1. The returned `Value` is the corresponding method-response object
//! per the same section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `accountNotSupportedByMethod`, `invalidArguments`,
//! `serverFail` — per RFC 8620 §3.6 and §5).

use jmap_mail_types::{query::EmailFilter, SearchSnippet};
use jmap_types::{Id, Invocation, JmapError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::MailBackend;
use crate::helpers::extract_account_id;
use jmap_server::server_fail_from_backend;

/// Handle a `SearchSnippet/get` method call (RFC 8621 §5.1).
///
/// `args` is the RFC 8621 §5.1 `SearchSnippet/get` request shape
/// (`accountId`, optional `filter`, `emailIds`); the returned `Value` is
/// the §5.1 response shape (`accountId`, `list`, `notFound`).
///
/// # Capability gating
///
/// If the backend returns `false` from
/// [`supports_type::<SearchSnippet>()`](MailBackend::supports_type) the handler
/// returns an `accountNotSupportedByMethod` error without touching the backend.
///
/// # Filter
///
/// The `filter` argument follows the same `Filter<EmailFilterCondition>` shape
/// as `Email/query`. Only a plain `Condition` variant is forwarded to the
/// backend; operator trees (`AND`/`OR`/`NOT`) are passed as `None` (no
/// highlight — valid per RFC 8621 §5.1 which allows `null` snippets).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_search_snippet_get<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Capability gate: check before touching any argument.
    if !backend.supports_type::<SearchSnippet>() {
        return Err(JmapError::account_not_supported_by_method());
    }

    let (account_id, args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let email_ids: Vec<Id> = match args.get("emailIds") {
        Some(v) if !v.is_null() => Vec::<Id>::deserialize(v)
            .map_err(|_| JmapError::invalid_arguments("emailIds must be an Id array"))?,
        _ => return Err(JmapError::invalid_arguments("emailIds is required")),
    };

    // Parse the optional filter as EmailFilter (operator tree or plain condition).
    // Only a plain Condition is forwarded to the backend; operator trees produce
    // no highlights (null subject/preview), which is valid per RFC 8621 §5.9.
    let condition = match args.get("filter") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let filter = EmailFilter::deserialize(v)
                .map_err(|_| JmapError::invalid_arguments("invalid filter"))?;
            match filter {
                jmap_mail_types::query::Filter::Condition(c) => Some(c),
                _ => None,
            }
        }
    };

    let snippets = backend
        .search_snippets(caller, &account_id, &email_ids, condition.as_ref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Build notFound: email_ids that the backend did not return a snippet for.
    let found_ids: std::collections::HashSet<&str> =
        snippets.iter().map(|s| s.email_id.as_ref()).collect();
    let not_found: Vec<Value> = email_ids
        .iter()
        .filter(|id| !found_ids.contains(id.as_ref()))
        .map(|id| Value::String(id.as_ref().to_owned()))
        .collect();

    let list_json: Vec<Value> = snippets
        .iter()
        .map(|s| serde_json::to_value(s).expect("derive(Serialize) on plain data is infallible"))
        .collect();

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "list": list_json,
            "notFound": Value::Array(not_found),
        }),
        vec![],
    ))
}

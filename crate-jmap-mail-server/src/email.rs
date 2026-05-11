//! Email/get, Email/changes, Email/query, Email/queryChanges, Email/set,
//! Email/copy, Email/import, Email/parse method handlers (RFC 8621 §4–5).

use std::collections::{HashMap, HashSet};

use jmap_mail_types::{Email, Keyword};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend};
use crate::helpers::{
    extract_account_id, filter_properties, finalize_set_response, find_immutable_patch_key,
    not_found_json, ser, set_error_value,
};

/// RFC 8621 §4.2 — default `Email/get` property list when `properties` is null.
const DEFAULT_EMAIL_GET_PROPERTIES: &[&str] = &[
    "id",
    "blobId",
    "threadId",
    "mailboxIds",
    "keywords",
    "size",
    "receivedAt",
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "bcc",
    "replyTo",
    "subject",
    "sentAt",
    "hasAttachment",
    "preview",
    "bodyValues",
    "textBody",
    "htmlBody",
    "attachments",
];

/// RFC 8621 §4.9 — default `Email/parse` property list when `properties` is null.
const DEFAULT_EMAIL_PARSE_PROPERTIES: &[&str] = &[
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "bcc",
    "replyTo",
    "subject",
    "sentAt",
    "hasAttachment",
    "preview",
    "bodyValues",
    "textBody",
    "htmlBody",
    "attachments",
];

/// RFC 8621 §4.2 — body-value fetch arguments bundled for a single request.
///
/// Passed by reference to [`apply_body_value_args`] so call sites read as named
/// fields rather than a run of positional booleans.
#[derive(Debug, Clone)]
struct BodyFetchArgs {
    fetch_text: bool,
    fetch_html: bool,
    fetch_all: bool,
    max_bytes: u64,
}

/// RFC 8621 §4.2 — default `bodyProperties` when the `bodyProperties` arg is null.
const DEFAULT_BODY_PROPERTIES: &[&str] = &[
    "partId",
    "blobId",
    "size",
    "name",
    "type",
    "charset",
    "disposition",
    "cid",
    "language",
    "location",
];

// ---------------------------------------------------------------------------
// RFC 8621 §4.1.3 — dynamic header: property support
// ---------------------------------------------------------------------------

/// Parsed form of one `header:<name>[:<form>][:all]` property request.
#[derive(Debug, Clone)]
struct HeaderPropertyRequest {
    /// Case-folded header field name (e.g. `"subject"`).
    name_lower: String,
    /// Requested form.
    form: HeaderForm,
    /// Whether `:all` was specified — return array of all values instead of last.
    all: bool,
}

/// The form in which a header field value is requested (RFC 8621 §4.1.3).
#[derive(Debug, Clone, PartialEq)]
enum HeaderForm {
    Raw,
    AsText,
    AsAddresses,
    AsGroupedAddresses,
    AsMessageIds,
    AsDate,
    AsURLs,
}

impl std::fmt::Display for HeaderForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            HeaderForm::Raw => "raw",
            HeaderForm::AsText => "asText",
            HeaderForm::AsAddresses => "asAddresses",
            HeaderForm::AsGroupedAddresses => "asGroupedAddresses",
            HeaderForm::AsMessageIds => "asMessageIds",
            HeaderForm::AsDate => "asDate",
            HeaderForm::AsURLs => "asURLs",
        };
        f.write_str(s)
    }
}

/// Parse a `header:…` property string into a [`HeaderPropertyRequest`].
///
/// Returns `Err(description)` for any syntax error; the caller maps that to
/// `invalidArguments`.
///
/// Valid syntax:
/// - `header:<Name>` — Raw form, single value
/// - `header:<Name>:all` — Raw form, all values
/// - `header:<Name>:<form>` — named form, single value
/// - `header:<Name>:<form>:all` — named form, all values
fn parse_header_property(prop: &str) -> Result<HeaderPropertyRequest, String> {
    // len > 7 means at least one byte follows the 7-byte "header:" prefix,
    // so &prop[7..] is non-empty and the name segment is non-empty.
    let rest = &prop["header:".len()..];

    // At most 3 segments: name, form, "all".
    let parts: Vec<&str> = rest.splitn(3, ':').collect();
    let name = parts[0];

    if name.is_empty() {
        return Err("header property name must not be empty".into());
    }
    // Validate name: printable US-ASCII (0x21–0x7e), no colon (already split on it),
    // no CRLF injection.
    for b in name.bytes() {
        if !(0x21..=0x7e).contains(&b) {
            return Err(format!(
                "header property name contains invalid byte 0x{b:02x}"
            ));
        }
    }

    let (form, all) = match parts.get(1) {
        // "header:Name"
        None => (HeaderForm::Raw, false),
        // "header:Name:all" — must not have any further segments
        Some(&"all") => {
            if let Some(&extra) = parts.get(2) {
                return Err(format!("unexpected suffix '{extra}' after 'all'"));
            }
            (HeaderForm::Raw, true)
        }
        Some(&form_str) => {
            let f = match form_str {
                "asText" => HeaderForm::AsText,
                "asAddresses" => HeaderForm::AsAddresses,
                "asGroupedAddresses" => HeaderForm::AsGroupedAddresses,
                "asMessageIds" => HeaderForm::AsMessageIds,
                "asDate" => HeaderForm::AsDate,
                "asURLs" => HeaderForm::AsURLs,
                other => return Err(format!("unknown header form: '{other}'")),
            };
            let all = match parts.get(2) {
                None => false,
                Some(&"all") => true,
                Some(&suffix) => {
                    return Err(format!("unexpected suffix '{suffix}' after form"));
                }
            };
            (f, all)
        }
    };

    Ok(HeaderPropertyRequest {
        name_lower: name.to_ascii_lowercase(),
        form,
        all,
    })
}

/// Return `Err` if `form` is incompatible with the well-known header `name_lower`.
///
/// RFC 8621 §4.1.2: when a server recognises a header as having a specific
/// semantic type and the client requests an incompatible form, the server MUST
/// return `invalidArguments`.
fn validate_header_form(name_lower: &str, form: &HeaderForm) -> Result<(), String> {
    use HeaderForm::*;

    const DATE_HEADERS: &[&str] = &["date", "resent-date"];
    const ADDR_HEADERS: &[&str] = &[
        "from",
        "to",
        "cc",
        "bcc",
        "sender",
        "reply-to",
        "resent-from",
        "resent-to",
        "resent-cc",
        "resent-bcc",
        "resent-sender",
        "resent-reply-to",
    ];
    const MSGID_HEADERS: &[&str] = &[
        "message-id",
        "in-reply-to",
        "references",
        "resent-message-id",
    ];
    const URL_HEADERS: &[&str] = &[
        "list-help",
        "list-unsubscribe",
        "list-subscribe",
        "list-post",
        "list-owner",
        "list-archive",
    ];

    // RFC 8621 §4.1.2 requires invalidArguments when a recognised header is
    // requested in an incompatible form (e.g. Date as asAddresses). Silently
    // returning null would violate the spec and allow clients to receive wrong
    // data without error.
    if DATE_HEADERS.contains(&name_lower)
        && matches!(
            form,
            AsAddresses | AsGroupedAddresses | AsMessageIds | AsURLs
        )
    {
        return Err(format!(
            "form {form} is not valid for date header '{name_lower}'"
        ));
    }
    if ADDR_HEADERS.contains(&name_lower) && matches!(form, AsDate | AsMessageIds | AsURLs) {
        return Err(format!(
            "form {form} is not valid for address header '{name_lower}'"
        ));
    }
    if MSGID_HEADERS.contains(&name_lower)
        && matches!(form, AsDate | AsAddresses | AsGroupedAddresses | AsURLs)
    {
        return Err(format!(
            "form {form} is not valid for message-id header '{name_lower}'"
        ));
    }
    if URL_HEADERS.contains(&name_lower)
        && matches!(
            form,
            AsDate | AsAddresses | AsGroupedAddresses | AsMessageIds
        )
    {
        return Err(format!(
            "form {form} is not valid for URL header '{name_lower}'"
        ));
    }
    Ok(())
}

/// Apply `form` to a single raw header field value string.
fn apply_header_form(raw_value: &str, form: &HeaderForm) -> Value {
    use HeaderForm::*;
    match form {
        Raw => {
            // Replace CRLF with LF; return as string (RFC 8621 §4.1.3).
            Value::String(raw_value.replace("\r\n", "\n"))
        }
        AsText => {
            // Unfold: remove CRLF (or LF) followed by WSP, then trim leading whitespace
            // (RFC 8621 §4.1.3 — "unfold then decode encoded-words if possible").
            // RFC 2047 encoded-word decoding is out of scope; we unfold only.
            let unfolded = raw_value
                .replace("\r\n ", " ")
                .replace("\r\n\t", " ")
                .replace("\n ", " ")
                .replace("\n\t", " ");
            Value::String(unfolded.trim_start().to_owned())
        }
        // Returns null silently — validate_header_form accepts AsAddresses for address headers,
        // so clients receive null with no error. Implement RFC 5322 address-list parsing or
        // change to return an error before production use.
        AsAddresses => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[jmap-mail-server] header form AsAddresses not yet implemented (bd:JMAP-g7wu.5) — returning null"
            );
            Value::Null
        }
        // Returns null silently — validate_header_form accepts AsGroupedAddresses for address
        // headers, so clients receive null with no error. Implement RFC 5322 group address
        // parsing or change to return an error before production use.
        AsGroupedAddresses => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[jmap-mail-server] header form AsGroupedAddresses not yet implemented (bd:JMAP-g7wu.5) — returning null"
            );
            Value::Null
        }
        // Returns null silently — validate_header_form accepts AsDate for date headers,
        // so clients receive null with no error. Implement RFC 5322 date parsing or
        // change to return an error before production use.
        AsDate => {
            #[cfg(debug_assertions)]
            eprintln!("[jmap-mail-server] header form AsDate not yet implemented (bd:JMAP-g7wu.5) — returning null");
            Value::Null
        }
        // Returns null silently — validate_header_form accepts AsMessageIds for message-id
        // headers, so clients receive null with no error. Implement RFC 5322 message-id
        // parsing or change to return an error before production use.
        AsMessageIds => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[jmap-mail-server] header form AsMessageIds not yet implemented (bd:JMAP-g7wu.5) — returning null"
            );
            Value::Null
        }
        // Returns null silently — validate_header_form accepts AsURLs for URL-list headers,
        // so clients receive null with no error. Implement URL-list parsing or change to
        // return an error before production use.
        AsURLs => {
            #[cfg(debug_assertions)]
            eprintln!("[jmap-mail-server] header form AsURLs not yet implemented (bd:JMAP-g7wu.5) — returning null");
            Value::Null
        }
    }
}

/// Extract header value(s) from `email_json["headers"]` for the given request.
///
/// `email_json` must be the serialised Email object as returned by the backend;
/// its `"headers"` key is an array of `{name, value}` objects.
fn extract_header_values(email_json: &Value, req: &HeaderPropertyRequest) -> Value {
    match email_json.get("headers").and_then(|v| v.as_array()) {
        None => {
            if req.all {
                Value::Array(vec![])
            } else {
                Value::Null
            }
        }
        Some(headers) => {
            // Collect all matching raw values in order (case-insensitive name comparison).
            let matching: Vec<&str> = headers
                .iter()
                .filter_map(|h| {
                    let name = h.get("name")?.as_str()?;
                    let value = h.get("value")?.as_str()?;
                    if name.eq_ignore_ascii_case(&req.name_lower) {
                        Some(value)
                    } else {
                        None
                    }
                })
                .collect();

            if req.all {
                Value::Array(
                    matching
                        .iter()
                        .map(|v| apply_header_form(v, &req.form))
                        .collect(),
                )
            } else {
                // RFC 8621 §4.1.3: without :all, return the *last* matching instance.
                match matching.last() {
                    Some(v) => apply_header_form(v, &req.form),
                    None => Value::Null,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Email/get (RFC 8621 §5.1)
// ---------------------------------------------------------------------------

/// Handle an `Email/get` method call (RFC 8621 §5.1).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    // RFC 8621 §4.2 — body-value fetch arguments (all default to false / 0 / default list).
    let body_properties: Vec<String> = match args.remove("bodyProperties") {
        None | Some(Value::Null) => DEFAULT_BODY_PROPERTIES
            .iter()
            .map(|&s| s.to_owned())
            .collect(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| JmapError::invalid_arguments(format!("bodyProperties: {e}")))?,
    };
    let fetch_text_body_values: bool = args
        .remove("fetchTextBodyValues")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let fetch_html_body_values: bool = args
        .remove("fetchHTMLBodyValues")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let fetch_all_body_values: bool = args
        .remove("fetchAllBodyValues")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_body_value_bytes: u64 = match args.remove("maxBodyValueBytes") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxBodyValueBytes must be a non-negative integer")
        })?,
    };

    // --- RFC 8621 §4.1.3: split out dynamic header: properties ---
    //
    // Parse and validate each `header:…` property before touching the backend,
    // so we can return `invalidArguments` without issuing any storage queries.
    let (header_props, regular_props): (Vec<&str>, Vec<&str>) = match properties.as_deref() {
        Some(props) => props
            .iter()
            .map(|s| s.as_str())
            .partition(|p| p.starts_with("header:") && p.len() > 7),
        None => (vec![], vec![]),
    };

    // Parse and validate each header: property.  Collect all errors before
    // returning so the first bad property surfaces immediately.
    // Each element is (original_prop_string, parsed_request).
    let parsed_header_reqs: Vec<(&str, HeaderPropertyRequest)> = header_props
        .iter()
        .map(|p| {
            let req = parse_header_property(p)
                .map_err(|e| JmapError::invalid_arguments(format!("property '{p}': {e}")))?;
            validate_header_form(&req.name_lower, &req.form)
                .map_err(|e| JmapError::invalid_arguments(format!("property '{p}': {e}")))?;
            Ok((*p, req))
        })
        .collect::<Result<Vec<_>, JmapError>>()?;

    // Build the effective property set for the backend/filter pass.
    // If the client asked for any header: properties we need "headers" from the
    // backend even if the client didn't explicitly ask for it.
    let client_wants_headers = match properties.as_deref() {
        Some(props) => props.iter().any(|p| p == "headers"),
        None => false,
    };
    // Without this inject-then-strip, header:Name properties would silently return null
    // because the raw 'headers' array would never be fetched from the backend.
    let headers_implicit = !header_props.is_empty() && !client_wants_headers;

    let effective_props: HashSet<&str> = if properties.is_none() {
        DEFAULT_EMAIL_GET_PROPERTIES.iter().copied().collect()
    } else {
        let mut set: HashSet<&str> = regular_props.iter().copied().collect();
        // RFC 8620 §5.1: `id` MUST always be present in /get responses.
        set.insert("id");
        if headers_implicit {
            set.insert("headers");
        }
        set
    };

    // Build the body-properties set once before the per-email loop so it is
    // not rebuilt on every call into apply_body_value_args.
    let body_prop_set: HashSet<&str> = body_properties.iter().map(|s| s.as_str()).collect();
    let body_fetch_args = BodyFetchArgs {
        fetch_text: fetch_text_body_values,
        fetch_html: fetch_html_body_values,
        fetch_all: fetch_all_body_values,
        max_bytes: max_body_value_bytes,
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Email>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|email| {
            let mut val = ser(email)?;
            // Apply body-value filtering and truncation before property filtering.
            apply_body_value_args(&mut val, &body_fetch_args, &body_prop_set);
            let mut obj = filter_properties(&val, &effective_props);

            // Inject dynamic header: property results into the filtered object.
            if !parsed_header_reqs.is_empty() {
                // `val` still holds the full serialised email; use it for extraction.
                if let Value::Object(ref mut map) = obj {
                    for (prop, req) in &parsed_header_reqs {
                        let extracted = extract_header_values(&val, req);
                        map.insert((*prop).to_owned(), extracted);
                    }
                    // Remove the injected "headers" key if the client didn't ask for it.
                    if headers_implicit {
                        map.remove("headers");
                    }
                }
            }

            Ok(obj)
        })
        .collect::<Result<Vec<_>, JmapError>>()?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/changes (RFC 8620 §5.2, as applied to Email)
// ---------------------------------------------------------------------------

/// Handle an `Email/changes` method call (RFC 8620 §5.2).
pub async fn handle_email_changes<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Email, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Email/query (RFC 8621 §4.4)
// ---------------------------------------------------------------------------

/// Handle an `Email/query` method call (RFC 8621 §4.4).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_query<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let filter: Option<jmap_mail_types::EmailFilter> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    let sort: Option<Vec<jmap_mail_types::EmailComparator>> = match args.remove("sort") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
        ),
    };

    // effective_limit is always a concrete u64 after parsing (default 256 when absent).
    // requested_limit tracks what the client sent so we know when to echo limit back.
    let (effective_limit, requested_limit): (u64, Option<u64>) = match args.remove("limit") {
        None | Some(Value::Null) => (256, None),
        Some(v) => match v.as_u64() {
            Some(n) => (n, Some(n)),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )));
            }
        },
    };

    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let collapse_threads: bool = args
        .remove("collapseThreads")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let calculate_total: bool = args
        .remove("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // RFC 8620 §5.5: anchor-based pagination overrides position.
    let anchor: Option<Id> = match args.remove("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {v}"
            )))
        }
    };
    let anchor_offset: i64 = match args.remove("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("anchorOffset: expected an integer, got {v}"))
        })?,
    };

    let sort_slice = sort.as_deref();

    // When collapseThreads or anchor is set we need the full result set in-process.
    // Without either, delegate limit/position directly to the backend.
    let (ids, total, query_state, can_calculate_changes, reported_position) =
        if collapse_threads || anchor.is_some() {
            // Fast path: effective_limit=0 and no total needed — skip the expensive full
            // fetch; a single zero-limit query gives queryState and canCalculateChanges.
            if effective_limit == 0 && !calculate_total {
                let empty = backend
                    .query_objects::<Email>(&account_id, filter.as_ref(), sort_slice, Some(0), 0)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;
                return Ok((
                    serde_json::json!({
                        "accountId": account_id.as_ref(),
                        "queryState": empty.query_state.as_ref(),
                        "canCalculateChanges": empty.can_calculate_changes,
                        "position": 0i64,
                        "ids": [],
                    }),
                    vec![],
                ));
            }

            // Fetch cap+1 to detect whether the backend had more results than
            // the cap.  If more than cap items come back the result was
            // truncated; truncate to cap and report total as an approximation.
            let collapse_cap = backend.max_collapse_threads_emails(&account_id);
            let all = backend
                .query_objects::<Email>(
                    &account_id,
                    filter.as_ref(),
                    sort_slice,
                    Some(collapse_cap as u64 + 1),
                    0,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            let fetched_count = all.ids.len();
            let query_state = all.query_state.clone();
            let can_calculate_changes = all.can_calculate_changes;

            // We requested cap+1 to detect overflow: more than cap returned means
            // the result set was larger than the cap and we must truncate.  Exactly
            // cap returned means we did NOT overflow (the +1 "probe" came back empty).
            let was_capped = fetched_count > collapse_cap;
            let ids_for_collapse: Vec<Id> = if was_capped {
                all.ids.into_iter().take(collapse_cap).collect()
            } else {
                all.ids
            };

            let all_ids = if collapse_threads {
                collapse_by_thread(backend, &account_id, ids_for_collapse)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?
            } else {
                ids_for_collapse
            };

            // When not capped, total is exact. When capped, use the cap as an
            // approximate lower bound (the true count is ≥ collapse_cap).
            // RFC 8620 §5.5 requires total to be present when calculateTotal=true,
            // so we return the cap rather than None — it is an honest lower bound.
            let total: Option<u64> = if !was_capped {
                Some(all_ids.len() as u64)
            } else {
                Some(collapse_cap as u64)
            };

            // Resolve start position: anchor overrides position.
            let start = if let Some(ref anchor_id) = anchor {
                let anchor_idx = all_ids
                    .iter()
                    .position(|id| id == anchor_id)
                    .ok_or_else(JmapError::anchor_not_found)?;
                // RFC 8620 §5.5: clamp effective position to [0, len].
                let raw = anchor_idx as i64 + anchor_offset;
                raw.max(0).min(all_ids.len() as i64) as usize
            } else if position >= 0 {
                (position as usize).min(all_ids.len())
            } else {
                // saturating_neg() avoids i64::MIN overflow.
                let neg = position.saturating_neg() as usize;
                all_ids.len().saturating_sub(neg)
            };

            let page: Vec<Id> = all_ids
                .into_iter()
                .skip(start)
                .take(effective_limit as usize)
                .collect();
            (
                page,
                total,
                query_state,
                can_calculate_changes,
                start as i64,
            )
        } else {
            let result = backend
                .query_objects::<Email>(
                    &account_id,
                    filter.as_ref(),
                    sort_slice,
                    Some(effective_limit),
                    position,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            let pos = result.position;
            let total = result.total;
            (
                result.ids,
                total,
                result.query_state,
                result.can_calculate_changes,
                pos,
            )
        };

    // RFC 8620 §5.5: total MUST be omitted when calculateTotal is false (default).
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": can_calculate_changes,
        "position": reported_position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total {
        if let Some(t) = total {
            resp["total"] = json!(t);
        }
    }
    // RFC 8620 §5.5: return limit if server applied a cap different from what the client sent.
    if requested_limit != Some(effective_limit) {
        resp["limit"] = json!(effective_limit);
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/queryChanges (RFC 8620 §5.6, as applied to Email)
// ---------------------------------------------------------------------------

/// Handle an `Email/queryChanges` method call.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_query_changes<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let since_query_state: State = match args.remove("sinceQueryState") {
        Some(Value::String(s)) => State::from(s.as_str()),
        _ => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let filter: Option<jmap_mail_types::EmailFilter> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    let sort: Option<Vec<jmap_mail_types::EmailComparator>> = match args.remove("sort") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
        ),
    };

    let max_changes: Option<u64> = match args.remove("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = match args.remove("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ))
        }
    };

    // RFC 8621 §4.5: collapseThreads mirrors the argument from the original
    // Email/query that produced the sinceQueryState. Backends that track
    // per-query result sets use it to return thread-collapsed deltas.
    let collapse_threads: bool = args
        .remove("collapseThreads")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let calculate_total: bool = args
        .remove("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let sort_slice = sort.as_deref();
    let result = backend
        .query_changes::<Email>(
            &account_id,
            &since_query_state,
            filter.as_ref(),
            sort_slice,
            max_changes,
            up_to_id.as_ref(),
            collapse_threads,
        )
        .await
        .map_err(JmapError::from)?;

    let added_json: Vec<Value> = result
        .added
        .iter()
        .map(|item| {
            json!({
                "id": item.id.as_ref(),
                "index": item.index,
            })
        })
        .collect();

    let removed_json: Vec<Value> = result
        .removed
        .iter()
        .map(|id| Value::String(id.as_ref().to_owned()))
        .collect();

    // RFC 8620 §5.6: total MUST be omitted unless calculateTotal is true.
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": removed_json,
        "added": added_json,
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/set (RFC 8621 §5.5)
// ---------------------------------------------------------------------------

/// Handle an `Email/set` method call (RFC 8621 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_set<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let old_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            // Validate: at least one mailboxId with value true is required (RFC 8621 §5.5.3).
            let mailbox_ids_ok = obj_val
                .get("mailboxIds")
                .and_then(|v| v.as_object())
                .map(|m| m.values().any(|v| v.as_bool() == Some(true)))
                .unwrap_or(false);

            if !mailbox_ids_ok {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": ["mailboxIds"],
                    }),
                );
                continue;
            }

            // Build the Email object from the creation payload.
            let email = match build_email_from_create(obj_val, &account_id, backend).await {
                Ok(e) => e,
                Err(desc) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "invalidProperties",
                            "description": desc,
                        }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<Email>(&account_id, create_id, email)
                .await
            {
                Ok((server_id, created_obj)) => {
                    // backend.create_object MUST replace the placeholder blobId; see MailBackend doc.
                    debug_assert!(
                        created_obj.blob_id.as_ref() != crate::helpers::PLACEHOLDER_BLOB_ID,
                        "create_object returned a placeholder blobId — backend must assign a real blobId"
                    );
                    mutated = true;
                    // RFC 8621 §5.5: created map contains only server-set fields.
                    created.insert(
                        create_id.clone(),
                        json!({
                            "id": server_id.as_ref(),
                            "blobId": created_obj.blob_id.as_ref(),
                            "threadId": created_obj.thread_id.as_ref(),
                            "size": created_obj.size,
                        }),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str.clone(),
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            // Check for immutable field violations in the patch keys.
            if let Some(bad_field) = find_immutable_patch_key(&patch) {
                not_updated.insert(
                    id_str.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": [bad_field],
                    }),
                );
                continue;
            }

            match backend
                .update_object::<Email>(&account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str.clone(),
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str.clone(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_updated.insert(
                        id_str.clone(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);

            match backend.destroy_object::<Email>(&account_id, &id).await {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    finalize_set_response::<B, Email>(
        backend,
        &account_id,
        old_state,
        mutated,
        created,
        updated,
        destroyed_list,
        not_created,
        not_updated,
        not_destroyed,
    )
    .await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply RFC 8621 §4.2 body-value fetch arguments to a serialized `Email` JSON value.
///
/// - `args.fetch_text/html/all`: control which `bodyValues` entries survive. When none
///   of the three flags are set, `bodyValues` is cleared to an empty object (RFC 8621 §4.2
///   default: all false).
/// - `args.max_bytes`: truncate each `bodyValue.value` string to at most this many bytes
///   (0 = unlimited). Truncation is on a UTF-8 char boundary to avoid producing invalid JSON.
/// - `body_prop_set`: pre-built set of property names to keep in each `EmailBodyPart`. The
///   caller builds this once before the per-email loop so it is not rebuilt on every call.
///
/// This function operates on the serialized JSON value because the body-value filtering rules
/// require cross-referencing `textBody`/`htmlBody` part ids against `bodyValues` keys.
fn apply_body_value_args(val: &mut Value, args: &BodyFetchArgs, body_prop_set: &HashSet<&str>) {
    let Value::Object(ref mut map) = val else {
        return;
    };

    // Collect part ids for text and html body lists so we can filter bodyValues.
    let text_part_ids: HashSet<String> = if args.fetch_text || args.fetch_all {
        map.get("textBody")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.get("partId").and_then(|v| v.as_str()).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let html_part_ids: HashSet<String> = if args.fetch_html || args.fetch_all {
        map.get("htmlBody")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.get("partId").and_then(|v| v.as_str()).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        HashSet::new()
    };

    // Filter bodyValues: keep only entries whose partId appears in the wanted sets.
    // When none of the three fetch flags are set, text_part_ids and html_part_ids
    // are both empty HashSets (constructed in the else branches above), so retain()
    // removes every entry — effectively clearing bodyValues. This is the correct
    // RFC 8621 §4.2 default (all flags false → no body values returned).
    if let Some(Value::Object(ref mut bv_map)) = map.get_mut("bodyValues") {
        if !args.fetch_all {
            bv_map.retain(|part_id, _| {
                text_part_ids.contains(part_id) || html_part_ids.contains(part_id)
            });
        }
        // Apply maxBodyValueBytes truncation to each surviving entry.
        if args.max_bytes > 0 {
            for entry in bv_map.values_mut() {
                if let Some(text) = entry
                    .as_object_mut()
                    .and_then(|e| e.get_mut("value"))
                    .and_then(|v| v.as_str().map(str::to_owned))
                {
                    let limit = args.max_bytes as usize;
                    if text.len() > limit {
                        // Truncate at the last UTF-8 char boundary at or before `limit`
                        // bytes so the output is AT MOST `limit` bytes. A direct slice
                        // at `limit` would panic if `limit` falls in the middle of a
                        // multi-byte sequence. Walking back from `limit` is O(1) because
                        // multi-byte sequences are at most 4 bytes, so we iterate at most
                        // 3 times.
                        let mut end = limit.min(text.len());
                        while !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        let truncated = text[..end].to_owned();
                        if let Some(obj) = entry.as_object_mut() {
                            obj.insert("value".to_owned(), Value::String(truncated));
                            obj.insert("isTruncated".to_owned(), Value::Bool(true));
                        }
                    }
                }
            }
        }
    }

    // Apply bodyProperties filtering to each EmailBodyPart list.
    for key in &["textBody", "htmlBody", "attachments"] {
        if let Some(Value::Array(ref mut parts)) = map.get_mut(*key) {
            for part in parts.iter_mut() {
                *part = apply_body_properties(part, body_prop_set);
            }
        }
    }
    // Also filter the recursive bodyStructure if present.
    if let Some(bs) = map.get_mut("bodyStructure") {
        apply_body_properties_recursive(bs, body_prop_set);
    }
}

/// Filter the fields of a single `EmailBodyPart` JSON object to only those in `props`.
fn apply_body_properties(part: &Value, props: &HashSet<&str>) -> Value {
    if let Value::Object(map) = part {
        let filtered: serde_json::Map<String, Value> = map
            .iter()
            .filter(|(k, _)| props.contains(k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Value::Object(filtered)
    } else {
        part.clone()
    }
}

/// Recursively apply `body_properties` filtering to a `bodyStructure` tree.
fn apply_body_properties_recursive(node: &mut Value, props: &HashSet<&str>) {
    // Recurse into subParts only when the client asked for subParts in
    // bodyProperties — if subParts is absent from props, apply_body_properties
    // will strip it from the output anyway, so recursing first is wasted work.
    if props.contains("subParts") {
        if let Value::Object(ref mut map) = node {
            if let Some(Value::Array(ref mut parts)) = map.get_mut("subParts") {
                for part in parts.iter_mut() {
                    apply_body_properties_recursive(part, props);
                }
            }
        }
    }
    // Filter this node's own keys.  When subParts is not in props, the key is
    // stripped here; when it is in props the children were already filtered above.
    *node = apply_body_properties(node, props);
}

const FORBIDDEN_KEYWORD_CHARS: &[u8] = b"(){]%*\"\\";
const MAX_KEYWORD_LEN: usize = 255;

/// Validate and normalize a raw keyword map from the wire format.
///
/// RFC 8621 §4.1.1 rules:
/// - Each keyword must be 1–255 bytes long.
/// - All bytes must be in printable ASCII range 0x21–0x7e.
/// - Must not contain: `( ) { ] % * " \`
/// - Keywords are normalized to ASCII lowercase before storage.
/// - False-valued entries are silently dropped (keyword is absent).
///
/// Returns the validated, normalized map or a descriptive error string for
/// use in an `invalidProperties` SetError.
fn validate_and_normalize_keywords(
    raw: HashMap<String, bool>,
) -> Result<HashMap<Keyword, bool>, String> {
    raw.into_iter()
        .filter(|(_, v)| *v) // false-valued keyword means absent — skip
        .map(|(kw, _)| {
            if kw.is_empty() || kw.len() > MAX_KEYWORD_LEN {
                return Err(format!(
                    "keyword '{}' has invalid length (must be 1–255 bytes)",
                    kw
                ));
            }
            if let Some(b) = kw
                .bytes()
                .find(|&b| !(0x21..=0x7e).contains(&b) || FORBIDDEN_KEYWORD_CHARS.contains(&b))
            {
                return Err(format!(
                    "keyword '{}' contains forbidden character 0x{:02x}",
                    kw, b
                ));
            }
            Ok((Keyword::from(kw.to_ascii_lowercase()), true))
        })
        .collect()
}

/// Parse and validate `mailboxIds` from an Email/import or Email/copy entry.
///
/// Returns the list of mailbox IDs (entries with `true` value), or an
/// `invalidProperties` error `Value` suitable for inserting into `notCreated`.
///
/// The `rfc_ref` string is embedded in the "required" error description so
/// callers can cite the correct RFC section (§5.7 for import, §6.1 for copy).
fn parse_mailbox_ids(entry: &Value, rfc_ref: &str) -> Result<Vec<Id>, Value> {
    let mailbox_ids: Vec<Id> = match entry.get("mailboxIds").and_then(|v| v.as_object()) {
        Some(m) => m
            .iter()
            .filter(|(_, v)| v.as_bool() == Some(true))
            .map(|(k, _)| Id::from(k.as_str()))
            .collect(),
        None => {
            return Err(json!({"type": "invalidProperties", "properties": ["mailboxIds"]}));
        }
    };
    if mailbox_ids.is_empty() {
        return Err(json!({
            "type": "invalidProperties",
            "properties": ["mailboxIds"],
            "description": format!("at least one mailboxId is required ({rfc_ref})")
        }));
    }
    Ok(mailbox_ids)
}

/// Parse and validate `keywords` from an Email/import or Email/copy entry.
///
/// Returns the normalized keyword list, or an `invalidProperties` error
/// `Value` suitable for inserting into `notCreated`.
fn parse_keywords_field(entry: &Value) -> Result<Vec<Keyword>, Value> {
    let raw_keywords_map: HashMap<String, bool> = match entry.get("keywords") {
        None | Some(Value::Null) => HashMap::new(),
        Some(v) => match HashMap::<String, bool>::deserialize(v) {
            Ok(kws) => kws,
            Err(_) => {
                return Err(json!({"type": "invalidProperties", "properties": ["keywords"]}));
            }
        },
    };
    match validate_and_normalize_keywords(raw_keywords_map) {
        Ok(kws) => Ok(kws.into_keys().collect()),
        Err(desc) => Err(json!({
            "type": "invalidProperties",
            "properties": ["keywords"],
            "description": desc
        })),
    }
}

/// RFC 8621 §4.6 — validate a single EmailBodyPart on creation.
///
/// Rules enforced:
/// - `partId` and `blobId` are mutually exclusive.
/// - If `partId` is present it MUST exist as a key in `body_values`.
/// - The corresponding `EmailBodyValue` MUST NOT have `isEncodingProblem` or
///   `isTruncated` set to `true`.
/// - Sub-parts are validated recursively.
fn validate_body_part(
    part: &Value,
    body_values: Option<&serde_json::Map<String, Value>>,
) -> Result<(), String> {
    let has_part_id = part.get("partId").map(|v| !v.is_null()).unwrap_or(false);
    let has_blob_id = part.get("blobId").map(|v| !v.is_null()).unwrap_or(false);
    if has_part_id && has_blob_id {
        return Err("EmailBodyPart must not specify both partId and blobId".into());
    }
    if has_part_id {
        let part_id = part["partId"].as_str().unwrap_or("");
        match body_values {
            None => {
                return Err(format!(
                    "bodyValues required but missing for partId '{part_id}'"
                ));
            }
            Some(bv) => {
                if !bv.contains_key(part_id) {
                    return Err(format!("bodyValues missing entry for partId '{part_id}'"));
                }
                let bv_entry = &bv[part_id];
                if bv_entry
                    .get("isEncodingProblem")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(format!(
                        "bodyValues['{part_id}'].isEncodingProblem must not be true on create"
                    ));
                }
                if bv_entry
                    .get("isTruncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(format!(
                        "bodyValues['{part_id}'].isTruncated must not be true on create"
                    ));
                }
            }
        }
    }
    // Recurse into sub-parts.
    if let Some(sub_parts) = part.get("subParts").and_then(|v| v.as_array()) {
        for sub in sub_parts {
            validate_body_part(sub, body_values)?;
        }
    }
    Ok(())
}

/// Build an [`Email`] from a creation payload (`obj_val`).
///
/// Extracts `mailboxIds`, `keywords`, and optional header fields from the
/// creation object. Sets `blobId` to [`crate::helpers::PLACEHOLDER_BLOB_ID`];
/// per RFC 8621 §5.5 `blobId` is server-set, so the backend replaces it with
/// the real value inside `create_object`. Assigns a thread id by searching
/// existing emails for matching `inReplyTo`/`references`.
async fn build_email_from_create<B: MailBackend>(
    obj_val: &Value,
    account_id: &Id,
    backend: &B,
) -> Result<Email, String> {
    // NOTE: The following fields are validated (body structure / envelope
    // rules are checked) but are NOT yet stored in the returned Email object:
    //   Envelope: from, to, cc, bcc, sender, replyTo, messageId
    //   Body:     textBody, htmlBody, attachments, bodyStructure, bodyValues
    // Wiring these fields into the stored Email requires jmap-mime integration
    // (tracked in JMAP-wgbh). Until then, clients that read back a created
    // Email will not see these fields even if they were supplied on create.
    //
    // Sentinel values: id="placeholder" and blob_id="placeholder-blob" are
    // set below. The backend MUST replace them — see MailBackend::create_object.

    // mailboxIds: required (already validated non-empty by caller).
    // RFC 8621 §5.5.3: values MUST be true; false means absent — filter out false entries,
    // same as keywords, so the stored object never has false mailboxId entries.
    let mailbox_ids: HashMap<Id, bool> = obj_val
        .get("mailboxIds")
        .and_then(|v| HashMap::<Id, bool>::deserialize(v).ok())
        .map(|m| m.into_iter().filter(|(_, v)| *v).collect())
        .unwrap_or_default();

    // keywords: optional; validate RFC 8621 §4.1.1 syntax, normalize to lowercase,
    // filter to true entries only.
    let raw_keywords: HashMap<String, bool> = match obj_val.get("keywords") {
        None | Some(Value::Null) => HashMap::new(),
        Some(v) => HashMap::<String, bool>::deserialize(v)
            .map_err(|_| "keywords: invalid keyword or format".to_owned())?,
    };
    let keywords =
        validate_and_normalize_keywords(raw_keywords).map_err(|e| format!("keywords: {e}"))?;

    // Subject, inReplyTo, references — used for thread assignment.
    let subject: Option<String> = obj_val
        .get("subject")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    let in_reply_to: Option<Vec<String>> = match obj_val.get("inReplyTo") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            Vec::<String>::deserialize(v)
                .map_err(|_| "inReplyTo: must be an array of strings".to_owned())?,
        ),
    };

    let references: Option<Vec<String>> = match obj_val.get("references") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            Vec::<String>::deserialize(v)
                .map_err(|_| "references: must be an array of strings".to_owned())?,
        ),
    };

    // Thread assignment: look for an existing email whose messageId matches
    // any of the inReplyTo/references tokens.
    let thread_id = assign_thread(
        backend,
        account_id,
        in_reply_to.as_deref().unwrap_or(&[]),
        references.as_deref().unwrap_or(&[]),
    )
    .await
    .map_err(|e| e.to_string())?;

    // size is server-set per RFC 8621 §5.5.3 — this handler always sets size=0 as a
    // placeholder. The backend MUST update this field to the actual blob size before
    // returning the created object from create_object. Backends that do not store raw
    // bytes on the Email/set create path (e.g. MemoryBackend) should use the length
    // of the serialized email JSON as a proxy. Never read size from the client.
    let size: u64 = 0;

    // receivedAt: use provided value or now (RFC 8621 §5.5.3).
    let received_at: UTCDate = obj_val
        .get("receivedAt")
        .and_then(|v| v.as_str())
        .map(UTCDate::from)
        .unwrap_or_else(|| UTCDate::from(crate::helpers::now_utc_string().as_str()));

    // blobId: always use a placeholder. Per RFC 8621 §5.5, blobId is server-set
    // and must not be accepted from the client on Email/set create (accepting it
    // would allow clients to reference blobs they do not own). The backend
    // assigns the real blobId in create_object.
    let blob_id: Id = Id::from(crate::helpers::PLACEHOLDER_BLOB_ID);

    // Use a placeholder id; create_object assigns the real one.
    let mut email = Email::new(
        Id::from("placeholder"),
        blob_id,
        thread_id,
        mailbox_ids,
        size,
        received_at,
    );
    email.keywords = keywords;
    email.subject = subject;
    email.in_reply_to = in_reply_to;
    email.references = references;

    validate_email_body(obj_val)?;

    Ok(email)
}

/// Validate the body-related fields of an Email creation payload (RFC 8621 §4.6).
///
/// Checks:
/// - `bodyStructure` is mutually exclusive with `textBody`, `htmlBody`, and `attachments`.
/// - `textBody` must be exactly one part of type `text/plain`.
/// - `htmlBody` must be exactly one part of type `text/html`.
/// - All body parts satisfy `validate_body_part`: `partId`/`blobId` rules, and
///   `bodyValues` entries must not have `isEncodingProblem` or `isTruncated` true.
fn validate_email_body(obj_val: &Value) -> Result<(), String> {
    // bodyStructure is mutually exclusive with textBody, htmlBody, and attachments.
    if obj_val.get("bodyStructure").is_some_and(|v| !v.is_null())
        && (obj_val.get("textBody").is_some_and(|v| !v.is_null())
            || obj_val.get("htmlBody").is_some_and(|v| !v.is_null())
            || obj_val.get("attachments").is_some_and(|v| !v.is_null()))
    {
        return Err(
            "bodyStructure is mutually exclusive with textBody, htmlBody, and attachments".into(),
        );
    }

    // textBody: must be exactly one part of type text/plain.
    if obj_val.get("textBody").is_some_and(|v| !v.is_null()) {
        let text_parts: &Vec<Value> = obj_val["textBody"]
            .as_array()
            .ok_or("textBody must be an array")?;
        if text_parts.len() != 1 {
            return Err("textBody must contain exactly one body part".into());
        }
        let part_type = text_parts[0]
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if part_type != "text/plain" {
            return Err("textBody part must be of type text/plain".into());
        }
    }

    // htmlBody: must be exactly one part of type text/html.
    if obj_val.get("htmlBody").is_some_and(|v| !v.is_null()) {
        let html_parts: &Vec<Value> = obj_val["htmlBody"]
            .as_array()
            .ok_or("htmlBody must be an array")?;
        if html_parts.len() != 1 {
            return Err("htmlBody must contain exactly one body part".into());
        }
        let part_type = html_parts[0]
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if part_type != "text/html" {
            return Err("htmlBody part must be of type text/html".into());
        }
    }

    let body_values = obj_val.get("bodyValues").and_then(|v| v.as_object());

    // Validate body parts: partId XOR blobId; partId must be in bodyValues;
    // bodyValues entries must not have isEncodingProblem or isTruncated true.
    if let Some(text_parts) = obj_val.get("textBody").and_then(|v| v.as_array()) {
        for part in text_parts {
            validate_body_part(part, body_values)?;
        }
    }
    if let Some(html_parts) = obj_val.get("htmlBody").and_then(|v| v.as_array()) {
        for part in html_parts {
            validate_body_part(part, body_values)?;
        }
    }
    if let Some(attachments) = obj_val.get("attachments").and_then(|v| v.as_array()) {
        for part in attachments {
            validate_body_part(part, body_values)?;
        }
    }
    if let Some(body_struct) = obj_val.get("bodyStructure") {
        if !body_struct.is_null() {
            validate_body_part(body_struct, body_values)?;
        }
    }

    Ok(())
}

/// Assign a thread id for a new email.
///
/// Calls [`MailBackend::find_thread_by_message_ids`] with the union of
/// `in_reply_to` and `references` tokens. Returns the matching thread id if
/// found, a freshly generated id if no existing email references these tokens,
/// or propagates the backend error so the caller can surface it.
async fn assign_thread<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    in_reply_to: &[String],
    references: &[String],
) -> Result<Id, B::Error> {
    if in_reply_to.is_empty() && references.is_empty() {
        return Ok(next_id());
    }

    let refs: Vec<&str> = in_reply_to
        .iter()
        .chain(references.iter())
        .map(|s| s.as_str())
        .collect();

    match backend
        .find_thread_by_message_ids(account_id, &refs)
        .await?
    {
        Some(thread_id) => Ok(thread_id),
        None => Ok(next_id()),
    }
}

/// Generate a unique opaque Id using an atomic counter seeded from the system clock.
///
/// The counter base is initialized to the current nanoseconds since the Unix epoch
/// on the first call. This makes IDs generated in separate process lifetimes
/// extremely unlikely to collide, which matters for persistent backends that store
/// thread IDs across restarts.
///
/// # Caveats
///
/// This is best-effort, not collision-proof. Persistent backends should override
/// [`MailBackend::find_thread_by_message_ids`] to supply thread IDs from their own
/// durable storage rather than relying on this counter.
fn next_id() -> Id {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static BASE: OnceLock<u64> = OnceLock::new();

    let base = *BASE.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1_000_000_000)
    });
    let n = base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed));
    Id::from(format!("{n:016x}"))
}

/// Deduplicate `ids` by `threadId`, keeping only the first email per thread.
///
/// Fetches the query-result emails from the backend to read their thread ids.
/// Propagates backend errors to the caller.
async fn collapse_by_thread<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    ids: Vec<Id>,
) -> Result<Vec<Id>, B::Error> {
    // Fetch only the query-result emails (not all emails) to get their thread ids.
    // Pass a properties hint so backends with column stores can skip body data.
    let thread_props = vec!["id".to_owned(), "threadId".to_owned()];
    let (emails, _) = backend
        .get_objects::<Email>(account_id, Some(&ids), Some(&thread_props))
        .await?;
    let thread_map: HashMap<Id, Id> = emails.into_iter().map(|e| (e.id, e.thread_id)).collect();

    let mut seen_threads: HashSet<Id> = HashSet::new();
    let mut result = Vec::with_capacity(ids.len());

    for id in ids {
        match thread_map.get(&id) {
            Some(tid) if seen_threads.insert(tid.clone()) => {
                result.push(id);
            }
            Some(_) => {
                // Thread already seen — drop this id from the collapsed view.
            }
            None => {
                // Email absent from thread map (concurrent delete). Skip it: without
                // thread info we cannot safely deduplicate, and including it risks
                // surfacing two emails from the same thread.
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Email/import (RFC 8621 §5.7)
// ---------------------------------------------------------------------------

/// Handle an `Email/import` method call (RFC 8621 §5.7).
///
/// Each entry in `emails` must name a blob already uploaded to the account.
/// The backend parses the raw bytes, assigns a thread, and stores the new email.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_import<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let emails = match args.remove("emails") {
        Some(Value::Object(m)) => m,
        _ => return Err(JmapError::invalid_arguments("emails is required")),
    };

    let old_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();

    for (import_id, entry) in emails {
        let blob_id: Id = match entry.get("blobId").and_then(|v| v.as_str()) {
            Some(s) => Id::from(s),
            None => {
                not_created.insert(
                    import_id,
                    json!({"type": "invalidProperties", "properties": ["blobId"]}),
                );
                continue;
            }
        };

        // Only include mailboxIds whose value is true (RFC 8621 §5.7 requires at least one).
        let mailbox_ids = match parse_mailbox_ids(&entry, "RFC 8621 §5.7") {
            Ok(ids) => ids,
            Err(err) => {
                not_created.insert(import_id, err);
                continue;
            }
        };

        // keywords: String[Boolean] wire format — validate RFC 8621 §4.1.1 syntax.
        let keywords: Vec<jmap_mail_types::Keyword> = match parse_keywords_field(&entry) {
            Ok(kws) => kws,
            Err(err) => {
                not_created.insert(import_id, err);
                continue;
            }
        };

        let received_at: Option<UTCDate> = entry
            .get("receivedAt")
            .and_then(|v| v.as_str())
            .map(UTCDate::from);

        match backend
            .import_email(
                &account_id,
                &blob_id,
                &mailbox_ids,
                &keywords,
                received_at.as_ref(),
            )
            .await
        {
            Ok((server_id, email)) => {
                // RFC 8621 §4.8: created entries contain only these 4 server-set fields.
                let obj = json!({
                    "id": server_id.as_ref(),
                    "blobId": email.blob_id.as_ref(),
                    "threadId": email.thread_id.as_ref(),
                    "size": email.size,
                });
                created.insert(import_id, obj);
            }
            Err(BackendSetError::SetError(set_err)) => {
                not_created.insert(import_id, set_error_value(&set_err));
            }
            Err(BackendSetError::Other(e)) => {
                not_created.insert(
                    import_id,
                    json!({ "type": "serverFail", "description": e.to_string() }),
                );
            }
            Err(_) => {
                not_created.insert(
                    import_id,
                    json!({
                        "type": "serverFail",
                        "description": "unhandled backend error variant",
                    }),
                );
            }
        }
    }

    let new_state = if created.is_empty() {
        // No successful imports: state has not changed.
        old_state.clone()
    } else {
        backend
            .get_state::<Email>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/parse (RFC 8621 §5.8)
// ---------------------------------------------------------------------------

/// Handle an `Email/parse` method call (RFC 8621 §5.8).
///
/// Parses the blobs identified by `blobIds` and returns Email objects without
/// storing them (RFC 8621 §5.8).
///
/// Blobs that exist but cannot be parsed → `notParsable`.
/// Blobs that do not exist → `notFound`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_parse<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let blob_ids: Vec<Id> = match args.remove("blobIds") {
        Some(v) => serde_json::from_value(v)
            .map_err(|_| JmapError::invalid_arguments("blobIds must be an Id array"))?,
        None => return Err(JmapError::invalid_arguments("blobIds is required")),
    };

    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    // RFC 8621 §4.9 — body-value fetch arguments (all default to false / 0 / default list).
    let body_properties: Vec<String> = match args.remove("bodyProperties") {
        None | Some(Value::Null) => DEFAULT_BODY_PROPERTIES
            .iter()
            .map(|&s| s.to_owned())
            .collect(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| JmapError::invalid_arguments(format!("bodyProperties: {e}")))?,
    };
    let fetch_text_body_values: bool = args
        .remove("fetchTextBodyValues")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let fetch_html_body_values: bool = args
        .remove("fetchHTMLBodyValues")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let fetch_all_body_values: bool = args
        .remove("fetchAllBodyValues")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_body_value_bytes: u64 = match args.remove("maxBodyValueBytes") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxBodyValueBytes must be a non-negative integer")
        })?,
    };

    // --- RFC 8621 §4.1.3: split out dynamic header: properties (mirrors handle_email_get) ---
    let (header_props, regular_props): (Vec<&str>, Vec<&str>) = match properties.as_deref() {
        Some(props) => props
            .iter()
            .map(|s| s.as_str())
            .partition(|p| p.starts_with("header:") && p.len() > 7),
        None => (vec![], vec![]),
    };

    // Parse and validate each header: property before touching the backend.
    // Each element is (original_prop_string, parsed_request).
    let parsed_header_reqs: Vec<(&str, HeaderPropertyRequest)> = header_props
        .iter()
        .map(|p| {
            let req = parse_header_property(p)
                .map_err(|e| JmapError::invalid_arguments(format!("property '{p}': {e}")))?;
            validate_header_form(&req.name_lower, &req.form)
                .map_err(|e| JmapError::invalid_arguments(format!("property '{p}': {e}")))?;
            Ok((*p, req))
        })
        .collect::<Result<Vec<_>, JmapError>>()?;

    let client_wants_headers = match properties.as_deref() {
        Some(props) => props.iter().any(|p| p == "headers"),
        None => false,
    };
    // Without this inject-then-strip, header:Name properties would silently return null
    // because the raw 'headers' array would never be fetched from the backend.
    let headers_implicit = !header_props.is_empty() && !client_wants_headers;

    // When `properties` is null, RFC 8621 §4.9 specifies the default property list.
    let effective_props: HashSet<&str> = if properties.is_none() {
        DEFAULT_EMAIL_PARSE_PROPERTIES.iter().copied().collect()
    } else {
        let mut set: HashSet<&str> = regular_props.iter().copied().collect();
        // RFC 8620 §5.1: `id` MUST always be present in /get responses.
        set.insert("id");
        if headers_implicit {
            set.insert("headers");
        }
        set
    };

    // Build the body-properties set once before the per-blob loop so it is
    // not rebuilt on every call into apply_body_value_args.
    let body_prop_set: HashSet<&str> = body_properties.iter().map(|s| s.as_str()).collect();
    let body_fetch_args = BodyFetchArgs {
        fetch_text: fetch_text_body_values,
        fetch_html: fetch_html_body_values,
        fetch_all: fetch_all_body_values,
        max_bytes: max_body_value_bytes,
    };

    let mut parsed = serde_json::Map::new();
    let mut not_parsable: Vec<Value> = Vec::new();
    let mut not_found: Vec<Value> = Vec::new();

    for blob_id in &blob_ids {
        match backend.parse_email(&account_id, blob_id).await {
            Ok(email) => {
                let mut val = serde_json::to_value(&email)
                    .expect("derive(Serialize) on plain data is infallible");
                apply_body_value_args(&mut val, &body_fetch_args, &body_prop_set);
                let mut obj = filter_properties(&val, &effective_props);
                // Inject dynamic header: property results (mirrors handle_email_get).
                if !parsed_header_reqs.is_empty() {
                    if let Value::Object(ref mut map) = obj {
                        for (prop, req) in &parsed_header_reqs {
                            let extracted = extract_header_values(&val, req);
                            map.insert((*prop).to_owned(), extracted);
                        }
                        if headers_implicit {
                            map.remove("headers");
                        }
                    }
                }
                // RFC 8621 §5.8: id, blobId, threadId, mailboxIds, keywords, and
                // receivedAt MUST be null in Email/parse responses — these fields have
                // no meaning for a parsed-but-not-stored blob.  The Email struct carries
                // them as non-Option so they always serialise to real values; force them
                // to null here after filter_properties so that explicit client requests
                // like properties:["mailboxIds"] still get null rather than {}.
                const PARSE_NULL_FIELDS: &[&str] = &[
                    "id",
                    "blobId",
                    "threadId",
                    "mailboxIds",
                    "keywords",
                    "receivedAt",
                ];
                if let Value::Object(ref mut map) = obj {
                    for &field in PARSE_NULL_FIELDS {
                        if map.contains_key(field) {
                            map.insert(field.to_owned(), Value::Null);
                        }
                    }
                }
                parsed.insert(blob_id.as_ref().to_owned(), obj);
            }
            Err(_) => {
                // RFC 8621 §5.8: distinguish "blob not found" from "not parsable".
                if backend.blob_exists(&account_id, blob_id).await {
                    not_parsable.push(Value::String(blob_id.as_ref().to_owned()));
                } else {
                    not_found.push(Value::String(blob_id.as_ref().to_owned()));
                }
            }
        }
    }

    let resp = json!({
        "accountId": account_id.as_ref(),
        "parsed": if parsed.is_empty() { Value::Null } else { Value::Object(parsed) },
        "notParsable": if not_parsable.is_empty() { Value::Null } else { Value::Array(not_parsable) },
        "notFound": if not_found.is_empty() { Value::Null } else { Value::Array(not_found) },
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/copy (RFC 8621 §6.1 / RFC 8620 §6.3)
// ---------------------------------------------------------------------------

/// Handle an `Email/copy` method call (RFC 8621 §6.1).
///
/// Copies one or more emails from `fromAccountId` into the current account.
/// Supports `onSuccessDestroyOriginal` and `onSuccessUpdateOriginal`.
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are
/// generated when `onSuccessDestroyOriginal: true` or `onSuccessUpdateOriginal`
/// is non-null, per RFC 8620 §6.3.
pub async fn handle_email_copy<B: MailBackend>(
    backend: &B,
    args: Value,
    call_id: &str,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::from(s),
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };

    // RFC 8620 §5.4: fromAccountId MUST differ from accountId.
    if from_account_id == account_id {
        return Err(JmapError::invalid_arguments(
            "fromAccountId must be different from accountId",
        ));
    }
    if !backend
        .account_exists(&from_account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::from_account_not_found());
    }

    let create = match args.remove("create") {
        Some(Value::Object(m)) => m,
        _ => return Err(JmapError::invalid_arguments("create is required")),
    };

    let on_success_destroy_original: bool = args
        .get("onSuccessDestroyOriginal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // ifFromInState: check source account state (RFC 8620 §5.4).
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<Email>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // ifInState: check destination account state (RFC 8620 §5.4).
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut copied_source_ids: Vec<(String, Id)> = Vec::new(); // (copy_id, source_id)

    for (copy_id, entry) in create {
        let source_id: Id = match entry.get("id").and_then(|v| v.as_str()) {
            Some(s) => Id::from(s),
            None => {
                not_created.insert(
                    copy_id,
                    json!({"type": "invalidProperties", "properties": ["id"]}),
                );
                continue;
            }
        };

        // Only include mailboxIds whose value is true (RFC 8621 §6.1 requires at least one).
        let mailbox_ids = match parse_mailbox_ids(&entry, "RFC 8621 §6.1") {
            Ok(ids) => ids,
            Err(err) => {
                not_created.insert(copy_id, err);
                continue;
            }
        };

        // keywords: String[Boolean] wire format — validate RFC 8621 §4.1.1 syntax.
        let keywords: Vec<Keyword> = match parse_keywords_field(&entry) {
            Ok(kws) => kws,
            Err(err) => {
                not_created.insert(copy_id, err);
                continue;
            }
        };

        // receivedAt may be overridden during copy (RFC 8621 §4.7).
        let received_at: Option<UTCDate> = entry
            .get("receivedAt")
            .and_then(|v| v.as_str())
            .map(UTCDate::from);

        match backend
            .copy_email(
                &from_account_id,
                &source_id,
                &account_id,
                &mailbox_ids,
                &keywords,
                received_at.as_ref(),
            )
            .await
        {
            Ok((new_id, new_email)) => {
                // RFC 8621 §4.7: created entries contain only these 4 server-set fields.
                let obj = json!({
                    "id": new_id.as_ref(),
                    "blobId": new_email.blob_id.as_ref(),
                    "threadId": new_email.thread_id.as_ref(),
                    "size": new_email.size,
                });
                created.insert(copy_id.clone(), obj);
                copied_source_ids.push((copy_id, source_id));
            }
            Err(BackendSetError::SetError(set_err)) => {
                not_created.insert(copy_id, set_error_value(&set_err));
            }
            Err(BackendSetError::Other(e)) => {
                not_created.insert(
                    copy_id,
                    json!({ "type": "serverFail", "description": e.to_string() }),
                );
            }
            Err(_) => {
                not_created.insert(
                    copy_id,
                    json!({
                        "type": "serverFail",
                        "description": "unhandled backend error variant",
                    }),
                );
            }
        }
    }

    let new_state = if created.is_empty() {
        old_state.clone()
    } else {
        backend
            .get_state::<Email>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    };

    let resp = json!({
        "fromAccountId": from_account_id.as_ref(),
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    // Execute onSuccess* side effects and build a single implicit Email/set
    // response (RFC 8620 §6.3).
    //
    // The dispatcher appends extra invocations verbatim to methodResponses, so
    // we must build the full response object here — not request args.
    let mut extra: Vec<Invocation> = Vec::new();

    let on_success_update_original: Option<serde_json::Map<String, Value>> =
        match args.remove("onSuccessUpdateOriginal") {
            Some(Value::Object(m)) => Some(m),
            Some(Value::Null) | None => None,
            _ => None,
        };

    if (on_success_destroy_original || on_success_update_original.is_some())
        && !copied_source_ids.is_empty()
    {
        let email_old_state = backend
            .get_state::<Email>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        let mut email_destroyed: Vec<Value> = Vec::new();
        let mut email_not_destroyed = serde_json::Map::new();
        let mut email_updated = serde_json::Map::new();
        let mut email_not_updated = serde_json::Map::new();

        // onSuccessDestroyOriginal: destroy each successfully copied source email.
        if on_success_destroy_original {
            for (_, source_id) in &copied_source_ids {
                match backend
                    .destroy_object::<Email>(&from_account_id, source_id)
                    .await
                {
                    Ok(()) => {
                        email_destroyed.push(Value::String(source_id.as_ref().to_owned()));
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        email_not_destroyed
                            .insert(source_id.as_ref().to_owned(), set_error_value(&set_err));
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_destroyed.insert(
                            source_id.as_ref().to_owned(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                    Err(_) => {
                        email_not_destroyed.insert(
                            source_id.as_ref().to_owned(),
                            json!({
                                "type": "serverFail",
                                "description": "unhandled backend error variant",
                            }),
                        );
                    }
                }
            }
        }

        // onSuccessUpdateOriginal: for each successfully copied email whose copy_id
        // appears in the map, apply the specified patch to the original.
        if let Some(mut on_success_update) = on_success_update_original {
            for (copy_id, source_id) in &copied_source_ids {
                if let Some(patch_val) = on_success_update.remove(copy_id) {
                    // Convert wire-format Value into a typed PatchObject.
                    // RFC 8620 §5.3: a PatchObject must be a JSON Object;
                    // non-object values produce `invalidPatch`.
                    let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                        Ok(p) => p,
                        Err(e) => {
                            email_not_updated.insert(
                                source_id.as_ref().to_owned(),
                                json!({
                                    "type": "invalidPatch",
                                    "description": e.to_string()
                                }),
                            );
                            continue;
                        }
                    };
                    // Apply same immutable-field guard as handle_email_set patches.
                    if let Some(bad_field) = find_immutable_patch_key(&patch) {
                        email_not_updated.insert(
                            source_id.as_ref().to_owned(),
                            json!({
                                "type": "invalidProperties",
                                "properties": [bad_field],
                            }),
                        );
                        continue;
                    }
                    match backend
                        .update_object::<Email>(&from_account_id, source_id, patch)
                        .await
                    {
                        Ok(Some(obj)) => {
                            email_updated.insert(
                                source_id.as_ref().to_owned(),
                                serde_json::to_value(&obj)
                                    .expect("derive(Serialize) on plain data is infallible"),
                            );
                        }
                        Ok(None) => {
                            email_updated.insert(source_id.as_ref().to_owned(), Value::Null);
                        }
                        Err(BackendSetError::SetError(set_err)) => {
                            email_not_updated
                                .insert(source_id.as_ref().to_owned(), set_error_value(&set_err));
                        }
                        Err(BackendSetError::Other(e)) => {
                            email_not_updated.insert(
                                source_id.as_ref().to_owned(),
                                json!({ "type": "serverFail", "description": e.to_string() }),
                            );
                        }
                        Err(_) => {
                            email_not_updated.insert(
                                source_id.as_ref().to_owned(),
                                json!({
                                    "type": "serverFail",
                                    "description": "unhandled backend error variant",
                                }),
                            );
                        }
                    }
                }
            }
        }

        let email_new_state = backend
            .get_state::<Email>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        // RFC 8620 §6.3: a single implicit Email/set response appended after
        // the Email/copy response.
        let set_resp = json!({
            "accountId": from_account_id.as_ref(),
            "oldState": email_old_state.as_ref(),
            "newState": email_new_state.as_ref(),
            "created": Value::Null,
            "updated": if email_updated.is_empty() { Value::Null } else { Value::Object(email_updated) },
            "destroyed": if email_destroyed.is_empty() { Value::Null } else { Value::Array(email_destroyed) },
            "notCreated": Value::Null,
            "notUpdated": if email_not_updated.is_empty() { Value::Null } else { Value::Object(email_not_updated) },
            "notDestroyed": if email_not_destroyed.is_empty() { Value::Null } else { Value::Object(email_not_destroyed) },
        });
        extra.push(("Email/set".to_owned(), set_resp, call_id.to_owned()));
    }

    Ok((resp, extra))
}

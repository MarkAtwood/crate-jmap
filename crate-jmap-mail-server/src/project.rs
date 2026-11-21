//! The Email/get property check and projection (RFC 8621 §4.1, §4.2),
//! shared by Email/get, Email/parse and, through [`project_email`],
//! `EmailPush` delivery.
//!
//! - [`check_email_property`] checks one requested property name;
//! - [`EmailProjection`] holds one request's `properties`,
//!   `bodyProperties` and body-value fetch arguments, parsed once, and
//!   projects each Email;
//! - [`project_email`] projects one Email onto a property list with the
//!   default `bodyProperties`, no body values and no implicit `id`.

use std::collections::HashSet;

use jmap_mail_types::{Email, EmailAddress, EmailAddressGroup};
use jmap_server::take_bool_arg;
use jmap_types::JmapError;
use serde_json::{Map, Value};

use crate::helpers::{filter_properties, serialize_value};

/// RFC 8621 §4.2 — default `Email/get` property list when `properties` is null.
pub(crate) const DEFAULT_EMAIL_GET_PROPERTIES: &[&str] = &[
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
pub(crate) const DEFAULT_EMAIL_PARSE_PROPERTIES: &[&str] = &[
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
pub(crate) struct BodyFetchArgs {
    pub(crate) fetch_text: bool,
    pub(crate) fetch_html: bool,
    pub(crate) fetch_all: bool,
    pub(crate) max_bytes: u64,
}

/// RFC 8621 §4.2 — default `bodyProperties` when the `bodyProperties` arg is null.
pub(crate) const DEFAULT_BODY_PROPERTIES: &[&str] = &[
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
pub(crate) struct HeaderPropertyRequest {
    /// Case-folded header field name (e.g. `"subject"`).
    pub(crate) name_lower: String,
    /// Requested form.
    pub(crate) form: HeaderForm,
    /// Whether `:all` was specified — return array of all values instead of last.
    pub(crate) all: bool,
}

/// The form in which a header field value is requested (RFC 8621 §4.1.3).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HeaderForm {
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
pub(crate) fn parse_header_property(prop: &str) -> Result<HeaderPropertyRequest, String> {
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

/// Return `Err` if `form` may not be fetched or set for header `name_lower`.
///
/// RFC 8621 §4.1.2: each parsed form "may only be fetched or set for" the
/// fields its subsection lists and "Any header field not defined in
/// \[RFC5322\] or \[RFC2369\]"; Raw is allowed for every field. So a field
/// RFC 5322 or RFC 2369 defines gets only its own forms (Subject, Comments
/// and Keywords: Text; From and the other address fields: Addresses and
/// `GroupedAddresses`; Received and Return-Path: Raw only), and any other
/// field, List-Id included, gets every form.
///
/// # Defense notes (bd:JMAP-q2wa.11, q2wa.47)
///
/// The `const` arrays below are the RFC 8621 §4.1.2.X lists, one per
/// subsection (`TEXT_HEADERS` §4.1.2.2, `ADDR_HEADERS` §4.1.2.3 and
/// §4.1.2.4, `MSGID_HEADERS` §4.1.2.5, `DATE_HEADERS` §4.1.2.6,
/// `URL_HEADERS` §4.1.2.7), so a code search for `"list-help"` lands on
/// its list and each array can be checked against its subsection. Keep
/// them as literal arrays rather than a map: they fit in cache and run at
/// most once per `header:` property.
pub(crate) fn validate_header_form(name_lower: &str, form: &HeaderForm) -> Result<(), String> {
    use HeaderForm::*;

    // RFC 8621 §4.1.2.4 — date-form headers
    const DATE_HEADERS: &[&str] = &["date", "resent-date"];
    // RFC 8621 §4.1.2.3 — address-form headers
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
    // RFC 8621 §4.1.2.5 — message-id-form headers
    const MSGID_HEADERS: &[&str] = &[
        "message-id",
        "in-reply-to",
        "references",
        "resent-message-id",
    ];
    // RFC 8621 §4.1.2.7 — URL-form headers
    const URL_HEADERS: &[&str] = &[
        "list-help",
        "list-unsubscribe",
        "list-subscribe",
        "list-post",
        "list-owner",
        "list-archive",
    ];
    // RFC 8621 §4.1.2.2 — Text-form headers among the defined ones
    const TEXT_HEADERS: &[&str] = &["subject", "comments", "keywords"];
    // The fields RFC 5322 §3.6 and RFC 2369 define (plus Resent-Reply-To,
    // which RFC 8621 lists with the address fields). Each form below "may
    // only be fetched or set" for its own fields and "Any header field not
    // defined in [RFC5322] or [RFC2369]"; Raw is always allowed.
    const OTHER_DEFINED: &[&str] = &["return-path", "received"];
    let defined = [
        DATE_HEADERS,
        ADDR_HEADERS,
        MSGID_HEADERS,
        URL_HEADERS,
        TEXT_HEADERS,
        OTHER_DEFINED,
    ]
    .iter()
    .any(|set| set.contains(&name_lower));
    let allowed: &[&str] = match form {
        Raw => return Ok(()),
        AsText => TEXT_HEADERS,
        AsAddresses | AsGroupedAddresses => ADDR_HEADERS,
        AsMessageIds => MSGID_HEADERS,
        AsDate => DATE_HEADERS,
        AsURLs => URL_HEADERS,
    };
    // RFC 8621 §4.2: "Attempting to fetch a form that is forbidden (e.g.,
    // "header:From:asDate") MUST result in the method call being rejected
    // with an "invalidArguments" error."
    if defined && !allowed.contains(&name_lower) {
        return Err(format!(
            "form {form} is not valid for header '{name_lower}' (RFC 8621 §4.1.2)"
        ));
    }
    Ok(())
}

/// Apply `form` to a single raw header field value string.
///
/// RFC 8621 §4.1.2 defines the parsed-form selectors that may be requested
/// via a `header:<name>:as<form>` property. `Raw` and `AsText` are
/// implemented inline; the five `As*` parse forms delegate to
/// `mime_tree::parse_header_typed`, which is the workspace's single
/// gateway to RFC 5322 header parsing (see bd:JMAP-g7wu.11).
fn apply_header_form(raw_value: &str, form: &HeaderForm) -> Result<Value, JmapError> {
    use HeaderForm::*;
    Ok(match form {
        // RFC 8621 §4.1.2.1: the octets as they are, folding included.
        Raw => Value::String(raw_value.to_owned()),
        // RFC 8621 §4.1.2.2: unfold, strip, decode RFC 2047 encoded-words
        // with a known charset, then NFC, as the `subject` property is.
        AsText => Value::String(mime_tree::parse_text(raw_value.as_bytes())),
        // RFC 8621 §4.1.2.3 — list of mailboxes; group structure discarded.
        AsAddresses => match mime_tree::parse_header_typed(
            mime_tree::HeaderForm::Addresses,
            &restore_crlf(raw_value),
        ) {
            mime_tree::HeaderValueTyped::Addresses(list) => Value::Array(
                list.into_iter()
                    .map(jmap_email_address_value)
                    .collect::<Result<_, _>>()?,
            ),
            _ => Value::Array(Vec::new()),
        },
        // RFC 8621 §4.1.2.4 — list of EmailAddressGroup; preserves group structure.
        AsGroupedAddresses => match mime_tree::parse_header_typed(
            mime_tree::HeaderForm::GroupedAddresses,
            &restore_crlf(raw_value),
        ) {
            mime_tree::HeaderValueTyped::GroupedAddresses(groups) => Value::Array(
                groups
                    .into_iter()
                    .map(jmap_email_address_group_value)
                    .collect::<Result<_, _>>()?,
            ),
            _ => Value::Array(Vec::new()),
        },
        // RFC 8621 §4.1.2.6 — RFC 3339 / ISO 8601 string, or null if the
        // header value did not parse as a `date-time`.
        AsDate => match mime_tree::parse_header_typed(
            mime_tree::HeaderForm::Date,
            &restore_crlf(raw_value),
        ) {
            mime_tree::HeaderValueTyped::DateTime(Some(dt)) => Value::String(dt.to_rfc3339()),
            _ => Value::Null,
        },
        // RFC 8621 §4.1.2.5 — list of bare msg-id strings (angle brackets stripped).
        AsMessageIds => match mime_tree::parse_header_typed(
            mime_tree::HeaderForm::MessageIds,
            &restore_crlf(raw_value),
        ) {
            mime_tree::HeaderValueTyped::MessageIds(ids) if !ids.is_empty() => {
                Value::Array(ids.into_iter().map(Value::String).collect())
            }
            // RFC 8621 §4.1.2.5: returns null when the header value does
            // not parse as a list of msg-id values (per the spec's
            // "List of MessageIds" form, which is nullable rather than an
            // empty array on parse failure).
            _ => Value::Null,
        },
        // RFC 8621 §4.1.2.7 — list of bare URL strings (angle brackets stripped).
        AsURLs => match mime_tree::parse_header_typed(
            mime_tree::HeaderForm::URLs,
            &restore_crlf(raw_value),
        ) {
            mime_tree::HeaderValueTyped::URLs(urls) if !urls.is_empty() => {
                Value::Array(urls.into_iter().map(Value::String).collect())
            }
            // RFC 8621 §4.1.2.7: returns null when the header value does
            // not parse as a list of URLs (per the spec's "List of URLs"
            // form, which is nullable rather than an empty array on parse
            // failure).
            _ => Value::Null,
        },
    })
}

/// Restore RFC 5322 wire CRLFs in a header field value before handing it
/// to `mime_tree::parse_header_typed`.
///
/// The JMAP "Raw" storage form has already collapsed `\r\n` to `\n` (RFC
/// 8621 §4.1.3), and `parse::parse_rfc5322_headers` also
/// folds continuation lines using `\n` as the separator. mail-parser
/// (mime-tree's underlying parser) expects RFC 5322 wire bytes with CRLF
/// line endings, so we re-expand `\n` → `\r\n` to give the parser the
/// folding it knows how to recognise. A standalone `\r` is left alone:
/// real wire bytes never contain it, and treating it as a fold separator
/// could corrupt the rare CR-in-display-name case.
///
/// # Defense notes (bd:JMAP-q2wa.12)
///
/// A future contributor might "simplify" this to a single
/// `raw_value.replace('\n', "\r\n")` call on the assumption that the
/// stored Raw form is guaranteed pure-LF per RFC 8621 §4.1.3. Do not
/// collapse:
///
///   1. The defensive collapse handles backends that store mixed-line-
///      ending Raw values — a corrupt or mid-migration store could have
///      either form, and a one-pass `\n → \r\n` over a `\r\n`-containing
///      input produces `\r\r\n` which most parsers reject.
///   2. Idempotence is testable: round-tripping any input through
///      `restore_crlf` produces consistent output. The one-pass
///      alternative is not idempotent under any input that already
///      contains CRLF.
///   3. mail-parser's tolerance for non-canonical line endings is
///      documented, but the bidirectional bug is on the side of THIS
///      function, not the parser — we can't push the brittleness
///      downstream.
///   4. The standalone `\r` case is explicitly handled by leaving it
///      alone: real wire bytes never contain a bare CR, and treating
///      one as a fold separator could corrupt the rare
///      CR-in-display-name case.
///
/// This logic is one of the few that touch raw byte sequences (vs
/// strings) and so deserves a defended status — wire-byte edge cases
/// are exactly where future "simplifications" tend to regress.
fn restore_crlf(raw_value: &str) -> Vec<u8> {
    // Two-pass replace to avoid expanding existing "\r\n" to "\r\r\n".
    // Step 1: collapse any stray "\r\n" already in the value back to "\n".
    let lf_only = raw_value.replace("\r\n", "\n");
    // Step 2: expand "\n" to "\r\n".
    lf_only.replace('\n', "\r\n").into_bytes()
}

/// Convert a `mime_tree::EmailAddress` (parser shape: `{name, address}`)
/// to the JMAP `EmailAddress` (`{name, email}`, RFC 8621 §4.1.2.3) and
/// then to a `serde_json::Value`.
///
/// Routing the conversion through the canonical `jmap_mail_types`
/// `EmailAddress` type guarantees the serialised wire format matches the
/// `from`/`to`/`sender`/etc. properties produced elsewhere — same field
/// names, same null-omission rules — instead of building the map by hand
/// and risking drift.
///
/// Per RFC 8621 §4.1.2.3, `addr-spec` is required (`email: "String"`);
/// `name` is `String|null`. mime-tree's parser may return
/// `address: None` for malformed input; in that case we substitute the
/// empty string so the JMAP wire-type invariant (non-nullable `email`)
/// is preserved.
fn jmap_email_address_value(addr: mime_tree::EmailAddress) -> Result<Value, JmapError> {
    let mut ea = EmailAddress::new(addr.address.unwrap_or_default());
    ea.name = addr.name;
    jmap_server::serialize_value(ea)
}

/// Convert a `mime_tree::AddressGroup` to the JMAP `EmailAddressGroup`
/// JSON shape (RFC 8621 §4.1.2.4) via the canonical wire-type so the
/// serialised shape stays aligned with the rest of the crate.
fn jmap_email_address_group_value(group: mime_tree::AddressGroup) -> Result<Value, JmapError> {
    let mut eag = EmailAddressGroup::new(
        group
            .addresses
            .into_iter()
            .map(|a| {
                let mut ea = EmailAddress::new(a.address.unwrap_or_default());
                ea.name = a.name;
                ea
            })
            .collect(),
    );
    eag.name = group.name;
    jmap_server::serialize_value(eag)
}

/// Extract header value(s) from `email_json["headers"]` for the given request.
///
/// `email_json` must be the serialised Email object as returned by the backend;
/// its `"headers"` key is an array of `{name, value}` objects.
fn extract_header_values(
    email_json: &Value,
    req: &HeaderPropertyRequest,
) -> Result<Value, JmapError> {
    Ok(match email_json.get("headers").and_then(|v| v.as_array()) {
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
                        .collect::<Result<_, _>>()?,
                )
            } else {
                // RFC 8621 §4.1.3: without :all, return the *last* matching instance.
                match matching.last() {
                    Some(v) => apply_header_form(v, &req.form)?,
                    None => Value::Null,
                }
            }
        }
    })
}

/// The partIds of the "text/*" parts in a `textBody` / `htmlBody` list:
/// RFC 8621 §4.2 fetchTextBodyValues "includes any "text/*" part in the
/// "textBody" property", and likewise for HTML (a textBody can also hold
/// an inline image).
fn text_part_ids_in(list: Option<&Value>) -> HashSet<String> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| {
            p.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.len() >= 5 && t[..5].eq_ignore_ascii_case("text/"))
        })
        .filter_map(|p| p.get("partId").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

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
    let Value::Object(map) = val else {
        return;
    };

    // Collect part ids for text and html body lists so we can filter bodyValues.
    let text_part_ids: HashSet<String> = if args.fetch_text || args.fetch_all {
        text_part_ids_in(map.get("textBody"))
    } else {
        HashSet::new()
    };
    let html_part_ids: HashSet<String> = if args.fetch_html || args.fetch_all {
        text_part_ids_in(map.get("htmlBody"))
    } else {
        HashSet::new()
    };

    // Filter bodyValues: keep only entries whose partId appears in the wanted sets.
    // When none of the three fetch flags are set, text_part_ids and html_part_ids
    // are both empty HashSets (constructed in the else branches above), so retain()
    // removes every entry — effectively clearing bodyValues. This is the correct
    // RFC 8621 §4.2 default (all flags false → no body values returned).
    if let Some(Value::Object(bv_map)) = map.get_mut("bodyValues") {
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
                    let limit = usize::try_from(args.max_bytes).unwrap_or(usize::MAX);
                    if text.len() > limit {
                        // At most `limit` octets (RFC 8621 §4.2), cut at an extended
                        // grapheme cluster boundary so no character or combining
                        // sequence is split (draft-atwood-jmap-updates-00 §6.5).
                        let truncated =
                            jmap_types::text::truncate_graphemes(&text, limit).to_owned();
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
        if let Some(Value::Array(parts)) = map.get_mut(*key) {
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
    if props.contains("subParts")
        && let Value::Object(map) = node
        && let Some(Value::Array(parts)) = map.get_mut("subParts")
    {
        for part in parts.iter_mut() {
            apply_body_properties_recursive(part, props);
        }
    }
    // Filter this node's own keys.  When subParts is not in props, the key is
    // stripped here; when it is in props the children were already filtered above.
    *node = apply_body_properties(node, props);
}

// ---------------------------------------------------------------------------
// Per-request projection
// ---------------------------------------------------------------------------

/// Removes the RFC 8621 §4.2 body arguments (`bodyProperties`,
/// `fetchTextBodyValues`, `fetchHTMLBodyValues`, `fetchAllBodyValues`,
/// `maxBodyValueBytes`) from `args`.
///
/// # Errors
///
/// `invalidArguments` when `bodyProperties` is not a string array or
/// `maxBodyValueBytes` is not a non-negative integer.
pub(crate) fn take_body_args(
    args: &mut Map<String, Value>,
) -> Result<(Vec<String>, BodyFetchArgs), JmapError> {
    let body_properties: Vec<String> = match args.remove("bodyProperties") {
        None | Some(Value::Null) => DEFAULT_BODY_PROPERTIES
            .iter()
            .map(|&s| s.to_owned())
            .collect(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| JmapError::invalid_arguments(format!("bodyProperties: {e}")))?,
    };
    let fetch_text = take_bool_arg(args, "fetchTextBodyValues", false);
    let fetch_html = take_bool_arg(args, "fetchHTMLBodyValues", false);
    let fetch_all = take_bool_arg(args, "fetchAllBodyValues", false);
    let max_bytes: u64 = match args.remove("maxBodyValueBytes") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxBodyValueBytes must be a non-negative integer")
        })?,
    };
    Ok((
        body_properties,
        BodyFetchArgs {
            fetch_text,
            fetch_html,
            fetch_all,
            max_bytes,
        },
    ))
}

/// Checks one requested Email property name: a `header:` property must be
/// well formed and use a form allowed for that field (RFC 8621 §4.1.3).
/// Other names pass; Email/get drops names it does not know.
///
/// # Errors
///
/// A description of what is wrong with the `header:` property.
#[cfg_attr(not(feature = "emailpush"), allow(dead_code))]
pub(crate) fn check_email_property(name: &str) -> Result<(), String> {
    if is_header_property(name) {
        let req = parse_header_property(name)?;
        validate_header_form(&req.name_lower, &req.form)?;
    }
    Ok(())
}

/// Whether Email/get returns property `name` (RFC 8621 §4.1): a property
/// of the default list, `headers`, `bodyStructure` or a `header:` form,
/// plus, when `metadata` (the backend lists Email for
/// draft-ietf-jmap-metadata-05), `metadata`, `privateMetadata` and their
/// `/<namespace>` subselectors. `objectHistory` is not included: the
/// history-aware Email/get removes it before this check.
pub(crate) fn is_email_get_property(name: &str, metadata: bool) -> bool {
    DEFAULT_EMAIL_GET_PROPERTIES.contains(&name)
        || matches!(name, "headers" | "bodyStructure")
        || is_header_property(name)
        || (metadata
            && ["metadata", "privateMetadata"].iter().any(|m| {
                name == *m
                    || name
                        .strip_prefix(m)
                        .is_some_and(|rest| rest.starts_with('/'))
            }))
}

/// Whether `name` is a `header:` property (RFC 8621 §4.1.3).
fn is_header_property(name: &str) -> bool {
    name.starts_with("header:") && name.len() > 7
}

/// One request's projection of Emails: the property set, the parsed
/// `header:` properties and the body arguments, built once before the
/// per-Email loop.
pub(crate) struct EmailProjection<'a> {
    /// The non-`header:` properties to keep.
    props: HashSet<&'a str>,
    /// Each `header:` property as sent, with its parsed request.
    header_reqs: Vec<(&'a str, HeaderPropertyRequest)>,
    /// `headers` was added only to extract `header:` properties, so it is
    /// removed from the result.
    headers_implicit: bool,
    /// The `bodyProperties` to keep in each `EmailBodyPart`.
    body_props: HashSet<&'a str>,
    body_fetch: BodyFetchArgs,
}

impl<'a> EmailProjection<'a> {
    /// Parses `properties` (`None` uses `defaults`) and validates each
    /// `header:` property. With `implicit_id`, `id` is always returned
    /// (RFC 8620 §5.1: "The id property of the object is always returned").
    ///
    /// # Errors
    ///
    /// `invalidArguments` naming a malformed `header:` property.
    pub(crate) fn new(
        properties: Option<&'a [String]>,
        defaults: &'static [&'static str],
        implicit_id: bool,
        body_properties: &'a [String],
        body_fetch: BodyFetchArgs,
    ) -> Result<Self, JmapError> {
        let (header_props, regular_props): (Vec<&str>, Vec<&str>) = match properties {
            Some(props) => props
                .iter()
                .map(String::as_str)
                .partition(|p| is_header_property(p)),
            None => (vec![], vec![]),
        };
        // Parse and validate each header: property before touching the
        // backend, so a bad one is invalidArguments without any storage query.
        let header_reqs = header_props
            .iter()
            .map(|p| {
                let req = parse_header_property(p)
                    .map_err(|e| JmapError::invalid_arguments(format!("property '{p}': {e}")))?;
                validate_header_form(&req.name_lower, &req.form)
                    .map_err(|e| JmapError::invalid_arguments(format!("property '{p}': {e}")))?;
                Ok((*p, req))
            })
            .collect::<Result<Vec<_>, JmapError>>()?;
        let client_wants_headers = properties.is_some_and(|p| p.iter().any(|p| p == "headers"));
        // Without this inject-then-strip, header:Name properties would
        // silently return null because the raw 'headers' array would never
        // be kept.
        let headers_implicit = !header_props.is_empty() && !client_wants_headers;
        let props: HashSet<&str> = if properties.is_none() {
            defaults.iter().copied().collect()
        } else {
            let mut set: HashSet<&str> = regular_props.into_iter().collect();
            if implicit_id {
                set.insert("id");
            }
            if headers_implicit {
                set.insert("headers");
            }
            set
        };
        Ok(Self {
            props,
            header_reqs,
            headers_implicit,
            body_props: body_properties.iter().map(String::as_str).collect(),
            body_fetch,
        })
    }

    /// Projects `email`. `extra` runs on the full serialized Email and the
    /// projected object after the plain properties are kept and before the
    /// `header:` properties are added (Email/get's metadata projection).
    ///
    /// # Errors
    ///
    /// `serverFail` if the Email does not serialize, or a `header:` value
    /// cannot be converted to its requested form.
    pub(crate) fn project(
        &self,
        email: &Email,
        extra: impl FnOnce(&Value, &mut Value),
    ) -> Result<Value, JmapError> {
        let mut val = serialize_value(email)?;
        // Body-value filtering and truncation come before property filtering.
        apply_body_value_args(&mut val, &self.body_fetch, &self.body_props);
        let mut obj = filter_properties(&val, &self.props);
        extra(&val, &mut obj);
        if !self.header_reqs.is_empty()
            && let Value::Object(ref mut map) = obj
        {
            // `val` still holds the full serialized Email.
            for (prop, req) in &self.header_reqs {
                map.insert((*prop).to_owned(), extract_header_values(&val, req)?);
            }
            if self.headers_implicit {
                map.remove("headers");
            }
        }
        Ok(obj)
    }
}

/// Projects `email` onto `properties`, as Email/get would with the default
/// `bodyProperties` and no body-value fetch arguments, except that `id` is
/// returned only when listed (draft-ietf-jmap-emailpush-05 §4: "the "id"
/// property is not included unless it is listed"). An empty list gives `{}`.
///
/// # Errors
///
/// As [`EmailProjection::new`] and [`EmailProjection::project`].
#[cfg_attr(not(feature = "emailpush"), allow(dead_code))]
pub(crate) fn project_email(
    email: &Email,
    properties: &[String],
) -> Result<Map<String, Value>, JmapError> {
    let body: Vec<String> = DEFAULT_BODY_PROPERTIES
        .iter()
        .map(|&s| s.to_owned())
        .collect();
    let projection = EmailProjection::new(
        Some(properties),
        &[],
        false,
        &body,
        BodyFetchArgs {
            fetch_text: false,
            fetch_html: false,
            fetch_all: false,
            max_bytes: 0,
        },
    )?;
    match projection.project(email, |_, _| {})? {
        Value::Object(map) => Ok(map),
        _ => Err(JmapError::server_fail(
            "Email did not serialize as an object",
        )),
    }
}

#[cfg(test)]
mod restore_crlf_tests {
    //! Tests pinning the [`restore_crlf`] defense-note claims (bd:JMAP-q2wa.12):
    //! pure-LF input expands to CRLF; CRLF input passes through unchanged;
    //! mixed input normalises to pure-CRLF; bare CR is left alone; and the
    //! function is idempotent under repeated application.

    use super::restore_crlf;

    #[test]
    fn pure_lf_input_expands_to_crlf() {
        // Independent oracle: hand-written byte sequence. RFC 5322 §2.1
        // requires CRLF line endings on the wire.
        let input = "Subject: hello\nFrom: a@b\n\nbody\n";
        let expected: &[u8] = b"Subject: hello\r\nFrom: a@b\r\n\r\nbody\r\n";
        assert_eq!(restore_crlf(input), expected);
    }

    #[test]
    fn crlf_input_passes_through_unchanged() {
        // Independent oracle: a backend that stored a Raw value with CRLF
        // line endings (e.g. mid-migration, non-conforming impl) must NOT
        // be doubled into "\r\r\n". This is the core defense claim.
        let input = "Subject: hello\r\nFrom: a@b\r\n\r\nbody\r\n";
        let expected: &[u8] = b"Subject: hello\r\nFrom: a@b\r\n\r\nbody\r\n";
        assert_eq!(restore_crlf(input), expected);
    }

    #[test]
    fn mixed_line_endings_normalise_to_crlf() {
        // Independent oracle: hand-written. A backend that stored a
        // mixed-line-ending Raw value (one line LF, one line CRLF)
        // must normalise the whole output to CRLF. The two-pass replace
        // is what makes this safe — a one-pass `\n -> \r\n` would
        // produce "\r\r\n" on the already-CRLF line.
        let input = "Header-A: lf\nHeader-B: crlf\r\nbody\n";
        let expected: &[u8] = b"Header-A: lf\r\nHeader-B: crlf\r\nbody\r\n";
        assert_eq!(restore_crlf(input), expected);
    }

    #[test]
    fn bare_cr_is_preserved_for_display_name_edge_case() {
        // Independent oracle: the docstring claims bare CR is left alone
        // because treating it as a fold separator could corrupt rare
        // CR-in-display-name cases. Pin that behaviour: a standalone CR
        // not followed by LF must survive intact.
        let input = "Subject: hello\rweird\n";
        let expected: &[u8] = b"Subject: hello\rweird\r\n";
        assert_eq!(restore_crlf(input), expected);
    }

    #[test]
    fn idempotent_under_repeated_application() {
        // Independent oracle: the docstring claims idempotence. A
        // function f is idempotent on input x iff f(f(x)) == f(x).
        // Verify across pure-LF, pure-CRLF, and mixed inputs.
        for input in [
            "Subject: hello\nFrom: a@b\n\nbody\n",
            "Subject: hello\r\nFrom: a@b\r\n\r\nbody\r\n",
            "Header-A: lf\nHeader-B: crlf\r\nbody\n",
            "Subject: hello\rweird\n",
        ] {
            let once = restore_crlf(input);
            let once_str = std::str::from_utf8(&once)
                .expect("restore_crlf output must be valid UTF-8 when input was");
            let twice = restore_crlf(once_str);
            assert_eq!(
                once, twice,
                "restore_crlf must be idempotent; input: {input:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    //! Oracles: RFC 8621 §4.1.3 header-form rules and §4.2 properties;
    //! draft-ietf-jmap-emailpush-05 §4 ("If empty, each Email object in the
    //! EmailPush is an empty object", "the "id" property is not included
    //! unless it is listed").
    use super::*;
    use serde_json::json;

    fn email() -> Email {
        serde_json::from_value(json!({
            "id": "M1", "blobId": "B1", "threadId": "T1",
            "mailboxIds": {"inbox": true}, "size": 10,
            "receivedAt": "2026-01-02T03:04:05Z",
            "from": [{"name": "John Smith", "email": "john@example.com"}],
            "subject": "Hi",
            "headers": [{"name": "Subject", "value": " Hi"}]
        }))
        .unwrap()
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn project_email_has_no_implicit_id() {
        assert_eq!(project_email(&email(), &[]).unwrap(), Map::new());
        let got = project_email(&email(), &names(&["from", "subject"])).unwrap();
        assert_eq!(
            Value::Object(got),
            json!({"from": [{"name": "John Smith", "email": "john@example.com"}],
                   "subject": "Hi"})
        );
        let got = project_email(&email(), &names(&["id"])).unwrap();
        assert_eq!(Value::Object(got), json!({"id": "M1"}));
    }

    #[test]
    fn project_email_header_forms() {
        let got = project_email(&email(), &names(&["header:Subject:asText"])).unwrap();
        assert_eq!(Value::Object(got), json!({"header:Subject:asText": "Hi"}));
    }

    #[test]
    fn check_email_property_validates_header_forms() {
        assert!(check_email_property("from").is_ok());
        assert!(check_email_property("header:Subject:asText").is_ok());
        // §4.1.2.4: Date is a date-form field, not an address list.
        assert!(check_email_property("header:Date:asAddresses").is_err());
        assert!(check_email_property("header:X:nonsense").is_err());
    }

    #[test]
    fn get_projection_keeps_implicit_id() {
        let props = names(&["subject"]);
        let body = names(&["partId"]);
        let (_, fetch) = take_body_args(&mut Map::new()).unwrap();
        let p = EmailProjection::new(
            Some(&props),
            DEFAULT_EMAIL_GET_PROPERTIES,
            true,
            &body,
            fetch,
        )
        .unwrap();
        assert_eq!(
            p.project(&email(), |_, _| {}).unwrap(),
            json!({"id": "M1", "subject": "Hi"})
        );
    }
}

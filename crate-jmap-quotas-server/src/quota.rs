//! `Quota/*` method handlers (RFC 9425 §4).
//!
//! Each handler wraps the generic RFC 8620 handler from `jmap-server` and
//! applies RFC 9425 §4.1's rule on `types`:
//!
//! - `/get` drops every type the request does not recognize from `types`,
//!   and leaves out a Quota left with none (listing it in `notFound` when
//!   the client asked for it by id);
//! - `/query` and `/queryChanges` match only Quotas with a recognized type,
//!   and evaluate a `type` condition against the cut-down `types`, so
//!   `position` and `total` count only Quotas the client can see;
//! - `/changes` is the generic handler: an id it reports for a hidden Quota
//!   comes back from `/get` in `notFound`.
//!
//! Method-level errors are those of the generic handlers: `accountNotFound`,
//! `invalidArguments`, `requestTooLarge` (`/get`), `cannotCalculateChanges`
//! and `tooManyChanges` (`/changes`, `/queryChanges`), `unsupportedFilter`,
//! `unsupportedSort` and `anchorNotFound` (`/query`), and `serverFail`.

use jmap_quotas_types::Quota;
use jmap_server::{HandlerOutput, uses_i18n};
use jmap_types::JmapError;
use jmap_types::i18n::{JMAP_I18N_URI, LanguageRange, accept_language_ranges, localized_value};
use serde_json::{Value, json};

use crate::QuotasBackend;

/// Handle `Quota/get` (RFC 9425 §4.2; RFC 8620 §5.1).
///
/// `name` and `description` are localizable (draft-atwood-jmap-updates-00
/// §5.2):
///
/// - with `urn:ietf:params:jmap:i18n` in `using`, `locale` and
///   `localizations` are returned and `language` selects a view (§4.2.1);
///   Accept-Language changes nothing;
/// - without it, `description` is the one Accept-Language
///   ([`JmapBackend::accept_language`](jmap_server::JmapBackend::accept_language))
///   selects by RFC 4647 Lookup, falling back to the stored base value, as
///   RFC 9425 §4.1 requires; `name` is the base value, and `locale`,
///   `localizations` and `language` are not available.
pub async fn handle_quota_get<B: QuotasBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let recognized = backend.recognized_types(caller, using);
    let by_id = args.get("ids").is_some_and(|v| !v.is_null());

    // RFC 9425 §4.1 selection by Accept-Language, only without i18n.
    let ranges: Vec<LanguageRange> = if uses_i18n(using) {
        Vec::new()
    } else {
        if args.get("language").is_some_and(|v| !v.is_null()) {
            return Err(JmapError::invalid_arguments(
                "language requires urn:ietf:params:jmap:i18n in using",
            ));
        }
        B::accept_language(caller)
            .map(accept_language_ranges)
            .unwrap_or_default()
    };
    let by_accept_language = !ranges.is_empty();

    // `types` is needed to decide what to drop, and `localizations` to
    // select `description`; fetch them and strip them again.
    let mut strip_types = false;
    if let Some(Value::Array(props)) = args.get_mut("properties") {
        if !props.iter().any(|p| p == "types") {
            props.push(json!("types"));
            strip_types = true;
        }
        if by_accept_language && props.iter().any(|p| p == "description") {
            props.extend([json!("locale"), json!("localizations")]);
        }
    }
    let mut out = if by_accept_language {
        // Read the stored localizations through the i18n path, which a
        // `using` without the capability would strip.
        let mut with_i18n = using.to_vec();
        with_i18n.push(JMAP_I18N_URI.to_owned());
        jmap_server::handle_get_with_metadata::<Quota, B>(backend, caller, args, &with_i18n).await?
    } else {
        jmap_server::handle_get_with_metadata::<Quota, B>(backend, caller, args, using).await?
    };

    let mut hidden: Vec<Value> = Vec::new();
    if let Some(Value::Array(list)) = out.primary.get_mut("list") {
        list.retain_mut(|q| {
            let visible = visible_types(q.get("types"), &recognized);
            if visible.is_empty() {
                if by_id && let Some(id) = q.get("id") {
                    hidden.push(id.clone());
                }
                return false;
            }
            if let Some(obj) = q.as_object_mut() {
                if strip_types {
                    obj.remove("types");
                } else {
                    obj.insert("types".to_owned(), json!(visible));
                }
                if by_accept_language {
                    if let Some(d) = localized_value(obj, "description", &ranges) {
                        obj.insert("description".to_owned(), d);
                    }
                    obj.remove("locale");
                    obj.remove("localizations");
                }
            }
            true
        });
    }
    if !hidden.is_empty() {
        match out.primary.get_mut("notFound") {
            Some(Value::Array(nf)) => nf.extend(hidden),
            _ => out.primary["notFound"] = Value::Array(hidden),
        }
    }
    Ok(out)
}

/// The entries of a Quota's `types` that are in `recognized`.
fn visible_types(types: Option<&Value>, recognized: &[String]) -> Vec<String> {
    types
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|t| recognized.iter().any(|r| r == t))
        .map(str::to_owned)
        .collect()
}

/// Handle `Quota/changes` (RFC 9425 §4.3; RFC 8620 §5.2).
///
/// `updatedProperties` is the backend's
/// [`ChangesResult::updated_properties`](jmap_server::ChangesResult::updated_properties):
/// `["used"]` when only usage changed.
pub async fn handle_quota_changes<B: QuotasBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Quota, B>(backend, caller, args, using).await
}

/// Handle `Quota/query` (RFC 9425 §4.4; RFC 8620 §5.5).
///
/// With `urn:ietf:params:jmap:i18n` in `using`, `language` makes the
/// `name` condition and sort use the localized `name`
/// (draft-atwood-jmap-updates-00 §4.2.2); Accept-Language never does.
pub async fn handle_quota_query<B: QuotasBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let args = restrict_filter(args, &backend.recognized_types(caller, using));
    jmap_server::handle_query_with_i18n::<Quota, B>(backend, caller, args, using).await
}

/// Handle `Quota/queryChanges` (RFC 9425 §4.5; RFC 8620 §5.6).
///
/// The filter is restricted exactly as in [`handle_quota_query`], so the
/// changes are to the query the client actually ran.
pub async fn handle_quota_query_changes<B: QuotasBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let args = restrict_filter(args, &backend.recognized_types(caller, using));
    jmap_server::handle_query_changes_with_i18n::<Quota, B>(backend, caller, args, using).await
}

/// A filter no Quota matches: an `OR` of nothing (RFC 8620 §5.5: "at least
/// one of the conditions must match").
fn never() -> Value {
    json!({ "operator": "OR", "conditions": [] })
}

/// Rewrite `args.filter` to `AND(client filter, OR(type = t for each
/// recognized t))`, with each client condition on an unrecognized `type`
/// replaced by [`never`]: the client cannot see that type in `types`.
fn restrict_filter(mut args: Value, recognized: &[String]) -> Value {
    let visible = json!({
        "operator": "OR",
        "conditions": recognized.iter().map(|t| json!({ "type": t })).collect::<Vec<_>>(),
    });
    let Some(obj) = args.as_object_mut() else {
        // Not an object: the generic handler rejects it.
        return args;
    };
    let filter = match obj.remove("filter") {
        None | Some(Value::Null) => visible,
        Some(mut client) => {
            mask_unrecognized_types(&mut client, recognized);
            json!({ "operator": "AND", "conditions": [client, visible] })
        }
    };
    obj.insert("filter".to_owned(), filter);
    args
}

fn mask_unrecognized_types(filter: &mut Value, recognized: &[String]) {
    let Some(obj) = filter.as_object_mut() else {
        return;
    };
    if obj.contains_key("operator") {
        if let Some(Value::Array(conditions)) = obj.get_mut("conditions") {
            for c in conditions {
                mask_unrecognized_types(c, recognized);
            }
        }
    } else if let Some(Value::String(t)) = obj.get("type")
        && !recognized.iter().any(|r| r == t)
    {
        *filter = never();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(types: &[&str]) -> Vec<String> {
        types.iter().map(|&t| t.to_owned()).collect()
    }

    #[test]
    fn no_client_filter_becomes_the_visible_or() {
        let out = restrict_filter(json!({ "accountId": "a" }), &rec(&["Email"]));
        assert_eq!(
            out["filter"],
            json!({ "operator": "OR", "conditions": [{ "type": "Email" }] })
        );
    }

    #[test]
    fn unrecognized_type_conditions_never_match() {
        let client = json!({ "operator": "NOT", "conditions": [
            { "type": "SieveScript", "scope": "account" }, { "type": "Email" }
        ] });
        let out = restrict_filter(json!({ "filter": client }), &rec(&["Email"]));
        assert_eq!(
            out["filter"]["conditions"][0],
            json!({ "operator": "NOT", "conditions": [never(), { "type": "Email" }] })
        );
    }
}

//! Request parsing and ResultReference resolution (RFC 8620 §3.3, §3.7).

use crate::{Invocation, JmapError, JmapRequest, ResultReference};
use serde_json::Value;

/// Parse and validate a JMAP request from a raw JSON value.
///
/// Validates:
/// - The body deserializes as a [`JmapRequest`].
/// - `using` is non-empty (RFC 8620 §3.3).
/// - The number of method calls does not exceed `max_calls` (RFC 8620 §3.3).
///
/// Capability URI checking is NOT performed here — that is the caller's
/// responsibility.
///
/// # Caller responsibility: `notJSON`
///
/// This function takes a pre-parsed [`serde_json::Value`], not raw bytes.  The
/// caller is responsible for the initial JSON parse of the HTTP request body.
/// If that parse fails (the body is not valid JSON), the caller must produce the
/// `notJSON` error response itself — [`crate::error_invocation`] and
/// [`crate::request_error`] with [`JmapError::not_json()`] handle that case.
/// `parse_request` only validates the JMAP structure of an already-parsed value.
///
/// # Errors
///
/// Returns [`JmapError::not_request()`] if the value does not match the
/// `JmapRequest` schema or if `using` is empty.  Returns
/// [`JmapError::limit("maxCallsInRequest")`][JmapError::limit] if the method
/// call count exceeds `max_calls`.
pub fn parse_request(body: Value, max_calls: usize) -> Result<JmapRequest, JmapError> {
    let req: JmapRequest = serde_json::from_value(body).map_err(|_| JmapError::not_request())?;

    if req.using.is_empty() {
        return Err(JmapError::not_request());
    }

    if req.method_calls.len() > max_calls {
        return Err(JmapError::limit("maxCallsInRequest"));
    }

    Ok(req)
}

/// Resolve all `#key` ResultReference fields in `args` against `prior_responses`.
///
/// For every key in `args` that starts with `#`:
/// 1. Parse the value as a [`ResultReference`].
/// 2. Find the prior response whose call-id matches `rr.result_of` (index 2 of tuple).
/// 3. Verify `rr.name` matches the method name of that response (index 0 of tuple).
/// 4. Apply `rr.path` as an RFC 6901 JSON Pointer (with RFC 8620 §3.7 `*` extension)
///    to the response args (index 1 of tuple).
/// 5. Collect `(plain_key, resolved_value)` pairs.
///
/// This is two-phase atomic: `args` is not modified at all unless every
/// `#key` resolves successfully.  If any resolution fails, `args` is returned
/// unchanged and an error is returned.
///
/// `prior_responses` entries are `(method_name, response_args, call_id)` — same
/// layout as [`Invocation`].
pub fn resolve_args(args: &mut Value, prior_responses: &[Invocation]) -> Result<(), JmapError> {
    let Some(obj) = args.as_object_mut() else {
        return Ok(()); // non-object args cannot contain #-key references
    };

    // Collect (#key, value) pairs up front; cannot borrow obj mutably while iterating.
    // obj.len() is an upper bound (not all keys need the # prefix).
    let mut ref_pairs: Vec<(String, Value)> = Vec::with_capacity(obj.len());
    ref_pairs.extend(
        obj.iter()
            .filter(|(k, _)| k.starts_with('#'))
            .map(|(k, v)| (k.clone(), v.clone())),
    );

    if ref_pairs.is_empty() {
        return Ok(());
    }

    // Phase 1: resolve every reference read-only; args are not touched yet.
    // If any step fails, return the error immediately without modifying args.
    let mut resolutions: Vec<(String, String, Value)> = Vec::with_capacity(ref_pairs.len());

    for (ref_key, ref_value) in ref_pairs {
        let plain_key = ref_key[1..].to_owned();

        // Parse the value as a ResultReference.
        let rr: ResultReference = serde_json::from_value(ref_value).map_err(|e| {
            JmapError::invalid_arguments(format!("invalid ResultReference for #{plain_key}: {e}"))
        })?;

        // Find the prior response by call-id (index 2 of the Invocation tuple).
        let (prior_method, prior_value) = prior_responses
            .iter()
            .find(|(_, _, call_id)| call_id == &rr.result_of)
            .map(|(method, value, _)| (method.as_str(), value))
            .ok_or_else(JmapError::invalid_result_reference)?;

        // Verify the name field matches the method name (RFC 8620 §3.7).
        if rr.name != prior_method {
            return Err(JmapError::invalid_result_reference());
        }

        // Apply the RFC 6901 JSON Pointer path with RFC 8620 §3.7 `*` wildcard.
        let resolved = json_pointer_ext(prior_value, &rr.path)
            .ok_or_else(JmapError::invalid_result_reference)?;

        // Check for key conflict: plain_key must not already exist in args.
        if obj.contains_key(&plain_key) {
            return Err(JmapError::invalid_arguments(format!(
                "argument key conflict: '{}' and '#{}' both present",
                plain_key, plain_key
            )));
        }

        resolutions.push((ref_key, plain_key, resolved));
    }

    // Phase 2: all resolutions succeeded — apply mutations atomically.
    for (ref_key, plain_key, resolved) in resolutions {
        obj.remove(&ref_key);
        obj.insert(plain_key, resolved);
    }

    Ok(())
}

/// Apply a path to a JSON value, supporting the RFC 8620 §3.7 `*` wildcard extension.
///
/// This is RFC 6901 JSON Pointer extended with `*` as an array-map operator.
/// When the current value is an array and the token is `*`, the remaining tokens
/// are applied to each element; array results are flattened into the output.
fn json_pointer_ext(value: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return Some(value.clone());
    }
    if !path.starts_with('/') {
        return None;
    }

    // Split off the first token.
    let after_slash = &path[1..];
    let (token, remaining) = match after_slash.find('/') {
        Some(pos) => (&after_slash[..pos], &after_slash[pos..]),
        None => (after_slash, ""),
    };

    if token == "*" {
        // RFC 8620 §3.7 wildcard: map over array, flatten array results.
        let arr = value.as_array()?;
        let mut result: Vec<Value> = Vec::new();
        for item in arr {
            match json_pointer_ext(item, remaining) {
                Some(Value::Array(inner)) => result.extend(inner),
                Some(other) => result.push(other),
                None => return None, // any failure = whole resolution fails
            }
        }
        Some(Value::Array(result))
    } else {
        // RFC 6901: unescape ~1 → /, ~0 → ~ (in that order).
        // Skip allocation when the token contains no ~ characters (common case).
        let key: std::borrow::Cow<str> = if token.contains('~') {
            token.replace("~1", "/").replace("~0", "~").into()
        } else {
            token.into()
        };
        let next = match value {
            Value::Object(obj) => obj.get(key.as_ref())?,
            Value::Array(arr) => {
                // RFC 6901 §4: leading zeros are not allowed in array index tokens.
                if key.len() > 1 && key.starts_with('0') {
                    return None;
                }
                let idx: usize = key.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
        json_pointer_ext(next, remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Oracle: RFC 8620 §3 (request format), §7.1 (error type strings).

    #[test]
    fn parse_request_valid() {
        let body = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["Foo/get", {"accountId": "a1"}, "0"]
            ]
        });
        let req = parse_request(body, 16).expect("valid request must parse");
        assert_eq!(req.using, vec!["urn:ietf:params:jmap:core"]);
        assert_eq!(req.method_calls.len(), 1);
    }

    #[test]
    fn parse_request_empty_using_is_error() {
        let body = json!({
            "using": [],
            "methodCalls": []
        });
        let err = parse_request(body, 16).unwrap_err();
        assert_eq!(
            err.error_type, "notRequest",
            "empty using violates request structure — must be notRequest per RFC 8620 §3.6.1"
        );
    }

    #[test]
    fn parse_request_too_many_calls() {
        let call = json!(["Foo/get", {}, "0"]);
        let calls: Vec<_> = (0..5).map(|_| call.clone()).collect();
        let body = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": calls
        });
        let err = parse_request(body, 4).unwrap_err();
        assert_eq!(
            err.error_type, "limit",
            "exceeding maxCallsInRequest must return limit per RFC 8620 §3.6.1"
        );
    }

    #[test]
    fn parse_request_at_max_calls_is_ok() {
        let call = json!(["Foo/get", {}, "0"]);
        let calls: Vec<_> = (0..4).map(|_| call.clone()).collect();
        let body = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": calls
        });
        parse_request(body, 4).expect("exactly max_calls must be accepted");
    }

    #[test]
    fn parse_request_malformed_body() {
        let body = json!("not an object");
        let err = parse_request(body, 16).unwrap_err();
        assert_eq!(
            err.error_type, "notRequest",
            "malformed body does not match Request type — must be notRequest per RFC 8620 §3.6.1"
        );
    }

    // Oracle: RFC 8620 §3.7 — #ids resolves to prior response's value at path.
    #[test]
    fn resolve_args_basic() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"list": [{"id": "x1"}], "state": "s0"}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/list/0/id"}
        });
        resolve_args(&mut args, &prior).expect("must resolve");
        assert_eq!(args, json!({"ids": "x1"}));
    }

    // Oracle: RFC 8620 §3.7 — unknown resultOf → invalidResultReference.
    #[test]
    fn resolve_args_unknown_result_of() {
        let prior: Vec<Invocation> = vec![];
        let mut args = json!({
            "#ids": {"resultOf": "missing", "name": "Foo/get", "path": "/ids"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidResultReference");
        // args must be unchanged on error (atomicity).
        assert_eq!(args, original);
    }

    // Oracle: RFC 8620 §3.7 — name mismatch → invalidResultReference.
    #[test]
    fn resolve_args_name_mismatch() {
        let prior = vec![("Foo/get".to_owned(), json!({"ids": ["a"]}), "c0".to_owned())];
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "Bar/get", "path": "/ids"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidResultReference");
        assert_eq!(args, original);
    }

    // Oracle: RFC 8620 §3.7 — path not found → invalidResultReference.
    #[test]
    fn resolve_args_path_not_found() {
        let prior = vec![("Foo/get".to_owned(), json!({"ids": ["a"]}), "c0".to_owned())];
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/nonexistent"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidResultReference");
        assert_eq!(args, original);
    }

    // Oracle: atomicity — if one of two refs fails, args must be completely unchanged.
    #[test]
    fn resolve_args_atomic_on_partial_failure() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"ids": ["a", "b"]}),
            "c0".to_owned(),
        )];
        // #ids is valid; #properties references a non-existent call.
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/ids"},
            "#properties": {"resultOf": "missing", "name": "Foo/get", "path": "/props"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidResultReference");
        assert_eq!(args, original);
    }

    // Oracle: non-object args pass through unchanged.
    #[test]
    fn resolve_args_non_object_passthrough() {
        let prior: Vec<Invocation> = vec![];
        let mut args = json!("not-an-object");
        resolve_args(&mut args, &prior).expect("non-object must not error");
        assert_eq!(args, json!("not-an-object"));
    }

    // Oracle: no #-prefixed keys → args unchanged, Ok returned.
    #[test]
    fn resolve_args_no_ref_keys() {
        let prior: Vec<Invocation> = vec![];
        let mut args = json!({"ids": ["a", "b"]});
        resolve_args(&mut args, &prior).expect("no ref keys must not error");
        assert_eq!(args, json!({"ids": ["a", "b"]}));
    }

    // Oracle: kith-jmap deviation — unknown capability URIs are silently accepted
    // at this layer; capability checking is the caller's responsibility.
    #[test]
    fn parse_request_unknown_capability_accepted() {
        let body = json!({
            "using": ["urn:ietf:params:jmap:core", "urn:example:unknown"],
            "methodCalls": [
                ["Foo/get", {}, "0"]
            ]
        });
        let req = parse_request(body, 16).expect("unknown capability must be accepted");
        assert_eq!(req.using.len(), 2);
    }

    // Oracle: RFC 8620 §3.3 — `using` is valid with any non-empty array.
    #[test]
    fn parse_request_core_only_accepted() {
        let body = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["Foo/get", {}, "0"]
            ]
        });
        parse_request(body, 16).expect("core-only using must be accepted");
    }

    // Oracle: boundary condition — max_calls=0, one call → limit (RFC 8620 §3.6.1).
    #[test]
    fn parse_request_zero_max_calls_rejects_any_call() {
        let body = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["Foo/get", {}, "0"]
            ]
        });
        let err = parse_request(body, 0).unwrap_err();
        assert_eq!(
            err.error_type, "limit",
            "zero max_calls means any call exceeds limit — must be limit per RFC 8620 §3.6.1"
        );
    }

    // Oracle: RFC 8620 §3.7 — multiple #-keys in the same args object all resolve
    // independently against the same prior response.
    #[test]
    fn resolve_args_multiple_refs_all_resolve() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"list": [{"id": "x1"}], "state": "s0"}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#ids":   {"resultOf": "c0", "name": "Foo/get", "path": "/list"},
            "#state": {"resultOf": "c0", "name": "Foo/get", "path": "/state"}
        });
        resolve_args(&mut args, &prior).expect("both refs must resolve");
        // No #-keys must remain.
        let obj = args.as_object().expect("must still be an object");
        assert!(!obj.contains_key("#ids"), "#ids must be removed");
        assert!(!obj.contains_key("#state"), "#state must be removed");
        assert_eq!(args["ids"], json!([{"id": "x1"}]));
        assert_eq!(args["state"], json!("s0"));
    }

    // Oracle: RFC 8620 §3.7 — having both `key` and `#key` in the same args
    // object is an error (key conflict).
    #[test]
    fn resolve_args_key_conflict_is_error() {
        let prior = vec![("Foo/get".to_owned(), json!({"ids": ["a"]}), "c0".to_owned())];
        let mut args = json!({
            "ids":  "existing",
            "#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/ids"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidArguments");
        // args must be completely unchanged on error (atomicity).
        assert_eq!(args, original);
    }

    // Oracle: RFC 8620 §3.7 — `#key` value must be a valid ResultReference object;
    // a non-object value is rejected with invalidArguments.
    #[test]
    fn resolve_args_invalid_ref_value_is_error() {
        let prior: Vec<Invocation> = vec![];
        let mut args = json!({"#ids": "not-an-object"});
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidArguments");
        assert_eq!(args, original);
    }

    // Oracle: RFC 8620 §3.7, JSON Pointer RFC 6901 §4 — path pointing to an
    // array resolves to that array value.
    #[test]
    fn resolve_args_array_path_resolves_to_array() {
        let prior = vec![(
            "List/query".to_owned(),
            json!({"ids": ["a", "b", "c"]}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "List/query", "path": "/ids"}
        });
        resolve_args(&mut args, &prior).expect("array path must resolve");
        assert_eq!(args, json!({"ids": ["a", "b", "c"]}));
    }

    // Oracle: RFC 8620 §3.7, JSON Pointer RFC 6901 §4 — multi-segment path
    // drills into nested structures.
    #[test]
    fn resolve_args_nested_path_resolves() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"list": [{"id": "deep1"}]}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#id": {"resultOf": "c0", "name": "Foo/get", "path": "/list/0/id"}
        });
        resolve_args(&mut args, &prior).expect("nested path must resolve");
        assert_eq!(args, json!({"id": "deep1"}));
    }

    // Oracle: RFC 6901 §7 — an array index that is out of bounds causes the
    // pointer to fail, which maps to invalidResultReference.
    #[test]
    fn resolve_args_path_array_oob_is_error() {
        let prior = vec![("Foo/get".to_owned(), json!({"ids": ["a"]}), "c0".to_owned())];
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/ids/5"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidResultReference");
        assert_eq!(args, original);
    }

    // Oracle: RFC 6901 §4 — array index tokens with a leading zero (other than
    // the single character "0") MUST be rejected as invalid.
    #[test]
    fn resolve_args_path_leading_zero_index_is_error() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"ids": ["a", "b"]}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#ids": {"resultOf": "c0", "name": "Foo/get", "path": "/ids/01"}
        });
        let original = args.clone();
        let err = resolve_args(&mut args, &prior).unwrap_err();
        assert_eq!(err.error_type, "invalidResultReference");
        assert_eq!(args, original, "args must be unchanged on error");
    }

    // Oracle: RFC 6901 §3 — `~1` is the escape sequence for `/` and `~0` for `~`
    // in JSON Pointer tokens.
    #[test]
    fn resolve_args_path_tilde_escaping() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"a/b": "slash-value"}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#val": {"resultOf": "c0", "name": "Foo/get", "path": "/a~1b"}
        });
        resolve_args(&mut args, &prior).expect("tilde-escaped path must resolve");
        assert_eq!(args, json!({"val": "slash-value"}));
    }

    // Oracle: RFC 6901 §3 — `~0` is the escape sequence for `~`.
    // Replacement order must be ~1 first then ~0; otherwise `~01` would
    // incorrectly become `/` instead of `~1`.
    #[test]
    fn resolve_args_path_tilde0_escaping() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"a~b": "tilde-value"}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#val": {"resultOf": "c0", "name": "Foo/get", "path": "/a~0b"}
        });
        resolve_args(&mut args, &prior).expect("~0-escaped path must resolve");
        assert_eq!(args, json!({"val": "tilde-value"}));
    }

    // Oracle: RFC 6901 §3 — `~01` must decode to the literal string `~1`,
    // NOT to `/`. ~1 is replaced first (yielding `~1`), then ~0 on what
    // remains would replace `~0` — but after the first pass `~01` → `~1`
    // there is no `~0` left; the result is `/`.
    // Wait — `~01`: replace ~1 first: `~01` has no `~1` at position 0 (it's `~0` then `1`).
    // So `~01` → replace ~1 → no match → `~01` → replace ~0 → `~` → result: `~1`.
    // i.e. `~01` decodes to `~1` (literal tilde followed by 1), NOT to `/`.
    #[test]
    fn resolve_args_path_tilde01_decodes_to_tilde1() {
        let prior = vec![(
            "Foo/get".to_owned(),
            json!({"~1": "tilde-one-value"}),
            "c0".to_owned(),
        )];
        let mut args = json!({
            "#val": {"resultOf": "c0", "name": "Foo/get", "path": "/~01"}
        });
        resolve_args(&mut args, &prior).expect("~01 must decode to literal key ~1");
        assert_eq!(args, json!({"val": "tilde-one-value"}));
    }

    // Oracle: RFC 8620 §3.7 — /list/*/threadId maps threadId from each list element.
    #[test]
    fn resolve_args_wildcard_maps_over_array() {
        let prior = vec![(
            "Thread/get".to_owned(),
            json!({
                "list": [{"threadId": "t1"}, {"threadId": "t2"}]
            }),
            "c0".to_owned(),
        )];
        let mut args =
            json!({"#ids": {"resultOf": "c0", "name": "Thread/get", "path": "/list/*/threadId"}});
        resolve_args(&mut args, &prior).expect("wildcard must resolve");
        assert_eq!(args, json!({"ids": ["t1", "t2"]}));
    }

    // Oracle: RFC 8620 §3.7 — when wildcard result is an array, it is flattened.
    #[test]
    fn resolve_args_wildcard_flattens_array_results() {
        let prior = vec![(
            "Email/get".to_owned(),
            json!({
                "list": [{"emailIds": ["e1", "e2"]}, {"emailIds": ["e3"]}]
            }),
            "c0".to_owned(),
        )];
        let mut args =
            json!({"#ids": {"resultOf": "c0", "name": "Email/get", "path": "/list/*/emailIds"}});
        resolve_args(&mut args, &prior).expect("wildcard flatten must resolve");
        assert_eq!(args, json!({"ids": ["e1", "e2", "e3"]}));
    }

    // Oracle: RFC 6901 §4 — basic path navigation.
    #[test]
    fn json_pointer_ext_plain_path() {
        let v = json!({"a": {"b": 42}});
        assert_eq!(json_pointer_ext(&v, "/a/b"), Some(json!(42)));
    }

    // Oracle: RFC 6901 §4 — empty path returns whole document.
    #[test]
    fn json_pointer_ext_empty_path_returns_root() {
        let v = json!({"x": 1});
        assert_eq!(json_pointer_ext(&v, ""), Some(v.clone()));
    }
}

//! draft-ietf-jmap-metadata-04 §3.6 — metadata filter predicate helpers.
//!
//! Provides three composable predicates that extension-server
//! `MemoryBackend`s call from their `query_objects` matchers to evaluate
//! the six metadata filter conditions:
//!
//! | FilterCondition field          | Helper                                            |
//! |-------------------------------|---------------------------------------------------|
//! | `metadataExists`              | [`metadata_path_exists`]`(&obj.metadata, path)`   |
//! | `privateMetadataExists`       | [`metadata_path_exists`]`(&obj.private, path)`    |
//! | `metadataTextContains`        | [`metadata_text_contains`]`(&obj.metadata, m)`    |
//! | `privateMetadataTextContains` | [`metadata_text_contains`]`(&obj.private, m)`     |
//! | `metadataTextEquals`          | [`metadata_text_equals`]`(&obj.metadata, m)`      |
//! | `privateMetadataTextEquals`   | [`metadata_text_equals`]`(&obj.private, m)`       |
//!
//! The caller extracts the `metadata` or `privateMetadata` field from the
//! object's JSON representation as a `serde_json::Map` and passes it to
//! the appropriate helper. The "private vs shared" distinction is the
//! caller's responsibility — these helpers are agnostic to it.
//!
//! The helpers evaluate the stored data whether or not the namespace is
//! supported on the data type (-04 §3.6: "Conditions are evaluated
//! against the stored data whether or not the namespace is supported on
//! the data type"). A path naming an absent namespace, or one that is not
//! a valid namespace identifier, matches nothing.
//!
//! A path that is not well formed MUST be rejected with an
//! `invalidArguments` error (-04 §3.6). The predicates return `false` for
//! such a path; the `/query` handler checks every condition path with
//! [`is_well_formed_metadata_path`] first and rejects the request.

use std::borrow::Cow;

use icu_casemap::CaseMapper;
use icu_normalizer::DecomposingNormalizerBorrowed;
use jmap_metadata_types::MetadataTextMatch;
use serde_json::{Map, Value};

/// Unescape a single RFC 6901 JSON Pointer reference token.
///
/// RFC 6901 §3 allows `~` only as `~0` (for `~`) or `~1` (for `/`).
/// Decoding in one pass gives the §4 result that `~01` becomes `~1`,
/// not `/`. Returns [`None`] for any other `~`.
fn unescape_rfc6901(token: &str) -> Option<Cow<'_, str>> {
    if !token.contains('~') {
        return Some(Cow::Borrowed(token));
    }
    let mut out = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            match chars.next() {
                Some('0') => out.push('~'),
                Some('1') => out.push('/'),
                _ => return None,
            }
        } else {
            out.push(c);
        }
    }
    Some(Cow::Owned(out))
}

/// Check whether `path` is a well-formed metadata path
/// (draft-ietf-jmap-metadata-04 §3.6).
///
/// -04 §3.6: "A path that is not well formed (for example, one with an
/// invalid ~ escape) MUST be rejected with an "invalidArguments" error."
/// A path is well formed when every `/`-separated token uses `~` only in
/// the RFC 6901 escapes `~0` and `~1`. Whether the first token is a valid
/// namespace identifier is not checked: -04 §3.6 says such a condition
/// "matches no objects; it does not produce an error."
///
/// A `/query` handler calls this for the path of every metadata
/// condition and, on `false`, rejects the request with `invalidArguments`.
///
/// # Examples
///
/// ```
/// use jmap_metadata_server::is_well_formed_metadata_path;
///
/// assert!(is_well_formed_metadata_path("acme.example.com/a~1b/c~0d"));
/// assert!(!is_well_formed_metadata_path("acme.example.com/a~2b"));
/// assert!(!is_well_formed_metadata_path("acme.example.com/a~"));
/// ```
pub fn is_well_formed_metadata_path(path: &str) -> bool {
    path.split('/')
        .all(|token| unescape_rfc6901(token).is_some())
}

/// Resolve a metadata path to a value within a metadata map.
///
/// Path format (-04 §3.6): a namespace identifier, optionally followed by
/// a JSON Pointer ([RFC 6901](https://www.rfc-editor.org/rfc/rfc6901))
/// into the namespace value, such as `<namespace>/<key>/<subkey>`.
///
/// Returns [`None`] if the namespace key is absent, if a later token is
/// not a member of the object reached so far, or if the path would
/// traverse an array or a scalar (-04 §3.6: "a path that would traverse
/// an array or a scalar addresses no value"). Also returns [`None`] for a
/// path that is not well formed.
fn resolve_path<'a>(map: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut tokens = path.split('/');
    let ns = unescape_rfc6901(tokens.next()?)?;
    let mut value = map.get(ns.as_ref())?;
    for token in tokens {
        value = value.as_object()?.get(unescape_rfc6901(token)?.as_ref())?;
    }
    Some(value)
}

/// Check whether a value exists at `path` in a metadata map
/// (draft-ietf-jmap-metadata-04 §3.6, `metadataExists` /
/// `privateMetadataExists`).
///
/// A namespace-only path matches if the namespace key is present **and**
/// its value is not the empty object `{}`. A longer path matches if every
/// token after the namespace names a member of the object reached so far.
/// Returns `false` for a path that is not well formed; see
/// [`is_well_formed_metadata_path`].
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use jmap_metadata_server::metadata_path_exists;
///
/// let map = json!({"photography": {"album": "sunset"}})
///     .as_object().unwrap().clone();
///
/// assert!(metadata_path_exists(&map, "photography"));
/// assert!(metadata_path_exists(&map, "photography/album"));
/// assert!(!metadata_path_exists(&map, "photography/album/deeper"));
/// assert!(!metadata_path_exists(&map, "photography/missing"));
/// assert!(!metadata_path_exists(&map, "absent"));
/// ```
pub fn metadata_path_exists(map: &Map<String, Value>, path: &str) -> bool {
    match resolve_path(map, path) {
        // Namespace-only: present and not the empty object.
        Some(Value::Object(obj)) if !path.contains('/') => !obj.is_empty(),
        value => value.is_some(),
    }
}

/// Check whether the string at `path` contains `text_match.value` as a
/// substring under the `i;unicode-casemap` collation
/// ([RFC 5051](https://www.rfc-editor.org/rfc/rfc5051))
/// (draft-ietf-jmap-metadata-04 §3.6, `metadataTextContains` /
/// `privateMetadataTextContains`).
///
/// Both strings are prepared as RFC 5051 §2 describes, then compared as
/// octets: each code point is replaced by its UnicodeData titlecase
/// mapping, then by its full compatibility decomposition. Characters are
/// not re-cased after decomposition, so U+FB01 LATIN SMALL LIGATURE FI
/// prepares to lowercase `fi` and does not contain `fi`, which prepares
/// to `FI`.
///
/// Returns `false` if the path does not exist or the value at the path
/// is not a string.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use jmap_metadata_types::MetadataTextMatch;
/// use jmap_metadata_server::metadata_text_contains;
///
/// let map = json!({"ns": {"memo": "Follow Up Soon"}})
///     .as_object().unwrap().clone();
/// let m: MetadataTextMatch = serde_json::from_value(json!({
///     "path": "ns/memo", "value": "follow up"
/// })).unwrap();
///
/// assert!(metadata_text_contains(&map, &m));
/// ```
pub fn metadata_text_contains(map: &Map<String, Value>, text_match: &MetadataTextMatch) -> bool {
    match resolve_path(map, &text_match.path) {
        Some(Value::String(s)) => casemap_prepare(s).contains(&casemap_prepare(&text_match.value)),
        _ => false,
    }
}

/// RFC 5051 §2 preparation: the "titlecased canonicalized UTF-8" form.
///
/// Step (2)(a) is the simple titlecase mapping; step (2)(b) decomposes
/// each code point on its own. NFKD of the whole string would also
/// reorder combining marks across code points, which RFC 5051 does not.
fn casemap_prepare(s: &str) -> String {
    let case = CaseMapper::new();
    let nfkd = DecomposingNormalizerBorrowed::new_nfkd();
    s.chars()
        .flat_map(|c| nfkd.normalize_iter(std::iter::once(case.simple_titlecase(c))))
        .collect()
}

/// Check whether the string at `path` is exactly equal to
/// `text_match.value`, comparing Unicode code points without case folding
/// or normalization (draft-ietf-jmap-metadata-04 §3.6,
/// `metadataTextEquals` / `privateMetadataTextEquals`).
///
/// Returns `false` if the path does not exist or the value at the path
/// is not a string.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use jmap_metadata_types::MetadataTextMatch;
/// use jmap_metadata_server::metadata_text_equals;
///
/// let map = json!({"ns": {"tag": "important"}})
///     .as_object().unwrap().clone();
/// let m: MetadataTextMatch = serde_json::from_value(json!({
///     "path": "ns/tag", "value": "important"
/// })).unwrap();
///
/// assert!(metadata_text_equals(&map, &m));
/// ```
pub fn metadata_text_equals(map: &Map<String, Value>, text_match: &MetadataTextMatch) -> bool {
    match resolve_path(map, &text_match.path) {
        Some(Value::String(s)) => s == &text_match.value,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- unescape_rfc6901 -------------------------------------------------

    #[test]
    fn unescape_no_tilde() {
        assert_eq!(
            unescape_rfc6901("photography").as_deref(),
            Some("photography")
        );
    }

    #[test]
    fn unescape_tilde_zero() {
        assert_eq!(unescape_rfc6901("a~0b").as_deref(), Some("a~b"));
    }

    #[test]
    fn unescape_tilde_one() {
        assert_eq!(unescape_rfc6901("a~1b").as_deref(), Some("a/b"));
    }

    #[test]
    fn unescape_order_matters_tilde_zero_one() {
        // ~01 must become ~1, NOT /
        assert_eq!(unescape_rfc6901("~01").as_deref(), Some("~1"));
    }

    #[test]
    fn unescape_rejects_invalid_escape() {
        // RFC 6901 §3: escaped = "~" ( "0" / "1" ).
        assert_eq!(unescape_rfc6901("a~2b"), None);
        assert_eq!(unescape_rfc6901("a~"), None);
        assert_eq!(unescape_rfc6901("~~0"), None);
    }

    // -- is_well_formed_metadata_path -------------------------------------

    #[test]
    fn well_formed_paths() {
        // -04 §3.6 example forms: <namespace>, <namespace>/<key>,
        // <namespace>/<key>/<subkey>.
        assert!(is_well_formed_metadata_path("ns"));
        assert!(is_well_formed_metadata_path("ns/key"));
        assert!(is_well_formed_metadata_path("ns/key/subkey"));
        assert!(is_well_formed_metadata_path("ns/a~0b/c~1d"));
        // RFC 6901 §3: an empty reference token is valid.
        assert!(is_well_formed_metadata_path("ns/"));
    }

    #[test]
    fn invalid_tilde_escape_is_not_well_formed() {
        // -04 §3.6: "one with an invalid ~ escape" MUST be rejected.
        assert!(!is_well_formed_metadata_path("ns/a~2b"));
        assert!(!is_well_formed_metadata_path("ns/key/sub~"));
        assert!(!is_well_formed_metadata_path("n~s"));
    }

    #[test]
    fn invalid_namespace_identifier_is_well_formed() {
        // -04 §3.6: "a string that is not a valid namespace identifier,
        // matches no objects; it does not produce an error."
        assert!(is_well_formed_metadata_path("Not A Namespace/key"));
    }

    // -- resolve_path -----------------------------------------------------

    #[test]
    fn resolve_namespace_only() {
        let map = json!({"ns": {"key": "val"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns"), Some(&json!({"key": "val"})));
    }

    #[test]
    fn resolve_namespace_key() {
        let map = json!({"ns": {"key": "val"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns/key"), Some(&json!("val")));
    }

    #[test]
    fn resolve_missing_namespace() {
        let map = json!({"ns": {"key": "val"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "absent"), None);
    }

    #[test]
    fn resolve_missing_key() {
        let map = json!({"ns": {"key": "val"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns/absent"), None);
    }

    #[test]
    fn resolve_namespace_not_object_for_key_path() {
        // Namespace value is a string, not an object — key lookup fails.
        let map = json!({"ns": "scalar"}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns/key"), None);
    }

    #[test]
    fn resolve_deep_path() {
        // -04 §3.6: <namespace>/<key>/<subkey> walks ns -> key -> subkey.
        let map = json!({"ns": {"a": {"b": {"c": "deep"}}}})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(resolve_path(&map, "ns/a/b/c"), Some(&json!("deep")));
        assert_eq!(resolve_path(&map, "ns/a/b"), Some(&json!({"c": "deep"})));
        assert_eq!(resolve_path(&map, "ns/a/missing"), None);
    }

    #[test]
    fn resolve_deep_path_is_not_a_single_key() {
        // "ns/a/b" walks two members; it does not name the key "a/b",
        // which RFC 6901 §3 would spell "a~1b".
        let map = json!({"ns": {"a/b": "flat"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns/a/b"), None);
    }

    #[test]
    fn resolve_through_array_addresses_no_value() {
        // -04 §3.6: "a path that would traverse an array or a scalar
        // addresses no value" — even with an RFC 6901 array index.
        let map = json!({"ns": {"list": ["x", {"k": "v"}]}})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            resolve_path(&map, "ns/list"),
            Some(&json!(["x", {"k": "v"}]))
        );
        assert_eq!(resolve_path(&map, "ns/list/0"), None);
        assert_eq!(resolve_path(&map, "ns/list/1/k"), None);
    }

    #[test]
    fn resolve_through_scalar_addresses_no_value() {
        let map = json!({"ns": {"memo": "text", "n": 1, "z": null}})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(resolve_path(&map, "ns/memo/x"), None);
        assert_eq!(resolve_path(&map, "ns/n/x"), None);
        assert_eq!(resolve_path(&map, "ns/z/x"), None);
    }

    #[test]
    fn resolve_malformed_path_is_none() {
        let map = json!({"ns": {"a~2b": "v"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns/a~2b"), None);
    }

    #[test]
    fn resolve_rfc6901_escaped_key() {
        // Key "a/b" is escaped as "a~1b" in the path.
        let map = json!({"ns": {"a/b": "found"}}).as_object().unwrap().clone();
        assert_eq!(resolve_path(&map, "ns/a~1b"), Some(&json!("found")));
    }

    // -- metadata_path_exists ---------------------------------------------

    #[test]
    fn exists_namespace_present_non_empty() {
        let map = json!({"photography": {"album": "sunset"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_path_exists(&map, "photography"));
    }

    #[test]
    fn exists_namespace_present_empty_object() {
        // §3.5: namespace-only matches if value is not {}.
        let map = json!({"photography": {}}).as_object().unwrap().clone();
        assert!(!metadata_path_exists(&map, "photography"));
    }

    #[test]
    fn exists_namespace_absent() {
        let map = json!({"other": {}}).as_object().unwrap().clone();
        assert!(!metadata_path_exists(&map, "photography"));
    }

    #[test]
    fn exists_key_present() {
        let map = json!({"ns": {"album": "sunset"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_path_exists(&map, "ns/album"));
    }

    #[test]
    fn exists_deep_path() {
        let map = json!({"ns": {"a": {"b": {}}, "list": [1]}})
            .as_object()
            .unwrap()
            .clone();
        // -04 §3.6: only a namespace-only path excludes `{}`; a deeper
        // path matches whenever a value is present.
        assert!(metadata_path_exists(&map, "ns/a/b"));
        assert!(!metadata_path_exists(&map, "ns/a/b/c"));
        assert!(!metadata_path_exists(&map, "ns/list/0"));
    }

    #[test]
    fn exists_namespace_present_scalar() {
        // -04 §3.6: matches if "the namespace key is present and its value
        // is not the empty object {}".
        let map = json!({"ns": "scalar"}).as_object().unwrap().clone();
        assert!(metadata_path_exists(&map, "ns"));
    }

    #[test]
    fn exists_key_absent() {
        let map = json!({"ns": {"album": "sunset"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(!metadata_path_exists(&map, "ns/missing"));
    }

    // -- metadata_text_contains -------------------------------------------

    fn text_match(path: &str, value: &str) -> MetadataTextMatch {
        serde_json::from_value(json!({"path": path, "value": value})).unwrap()
    }

    #[test]
    fn text_contains_case_insensitive_substring() {
        let map = json!({"acme.example.com": {"memo": "Follow Up Soon"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_contains(
            &map,
            &text_match("acme.example.com/memo", "follow up")
        ));
    }

    #[test]
    fn text_contains_no_match() {
        let map = json!({"ns": {"memo": "hello"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(!metadata_text_contains(
            &map,
            &text_match("ns/memo", "goodbye")
        ));
    }

    #[test]
    fn text_contains_non_string_value() {
        let map = json!({"ns": {"count": 42}}).as_object().unwrap().clone();
        assert!(!metadata_text_contains(&map, &text_match("ns/count", "42")));
    }

    #[test]
    fn text_contains_missing_path() {
        let map = json!({"ns": {}}).as_object().unwrap().clone();
        assert!(!metadata_text_contains(
            &map,
            &text_match("ns/missing", "x")
        ));
    }

    #[test]
    fn text_contains_deep_path() {
        let map = json!({"ns": {"note": {"body": "Call Back"}}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_contains(
            &map,
            &text_match("ns/note/body", "call")
        ));
    }

    // RFC 5051 §2 vectors. Each expected result is derived by hand from
    // the RFC 5051 §2 steps and the UnicodeData.txt fields named in the
    // test, not from the code under test.

    #[test]
    fn text_contains_rfc5051_example_digraph() {
        // RFC 5051 §2 example: U+01C4 prepares to U+0044 U+007A U+030C.
        // U+01C6 has titlecase U+01C5, which decomposes the same way.
        let map = json!({"ns": {"memo": "\u{01C4}"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_contains(
            &map,
            &text_match("ns/memo", "\u{01C6}")
        ));
        // U+0044 U+017D: U+017D has no titlecase property and decomposes
        // to U+005A U+030C, so the needle is "DZ" + caron, which is not
        // "Dz" + caron.
        assert!(!metadata_text_contains(
            &map,
            &text_match("ns/memo", "D\u{017D}")
        ));
    }

    #[test]
    fn text_contains_rfc5051_compatibility_decomposition() {
        // U+FF21..U+FF23 FULLWIDTH LATIN CAPITAL A..C: no titlecase
        // property, decomposition <wide> U+0041..U+0043. The needle "abc"
        // titlecases to "ABC".
        let map = json!({"ns": {"memo": "x\u{FF21}\u{FF22}\u{FF23}y"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_contains(&map, &text_match("ns/memo", "abc")));
    }

    #[test]
    fn text_contains_rfc5051_canonical_decomposition() {
        // U+00E9 has titlecase U+00C9, which decomposes to U+0045 U+0301,
        // and "t" titlecases to "T". Both strings prepare to
        // U+0045 U+0301 U+0054 U+0045 U+0301.
        let map = json!({"ns": {"memo": "\u{00E9}t\u{00E9}"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_contains(
            &map,
            &text_match("ns/memo", "E\u{0301}TE\u{0301}")
        ));
    }

    #[test]
    fn text_contains_rfc5051_no_recase_after_decomposition() {
        // U+FB01 LATIN SMALL LIGATURE FI: no titlecase property,
        // decomposition <compat> U+0066 U+0069 (lowercase). The needle
        // "fi" titlecases to "FI", so it does not match.
        let map = json!({"ns": {"memo": "\u{FB01}le"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(!metadata_text_contains(&map, &text_match("ns/memo", "fi")));
        assert!(!metadata_text_contains(&map, &text_match("ns/memo", "FI")));
    }

    // -- metadata_text_equals ---------------------------------------------

    #[test]
    fn text_equals_exact_match() {
        let map = json!({"ns": {"tag": "important"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_equals(
            &map,
            &text_match("ns/tag", "important")
        ));
    }

    #[test]
    fn text_equals_case_sensitive() {
        let map = json!({"ns": {"tag": "Important"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(!metadata_text_equals(
            &map,
            &text_match("ns/tag", "important")
        ));
    }

    #[test]
    fn text_equals_no_normalization() {
        // -04 §3.6: "comparing Unicode code points without case folding
        // or normalization". U+00E9 and U+0065 U+0301 are canonically
        // equivalent but are different code point sequences.
        let map = json!({"ns": {"tag": "\u{00E9}t\u{00E9}"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(!metadata_text_equals(
            &map,
            &text_match("ns/tag", "e\u{0301}te\u{0301}")
        ));
    }

    #[test]
    fn text_equals_non_string() {
        let map = json!({"ns": {"flag": true}}).as_object().unwrap().clone();
        assert!(!metadata_text_equals(&map, &text_match("ns/flag", "true")));
    }

    #[test]
    fn text_equals_deep_path() {
        let map = json!({"ns": {"tags": {"primary": "red"}, "list": ["red"]}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_equals(
            &map,
            &text_match("ns/tags/primary", "red")
        ));
        assert!(!metadata_text_equals(&map, &text_match("ns/list/0", "red")));
    }

    #[test]
    fn text_equals_missing_path() {
        let map = json!({}).as_object().unwrap().clone();
        assert!(!metadata_text_equals(
            &map,
            &text_match("ns/tag", "anything")
        ));
    }

    // -- §6.7 spec example ------------------------------------------------

    #[test]
    fn spec_example_section_6_7_text_contains() {
        // Oracle: draft-ietf-jmap-metadata-02 §6.7.
        let map = json!({"acme.example.com": {"memo": "follow up"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(metadata_text_contains(
            &map,
            &text_match("acme.example.com/memo", "follow up")
        ));
    }
}

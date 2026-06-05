//! draft-ietf-jmap-metadata-02 §3.5 — metadata filter predicate helpers.
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

use std::borrow::Cow;

use jmap_metadata_types::MetadataTextMatch;
use serde_json::{Map, Value};

/// Unescape a single RFC 6901 JSON Pointer reference token.
///
/// Per RFC 6901 §4: first replace `~1` with `/`, then `~0` with `~`.
/// Order matters — `~01` must become `~1`, not `/`.
fn unescape_rfc6901(segment: &str) -> Cow<'_, str> {
    if segment.contains('~') {
        Cow::Owned(segment.replace("~1", "/").replace("~0", "~"))
    } else {
        Cow::Borrowed(segment)
    }
}

/// Resolve a metadata path to a value within a metadata map.
///
/// Path format (§3.5): `<namespace>` or `<namespace>/<key>`, with `/`
/// and `~` escaped per [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901).
///
/// Returns [`None`] if the namespace key is absent, or if the path has
/// a `<key>` segment and the namespace value is not an object or does
/// not contain that key.
fn resolve_path<'a>(map: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    if let Some((ns_raw, key_raw)) = path.split_once('/') {
        let ns = unescape_rfc6901(ns_raw);
        let key = unescape_rfc6901(key_raw);
        let ns_value = map.get(ns.as_ref())?;
        ns_value.as_object()?.get(key.as_ref())
    } else {
        let ns = unescape_rfc6901(path);
        map.get(ns.as_ref())
    }
}

/// Check whether a value exists at `path` in a metadata map
/// (draft-ietf-jmap-metadata-02 §3.5, `metadataExists` /
/// `privateMetadataExists`).
///
/// A namespace-only path matches if the namespace key is present **and**
/// its value is not the empty object `{}`. A `<namespace>/<key>` path
/// matches if the namespace value is an object containing `<key>`.
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
/// assert!(!metadata_path_exists(&map, "photography/missing"));
/// assert!(!metadata_path_exists(&map, "absent"));
/// ```
pub fn metadata_path_exists(map: &Map<String, Value>, path: &str) -> bool {
    // Namespace-only: present and not empty object.
    if !path.contains('/') {
        let ns = unescape_rfc6901(path);
        match map.get(ns.as_ref()) {
            Some(Value::Object(obj)) => !obj.is_empty(),
            Some(_) => true,
            None => false,
        }
    } else {
        resolve_path(map, path).is_some()
    }
}

/// Check whether the string at `path` contains `text_match.value` as a
/// case-insensitive substring (draft-ietf-jmap-metadata-02 §3.5,
/// `metadataTextContains` / `privateMetadataTextContains`).
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
        Some(Value::String(s)) => s.to_lowercase().contains(&text_match.value.to_lowercase()),
        _ => false,
    }
}

/// Check whether the string at `path` is exactly equal (case-sensitive,
/// byte-for-byte) to `text_match.value` (draft-ietf-jmap-metadata-02
/// §3.5, `metadataTextEquals` / `privateMetadataTextEquals`).
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
        assert_eq!(unescape_rfc6901("photography"), "photography");
    }

    #[test]
    fn unescape_tilde_zero() {
        assert_eq!(unescape_rfc6901("a~0b"), "a~b");
    }

    #[test]
    fn unescape_tilde_one() {
        assert_eq!(unescape_rfc6901("a~1b"), "a/b");
    }

    #[test]
    fn unescape_order_matters_tilde_zero_one() {
        // ~01 must become ~1, NOT /
        assert_eq!(unescape_rfc6901("~01"), "~1");
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
        assert!(!metadata_text_contains(
            &map,
            &text_match("ns/count", "42")
        ));
    }

    #[test]
    fn text_contains_missing_path() {
        let map = json!({"ns": {}}).as_object().unwrap().clone();
        assert!(!metadata_text_contains(
            &map,
            &text_match("ns/missing", "x")
        ));
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
    fn text_equals_non_string() {
        let map = json!({"ns": {"flag": true}}).as_object().unwrap().clone();
        assert!(!metadata_text_equals(
            &map,
            &text_match("ns/flag", "true")
        ));
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

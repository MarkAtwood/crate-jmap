//! RFC 8620 §5.3 [`PatchObject`] — the typed wire-format wrapper for
//! JMAP `*/set` `update` argument values.
//!
//! `PatchObject` is a string-keyed JSON map where:
//!
//! - Keys are RFC 6901 JSON Pointers with an *implicit* leading `/`
//!   (i.e. the wire form is `"alerts/1/offset"`, evaluated as
//!   `"/alerts/1/offset"` for the JSON Pointer algorithm).
//! - A `null` leaf value either restores a default value (if the property
//!   has a spec-defined default) or removes that property from the object.
//! - Any other leaf value replaces or adds the value at that path.
//!
//! The type is intentionally a thin newtype around
//! [`serde_json::Map<String, serde_json::Value>`] with `#[serde(transparent)]`,
//! so the wire format is byte-identical to a bare JSON object. The newtype
//! exists to bind the RFC 8620 §5.3 contract to the type system: when a
//! function signature carries `PatchObject`, every reader knows the contained
//! map is a JMAP patch (with JSON Pointer key semantics and the null-leaf
//! removal rule), not arbitrary JSON.
//!
//! # Path-syntax validation is the handler's job
//!
//! Per RFC 8620 §5.3, path violations (paths that traverse arrays, paths
//! whose parent does not yet exist on the patched object, paths that prefix
//! another patch in the same `PatchObject`) MUST be rejected by the server
//! with an `invalidPatch` error. This crate intentionally does **not**
//! perform that validation: the wire-format type is value-agnostic, and
//! the meaning of each path is method-specific (e.g. `Email/set` patches
//! mean something different from `Mailbox/set` patches). Validation lives
//! in the method handler that knows the target object's schema.
//!
//! # Future adoption
//!
//! [`crate::SetObject::Patch`] is currently typed as
//! `serde::Serialize + serde::de::DeserializeOwned`, which downstream
//! crates fill with `serde_json::Value`. Migrating those impls to use
//! `PatchObject` is a separate, opt-in refactor (see bd JMAP-o7cj
//! follow-ups) and does not change the wire format.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A JMAP `PatchObject` (RFC 8620 §5.3) — a String → JSON map describing
/// the changes to apply to a record during `*/set` `update`.
///
/// See the [module documentation](crate::patch) for the path-key semantics
/// and null-leaf removal rule.
///
/// # Wire format
///
/// `PatchObject` serialises and deserialises as a plain JSON object via
/// `#[serde(transparent)]`. There is no discriminant or wrapper key on the
/// wire — `{"name": "New"}` and `PatchObject::from_map([("name", json!("New"))])`
/// are equivalent.
///
/// # Example
///
/// ```
/// use jmap_types::PatchObject;
/// use serde_json::{json, Map, Value};
///
/// let mut m = Map::new();
/// m.insert("name".to_owned(), json!("Renamed"));
/// m.insert("keywords/$flagged".to_owned(), Value::Null);
/// let patch = PatchObject::from_map(m);
///
/// // Wire form is byte-identical to the inner map.
/// let wire = serde_json::to_string(&patch).unwrap();
/// assert!(wire.contains("\"name\":\"Renamed\""));
/// assert!(wire.contains("\"keywords/$flagged\":null"));
/// ```
//
// `#[non_exhaustive]` reserves the right to add fields later (e.g. a
// metadata or audit-trail companion) without a SemVer break. The inner
// field is intentionally private so the only construction paths are the
// explicit constructors below; this matches the `Id`/`UTCDate`/`State`
// pattern already used in this crate.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct PatchObject(Map<String, Value>);

impl PatchObject {
    /// Construct an empty patch.
    ///
    /// An empty patch is a valid no-op `update` value per RFC 8620 §5.3:
    /// the server accepts it and applies no changes to the target record.
    pub fn new() -> Self {
        Self(Map::new())
    }

    /// Wrap an existing [`serde_json::Map`] as a [`PatchObject`].
    ///
    /// No validation is performed — the caller is responsible for the
    /// keys conforming to RFC 8620 §5.3 path syntax (or the server will
    /// reject the patch with `invalidPatch`).
    pub fn from_map(map: Map<String, Value>) -> Self {
        Self(map)
    }

    /// Borrow the inner map.
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }

    /// Mutably borrow the inner map for in-place edits.
    pub fn as_map_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.0
    }

    /// Consume the [`PatchObject`] and return the underlying map.
    pub fn into_inner(self) -> Map<String, Value> {
        self.0
    }

    /// `true` if the patch contains no key/value pairs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of key/value pairs in the patch.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<Map<String, Value>> for PatchObject {
    fn from(map: Map<String, Value>) -> Self {
        Self(map)
    }
}

impl From<PatchObject> for Map<String, Value> {
    fn from(patch: PatchObject) -> Self {
        patch.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Independent oracle: hand-written JSON literal for an empty patch.
    // RFC 8620 §5.3 implicitly admits an empty PatchObject as a no-op
    // (it lists no rule that rejects an empty map).
    #[test]
    fn empty_patch_serializes_as_empty_object() {
        let p = PatchObject::new();
        let wire = serde_json::to_string(&p).expect("serialize empty patch");
        assert_eq!(wire, "{}");
    }

    // Independent oracle: an empty JSON object must round-trip into an
    // empty PatchObject.
    #[test]
    fn empty_object_deserializes_as_empty_patch() {
        let p: PatchObject = serde_json::from_str("{}").expect("deserialize empty patch");
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    // Independent oracle: RFC 8620 §5.3 line 1937-1941 — "an entire Foo
    // object is also a valid PatchObject". Here we exercise the
    // whole-object replacement form: top-level field names without `/`
    // separators are valid wire-format keys.
    #[test]
    fn whole_object_form_round_trips() {
        let wire = r#"{"name":"Inbox","sortOrder":0,"isSubscribed":true}"#;
        let p: PatchObject = serde_json::from_str(wire).expect("deserialize whole-object patch");
        assert_eq!(p.len(), 3);
        assert_eq!(p.as_map().get("name"), Some(&json!("Inbox")));
        assert_eq!(p.as_map().get("sortOrder"), Some(&json!(0)));
        assert_eq!(p.as_map().get("isSubscribed"), Some(&json!(true)));
        // Round-trip back to wire — same object, byte-equal modulo key order.
        let reserialized = serde_json::to_string(&p).expect("reserialize");
        let reparsed: serde_json::Value = serde_json::from_str(&reserialized).unwrap();
        let original_parsed: serde_json::Value = serde_json::from_str(wire).unwrap();
        assert_eq!(reparsed, original_parsed);
    }

    // Independent oracle: RFC 8620 §5.3 line 1925-1927 — "If null, set to
    // the default value if specified for this property; otherwise, remove
    // the property from the patched object." A null leaf is the canonical
    // "remove this field" wire form. Verify it round-trips losslessly.
    #[test]
    fn null_leaf_round_trips() {
        let wire = r#"{"keywords/$flagged":null}"#;
        let p: PatchObject = serde_json::from_str(wire).expect("deserialize null-leaf patch");
        assert_eq!(p.as_map().get("keywords/$flagged"), Some(&Value::Null));
        let reserialized = serde_json::to_string(&p).expect("reserialize");
        assert_eq!(reserialized, wire);
    }

    // Independent oracle: RFC 8620 §5.3 line 1918-1920 — patch keys are
    // JSON Pointer paths with an implicit leading `/`. The example in the
    // RFC text uses `alerts/1/offset` (an alerts-array element offset).
    // Here we verify the multi-segment path key serialises and parses
    // verbatim — the wire-format type is path-agnostic and does not
    // attempt JSON Pointer interpretation.
    #[test]
    fn pointer_path_key_round_trips() {
        let wire = r#"{"alerts/abc/offset":"PT5M"}"#;
        let p: PatchObject = serde_json::from_str(wire).expect("deserialize pointer-path patch");
        assert_eq!(p.as_map().get("alerts/abc/offset"), Some(&json!("PT5M")));
        let reserialized = serde_json::to_string(&p).expect("reserialize");
        assert_eq!(reserialized, wire);
    }

    // Independent oracle: a non-null nested object value at a path key.
    // From RFC 8620 §5.3 line 1929-1930 — "Anything else: The value to
    // set for this property". Setting a whole sub-object at a path is
    // valid and common (e.g. setting `alerts/abc` to a full Alert object).
    #[test]
    fn nested_object_value_round_trips() {
        let inner = json!({"offset": "PT5M", "type": "display"});
        let mut m = Map::new();
        m.insert("alerts/abc".to_owned(), inner.clone());
        let p = PatchObject::from_map(m);
        let wire = serde_json::to_string(&p).expect("serialize nested object");
        // Round-trip: parse -> compare structurally to avoid key-order
        // dependence inside the inner object.
        let parsed: PatchObject = serde_json::from_str(&wire).expect("reparse");
        assert_eq!(parsed.as_map().get("alerts/abc"), Some(&inner));
    }

    // Independent oracle: the wire format MUST be byte-identical to a
    // plain JSON object — this is what `#[serde(transparent)]` guarantees
    // and is the entire reason for the newtype's design. If a future
    // refactor accidentally drops `#[serde(transparent)]` this test will
    // fail with an extra wrapper key on the wire.
    #[test]
    fn wire_format_is_transparent_json_object() {
        let mut m = Map::new();
        m.insert("name".to_owned(), json!("Test"));
        let p = PatchObject::from_map(m.clone());

        let from_patch = serde_json::to_string(&p).unwrap();
        let from_map = serde_json::to_string(&m).unwrap();
        assert_eq!(from_patch, from_map);
    }

    // Independent oracle: From<Map> and Into<Map> conversions are total
    // and lossless.
    #[test]
    fn from_into_map_round_trip() {
        let mut m = Map::new();
        m.insert("k".to_owned(), json!(42));

        let p: PatchObject = m.clone().into();
        let back: Map<String, Value> = p.into();
        assert_eq!(back, m);
    }

    // Independent oracle: Default impl yields an empty patch — same as ::new().
    #[test]
    fn default_is_empty() {
        let p: PatchObject = Default::default();
        assert!(p.is_empty());
        assert_eq!(p, PatchObject::new());
    }

    // Independent oracle: as_map_mut allows in-place modification, and
    // the modification is visible on subsequent serialise.
    #[test]
    fn as_map_mut_allows_in_place_edits() {
        let mut p = PatchObject::new();
        p.as_map_mut().insert("name".to_owned(), json!("Edited"));
        let wire = serde_json::to_string(&p).unwrap();
        assert_eq!(wire, r#"{"name":"Edited"}"#);
    }

    // Independent oracle: a non-object JSON value MUST fail to deserialize
    // as a PatchObject. RFC 8620 §5.3 mandates that a PatchObject is a
    // map (`String[*]`); arrays, scalars, and null are not valid wire
    // values for this type.
    #[test]
    fn non_object_json_fails_to_deserialize() {
        assert!(serde_json::from_str::<PatchObject>("[]").is_err());
        assert!(serde_json::from_str::<PatchObject>("42").is_err());
        assert!(serde_json::from_str::<PatchObject>("\"string\"").is_err());
        assert!(serde_json::from_str::<PatchObject>("true").is_err());
        assert!(serde_json::from_str::<PatchObject>("null").is_err());
    }
}

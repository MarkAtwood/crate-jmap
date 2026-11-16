//! draft-ietf-jmap-conditional-02 wire types: the capability, the
//! `ifUnchangedBy` and `atomic` Foo/set arguments, and the
//! `stateMismatch`, `atomicFailure` and `cannotApplyAtomically` method
//! errors.
//!
//! The two new error codes are `&str` constants used with
//! [`JmapError::custom`], like other extension codes: they decode to
//! [`MethodErrorType::Other`], and no
//! [`MethodErrorType`] variant is added
//! (docs/design/DECISIONS.md decision 7). Their extra properties live in
//! [`JmapError::extra`], read and written by the constructors and readers
//! here.
//!
//! # Example
//!
//! ```rust
//! use jmap_types::JmapError;
//! use jmap_types::conditional::{AtomicFailure, ATOMIC_FAILURE};
//!
//! let err = JmapError::state_mismatch_failed(vec![jmap_types::Id::from("f42")]);
//! assert_eq!(
//!     serde_json::to_value(&err).unwrap(),
//!     serde_json::json!({"type": "stateMismatch", "failed": ["f42"]})
//! );
//!
//! let err = JmapError::atomic_failure(AtomicFailure::default());
//! assert_eq!(err.error_type.as_str(), ATOMIC_FAILURE);
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Id, JmapError, MethodErrorType, PatchObject, SetError};

/// The capability URI (§2.1, §8.1).
pub const JMAP_CONDITIONAL_URI: &str = "urn:ietf:params:jmap:conditional";

/// The `atomicFailure` method error (§4.2, §8.2): an atomic Foo/set could
/// not commit all its changes, so it made none.
pub const ATOMIC_FAILURE: &str = "atomicFailure";

/// The `cannotApplyAtomically` method error (§4.3, §8.3): the server
/// cannot commit this Foo/set as a unit, so it made no change.
pub const CANNOT_APPLY_ATOMICALLY: &str = "cannotApplyAtomically";

/// The Session capability object (§2.1): "The value of this property MUST
/// be an empty object." Unknown members are kept in
/// [`extra`](Self::extra).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionalCapability {
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// The `ifUnchangedBy` argument (§3): `"Id[PatchObject|null]"`.
///
/// Keys are `String`, not [`Id`]: §3.1 allows a `#`-prefixed creation id,
/// which `Id` does not accept. `None` is the JSON `null` precondition,
/// "the object does not exist".
pub type IfUnchangedBy = BTreeMap<String, Option<PatchObject>>;

/// The two §3 Foo/set arguments, for a client to merge into a Foo/set
/// call. Defaults (empty `ifUnchangedBy`, `atomic` false) are not
/// written.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalSetArgs {
    /// `ifUnchangedBy`: preconditions by object id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub if_unchanged_by: IfUnchangedBy,
    /// `atomic`: commit every change or none (§4).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub atomic: bool,
}

impl ConditionalSetArgs {
    /// True when both arguments are at their defaults: there is nothing to
    /// send, and the request needs no conditional capability.
    pub fn is_default(&self) -> bool {
        self.if_unchanged_by.is_empty() && !self.atomic
    }
}

/// The details of an `atomicFailure` error (§4.2): the Foo/set-shaped
/// `notCreated`, `notUpdated` and `notDestroyed` maps, each optional ("A
/// server MAY report only the first failure it encounters").
///
/// Other members, in particular object-history `notRestored`, are kept in
/// [`extra`](Self::extra); [`not_restored`](Self::not_restored) reads it
/// (decision 6(b): neither draft names it yet, so it is not a field).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicFailure {
    /// `notCreated`: creation id → error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_created: Option<BTreeMap<String, SetError>>,
    /// `notUpdated`: object id → error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_updated: Option<BTreeMap<Id, SetError>>,
    /// `notDestroyed`: object id → error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_destroyed: Option<BTreeMap<Id, SetError>>,
    /// Unknown members, kept for round-trip.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// The `extra` key of object-history's stopgap `notRestored` map.
const NOT_RESTORED: &str = "notRestored";

impl AtomicFailure {
    /// `notRestored` (object history; decision 6(b) stopgap name), or
    /// `None` when it is absent or malformed.
    pub fn not_restored(&self) -> Option<BTreeMap<Id, SetError>> {
        serde_json::from_value(self.extra.get(NOT_RESTORED)?.clone()).ok()
    }

    /// Set `notRestored` (decision 6(b) stopgap name).
    #[must_use]
    pub fn with_not_restored(mut self, not_restored: BTreeMap<Id, SetError>) -> Self {
        // A map of `Id` to `SetError` always serializes.
        if let Ok(v) = serde_json::to_value(not_restored) {
            self.extra.insert(NOT_RESTORED.to_owned(), v);
        }
        self
    }
}

/// The `extra` key of the §3.3 `stateMismatch` `failed` list.
const FAILED: &str = "failed";

impl JmapError {
    /// A `stateMismatch` error for failed `ifUnchangedBy` preconditions,
    /// naming the objects (§3.3 `failed`: "The ids of the objects whose
    /// preconditions were not satisfied").
    pub fn state_mismatch_failed(failed: Vec<Id>) -> Self {
        let mut e = Self::state_mismatch();
        if let Ok(v) = serde_json::to_value(failed) {
            e.extra.insert(FAILED.to_owned(), v);
        }
        e
    }

    /// An `atomicFailure` error (§4.2) carrying `details`' maps as
    /// properties of the error object.
    pub fn atomic_failure(details: AtomicFailure) -> Self {
        let mut e = Self::custom(ATOMIC_FAILURE);
        if let Ok(Value::Object(map)) = serde_json::to_value(details) {
            e.extra = map;
        }
        e
    }

    /// A `cannotApplyAtomically` error (§4.3).
    pub fn cannot_apply_atomically() -> Self {
        Self::custom(CANNOT_APPLY_ATOMICALLY)
    }

    /// The §3.3 `failed` ids of a `stateMismatch` error, or `None` when
    /// this is another error or `failed` is absent or malformed.
    pub fn failed(&self) -> Option<Vec<Id>> {
        if self.error_type != MethodErrorType::StateMismatch {
            return None;
        }
        serde_json::from_value(self.extra.get(FAILED)?.clone()).ok()
    }

    /// The §4.2 details of an `atomicFailure` error, or `None` when this is
    /// another error or the maps are malformed.
    pub fn atomic_failure_details(&self) -> Option<AtomicFailure> {
        if self.error_type.as_str() != ATOMIC_FAILURE {
            return None;
        }
        serde_json::from_value(Value::Object(self.extra.clone())).ok()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::SetErrorType;

    #[test]
    fn capability_is_an_empty_object() {
        let cap: ConditionalCapability = serde_json::from_value(json!({})).unwrap();
        assert_eq!(serde_json::to_value(&cap).unwrap(), json!({}));
        let cap: ConditionalCapability = serde_json::from_value(json!({"x": 1})).unwrap();
        assert_eq!(cap.extra["x"], 1);
    }

    #[test]
    fn set_args() {
        let args: ConditionalSetArgs =
            serde_json::from_value(json!({"ifUnchangedBy": {"r": null, "f42": {}}})).unwrap();
        assert_eq!(args.if_unchanged_by["r"], None);
        assert!(args.if_unchanged_by["f42"].is_some());
        assert!(!args.atomic);
        assert!(!args.is_default());
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            json!({"ifUnchangedBy": {"f42": {}, "r": null}})
        );
        assert!(ConditionalSetArgs::default().is_default());
        assert_eq!(
            serde_json::to_value(ConditionalSetArgs::default()).unwrap(),
            json!({})
        );
        let atomic: ConditionalSetArgs = serde_json::from_value(json!({"atomic": true})).unwrap();
        assert!(!atomic.is_default());
    }

    /// §5.1: the stateMismatch error JSON, verbatim.
    #[test]
    fn example_5_1_state_mismatch() {
        let wire = json!({"type": "stateMismatch", "failed": ["f42"]});
        let e: JmapError = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(e.error_type, MethodErrorType::StateMismatch);
        assert_eq!(e.failed(), Some(vec![Id::from("f42")]));
        assert_eq!(serde_json::to_value(&e).unwrap(), wire);
        assert_eq!(JmapError::state_mismatch_failed(vec![Id::from("f42")]), e);
        // `failed` belongs to stateMismatch only.
        let mut other = JmapError::invalid_arguments("x");
        other.extra = e.extra.clone();
        assert_eq!(other.failed(), None);
        assert_eq!(JmapError::state_mismatch().failed(), None);
    }

    #[test]
    fn atomic_failure_round_trip() {
        let wire = json!({
            "type": "atomicFailure",
            "notCreated": {"c1": {"type": "invalidProperties", "properties": ["name"]}},
            "notUpdated": {"f1": {"type": "notFound"}},
            "notDestroyed": {"f2": {"type": "forbidden"}},
            "notRestored": {"f3": {"type": "notFound"}}
        });
        let e: JmapError = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            e.error_type,
            MethodErrorType::Other(ATOMIC_FAILURE.to_owned())
        );
        assert_eq!(serde_json::to_value(&e).unwrap(), wire);
        let details = e.atomic_failure_details().unwrap();
        assert_eq!(
            details.not_created.as_ref().unwrap()["c1"].error_type,
            SetErrorType::InvalidProperties
        );
        assert_eq!(details.not_updated.as_ref().unwrap().len(), 1);
        assert_eq!(details.not_destroyed.as_ref().unwrap().len(), 1);
        let restored = details.not_restored().unwrap();
        assert_eq!(restored[&Id::from("f3")].error_type, SetErrorType::NotFound);
        // The constructor writes the same error.
        assert_eq!(JmapError::atomic_failure(details.clone()), e);
        // Rebuilt from the typed parts, `with_not_restored` writes the
        // same details.
        let rebuilt = AtomicFailure {
            not_created: details.not_created.clone(),
            not_updated: details.not_updated.clone(),
            not_destroyed: details.not_destroyed.clone(),
            extra: Map::new(),
        }
        .with_not_restored(restored);
        assert_eq!(rebuilt, details);
    }

    #[test]
    fn atomic_failure_maps_are_optional() {
        let e: JmapError = serde_json::from_value(json!({"type": "atomicFailure"})).unwrap();
        assert_eq!(e.atomic_failure_details(), Some(AtomicFailure::default()));
        assert_eq!(
            serde_json::to_value(JmapError::atomic_failure(AtomicFailure::default())).unwrap(),
            json!({"type": "atomicFailure"})
        );
        assert_eq!(
            JmapError::cannot_apply_atomically().atomic_failure_details(),
            None
        );
        // A malformed map gives None, not a panic.
        let bad: JmapError =
            serde_json::from_value(json!({"type": "atomicFailure", "notUpdated": 3})).unwrap();
        assert_eq!(bad.atomic_failure_details(), None);
    }

    /// Decision 7: both codes are extension codes, `Other`, and the
    /// constructors match `custom`.
    #[test]
    fn codes_decode_to_other() {
        for code in [ATOMIC_FAILURE, CANNOT_APPLY_ATOMICALLY] {
            let e: JmapError = serde_json::from_value(json!({"type": code})).unwrap();
            assert_eq!(e.error_type, MethodErrorType::Other(code.to_owned()));
        }
        assert_eq!(
            JmapError::custom(ATOMIC_FAILURE),
            JmapError::atomic_failure(AtomicFailure::default())
        );
        assert_eq!(
            JmapError::custom(CANNOT_APPLY_ATOMICALLY),
            JmapError::cannot_apply_atomically()
        );
    }
}

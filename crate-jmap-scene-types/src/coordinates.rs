//! draft-atwood-jmap-scene-00 §Coordinate System and §SceneBounds.
//!
//! "Implementations MUST use this convention for all position,
//! orientation, and scale values": right-handed, Y-up, meters,
//! region-local origin, unit quaternions. The convention matches glTF 2.0.
//!
//! The formats are fixed-length JSON arrays, so they are plain Rust arrays:
//! a JSON array of any other length fails to deserialize.

use serde::{Deserialize, Serialize};

/// A position `[x, y, z]` in meters, region-local (`Number[3]`).
pub type Position = [f64; 3];

/// An orientation as a unit quaternion `[x, y, z, w]` (`Number[4]`).
///
/// "Clients SHOULD send unit quaternions." Unit length is not enforced
/// here: the server either rejects or normalizes non-unit values.
pub type Orientation = [f64; 4];

/// Scale factors `[sx, sy, sz]` (`Number[3]`). "Negative values are
/// permitted and indicate mirroring along that axis."
pub type Scale = [f64; 3];

/// "The identity orientation (no rotation) is `[0, 0, 0, 1]`."
pub const IDENTITY_ORIENTATION: Orientation = [0.0, 0.0, 0.0, 1.0];

/// "`[1, 1, 1]` is the identity (unscaled)."
pub const IDENTITY_SCALE: Scale = [1.0, 1.0, 1.0];

/// Serde default for orientation properties whose draft default is the
/// identity.
pub(crate) fn identity_orientation() -> Orientation {
    IDENTITY_ORIENTATION
}

/// Serde default for scale properties whose draft default is the identity.
pub(crate) fn identity_scale() -> Scale {
    IDENTITY_SCALE
}

/// An axis-aligned bounding box in region-local coordinates
/// (draft-atwood-jmap-scene-00 §SceneBounds). Embedded, not a top-level
/// JMAP data type.
///
/// Each component of `max` "MUST be greater than or equal to the
/// corresponding component of `min`"; this is documented, not enforced on
/// deserialize.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneBounds {
    /// The minimum corner `[x, y, z]`.
    pub min: Position,
    /// The maximum corner `[x, y, z]`.
    pub max: Position,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneBounds {
    /// A bounding box from `min` to `max` with no extension fields.
    pub fn new(min: Position, max: Position) -> Self {
        Self {
            min,
            max,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identity_values_match_draft() {
        // Oracle: draft-atwood-jmap-scene-00 L140 "The identity orientation
        // (no rotation) is `[0, 0, 0, 1]`." and L148 "`[1, 1, 1]` is the
        // identity (unscaled)."
        let o: Orientation = serde_json::from_value(json!([0, 0, 0, 1])).unwrap();
        assert_eq!(o, IDENTITY_ORIENTATION);
        let s: Scale = serde_json::from_value(json!([1, 1, 1])).unwrap();
        assert_eq!(s, IDENTITY_SCALE);
    }

    #[test]
    fn coordinate_arrays_have_exact_length() {
        // Oracle: L117 "`Number[N]` denotes a JSON array of exactly N
        // Number values."
        assert!(serde_json::from_value::<Position>(json!([1, 2])).is_err());
        assert!(serde_json::from_value::<Position>(json!([1, 2, 3, 4])).is_err());
        assert!(serde_json::from_value::<Orientation>(json!([0, 0, 1])).is_err());
        assert!(serde_json::from_value::<Position>(json!(["1", 2, 3])).is_err());
    }

    #[test]
    fn negative_scale_is_accepted() {
        // Oracle: L148 "Negative values are permitted and indicate
        // mirroring along that axis."
        let s: Scale = serde_json::from_value(json!([-1, 1, 1])).unwrap();
        assert_eq!(s, [-1.0, 1.0, 1.0]);
    }

    #[test]
    fn bounds_example_parses() {
        // Oracle: L199-206, the SceneBounds example, which "describes a
        // 1000m x 210m x 1000m region".
        let b: SceneBounds = serde_json::from_value(json!({
            "min": [-500, -10, -500],
            "max": [500, 200, 500]
        }))
        .unwrap();
        assert_eq!(
            b,
            SceneBounds::new([-500.0, -10.0, -500.0], [500.0, 200.0, 500.0])
        );
        assert_eq!(
            [
                b.max[0] - b.min[0],
                b.max[1] - b.min[1],
                b.max[2] - b.min[2]
            ],
            [1000.0, 210.0, 1000.0]
        );
    }

    #[test]
    fn bounds_require_min_and_max() {
        // Oracle: L191-195 list `min` and `max` without an "optional"
        // marker.
        assert!(serde_json::from_value::<SceneBounds>(json!({"min": [0, 0, 0]})).is_err());
        assert!(serde_json::from_value::<SceneBounds>(json!({"max": [0, 0, 0]})).is_err());
    }

    #[test]
    fn bounds_vendor_extras_preserved() {
        let raw = json!({"min": [0.5, 0.0, 0.0], "max": [1.5, 1.0, 1.0], "vendor.example:tag": 1});
        let b: SceneBounds = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&b).unwrap(), raw);
    }
}

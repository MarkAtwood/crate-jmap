//! Private serde helpers shared by the Scene wire types.

use serde::{Deserialize, Deserializer};

/// Deserialize a property that may be absent, `null` or a value into
/// `Option<Option<T>>`: absent is `None` (via `#[serde(default)]`), `null`
/// is `Some(None)` and a value is `Some(Some(v))`. Paired with
/// `skip_serializing_if = "Option::is_none"`, the three states round-trip.
pub(crate) fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

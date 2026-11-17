//! Private serde helpers: copies of the private null⇄empty helpers in
//! `jmap_types::methods`, which this crate cannot reach.

use serde::{Deserialize, Serialize};

/// Deserialize `null` (or an absent field, with `#[serde(default)]`) as an
/// empty collection: the RFC 8620 §5.3 wire form of an empty `/set` result.
pub(crate) fn null_as_empty<'de, D, C>(d: D) -> Result<C, D::Error>
where
    D: serde::Deserializer<'de>,
    C: Deserialize<'de> + Default,
{
    Ok(Option::<C>::deserialize(d)?.unwrap_or_default())
}

/// Serialize an empty collection as `null`, the RFC 8620 §5.3 wire form
/// for a `/set` result field with no entries.
pub(crate) fn empty_as_null<S, C>(c: &C, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    C: Serialize,
    for<'a> &'a C: IntoIterator,
{
    if c.into_iter().next().is_none() {
        s.serialize_none()
    } else {
        s.serialize_some(c)
    }
}

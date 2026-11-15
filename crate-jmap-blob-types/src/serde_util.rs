//! Private serde helpers shared by the wire types.
//!
//! `null_as_empty`, `empty_as_null` and `present` are copies of the
//! private helpers in `jmap_types::methods`, which this crate cannot
//! reach.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::digest::DigestAlgorithm;

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

/// Deserialize a present field as `Some(value)`, so that with
/// `#[serde(default)]` an absent field stays `None` and a `null` becomes
/// `Some(None)`.
pub(crate) fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Move every `digest:<alg>` member of `rest` into a digest map.
///
/// A `null` digest value is treated as absent ("String|null", default
/// null). Any other non-string value is an error naming the key.
pub(crate) fn take_digests(
    rest: &mut Map<String, Value>,
) -> Result<BTreeMap<DigestAlgorithm, String>, String> {
    let keys: Vec<String> = rest
        .keys()
        .filter(|k| DigestAlgorithm::from_property_name(k).is_some())
        .cloned()
        .collect();
    let mut out = BTreeMap::new();
    for key in keys {
        let Some(alg) = DigestAlgorithm::from_property_name(&key) else {
            continue;
        };
        match rest.remove(&key) {
            Some(Value::String(v)) => {
                out.insert(alg, v);
            }
            Some(Value::Null) | None => {}
            Some(_) => return Err(format!("{key}: expected a string or null")),
        }
    }
    Ok(out)
}

/// Write `digests` as `digest:<alg>` members, then `extra`, into one map.
pub(crate) fn merge_digests(
    digests: &BTreeMap<DigestAlgorithm, String>,
    extra: &Map<String, Value>,
) -> Map<String, Value> {
    let mut m = extra.clone();
    for (alg, v) in digests {
        m.insert(alg.property_name(), Value::String(v.clone()));
    }
    m
}

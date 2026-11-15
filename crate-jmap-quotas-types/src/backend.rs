//! Property selector enum and [`jmap_types::JmapObject`] impls for JMAP
//! Quotas types.
//!
//! These are defined here so that `jmap-quotas-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the quota types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, QueryObject};

/// Property selector for [`crate::Quota`] `/get`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QuotaProperty {
    /// The `id` property (RFC 9425 §4.1).
    Id,
    /// The `resourceType` property (RFC 9425 §4.1).
    ResourceType,
    /// The `used` property (RFC 9425 §4.1).
    Used,
    /// The `hardLimit` property (RFC 9425 §4.1).
    HardLimit,
    /// The `scope` property (RFC 9425 §4.1).
    Scope,
    /// The `name` property (RFC 9425 §4.1).
    Name,
    /// The `types` property (RFC 9425 §4.1).
    Types,
    /// The `warnLimit` property (RFC 9425 §4.1).
    WarnLimit,
    /// The `softLimit` property (RFC 9425 §4.1).
    SoftLimit,
    /// The `description` property (RFC 9425 §4.1).
    Description,
}

/// RFC 9425 §4: Quota supports `/get`, `/changes`, `/query` and
/// `/queryChanges`, and no `/set`: every property is server-set.
impl JmapObject for crate::Quota {
    const TYPE_NAME: &'static str = "Quota";
    type Property = QuotaProperty;
}

impl GetObject for crate::Quota {}

impl QueryObject for crate::Quota {
    type Filter = crate::QuotaFilter;
    type Comparator = crate::QuotaComparator;
}

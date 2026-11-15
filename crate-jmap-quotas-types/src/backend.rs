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
    /// The `locale` property (draft-atwood-jmap-updates-00 §4.1).
    Locale,
    /// The `localizations` property (draft-atwood-jmap-updates-00 §4.1).
    Localizations,
}

/// RFC 9425 §4: Quota supports `/get`, `/changes`, `/query` and
/// `/queryChanges`, and no `/set`: every property is server-set.
///
/// draft-atwood-jmap-updates-00 §5.2: `name` and
/// `description` are localizable.
impl JmapObject for crate::Quota {
    const TYPE_NAME: &'static str = "Quota";
    type Property = QuotaProperty;
    const LOCALIZABLE: &'static [jmap_types::i18n::LocalizableProperty] = &[
        jmap_types::i18n::LocalizableProperty::new("name", false),
        jmap_types::i18n::LocalizableProperty::new("description", true),
    ];
}

impl GetObject for crate::Quota {}

impl QueryObject for crate::Quota {
    type Filter = crate::QuotaFilter;
    type Comparator = crate::QuotaComparator;
}

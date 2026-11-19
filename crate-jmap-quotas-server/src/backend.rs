//! The [`QuotasBackend`] trait.
//!
//! The read operations Quota needs (`get_objects`, `get_state`,
//! `get_changes`, `query_objects`, `query_changes`) are on the
//! [`jmap_server::JmapBackend`] supertrait. Quota has no `/set`, so this
//! trait adds no write operations; a backend changes quotas itself.

use jmap_server::JmapBackend;
use jmap_types::data_types::DATA_TYPES;

/// Storage for Quota objects.
///
/// A backend's [`JmapReads::get_changes`](jmap_server::JmapReads::get_changes) for Quota should set
/// [`ChangesResult::updated_properties`](jmap_server::ChangesResult::updated_properties)
/// to `["used"]` when only `used` changed on every updated quota, and leave
/// it `None` when it cannot tell (RFC 9425 §4.3).
///
/// Its [`JmapReads::query_objects`](jmap_server::JmapReads::query_objects) must evaluate the `type` condition
/// and `OR` / `AND` / `NOT` operators even when the client sent no filter:
/// the handlers add a filter on the recognized types to every query.
pub trait QuotasBackend: JmapBackend {
    /// The type names a request with capabilities `using` recognizes: a
    /// Quota's `types` is cut down to these, and a Quota left with none is
    /// not returned (RFC 9425 §4.1).
    ///
    /// The default is [`registry_types`]. Override it to add private types
    /// that are not in the "JMAP Data Types" registry.
    fn recognized_types(&self, caller: &Self::CallerCtx, using: &[String]) -> Vec<String> {
        let _ = caller;
        registry_types(using)
    }
}

/// The registered type names (`jmap_types::data_types`) whose capability is
/// in `using`.
pub fn registry_types(using: &[String]) -> Vec<String> {
    DATA_TYPES
        .iter()
        .filter(|e| using.iter().any(|u| u == e.capability))
        .map(|e| e.type_name.to_owned())
        .collect()
}

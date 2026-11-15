//! The umbrella's re-exports under each feature set.
//!
//! Oracle: RFC 9425 §2.1 (the capability URI and its `{}` value) and the
//! crate's documented feature table.

use jmap_quotas::types::{JMAP_QUOTA_URI, QuotaCapability};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_QUOTA_URI, "urn:ietf:params:jmap:quota");
    let cap: QuotaCapability = serde_json::from_str("{}").unwrap();
    assert_eq!(serde_json::to_string(&cap).unwrap(), "{}");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-quotas-client.
    fn _takes<T: jmap_quotas::client::JmapQuotasExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    let using = vec!["urn:ietf:params:jmap:mail".to_owned()];
    assert!(jmap_quotas::server::registry_types(&using).contains(&"Email".to_owned()));
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    let _ = jmap_quotas::server::memory::MemoryBackend::new();
}

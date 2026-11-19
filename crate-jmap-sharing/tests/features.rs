//! The umbrella's re-exports under each feature set.
//!
//! Oracles: RFC 9670 (§1.5, the capability URIs and the account capability)
//! and the crate's documented feature table.

use jmap_sharing::types::{JMAP_PRINCIPALS_URI, PrincipalsCapability};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_PRINCIPALS_URI, "urn:ietf:params:jmap:principals");
    let cap: PrincipalsCapability =
        serde_json::from_str(r#"{"currentUserPrincipalId": "P1"}"#).unwrap();
    assert!(cap.current_user_principal_id.is_some());
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-sharing-client.
    fn _takes<T: jmap_sharing::client::JmapSharingExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-sharing-server.
    fn _takes<B: jmap_sharing::server::SharingBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_sharing::server::SharingBackend>(_: &B) {}
    implements(&jmap_sharing::server::memory::MemoryBackend::new());
}

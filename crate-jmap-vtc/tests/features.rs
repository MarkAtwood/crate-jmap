//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-atwood-jmap-vtc-00 (the two capability URIs) and the
//! crate's documented feature table.

use jmap_vtc::types::{JMAP_VTC_URI, JMAP_VTC_WEBSOCKET_URI};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_VTC_URI, "urn:ietf:params:jmap:vtc");
    assert_eq!(JMAP_VTC_WEBSOCKET_URI, "urn:ietf:params:jmap:vtc:websocket");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-vtc-client.
    fn _takes<T: jmap_vtc::client::JmapVtcExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-vtc-server.
    fn _takes<B: jmap_vtc::server::VtcBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_vtc::server::VtcBackend>(_: &B) {}
    implements(&jmap_vtc::server::memory::MemoryBackend::new());
}

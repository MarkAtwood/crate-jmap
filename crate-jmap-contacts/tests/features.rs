//! The umbrella's re-exports under each feature set.
//!
//! Oracles: RFC 9610 (the capability URI) and the crate's
//! documented feature table.

use jmap_contacts::types::JMAP_CONTACTS_URI;

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_CONTACTS_URI, "urn:ietf:params:jmap:contacts");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-contacts-client.
    fn _takes<T: jmap_contacts::client::JmapContactsExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-contacts-server.
    fn _takes<B: jmap_contacts::server::ContactsBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_contacts::server::ContactsBackend>(_: &B) {}
    let backend: jmap_contacts::server::memory::MemoryBackend = Default::default();
    implements(&backend);
}

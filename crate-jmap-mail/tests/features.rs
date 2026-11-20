//! The umbrella's re-exports under each feature set.
//!
//! Oracles: RFC 8621 (the capability URI) and the crate's
//! documented feature table.

use jmap_mail::types::JMAP_MAIL_URI;

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_MAIL_URI, "urn:ietf:params:jmap:mail");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-mail-client.
    fn _takes<T: jmap_mail::client::JmapMailExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-mail-server.
    fn _takes<B: jmap_mail::server::MailBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_mail::server::MailBackend>(_: &B) {}
    let backend: jmap_mail::server::memory::MemoryBackend = Default::default();
    implements(&backend);
}

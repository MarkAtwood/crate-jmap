//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-calendars-34 (the capability URI) and the crate's
//! documented feature table.

use jmap_calendars::types::JMAP_CALENDARS_URI;

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_CALENDARS_URI, "urn:ietf:params:jmap:calendars");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-calendars-client.
    fn _takes<T: jmap_calendars::client::JmapCalendarsExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-calendars-server.
    fn _takes<B: jmap_calendars::server::CalendarsBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_calendars::server::CalendarsBackend>(_: &B) {}
    let backend: jmap_calendars::server::memory::MemoryBackend = Default::default();
    implements(&backend);
}

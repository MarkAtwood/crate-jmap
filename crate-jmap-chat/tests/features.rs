//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-atwood-jmap-chat-00 (the capability URI) and the crate's
//! documented feature table.

use jmap_chat::types::JMAP_CHAT_URI;

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_CHAT_URI, "urn:ietf:params:jmap:chat");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-chat-client.
    fn _takes<T: jmap_chat::client::JmapChatExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-chat-server.
    fn _takes<B: jmap_chat::server::ChatBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_chat::server::ChatBackend>(_: &B) {}
    let backend: jmap_chat::server::memory::MemoryBackend = Default::default();
    implements(&backend);
}

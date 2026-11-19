//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-atwood-jmap-scene-00 (the two capability URIs) and the
//! crate's documented feature table.

use jmap_scene::types::{JMAP_SCENE_URI, JMAP_SCENE_WEBSOCKET_URI};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_SCENE_URI, "urn:ietf:params:jmap:scene");
    assert_eq!(
        JMAP_SCENE_WEBSOCKET_URI,
        "urn:ietf:params:jmap:scene:websocket"
    );
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-scene-client.
    fn _takes<T: jmap_scene::client::JmapSceneExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-scene-server.
    fn _takes<B: jmap_scene::server::SceneBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_scene::server::SceneBackend>(_: &B) {}
    implements(&jmap_scene::server::memory::MemoryBackend::new());
}

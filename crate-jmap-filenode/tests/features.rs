//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-filenode-20 (the capability URI and a Directory
//! node) and the crate's documented feature table.

use jmap_filenode::types::{FileNode, JMAP_FILENODE_URI, NodeType};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_FILENODE_URI, "urn:ietf:params:jmap:filenode");
    let dir: FileNode = serde_json::from_str(
        r#"{"@type": "Directory", "id": "dir1",
        "parentId": null, "name": "Documents", "role": null,
        "created": "2024-06-01T00:00:00Z", "modified": "2025-01-20T14:30:00Z",
        "shareWith": null}"#,
    )
    .unwrap();
    assert_eq!(dir.node_type, Some(NodeType::Directory));
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-filenode-client.
    fn _takes<T: jmap_filenode::client::JmapFilenodeExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-filenode-server.
    fn _takes<B: jmap_filenode::server::FileNodeBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_filenode::server::FileNodeBackend>(_: &B) {}
    implements(&jmap_filenode::server::memory::MemoryBackend::new());
}

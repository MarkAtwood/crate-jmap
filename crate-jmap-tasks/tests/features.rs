//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-tasks-06 (the capability URI) and the crate's
//! documented feature table.

use jmap_tasks::types::JMAP_TASKS_URI;

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_TASKS_URI, "urn:ietf:params:jmap:tasks");
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-tasks-client.
    fn _takes<T: jmap_tasks::client::JmapTasksExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-tasks-server.
    fn _takes<B: jmap_tasks::server::TasksBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_tasks::server::TasksBackend>(_: &B) {}
    let backend: jmap_tasks::server::memory::MemoryBackend = Default::default();
    implements(&backend);
}

// jmap-filenode-client — JMAP FileNode method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

/// Extension trait adding JMAP FileNode methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_filenode_client::JmapFileNodeExt;`
pub trait JmapFileNodeExt {
    // Methods will be added in implementation beads.
}

impl JmapFileNodeExt for jmap_base_client::JmapClient {}

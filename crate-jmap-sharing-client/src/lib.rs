// jmap-sharing-client — JMAP Sharing method implementations (RFC 9670).
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

/// Extension trait adding JMAP Sharing methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_sharing_client::JmapSharingExt;`
pub trait JmapSharingExt {
    // Methods will be added in implementation beads.
}

impl JmapSharingExt for jmap_base_client::JmapClient {}

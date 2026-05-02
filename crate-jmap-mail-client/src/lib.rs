// jmap-mail-client — RFC 8621 JMAP for Mail method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

/// Extension trait adding RFC 8621 (JMAP for Mail) methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_mail_client::JmapMailExt;`
pub trait JmapMailExt {
    // Methods will be added in implementation beads.
}

impl JmapMailExt for jmap_base_client::JmapClient {}

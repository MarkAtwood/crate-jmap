// jmap-contacts-client — JMAP Contacts method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

/// Extension trait adding JMAP Contacts methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_contacts_client::JmapContactsExt;`
pub trait JmapContactsExt {
    // Methods will be added in implementation beads.
}

impl JmapContactsExt for jmap_base_client::JmapClient {}

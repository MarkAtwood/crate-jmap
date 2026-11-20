//! The facade's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-mail-sharing-02 (§1.2.1 the capability URI,
//! §2 the principals:owner capability that `shareWith` keys refer to),
//! RFC 9670 §1.5 (the Principals capability URIs) and the crate's
//! documented feature table.

use jmap_mail_sharing::types::{
    JMAP_PRINCIPALS_OWNER_URI, JMAP_PRINCIPALS_URI, PrincipalsOwnerCapability,
};

#[test]
fn types_are_always_there() {
    assert_eq!(
        jmap_mail_sharing::CAPABILITY,
        "urn:ietf:params:jmap:mail:share"
    );
    assert_eq!(JMAP_PRINCIPALS_URI, "urn:ietf:params:jmap:principals");
    assert_eq!(
        JMAP_PRINCIPALS_OWNER_URI,
        "urn:ietf:params:jmap:principals:owner"
    );
    let owner: PrincipalsOwnerCapability =
        serde_json::from_str(r#"{"accountIdForPrincipal": "A1", "principalId": "P1"}"#).unwrap();
    assert_eq!(
        serde_json::to_value(&owner.account_id_for_principal).unwrap(),
        "A1"
    );
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-sharing-client.
    fn _takes<T: jmap_mail_sharing::client::JmapSharingExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-sharing-server.
    fn _takes<B: jmap_mail_sharing::server::SharingBackend>(_: &B) {}
}

#[cfg(feature = "memory")]
#[test]
fn memory_backend_is_reexported() {
    fn implements<B: jmap_mail_sharing::server::SharingBackend>(_: &B) {}
    implements(&jmap_mail_sharing::server::memory::MemoryBackend::new());
}

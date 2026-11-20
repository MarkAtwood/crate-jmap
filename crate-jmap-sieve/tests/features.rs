//! The facade's re-exports under each feature set.
//!
//! Oracles: RFC 9661 (§1.2 the capability URI and objects, the §1.2.2
//! Session example, §2.1 the SieveScript properties) and the crate's
//! documented feature table.

use jmap_sieve::types::{
    JMAP_SIEVE_SCRIPTS_URI, SieveAccountCapability, SieveCapability, SieveScript,
};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_SIEVE_SCRIPTS_URI, "urn:ietf:params:jmap:sieve");
    assert_eq!(jmap_sieve::CAPABILITY, JMAP_SIEVE_SCRIPTS_URI);
    let cap: SieveCapability =
        serde_json::from_str(r#"{"implementation": "ACME Email Filtering"}"#).unwrap();
    assert_eq!(cap.implementation, "ACME Email Filtering");
    // §1.2.2, with the example's "..." elisions removed.
    let account: SieveAccountCapability = serde_json::from_str(
        r#"{"maxSizeScriptName": 512, "maxSizeScript": 65536,
            "maxNumberScripts": 5, "maxNumberRedirects": null,
            "sieveExtensions": ["fileinto", "imap4flags", "enotify"],
            "notificationMethods": ["mailto"], "externalLists": null}"#,
    )
    .unwrap();
    assert_eq!(account.max_size_script_name, 512);
    assert_eq!(account.max_number_redirects, None);
    let script: SieveScript = serde_json::from_str(
        r#"{"id": "S1", "name": "vacation", "blobId": "B1", "isActive": true}"#,
    )
    .unwrap();
    assert!(script.is_active);
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // Naming the trait is the check: it exists only with jmap-mail-server's `sieve`.
    fn _takes<B: jmap_sieve::server::SieveBackend>(_: &B) {}
}

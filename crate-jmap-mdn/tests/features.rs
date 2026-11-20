//! The facade's re-exports under each feature set.
//!
//! Oracles: RFC 9007 (§1.3 the capability URI, §2 the MDN object and its
//! disposition, from RFC 8098 §3.2.6) and the crate's documented feature
//! table.

use jmap_mdn::types::{ActionMode, Disposition, DispositionType, JMAP_MDN_URI, SendingMode};

#[test]
fn types_are_always_there() {
    assert_eq!(JMAP_MDN_URI, "urn:ietf:params:jmap:mdn");
    assert_eq!(jmap_mdn::CAPABILITY, JMAP_MDN_URI);
    let d = Disposition::new(
        ActionMode::AutomaticAction,
        SendingMode::MdnSentAutomatically,
        DispositionType::Deleted,
    );
    assert_eq!(
        serde_json::to_value(&d).unwrap(),
        serde_json::json!({"actionMode": "automatic-action",
            "sendingMode": "mdn-sent-automatically", "type": "deleted"})
    );
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // The handler's default cap on MDN/parse blobIds.
    assert_eq!(jmap_mdn::server::MDN_PARSE_MAX_BLOB_IDS, 16);
    // Naming the trait is the check: it exists only with jmap-mail-server's `mdn`.
    fn _takes<B: jmap_mdn::server::MdnBackend>(_: &B) {}
}

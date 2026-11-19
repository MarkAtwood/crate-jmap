//! The umbrella's re-exports under each feature set.
//!
//! Oracles: draft-ietf-jmap-object-history-03 (the capability URI and
//! account capability) and the crate's documented feature table.

use jmap_object_history::types::{JMAP_OBJECT_HISTORY_URI, ObjectHistoryAccountCapability};

#[test]
fn types_are_always_there() {
    assert_eq!(
        JMAP_OBJECT_HISTORY_URI,
        "urn:ietf:params:jmap:object-history"
    );
    assert_eq!(jmap_object_history::CAPABILITY, JMAP_OBJECT_HISTORY_URI);
    let cap: ObjectHistoryAccountCapability = serde_json::from_str(
        r#"{
        "maxHistoryDuration": 2592000, "supportedTypes": ["ContactCard", "Email"]}"#,
    )
    .unwrap();
    assert!(cap.supports_type("Email"));
}

#[cfg(feature = "client")]
#[test]
fn client_is_reexported() {
    // Naming the trait is the check: it exists only in jmap-object-history-client.
    fn _takes<T: jmap_object_history::client::JmapObjectHistoryExt>(_: &T) {}
}

#[cfg(feature = "server")]
#[test]
fn server_is_reexported() {
    // RFC 8620 §1.4 UTCDate: the Unix epoch, no fractional second.
    let epoch = jmap_object_history::server::utc_date_from_millis(0);
    assert_eq!(<_ as AsRef<str>>::as_ref(&epoch), "1970-01-01T00:00:00Z");
}

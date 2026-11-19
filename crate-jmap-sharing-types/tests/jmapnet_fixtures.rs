//! Cross-implementation fixtures from JMAP.Net (github.com/JMAP-Net/JMAP.Net
//! at db60d47, MIT, Copyright (c) 2025 JMAP.Net), bd JMAP-48o0. They are
//! that library's serializer output, checked by its own test suite (165 of
//! 166 passing; the failure is an HTTP limit test, not a fixture). JMAP.Net
//! targets draft-ietf-jmap-calendars-26 and RFC 8620; where it and this
//! crate disagree the draft text decides (calendars-34 here), and each
//! disagreement is asserted with its citation. See tests/fixtures/jmapnet/README.md.

use jmap_sharing_types::{Principal, ShareNotification};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/jmapnet/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("JSON")
}

/// RFC 9670 §2 Principal, unchanged by a round trip.
#[test]
fn principal_round_trips() {
    let original = fixture("principal");
    let p: Principal = serde_json::from_value(original.clone()).expect("Principal");
    assert_eq!(serde_json::to_value(p).unwrap(), original);
}

/// RFC 9670 §5 ShareNotification, unchanged by a round trip.
#[test]
fn share_notification_round_trips() {
    let original = fixture("share-notification");
    let n: ShareNotification = serde_json::from_value(original.clone()).expect("ShareNotification");
    assert_eq!(serde_json::to_value(n).unwrap(), original);
}

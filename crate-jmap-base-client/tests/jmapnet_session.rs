//! Cross-implementation fixtures from JMAP.Net (github.com/JMAP-Net/JMAP.Net
//! at db60d47, MIT, Copyright (c) 2025 JMAP.Net), bd JMAP-48o0. They are
//! that library's serializer output, checked by its own test suite (165 of
//! 166 passing; the failure is an HTTP limit test, not a fixture). JMAP.Net
//! targets draft-ietf-jmap-calendars-26 and RFC 8620; where it and this
//! crate disagree the draft text decides (calendars-34 here), and each
//! disagreement is asserted with its citation. See tests/fixtures/jmapnet/README.md.

use jmap_base_client::Session;
use jmap_types::Id;

fn session(name: &str) -> Session {
    let path = format!(
        "{}/tests/fixtures/jmapnet/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture"))
        .expect("RFC 8620 §2 Session")
}

/// RFC 8620 §2: the Session resource's members, read as JMAP.Net wrote them.
#[test]
fn session_with_accounts() {
    let s = session("jmap-session-with-accounts");
    assert!(s.capabilities.contains_key("urn:ietf:params:jmap:core"));
    assert!(!s.accounts.is_empty());
    for id in s.primary_accounts.values() {
        assert!(
            s.accounts.contains_key(id),
            "primaryAccounts names an account: {id}"
        );
    }
}

/// A Session advertising Calendars, Principals and availability
/// (calendars-26 §1.5, RFC 9670 §1.5).
#[test]
fn session_with_calendar_capabilities() {
    let s = session("jmap-session-with-calendar-capabilities");
    assert_eq!(s.username, "user@example.com");
    assert_eq!(
        s.capabilities["urn:ietf:params:jmap:core"]["maxObjectsInGet"],
        500
    );
    let account = &s.accounts[&Id::from("account1")];
    assert!(
        account
            .account_capabilities
            .contains_key("urn:ietf:params:jmap:calendars")
    );
    assert_eq!(
        s.primary_accounts["urn:ietf:params:jmap:calendars"],
        Id::from("account1")
    );
}

//! Hardcoded JMAP Session resource (RFC 8620 §2) for the testjig.
//!
//! Returned verbatim by `GET /.well-known/jmap`. Single hardcoded
//! account under a single hardcoded principal; all 9 workspace
//! capability URIs advertised so any client built against any of the
//! workspace's extension crates can opt in.
//!
//! The Session JSON is **hand-written and pinned**, not derived from
//! any per-extension session-builder. A future slice may make it
//! dynamic if a per-test customisation point is needed; for the MVP
//! the static value is sufficient and keeps the surface immune to
//! drift in extension crates' session-fragment shapes.

use serde_json::{json, Value};

/// The single hardcoded account-id the testjig serves.
///
/// Every advertised capability has its primary account set to this id,
/// and the `accounts` map has a single entry keyed by this id.
pub const ACCOUNT_ID: &str = "testjig-account";

/// The single hardcoded principal username the testjig authenticates as.
///
/// Returned in the Session's `username` field. Not a real email
/// address; the `.local` TLD is a reserved private-use TLD per
/// RFC 6762.
pub const USERNAME: &str = "testuser@testjig.local";

/// The pinned `sessionState` token returned in every Session response
/// and echoed by every JMAP API response.
///
/// The testjig never mutates session-level metadata, so this value is
/// constant across the process lifetime. Real servers rotate this on
/// every change to session-level data per RFC 8620 §2.
pub const STATE: &str = "testjig-state-0";

/// Base URL the testjig binds on by default. Used to construct the
/// API / download / upload / event-source URLs in the Session.
///
/// Operators who run the jig on a different port must regenerate the
/// Session JSON; for the MVP this is hardcoded.
pub const BASE_URL: &str = "http://127.0.0.1:8080";

/// The 9 capability URIs the testjig advertises: RFC 8620 base `core`
/// plus the 8 workspace extension capabilities (mail, chat, calendars,
/// tasks, contacts, filenode, sharing, metadata).
///
/// Each URI also appears as a key in `primaryAccounts` mapping to
/// [`ACCOUNT_ID`], so a client opting into any of them sees the same
/// single test account.
pub const ADVERTISED_CAPABILITIES: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
    "urn:ietf:params:jmap:chat",
    "urn:ietf:params:jmap:calendars",
    "urn:ietf:params:jmap:tasks",
    "urn:ietf:params:jmap:contacts",
    "urn:ietf:params:jmap:filenode",
    "urn:ietf:params:jmap:sharing",
    "urn:ietf:params:jmap:metadata",
];

/// Build the testjig's hardcoded Session JSON.
///
/// Returns a fresh `serde_json::Value` every call; the value is small
/// enough that re-building is cheaper than caching with `OnceLock`.
///
/// Shape matches RFC 8620 §2:
///
/// - `capabilities`: every URI in [`ADVERTISED_CAPABILITIES`] maps to
///   an empty object `{}`. The Core capability MUST include
///   `urn:ietf:params:jmap:core` with the RFC-defined limit fields, so
///   that key gets the suggested-minimum limits from RFC 8620 §2.
///   Other capabilities ship with empty objects at the MVP; per-spec
///   typed fields can be added by future slices as needed.
/// - `accounts`: single entry keyed by [`ACCOUNT_ID`]; `name` =
///   [`USERNAME`]; not personal, not read-only;
///   `accountCapabilities` re-advertises all 8 extension URIs (Core
///   is conventionally session-only, not per-account).
/// - `primaryAccounts`: every extension URI maps to [`ACCOUNT_ID`].
///   `urn:ietf:params:jmap:core` is NOT in `primaryAccounts` per
///   RFC 8620 §2 ("a value MUST be present for each capability in the
///   capabilities object that has methods callable on an account") —
///   Core has no account-scoped methods.
/// - URLs constructed from [`BASE_URL`] with RFC 6570 (level 1)
///   templates for `downloadUrl` / `uploadUrl` / `eventSourceUrl`.
/// - `state`: [`STATE`].
pub fn session_json() -> Value {
    // Core capability object per RFC 8620 §2: required UnsignedInt and
    // String[] fields with the spec's suggested minima.
    let core_capability = json!({
        "maxSizeUpload": 50_000_000_u64,
        "maxConcurrentUpload": 4_u32,
        "maxSizeRequest": crate::http::MAX_REQUEST_BYTES,
        "maxConcurrentRequests": 4_u32,
        "maxCallsInRequest": crate::http::MAX_CALLS_IN_REQUEST,
        "maxObjectsInGet": 500_u32,
        "maxObjectsInSet": 500_u32,
        // RFC 8620 §2 references RFC 4790 collation identifiers; the
        // i;ascii-casemap collation is the most universally supported
        // and is the minimum any RFC 4790-aware client expects.
        "collationAlgorithms": ["i;ascii-casemap"],
    });

    let mut capabilities = serde_json::Map::with_capacity(ADVERTISED_CAPABILITIES.len());
    capabilities.insert("urn:ietf:params:jmap:core".to_owned(), core_capability);
    for uri in ADVERTISED_CAPABILITIES
        .iter()
        .filter(|u| **u != "urn:ietf:params:jmap:core")
    {
        capabilities.insert((*uri).to_owned(), json!({}));
    }

    // Account-capabilities: the 8 extension URIs (Core is session-only).
    let mut account_capabilities =
        serde_json::Map::with_capacity(ADVERTISED_CAPABILITIES.len() - 1);
    for uri in ADVERTISED_CAPABILITIES
        .iter()
        .filter(|u| **u != "urn:ietf:params:jmap:core")
    {
        account_capabilities.insert((*uri).to_owned(), json!({}));
    }

    let accounts = json!({
        ACCOUNT_ID: {
            "name": USERNAME,
            "isPersonal": true,
            "isReadOnly": false,
            "accountCapabilities": Value::Object(account_capabilities),
        }
    });

    // primaryAccounts: every account-scoped capability points to the
    // single test account. Core is intentionally omitted (no
    // account-scoped methods).
    let mut primary_accounts = serde_json::Map::with_capacity(ADVERTISED_CAPABILITIES.len() - 1);
    for uri in ADVERTISED_CAPABILITIES
        .iter()
        .filter(|u| **u != "urn:ietf:params:jmap:core")
    {
        primary_accounts.insert((*uri).to_owned(), Value::String(ACCOUNT_ID.to_owned()));
    }

    json!({
        "capabilities": Value::Object(capabilities),
        "accounts": accounts,
        "primaryAccounts": Value::Object(primary_accounts),
        "username": USERNAME,
        "apiUrl": format!("{BASE_URL}/jmap"),
        "downloadUrl": format!("{BASE_URL}/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
        "uploadUrl": format!("{BASE_URL}/upload/{{accountId}}/"),
        "eventSourceUrl": format!("{BASE_URL}/events?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
        "state": STATE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: RFC 8620 §2 — Session top-level fields and their
    /// expected shapes.
    #[test]
    fn session_has_required_top_level_fields() {
        let s = session_json();
        let obj = s.as_object().expect("Session must be a JSON object");
        for required in [
            "capabilities",
            "accounts",
            "primaryAccounts",
            "username",
            "apiUrl",
            "downloadUrl",
            "uploadUrl",
            "eventSourceUrl",
            "state",
        ] {
            assert!(
                obj.contains_key(required),
                "Session must contain RFC 8620 §2 required field: {required}"
            );
        }
    }

    /// Oracle: RFC 8620 §2 — the `capabilities` object MUST include
    /// `urn:ietf:params:jmap:core`.
    #[test]
    fn session_advertises_core_capability() {
        let s = session_json();
        assert!(
            s["capabilities"]["urn:ietf:params:jmap:core"].is_object(),
            "Session.capabilities MUST include urn:ietf:params:jmap:core per RFC 8620 §2"
        );
    }

    /// Oracle: the testjig's claimed advertisement (bead acceptance
    /// criterion: "all 8 capability URIs advertised", expanded to the
    /// 9 URIs the bead's example list actually enumerates).
    #[test]
    fn session_advertises_all_nine_workspace_capabilities() {
        let s = session_json();
        let caps = s["capabilities"]
            .as_object()
            .expect("capabilities must be an object");
        for uri in ADVERTISED_CAPABILITIES {
            assert!(
                caps.contains_key(*uri),
                "advertised capability missing from Session: {uri}"
            );
        }
    }

    /// Oracle: RFC 8620 §2 — `urn:ietf:params:jmap:core` value object
    /// MUST contain the eight defined fields (maxSizeUpload,
    /// maxConcurrentUpload, maxSizeRequest, maxConcurrentRequests,
    /// maxCallsInRequest, maxObjectsInGet, maxObjectsInSet,
    /// collationAlgorithms).
    #[test]
    fn core_capability_has_rfc_required_fields() {
        let s = session_json();
        let core = &s["capabilities"]["urn:ietf:params:jmap:core"];
        for field in [
            "maxSizeUpload",
            "maxConcurrentUpload",
            "maxSizeRequest",
            "maxConcurrentRequests",
            "maxCallsInRequest",
            "maxObjectsInGet",
            "maxObjectsInSet",
            "collationAlgorithms",
        ] {
            assert!(
                !core[field].is_null(),
                "urn:ietf:params:jmap:core missing required field: {field}"
            );
        }
    }

    /// Oracle: RFC 8620 §2 — every entry in `primaryAccounts` MUST
    /// reference an id that appears in `accounts`.
    #[test]
    fn primary_accounts_reference_known_account() {
        let s = session_json();
        let accounts = s["accounts"]
            .as_object()
            .expect("accounts must be an object");
        let primary = s["primaryAccounts"]
            .as_object()
            .expect("primaryAccounts must be an object");
        for (cap_uri, account_id) in primary {
            let id = account_id
                .as_str()
                .unwrap_or_else(|| panic!("primaryAccounts[{cap_uri}] must be a string"));
            assert!(
                accounts.contains_key(id),
                "primaryAccounts[{cap_uri}] = {id}, but accounts has no such id"
            );
        }
    }

    /// Oracle: RFC 8620 §2 — `urn:ietf:params:jmap:core` MUST NOT
    /// appear in `primaryAccounts` (it has no account-scoped methods).
    #[test]
    fn primary_accounts_excludes_core() {
        let s = session_json();
        assert!(
            !s["primaryAccounts"]
                .as_object()
                .unwrap()
                .contains_key("urn:ietf:params:jmap:core"),
            "Core has no account-scoped methods; primaryAccounts must omit it per RFC 8620 §2"
        );
    }

    /// Oracle: the testjig's hardcoded account id is the same value
    /// every `primaryAccounts` entry points to.
    #[test]
    fn primary_accounts_all_point_to_testjig_account() {
        let s = session_json();
        let primary = s["primaryAccounts"].as_object().unwrap();
        for (cap_uri, account_id) in primary {
            assert_eq!(
                account_id.as_str(),
                Some(ACCOUNT_ID),
                "primaryAccounts[{cap_uri}] must point to ACCOUNT_ID"
            );
        }
    }
}

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

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

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

/// Address the testjig binds on by default (`cargo run` without
/// `--port`).
///
/// [`session_json`] advertises URLs for this address; a jig bound
/// anywhere else serves [`session_json_for`] its actual bound address
/// so the advertised `apiUrl` reaches it (RFC 8620 §2: `apiUrl` is
/// "the URL to use for JMAP API requests").
pub const DEFAULT_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080));

/// Base URL of [`DEFAULT_ADDR`]: the origin of the API / download /
/// upload / event-source URLs in [`session_json`].
pub const BASE_URL: &str = "http://127.0.0.1:8080";

/// The 9 capability URIs the testjig advertises: RFC 8620 base `core`
/// plus the 8 workspace extension capabilities (mail, chat, calendars,
/// tasks, contacts, filenode, sharing, metadata).
///
/// Each URI also appears as a key in `primaryAccounts` mapping to
/// [`ACCOUNT_ID`], so a client opting into any of them sees the same
/// single test account.
///
/// The RFC 8887 `urn:ietf:params:jmap:websocket` capability is also
/// advertised but is intentionally NOT in this list — it has no
/// account-scoped methods (a session-level transport binding) and so
/// is omitted from `primaryAccounts` and `accountCapabilities`. It is
/// inserted directly into the `capabilities` object by
/// [`session_json`] alongside the rest.
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

/// The RFC 8887 WebSocket subprotocol capability URI. Not in
/// [`ADVERTISED_CAPABILITIES`] because it carries no account-scoped
/// methods — see that constant's doc-comment.
pub const WEBSOCKET_CAPABILITY: &str = "urn:ietf:params:jmap:websocket";

/// WebSocket endpoint [`session_json`] advertises in the
/// [`WEBSOCKET_CAPABILITY`]'s `url` field (the `ws://` form of
/// [`DEFAULT_ADDR`]).
///
/// RFC 8887 §3 says this MUST be a `wss://` URI for production; the
/// testjig binds plaintext HTTP on localhost only, so we advertise
/// the matching `ws://` URI alongside the rest of the
/// [`BASE_URL`]-derived endpoints. Production deployments behind a
/// TLS-terminating reverse proxy MAY surface a `wss://` URL here
/// (RFC 8887 §5.1 explicitly allows this for TLS-terminated
/// upstreams).
pub const WEBSOCKET_URL: &str = "ws://127.0.0.1:8080/ws";

/// Build the testjig's Session JSON for a jig bound on [`DEFAULT_ADDR`].
///
/// Equivalent to `session_json_for(DEFAULT_ADDR)`.
pub fn session_json() -> Value {
    session_json_for(DEFAULT_ADDR)
}

/// Build the testjig's Session JSON for a jig bound on `addr`.
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
/// - `accountCapabilities["urn:ietf:params:jmap:chat"]` carries the
///   fields draft-atwood-jmap-chat-00 §Account-Level Capability Object
///   requires; every other account capability is `{}`.
/// - `accounts`: single entry keyed by [`ACCOUNT_ID`]; `name` =
///   [`USERNAME`]; not personal, not read-only;
///   `accountCapabilities` re-advertises all 8 extension URIs (Core
///   is conventionally session-only, not per-account).
/// - `primaryAccounts`: every extension URI maps to [`ACCOUNT_ID`].
///   `urn:ietf:params:jmap:core` is NOT in `primaryAccounts` per
///   RFC 8620 §2 ("a value MUST be present for each capability in the
///   capabilities object that has methods callable on an account") —
///   Core has no account-scoped methods.
/// - URLs constructed from `http://{addr}` with RFC 6570 (level 1)
///   templates for `downloadUrl` / `uploadUrl` / `eventSourceUrl`;
///   the WebSocket `url` is `ws://{addr}/ws`.
/// - `state`: [`STATE`].
pub fn session_json_for(addr: SocketAddr) -> Value {
    let base_url = format!("http://{addr}");
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

    let mut capabilities = serde_json::Map::with_capacity(ADVERTISED_CAPABILITIES.len() + 1);
    capabilities.insert("urn:ietf:params:jmap:core".to_owned(), core_capability);
    for uri in ADVERTISED_CAPABILITIES
        .iter()
        .filter(|u| **u != "urn:ietf:params:jmap:core")
    {
        capabilities.insert((*uri).to_owned(), json!({}));
    }

    // RFC 8887 §3 WebSocket transport binding. `supportsPush=true`
    // because slice bd:JMAP-cf7p.5 implements the PushEnable/Disable
    // handshake; the push payload format matches the SSE state-change
    // poller (bd:JMAP-cf7p.4).
    capabilities.insert(
        WEBSOCKET_CAPABILITY.to_owned(),
        json!({
            "url": format!("ws://{addr}/ws"),
            "supportsPush": true,
        }),
    );

    // Account-capabilities: the 8 extension URIs (Core is session-only).
    let mut account_capabilities =
        serde_json::Map::with_capacity(ADVERTISED_CAPABILITIES.len() - 1);
    for uri in ADVERTISED_CAPABILITIES
        .iter()
        .filter(|u| **u != "urn:ietf:params:jmap:core")
    {
        account_capabilities.insert((*uri).to_owned(), json!({}));
    }
    // ponytail: the reference chat backend enforces only the body cap
    // (message.rs rejects bodies over 100_000 bytes) and the body types
    // (message.rs SUPPORTED_BODY_TYPES), and does not parse
    // attachments, so the attachment limits mirror the upload cap and
    // an arbitrary count rather than an enforced rule. It keeps the
    // default `ChatLimits::supports_threads` of `false`, hence
    // `supportsThreads: false`.
    account_capabilities.insert(
        "urn:ietf:params:jmap:chat".to_owned(),
        json!({
            "maxBodyBytes": 100_000_u64,
            "maxAttachmentBytes": 50_000_000_u64,
            "maxAttachmentsPerMessage": 16_u64,
            "supportedBodyTypes": jmap_chat_server::message::SUPPORTED_BODY_TYPES,
            "supportsThreads": false,
        }),
    );

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
        "apiUrl": format!("{base_url}/jmap"),
        "downloadUrl": format!("{base_url}/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
        "uploadUrl": format!("{base_url}/upload/{{accountId}}/"),
        "eventSourceUrl": format!("{base_url}/events?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
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

    /// Oracle: RFC 8620 §2 — `apiUrl` is "The URL to use for JMAP API
    /// requests", so every advertised endpoint must name the address the
    /// jig is actually listening on, not the default 8080.
    #[test]
    fn session_json_for_advertises_bound_addr() {
        let addr: SocketAddr = "127.0.0.1:4321".parse().unwrap();
        let s = session_json_for(addr);
        assert_eq!(s["apiUrl"], "http://127.0.0.1:4321/jmap");
        assert_eq!(
            s["downloadUrl"],
            "http://127.0.0.1:4321/download/{accountId}/{blobId}/{name}?accept={type}"
        );
        assert_eq!(s["uploadUrl"], "http://127.0.0.1:4321/upload/{accountId}/");
        assert_eq!(
            s["eventSourceUrl"],
            "http://127.0.0.1:4321/events?types={types}&closeafter={closeafter}&ping={ping}"
        );
        assert_eq!(
            s["capabilities"][WEBSOCKET_CAPABILITY]["url"],
            "ws://127.0.0.1:4321/ws"
        );
    }

    /// Oracle: draft-atwood-jmap-chat-00 §Account-Level Capability
    /// Object — "The value of `accountCapabilities["urn:ietf:params:jmap:chat"]`
    /// is a JSON object with the following fields: `maxBodyBytes`,
    /// `maxAttachmentBytes`, `maxAttachmentsPerMessage`,
    /// `supportedBodyTypes`, `supportsThreads`", and `supportedBodyTypes`
    /// "MUST include `"text/plain"`".
    #[test]
    fn chat_account_capability_has_spec_required_fields() {
        let s = session_json();
        let cap = &s["accounts"][ACCOUNT_ID]["accountCapabilities"]["urn:ietf:params:jmap:chat"];
        for field in [
            "maxBodyBytes",
            "maxAttachmentBytes",
            "maxAttachmentsPerMessage",
        ] {
            assert!(cap[field].is_u64(), "{field} must be an UnsignedInt: {cap}");
        }
        assert!(cap["supportsThreads"].is_boolean(), "{cap}");
        let body_types = cap["supportedBodyTypes"]
            .as_array()
            .expect("supportedBodyTypes must be a String[]");
        assert!(body_types.contains(&json!("text/plain")), "{cap}");
    }
}

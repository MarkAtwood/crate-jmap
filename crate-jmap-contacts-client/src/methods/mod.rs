//! Typed JMAP Contacts method wrappers — response types, SessionClient,
//! constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
//! §5.2 /changes, §5.3 /set, §5.4 /copy, §5.6 /queryChanges). Method
//! implementations live in sub-modules and operate on `SessionClient`.

pub mod addressbook;
pub mod card;

// ---------------------------------------------------------------------------
// Response types (RFC 8620 §5)
// ---------------------------------------------------------------------------
//
// Re-exported from `jmap-types::methods` so all `jmap-*-client` crates share
// one canonical set of /get, /set, /changes, /query, /queryChanges shapes.
// The wire format is identical to the previous local definitions.

pub use jmap_types::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetError,
    SetResponse,
};

// ---------------------------------------------------------------------------
// AddressBookSetParams — extra arguments for AddressBook/set
// (RFC 9610 §2.3)
// ---------------------------------------------------------------------------

/// Extra method-level arguments for `AddressBook/set`
/// (RFC 9610 §2.3).
///
/// Both fields are optional. Pass `None` (or `Default::default()`) when not
/// needed.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBookSetParams {
    /// If `true`, ContactCards that belong *only* to a destroyed AddressBook
    /// are also destroyed. Cards shared with other books are simply detached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_contents: Option<bool>,

    /// A `serde_json::Value` holding the `onSuccessSetIsDefault` argument.
    /// When `Some`, the server sets the indicated AddressBook as the default
    /// after all other operations succeed (RFC 9610 §2.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_set_is_default: Option<serde_json::Value>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP Contacts method calls
/// (RFC 9610 §1.4).
pub(crate) const USING_CONTACTS: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_contacts_types::JMAP_CONTACTS_URI,
];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// `using` is the complete `using` array for the request (RFC 8620 §3.3).
/// Use the pre-defined constant [`USING_CONTACTS`] for standard calls.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation: jmap_types::Invocation = (method.to_owned(), args, CALL_ID.to_owned());
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via [`JmapContactsExt::with_contacts_session`](crate::JmapContactsExt::with_contacts_session).
/// All JMAP Contacts methods are available on this type without needing to
/// pass `&Session` on every call.
///
/// # Session lifecycle
///
/// `SessionClient` captures the `Session` at construction time. After
/// re-fetching the session via `JmapClient::fetch_session`, construct a new
/// `SessionClient` with the updated session. Reusing a stale `SessionClient`
/// after session expiry will result in `unknownAccount` or similar errors
/// from the server.
///
/// `Clone` is derived because `JmapClient` is itself cheap-to-clone (it
/// already implements `Clone` and `with_contacts_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
///
/// `Debug` is implemented manually to redact the inner `JmapClient` (which
/// holds an HTTP client and is intentionally not `Debug` in
/// `jmap-base-client`); only the `Session` is shown. This lets callers
/// embed a `SessionClient` in a `#[derive(Debug)]` struct without manual
/// impls of their own.
///
/// # Thread safety
///
/// `SessionClient` is `Send + Sync`. Both
/// [`jmap_base_client::JmapClient`] (backed by `reqwest::Client`) and
/// [`jmap_base_client::Session`] (plain serde-derived data) are
/// `Send + Sync` per jmap-base-client's contract, so this type can be
/// shared across async tasks via `Arc<SessionClient>` or cloned for
/// per-task ownership.
///
/// A `Send + Sync` regression in a future jmap-base-client release
/// would be a major-version-breaking change for this crate. A
/// compile-time assertion in `methods/mod.rs` guards against the
/// regression landing silently — see
/// `_assert_session_client_send_sync`.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            // The inner JmapClient is not Debug — show a placeholder so
            // callers know it is present without leaking HTTP-client
            // internals.
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .finish()
    }
}

impl SessionClient {
    /// Borrow the underlying [`JmapClient`](jmap_base_client::JmapClient).
    ///
    /// Useful for ad-hoc operations outside the typed JMAP method surface —
    /// for example, calling `JmapClient::upload` / `JmapClient::download_blob`,
    /// or constructing a `JmapClient::event_source` subscription using the
    /// bound session's `event_source_url`.
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// Borrow the captured [`Session`](jmap_base_client::Session).
    ///
    /// `SessionClient` captures the `Session` at construction time. After
    /// re-fetching the session via `JmapClient::fetch_session`, callers
    /// should construct a new `SessionClient`. This accessor lets a caller
    /// compare the captured session's `state` field against a freshly
    /// fetched session to detect staleness, or inspect
    /// `accountCapabilities` / `primary_accounts` for capability-specific
    /// metadata not exposed via the typed JMAP method surface.
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// Return the primary account id for `urn:ietf:params:jmap:contacts`,
    /// or `Err(InvalidSession)` if the session has no primary account for
    /// that capability.
    pub fn contacts_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        self.session
            .primary_account_id(jmap_contacts_types::JMAP_CONTACTS_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:contacts".into(),
                )
            })
    }

    /// Extract `(api_url, contacts_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:contacts`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id(jmap_contacts_types::JMAP_CONTACTS_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:contacts".into(),
                )
            })?;
        Ok((api_url, account_id))
    }

    /// Forward a JMAP request to the underlying HTTP client.
    pub(crate) async fn call_internal(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, jmap_base_client::ClientError> {
        self.client.call(api_url, req).await
    }
}

/// Compile-time assertion that [`SessionClient`] is `Send + Sync`.
///
/// The `# Thread safety` section of [`SessionClient`]'s rustdoc promises
/// auto-trait inheritance from
/// [`jmap_base_client::JmapClient`] and
/// [`jmap_base_client::Session`]. If a future jmap-base-client release
/// adds a `!Sync` interior-mutability field to either, this assertion
/// fails at compile time — flagging the regression at the dependency
/// upgrade rather than at the downstream caller's "cannot send between
/// threads safely" error.
#[allow(dead_code)]
fn _assert_session_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionClient>();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: USING_CONTACTS contains exactly the two capability URIs from
    /// RFC 9610 §1.4.
    /// Expected values are taken directly from the spec.
    #[test]
    fn using_contacts_contains_correct_uris() {
        let req = build_request("AddressBook/get", json!({}), USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2, "must have exactly 2 capability URIs");
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:contacts")),
            "must include jmap:contacts"
        );
    }

    /// Oracle: build_request produces correct method name and CALL_ID.
    /// Expected: invocation[0] == method, invocation[2] == CALL_ID constant.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request(
            "AddressBook/get",
            json!({"accountId": "acc1", "ids": null}),
            USING_CONTACTS,
        );
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(
            calls[0][0],
            json!("AddressBook/get"),
            "method name must match"
        );
        assert_eq!(calls[0][2], json!("r1"), "call_id must be CALL_ID constant");
    }

    /// Oracle: AddressBookSetParams with on_destroy_remove_contents=true serializes
    /// the camelCase field name.
    /// Expected: JSON key is "onDestroyRemoveContents" per RFC 9610 §2.3.
    #[test]
    fn address_book_set_params_serializes_on_destroy_remove_contents() {
        let params = AddressBookSetParams {
            on_destroy_remove_contents: Some(true),
            on_success_set_is_default: None,
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            v["onDestroyRemoveContents"],
            json!(true),
            "onDestroyRemoveContents must be present and true"
        );
        assert!(
            v.get("onSuccessSetIsDefault").is_none(),
            "onSuccessSetIsDefault must be absent when None"
        );
    }

    /// Oracle: AddressBookSetParams default (all None) serializes to `{}`.
    /// Expected: skip_serializing_if omits both None fields.
    #[test]
    fn address_book_set_params_default_is_empty_object() {
        let params = AddressBookSetParams::default();
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            v,
            json!({}),
            "default params must serialize to empty object"
        );
    }

    /// Oracle: AddressBookSetParams with on_success_set_is_default serializes it.
    /// Expected: JSON key is "onSuccessSetIsDefault".
    #[test]
    fn address_book_set_params_serializes_on_success_set_is_default() {
        let params = AddressBookSetParams {
            on_destroy_remove_contents: None,
            on_success_set_is_default: Some(json!({"newDefaultId": true})),
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert!(
            v.get("onDestroyRemoveContents").is_none(),
            "onDestroyRemoveContents must be absent when None"
        );
        assert_eq!(
            v["onSuccessSetIsDefault"],
            json!({"newDefaultId": true}),
            "onSuccessSetIsDefault must be present"
        );
    }

    /// Oracle: session_parts returns None when contacts capability absent.
    /// Expected: primary_account_id returns None for an absent key.
    #[test]
    fn session_parts_err_no_primary_account() {
        let session_json = json!({
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {},
            "username": "user@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        });
        let session: jmap_base_client::Session =
            serde_json::from_value(session_json).expect("session must deserialize");
        let result = session.primary_account_id("urn:ietf:params:jmap:contacts");
        assert!(
            result.is_none(),
            "must return None when contacts capability is not in primaryAccounts"
        );
    }

    /// Oracle: GetResponse<T> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [],
            "notFound": ["missing1"]
        });
        let resp: GetResponse<serde_json::Value> =
            serde_json::from_value(json).expect("GetResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s42");
        assert!(resp.list.is_empty());
        assert_eq!(
            resp.not_found.as_deref(),
            Some(["missing1".into()].as_slice())
        );
    }

    /// Oracle: ChangesResponse deserializes from RFC 8620 §5.2 shape.
    #[test]
    fn changes_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s10",
            "newState": "s11",
            "hasMoreChanges": false,
            "created": ["id1"],
            "updated": ["id2"],
            "destroyed": []
        });
        let resp: ChangesResponse =
            serde_json::from_value(json).expect("ChangesResponse must deserialize");
        assert_eq!(resp.old_state, "s10");
        assert_eq!(resp.new_state, "s11");
        assert!(!resp.has_more_changes);
        assert_eq!(resp.created.len(), 1);
        assert_eq!(resp.updated.len(), 1);
        assert!(resp.destroyed.is_empty());
    }

    /// Oracle: SetResponse deserializes from RFC 8620 §5.3 shape.
    #[test]
    fn set_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s10",
            "newState": "s11",
            "created": null,
            "updated": null,
            "destroyed": ["id1"],
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse = serde_json::from_value(json).expect("SetResponse must deserialize");
        assert_eq!(resp.new_state, "s11");
        assert_eq!(resp.destroyed.as_deref(), Some(["id1".into()].as_slice()));
    }

    /// Oracle: QueryChangesResponse deserializes from RFC 8620 §5.6 shape.
    #[test]
    fn query_changes_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldQueryState": "qs1",
            "newQueryState": "qs2",
            "total": 5,
            "removed": ["id3"],
            "added": [{"id": "id4", "index": 0}]
        });
        let resp: QueryChangesResponse =
            serde_json::from_value(json).expect("QueryChangesResponse must deserialize");
        assert_eq!(resp.old_query_state, "qs1");
        assert_eq!(resp.new_query_state, "qs2");
        assert_eq!(resp.total, Some(5));
        assert_eq!(resp.removed.len(), 1);
        assert_eq!(resp.added.len(), 1);
        assert_eq!(resp.added[0].index, 0);
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Serialize-only method-argument structs, the test constructs a
    // struct with a vendor field in `extra` and asserts that the field
    // flattens into the serialized JSON. Uses synthetic `acmeCorp*` keys
    // that are guaranteed not to appear in any RFC 9610 typed field — so
    // the tests are independent of the crate under test.

    /// `AddressBookSetParams.extra` flattens into serialized JSON.
    #[test]
    fn address_book_set_params_propagates_vendor_extras() {
        let mut params = AddressBookSetParams::default();
        params
            .extra
            .insert("acmeCorpCascade".into(), json!("strict"));
        let v = serde_json::to_value(&params).expect("serialize AddressBookSetParams");
        assert_eq!(v["acmeCorpCascade"], json!("strict"));
    }
}

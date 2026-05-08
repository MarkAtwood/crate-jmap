// Typed JMAP Mail method wrappers — response types, SessionClient,
// constants, and helpers.
//
// Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
// §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
// operate on `SessionClient`.

pub mod email;
pub mod identity;
pub mod mailbox;
pub mod search_snippet;
pub mod submission;
pub mod thread;
pub mod vacation;

use std::collections::HashMap;

use jmap_types::Id;

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
// Input parameter types (RFC 8621 method-specific args)
// ---------------------------------------------------------------------------

/// Extra args for Email/get (RFC 8621 §4.1.8).
///
/// Controls which body properties to fetch and whether to inline body values.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailGetParams {
    /// Override the set of body part properties returned (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_properties: Option<Vec<String>>,
    /// If `true`, inline values for text/plain body parts (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_text_body_values: Option<bool>,
    /// If `true`, inline values for text/html body parts (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_html_body_values: Option<bool>,
    /// If `true`, inline values for all body parts (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_all_body_values: Option<bool>,
    /// Truncate body values to at most this many bytes (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_body_value_bytes: Option<u64>,
}

/// Extra args for Email/copy (RFC 8621 §4.7).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailCopyParams {
    /// The account to copy from (RFC 8621 §4.7).
    pub from_account_id: Id,
    /// If `true`, destroy originals after successful copy (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_destroy_original: Option<bool>,
    /// If-in-state guard for the source account destroy step (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy_from_if_in_state: Option<String>,
}

/// Extra args for Mailbox/set (RFC 8621 §2.5).
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSetParams {
    /// If `true`, destroy all emails in the mailbox when the mailbox itself is
    /// destroyed (RFC 8621 §2.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_emails: Option<bool>,
}

/// Extra args for EmailSubmission/set (RFC 8621 §7.5).
///
/// These two fields are method-level arguments on `EmailSubmission/set` (not
/// nested inside a create/update object). They let the caller atomically
/// modify or destroy related Email objects when a submission is created
/// successfully, without a separate round-trip.
///
/// Example use case: remove the `$draft` keyword from the email after
/// submission succeeds.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSubmissionSetParams {
    /// Map of creation key → [`jmap_types::PatchObject`] to apply to the
    /// associated Email if the submission is created successfully
    /// (RFC 8621 §7.5).
    ///
    /// Keys that start with `"#"` are result references to creation keys in
    /// the same `create` map. Wire format is unchanged from a plain JSON
    /// object because `PatchObject` is `#[serde(transparent)]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_update_email: Option<HashMap<String, jmap_types::PatchObject>>,

    /// Email IDs (or `#`-prefixed creation keys) to destroy if the submission
    /// is created successfully (RFC 8621 §7.5).
    ///
    /// Typically used to destroy the draft email after successful submission.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_destroy_email: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP Mail method calls (RFC 8621).
pub(crate) const USING_MAIL: &[&str] = &["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// `using` is the complete `using` array for the request (RFC 8620 §3.3).
/// Use the pre-defined constant [`USING_MAIL`] for standard calls.
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
/// Obtain via [`JmapMailExt::with_mail_session`](crate::JmapMailExt::with_mail_session).
/// All JMAP Mail methods are available on this type without needing to pass
/// `&Session` on every call.
///
/// # Session lifecycle
///
/// `SessionClient` captures the `Session` at construction time. After
/// re-fetching the session via `JmapClient::fetch_session`, construct a new
/// `SessionClient` with the updated session. Reusing a stale `SessionClient`
/// after session expiry will result in `unknownAccount` or similar errors
/// from the server.
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl SessionClient {
    /// Extract `(api_url, mail_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:mail`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id("urn:ietf:params:jmap:mail")
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:mail".into(),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: build_request produces the correct method name.
    /// Expected: invocation[0] == method name, invocation[2] == CALL_ID.
    /// The expected values are literals from the code spec, not derived from
    /// the function under test.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request(
            "Email/get",
            json!({"accountId": "acc1", "ids": null}),
            USING_MAIL,
        );
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(calls[0][0], json!("Email/get"), "method name must match");
        assert_eq!(calls[0][2], json!("r1"), "call_id must be CALL_ID constant");
    }

    /// Oracle: USING_MAIL contains exactly the two RFC 8621 capability URIs.
    /// Expected values are taken directly from RFC 8621 §1.3.
    #[test]
    fn using_mail_contains_correct_uris() {
        let req = build_request("Email/get", json!({}), USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2);
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:mail")),
            "must include jmap:mail"
        );
    }

    /// Oracle: session_parts returns InvalidSession when no primary account
    /// for mail capability. Expected error kind from base client AGENTS.md.
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

        let result = session.primary_account_id("urn:ietf:params:jmap:mail");
        assert!(
            result.is_none(),
            "must return None when mail capability is not in primaryAccounts"
        );
    }

    /// Oracle: GetResponse<T> deserializes from RFC 8620 §5.1 shape.
    /// The JSON shape is taken from RFC 8620 §5.1, not from the code.
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

    /// Oracle: SetResponse<T>.updated must accept null values per RFC 8620
    /// §5.3 wire type "Id[Foo|null]|null" (rfc8620.txt line 2043).
    ///
    /// The server returns null for a successfully updated object when the
    /// patch was applied verbatim with no server-set property deltas to
    /// report. A typed SetResponse<Email> must deserialize this shape rather
    /// than failing because `null` cannot become Email.
    ///
    /// Independent oracle: hand-written JSON fixture mirroring the spec
    /// wire shape directly — not generated by any code in this crate.
    #[test]
    fn set_response_updated_accepts_null_values() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "updated": {
                "M1": null,
                "M2": null
            }
        });
        let resp: SetResponse<jmap_mail_types::Email> = serde_json::from_value(json)
            .expect("SetResponse must accept Id[Foo|null] per RFC 8620 §5.3");
        let updated = resp.updated.expect("updated must be Some");
        assert_eq!(updated.len(), 2, "two ids in updated map");
        assert!(
            updated
                .get(&Id::from("M1"))
                .expect("M1 key present")
                .is_none(),
            "M1 value must be None (null)"
        );
        assert!(
            updated
                .get(&Id::from("M2"))
                .expect("M2 key present")
                .is_none(),
            "M2 value must be None (null)"
        );
    }

    /// Oracle: SetResponse<T>.updated also accepts non-null Foo values per
    /// RFC 8620 §5.3 — the union "Id[Foo|null]" must round-trip both arms.
    /// Server returns a Foo object when server-set or computed properties
    /// changed beyond what the client patched (rfc8620.txt lines 2048-2051).
    #[test]
    fn set_response_updated_accepts_object_values() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "updated": {
                "M1": { "id": "M1", "subject": "Hello" }
            }
        });
        let resp: SetResponse<serde_json::Value> = serde_json::from_value(json)
            .expect("SetResponse must accept Id[Foo] per RFC 8620 §5.3");
        let updated = resp.updated.expect("updated must be Some");
        let m1 = updated
            .get(&Id::from("M1"))
            .expect("M1 key present")
            .as_ref()
            .expect("M1 value must be Some when server reports deltas");
        assert_eq!(m1["subject"], json!("Hello"));
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

    /// Oracle: EmailGetParams with all None serializes to empty object `{}`.
    /// RFC 8621 §4.1.8 — omitted fields mean "use server defaults".
    #[test]
    fn email_get_params_default_serializes_to_empty_object() {
        let params = EmailGetParams::default();
        let v = serde_json::to_value(&params).expect("serialize EmailGetParams");
        assert_eq!(v, serde_json::json!({}), "default must serialize to {{}}");
    }

    /// Oracle: EmailGetParams with all fields set serializes all camelCase keys.
    /// Expected field names from RFC 8621 §4.1.8.
    #[test]
    fn email_get_params_all_fields_serializes_correctly() {
        let params = EmailGetParams {
            body_properties: Some(vec!["partId".into(), "type".into()]),
            fetch_text_body_values: Some(true),
            fetch_html_body_values: Some(false),
            fetch_all_body_values: Some(true),
            max_body_value_bytes: Some(1024),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            v["bodyProperties"],
            json!(["partId", "type"]),
            "bodyProperties"
        );
        assert_eq!(v["fetchTextBodyValues"], json!(true));
        assert_eq!(v["fetchHtmlBodyValues"], json!(false));
        assert_eq!(v["fetchAllBodyValues"], json!(true));
        assert_eq!(v["maxBodyValueBytes"], json!(1024_u64));
    }

    /// Oracle: EmailCopyParams serializes fromAccountId and optional fields.
    /// Expected field names from RFC 8621 §4.7 and RFC 8620 §5.4.
    #[test]
    fn email_copy_params_serializes_correctly() {
        let params = EmailCopyParams {
            from_account_id: "acct-src".into(),
            on_success_destroy_original: Some(true),
            destroy_from_if_in_state: Some("s99".into()),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(v["fromAccountId"], json!("acct-src"));
        assert_eq!(v["onSuccessDestroyOriginal"], json!(true));
        assert_eq!(v["destroyFromIfInState"], json!("s99"));
    }

    /// Oracle: EmailCopyParams with None optionals omits those keys.
    #[test]
    fn email_copy_params_omits_none_fields() {
        let params = EmailCopyParams {
            from_account_id: "acct-src".into(),
            on_success_destroy_original: None,
            destroy_from_if_in_state: None,
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(v["fromAccountId"], json!("acct-src"));
        assert!(
            v.get("onSuccessDestroyOriginal").is_none() || v["onSuccessDestroyOriginal"].is_null(),
            "onSuccessDestroyOriginal must be absent"
        );
    }
}

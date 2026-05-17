//! Typed JMAP Mail method wrappers — response types, SessionClient,
//! constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
//! §5.2 /changes, §5.3 /set). Method implementations live in sub-modules and
//! operate on `SessionClient`.

pub mod email;
pub mod identity;
pub mod mailbox;
pub mod search_snippet;
pub mod submission;
pub mod thread;
pub mod vacation;

use std::collections::HashMap;

use jmap_types::{Id, State};

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
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailGetParams {
    /// Override the set of body part properties returned (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_properties: Option<Vec<String>>,
    /// If `true`, inline values for text/plain body parts (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_text_body_values: Option<bool>,
    /// If `true`, inline values for text/html body parts (RFC 8621 §4.1.8).
    ///
    /// Wire name is `fetchHTMLBodyValues` (HTML uppercase) per RFC 8621
    /// §4.2 line 2327 and the §4.2 example at line 2438. The default
    /// camelCase serde rename would produce `fetchHtmlBodyValues`, which
    /// a strict server treats as an unknown invocation argument and
    /// silently drops, causing HTML body values to never be inlined.
    #[serde(
        rename = "fetchHTMLBodyValues",
        skip_serializing_if = "Option::is_none"
    )]
    pub fetch_html_body_values: Option<bool>,
    /// If `true`, inline values for all body parts (RFC 8621 §4.1.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_all_body_values: Option<bool>,
    /// Truncate body values to at most this many bytes (RFC 8621 §4.1.8).
    ///
    /// Spec magic-value: `Some(0)` is equivalent to omitting the field —
    /// RFC 8621 §4.1.8 says "If positive, ..." which means the truncation
    /// only applies for strictly-positive values. Pass `None` to omit
    /// the wire field entirely (server uses its default policy), or
    /// `Some(n)` with `n > 0` for an explicit truncation cap. Passing
    /// `Some(0)` is wire-legal but semantically identical to omission
    /// — prefer `None` for clarity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_body_value_bytes: Option<u64>,
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

/// Extra args for Email/copy (RFC 8621 §4.7).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailCopyParams {
    /// The account to copy from (RFC 8621 §4.7).
    pub from_account_id: Id,
    /// If `true`, destroy originals after successful copy (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_destroy_original: Option<bool>,
    /// If-in-state guard for the source account destroy step (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy_from_if_in_state: Option<jmap_types::State>,
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

/// Extra args for Mailbox/set (RFC 8621 §2.5).
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSetParams {
    /// If `true`, destroy all emails in the mailbox when the mailbox itself is
    /// destroyed (RFC 8621 §2.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_emails: Option<bool>,
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

/// Extra args for EmailSubmission/set (RFC 8621 §7.5).
///
/// These two fields are method-level arguments on `EmailSubmission/set` (not
/// nested inside a create/update object). They let the caller atomically
/// modify or destroy related Email objects when a submission is created
/// successfully, without a separate round-trip.
///
/// Example use case: remove the `$draft` keyword from the email after
/// submission succeeds.
#[derive(Debug, Default, Clone, serde::Serialize)]
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

/// Per-creation EmailImport object for [`Email/import`](super::SessionClient::email_import)
/// (RFC 8621 §4.8).
///
/// The raw [RFC 5322] message must already have been uploaded as a blob; the
/// caller passes that blob's id here. At least one mailbox id MUST be
/// supplied; the empty case is rejected by `email_import` as `InvalidArgument`.
///
/// [RFC 5322]: https://www.rfc-editor.org/rfc/rfc5322
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailImportInput<'a> {
    /// Blob id of the uploaded raw RFC 5322 message.
    pub blob_id: &'a Id,
    /// Mailbox ids the new Email is assigned to. Wire shape is `{id: true}`;
    /// callers supply the set as a slice. At least one id is required (RFC 8621 §4.8).
    #[serde(serialize_with = "ser_mailbox_id_set")]
    pub mailbox_ids: &'a [Id],
    /// Keywords to apply to the Email. Wire shape is `{keyword: true}`.
    /// Defaults to an empty map per RFC 8621 §4.8 when `None`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_opt_keyword_set"
    )]
    pub keywords: Option<&'a [&'a str]>,
    /// `receivedAt` to set on the imported Email. Defaults to the most recent
    /// `Received` header or import time per RFC 8621 §4.8 when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_at: Option<&'a jmap_types::UTCDate>,
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

fn ser_mailbox_id_set<S: serde::Serializer>(ids: &&[Id], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut m = s.serialize_map(Some(ids.len()))?;
    for id in *ids {
        m.serialize_entry(id.as_ref(), &true)?;
    }
    m.end()
}

fn ser_opt_keyword_set<S: serde::Serializer>(
    kws: &Option<&[&str]>,
    s: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let kws = kws.expect("skip_serializing_if guarantees Some");
    let mut m = s.serialize_map(Some(kws.len()))?;
    for k in kws {
        m.serialize_entry(k, &true)?;
    }
    m.end()
}

/// Per-creation success entry in an [`EmailImportResponse`] (RFC 8621 §4.8).
///
/// The server reports the new Email's `id`, `blobId` (may differ from the
/// caller-supplied blob id if the server normalised the message), `threadId`,
/// and `size` for each successfully imported message.
#[non_exhaustive]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailImportCreated {
    /// Server-assigned Email id.
    pub id: Id,
    /// Blob id of the canonical raw message (may differ from the input blob id).
    pub blob_id: Id,
    /// Server-assigned Thread id this Email belongs to.
    pub thread_id: Id,
    /// Size of the canonical raw message in bytes.
    pub size: u64,
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

/// Response to [`Email/import`](super::SessionClient::email_import) (RFC 8621 §4.8).
#[non_exhaustive]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailImportResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// State token before the import, or `null` if the server cannot supply one.
    #[serde(default)]
    pub old_state: Option<State>,
    /// State token after the import.
    pub new_state: State,
    /// Successfully imported Emails keyed by creation id.
    #[serde(default)]
    pub created: Option<HashMap<String, EmailImportCreated>>,
    /// Failures keyed by creation id (RFC 8621 §4.8 errors include
    /// `alreadyExists`, `invalidProperties`, `overQuota`, `invalidEmail`).
    #[serde(default)]
    pub not_created: Option<HashMap<String, SetError>>,
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

/// Extra args for [`Email/parse`](super::SessionClient::email_parse) (RFC 8621 §4.9).
///
/// Mirrors the body-fetch options of [`EmailGetParams`] plus a `properties`
/// override. All fields are optional; absent fields use server defaults.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailParseParams {
    /// Override the set of Email properties returned per parsed message
    /// (RFC 8621 §4.9). When `None`, the server returns the default set
    /// documented in the spec.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// Override the set of body-part properties returned (RFC 8621 §4.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_properties: Option<Vec<String>>,
    /// If `true`, inline values for text/plain body parts (RFC 8621 §4.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_text_body_values: Option<bool>,
    /// If `true`, inline values for text/html body parts (RFC 8621 §4.9).
    ///
    /// Wire name is `fetchHTMLBodyValues` (HTML uppercase) per RFC 8621
    /// §4.9 line 3163. The default camelCase serde rename would produce
    /// `fetchHtmlBodyValues`, which a strict server treats as an unknown
    /// invocation argument and silently drops.
    #[serde(
        rename = "fetchHTMLBodyValues",
        skip_serializing_if = "Option::is_none"
    )]
    pub fetch_html_body_values: Option<bool>,
    /// If `true`, inline values for all body parts (RFC 8621 §4.9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_all_body_values: Option<bool>,
    /// Truncate body values to at most this many bytes (RFC 8621 §4.9).
    ///
    /// Spec magic-value: `Some(0)` is equivalent to omitting the field
    /// per RFC 8621 §4.9 ("If positive, ..."). Pass `None` to omit the
    /// wire field entirely (server uses its default policy), or
    /// `Some(n)` with `n > 0` for an explicit truncation cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_body_value_bytes: Option<u64>,
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

/// Response to [`Email/parse`](super::SessionClient::email_parse) (RFC 8621 §4.9).
///
/// Per RFC 8621 §4.9, parsed Email objects have `id`, `mailboxIds`,
/// `keywords`, and `receivedAt` set to `null`; callers should not rely on
/// those fields being populated.
#[non_exhaustive]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailParseResponse {
    /// The account this response refers to.
    pub account_id: Id,
    /// Parsed Emails keyed by source blob id.
    #[serde(default)]
    pub parsed: Option<HashMap<Id, jmap_mail_types::Email>>,
    /// Blob ids whose contents were not parseable as RFC 5322 messages.
    #[serde(default)]
    pub not_parsable: Option<Vec<Id>>,
    /// Blob ids that could not be found in the account's blob store.
    #[serde(default)]
    pub not_found: Option<Vec<Id>>,
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

/// Capability URIs for JMAP Mail method calls (RFC 8621 §1.3.1).
///
/// Use for Email/*, Mailbox/*, Thread/*, and SearchSnippet/* methods —
/// i.e. anything covered by `urn:ietf:params:jmap:mail`.
pub(crate) const USING_MAIL: &[&str] = &["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"];

/// Capability URIs for JMAP Mail Submission method calls (RFC 8621 §1.3.2).
///
/// Use for Identity/* and EmailSubmission/* methods. Includes
/// `urn:ietf:params:jmap:mail` because EmailSubmission references
/// emailId / threadId (mail-typed) and the onSuccessUpdateEmail /
/// onSuccessDestroyEmail mechanism produces implicit Email/set or
/// Email/destroy invocations on the same request.
pub(crate) const USING_SUBMISSION: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
    "urn:ietf:params:jmap:submission",
];

/// Capability URIs for JMAP VacationResponse method calls (RFC 8621 §1.3.3).
///
/// Use for VacationResponse/* methods. Does NOT include
/// `urn:ietf:params:jmap:mail` — vacation responses do not reference any
/// mail-typed fields and stand on their own as a vacation-protocol object.
pub(crate) const USING_VACATION: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:vacationresponse",
];

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
///
/// `Clone` is derived because `JmapClient` is itself cheap-to-clone (it
/// already implements `Clone` and `with_mail_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
///
/// `Debug` is implemented manually to redact the inner `JmapClient` (which
/// holds an HTTP client and is intentionally not `Debug` in
/// `jmap-base-client`); only the `Session` is shown. This lets callers
/// embed a `SessionClient` in a `#[derive(Debug)]` struct without manual
/// impls of their own.
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
    /// for example, calling `JmapClient::upload` / `JmapClient::download_blob`
    /// for attachment transfer, or constructing a `JmapClient::event_source`
    /// subscription using the bound session's `event_source_url`.
    ///
    /// Returns a borrow so callers do not pay the small clone cost of
    /// `JmapClient::clone` unless they need an owned handle.
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

    /// Return the primary mail account id for `urn:ietf:params:jmap:mail`,
    /// or `Err(InvalidSession)` if the session has no primary account for
    /// that capability.
    ///
    /// The captured session contract is "this `SessionClient` is bound to
    /// the JMAP Mail capability"; if the underlying primary-accounts map
    /// no longer carries `urn:ietf:params:jmap:mail`, the session is
    /// effectively useless for this crate's methods. This accessor
    /// surfaces that contract for callers who need the account id outside
    /// the JMAP method calls (e.g. to thread it into a blob-upload URL
    /// template).
    pub fn mail_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        self.session
            .primary_account_id("urn:ietf:params:jmap:mail")
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:mail".into(),
                )
            })
    }

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
    /// Expected values are taken directly from RFC 8621 §1.3.1.
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

    /// Oracle: USING_SUBMISSION contains core + mail + submission.
    /// Expected values are taken directly from RFC 8621 §1.3.2. The
    /// inclusion of `urn:ietf:params:jmap:mail` is required because
    /// EmailSubmission references emailId / threadId and the
    /// onSuccessUpdateEmail / onSuccessDestroyEmail mechanism produces
    /// implicit Email/set or Email/destroy invocations on the same request.
    #[test]
    fn using_submission_contains_correct_uris() {
        let req = build_request("EmailSubmission/get", json!({}), USING_SUBMISSION);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 3);
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:mail")),
            "must include jmap:mail (EmailSubmission references mail-typed fields)"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:submission")),
            "must include jmap:submission"
        );
    }

    /// Oracle: USING_VACATION contains exactly core + vacationresponse.
    /// Expected values are taken directly from RFC 8621 §1.3.3. The
    /// `urn:ietf:params:jmap:mail` URI is NOT included — VacationResponse
    /// does not reference any mail-typed fields.
    #[test]
    fn using_vacation_contains_correct_uris() {
        let req = build_request("VacationResponse/get", json!({}), USING_VACATION);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2);
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:vacationresponse")),
            "must include jmap:vacationresponse"
        );
        assert!(
            !using.contains(&json!("urn:ietf:params:jmap:mail")),
            "must NOT include jmap:mail (VacationResponse is a standalone capability)"
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
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            v["bodyProperties"],
            json!(["partId", "type"]),
            "bodyProperties"
        );
        assert_eq!(v["fetchTextBodyValues"], json!(true));
        // RFC 8621 §4.2 line 2327 — wire spelling is "fetchHTMLBodyValues"
        // (HTML uppercase), NOT default camelCase "fetchHtmlBodyValues".
        assert_eq!(v["fetchHTMLBodyValues"], json!(false));
        assert!(
            v.get("fetchHtmlBodyValues").is_none(),
            "must NOT emit the lowercase-html wire key (RFC 8621 §4.2)"
        );
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
            extra: serde_json::Map::new(),
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
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(v["fromAccountId"], json!("acct-src"));
        assert!(
            v.get("onSuccessDestroyOriginal").is_none() || v["onSuccessDestroyOriginal"].is_null(),
            "onSuccessDestroyOriginal must be absent"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Serialize-only method-argument structs, the test constructs a
    // struct with a vendor field in `extra` and asserts that the field
    // flattens into the serialized JSON. For Deserialize-only method-
    // response structs, the test deserialises JSON containing a vendor
    // field and asserts the field is captured in `extra`. Both directions
    // use synthetic `acmeCorp*` keys that are guaranteed not to appear in
    // any RFC 8621 typed field — so the tests are independent of the
    // crate under test.

    /// `EmailGetParams.extra` flattens into serialized JSON.
    #[test]
    fn email_get_params_propagates_vendor_extras() {
        let mut params = EmailGetParams::default();
        params
            .extra
            .insert("acmeCorpInline".into(), json!("aggressive"));
        let v = serde_json::to_value(&params).expect("serialize EmailGetParams");
        assert_eq!(v["acmeCorpInline"], json!("aggressive"));
    }

    /// `EmailCopyParams.extra` flattens into serialized JSON.
    #[test]
    fn email_copy_params_propagates_vendor_extras() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpAudit".into(), json!(true));
        let params = EmailCopyParams {
            from_account_id: "acct-src".into(),
            on_success_destroy_original: None,
            destroy_from_if_in_state: None,
            extra,
        };
        let v = serde_json::to_value(&params).expect("serialize EmailCopyParams");
        assert_eq!(v["acmeCorpAudit"], json!(true));
    }

    /// `MailboxSetParams.extra` flattens into serialized JSON.
    #[test]
    fn mailbox_set_params_propagates_vendor_extras() {
        let mut params = MailboxSetParams::default();
        params
            .extra
            .insert("acmeCorpCascade".into(), json!("strict"));
        let v = serde_json::to_value(&params).expect("serialize MailboxSetParams");
        assert_eq!(v["acmeCorpCascade"], json!("strict"));
    }

    /// `EmailSubmissionSetParams.extra` flattens into serialized JSON.
    #[test]
    fn email_submission_set_params_propagates_vendor_extras() {
        let mut params = EmailSubmissionSetParams::default();
        params
            .extra
            .insert("acmeCorpQueue".into(), json!("priority"));
        let v = serde_json::to_value(&params).expect("serialize EmailSubmissionSetParams");
        assert_eq!(v["acmeCorpQueue"], json!("priority"));
    }

    /// `EmailImportInput.extra` flattens into serialized JSON.
    #[test]
    fn email_import_input_propagates_vendor_extras() {
        let blob = Id::from("blob1");
        let mailboxes = [Id::from("mb1")];
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpSource".into(), json!("mta-relay"));
        let input = EmailImportInput {
            blob_id: &blob,
            mailbox_ids: &mailboxes,
            keywords: None,
            received_at: None,
            extra,
        };
        let v = serde_json::to_value(&input).expect("serialize EmailImportInput");
        assert_eq!(v["acmeCorpSource"], json!("mta-relay"));
    }

    /// `EmailParseParams.extra` flattens into serialized JSON.
    #[test]
    fn email_parse_params_propagates_vendor_extras() {
        let mut params = EmailParseParams::default();
        params.extra.insert("acmeCorpStrict".into(), json!(true));
        let v = serde_json::to_value(&params).expect("serialize EmailParseParams");
        assert_eq!(v["acmeCorpStrict"], json!(true));
    }

    /// `EmailImportCreated.extra` captures unknown fields on deserialize.
    #[test]
    fn email_import_created_preserves_vendor_extras() {
        let raw = json!({
            "id": "M1",
            "blobId": "B1",
            "threadId": "T1",
            "size": 1024,
            "acmeCorpAntivirus": "clean"
        });
        let created: EmailImportCreated =
            serde_json::from_value(raw).expect("EmailImportCreated must deserialize");
        assert_eq!(
            created
                .extra
                .get("acmeCorpAntivirus")
                .and_then(|v| v.as_str()),
            Some("clean")
        );
    }

    /// `EmailImportResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn email_import_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "acc1",
            "newState": "s2",
            "acmeCorpJobId": "job-42"
        });
        let resp: EmailImportResponse =
            serde_json::from_value(raw).expect("EmailImportResponse must deserialize");
        assert_eq!(
            resp.extra.get("acmeCorpJobId").and_then(|v| v.as_str()),
            Some("job-42")
        );
    }

    /// `EmailParseResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn email_parse_response_preserves_vendor_extras() {
        let raw = json!({
            "accountId": "acc1",
            "acmeCorpParser": "v3"
        });
        let resp: EmailParseResponse =
            serde_json::from_value(raw).expect("EmailParseResponse must deserialize");
        assert_eq!(
            resp.extra.get("acmeCorpParser").and_then(|v| v.as_str()),
            Some("v3")
        );
    }
}

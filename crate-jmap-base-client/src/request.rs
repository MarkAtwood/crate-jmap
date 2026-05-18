//! Base JMAP request and session types: [`JmapRequestBuilder`], [`Session`],
//! [`AccountInfo`], [`WebSocketCapability`].
//!
//! Types that belong to the base JMAP client layer (RFC 8620 §2, §3.3, RFC 8887).
//! Chat-specific and Mail-specific types live in their own crates.
//!
//! Types already in `jmap-types` and NOT redefined here:
//! `Id`, `UTCDate`, `State`, `Date`, `JmapRequest`, `JmapResponse`, `Invocation`,
//! `ResultReference`.

use std::collections::HashMap;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use jmap_types::{Invocation, JmapRequest, State};

use crate::error::ClientError;

// ---------------------------------------------------------------------------
// JmapUrl / JmapUrlTemplate (bd:JMAP-6r7c.40)
// ---------------------------------------------------------------------------

/// A plain JMAP URL — no RFC 6570 template variables expected.
///
/// This is the typed counterpart to [`JmapUrlTemplate`] (which requires
/// expansion before use). The Session document distinguishes the two at
/// the type level so callers cannot accidentally pass an unexpanded
/// template (e.g. `https://server/download/{accountId}/{blobId}/{name}`)
/// to a function that wants a plain URL.
///
/// Construct via [`JmapUrl::new`]. The string is taken as-is — no URL
/// parsing or validation; downstream consumers (reqwest, http crate)
/// validate at the actual request site. Borrow the inner string via
/// [`as_str`](Self::as_str) for `&str`-accepting APIs.
///
/// Deliberately does NOT implement `Deref<Target = str>`. Auto-coercion
/// would defeat the type distinction with [`JmapUrlTemplate`]: both
/// would coerce to `&str` and pass any `&str`-accepting function. Use
/// `.as_str()` at the call site so the type transition is visible in
/// code review.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct JmapUrl(String);

impl JmapUrl {
    /// Wrap a string as a plain JMAP URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// Borrow the inner URL string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the inner `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for JmapUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JmapUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for JmapUrl {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for JmapUrl {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<JmapUrl> for str {
    fn eq(&self, other: &JmapUrl) -> bool {
        self == other.0
    }
}

impl PartialEq<JmapUrl> for &str {
    fn eq(&self, other: &JmapUrl) -> bool {
        *self == other.0
    }
}

/// An RFC 6570 Level-1 URI template — requires variable substitution
/// before use as a request URL.
///
/// Typed counterpart to [`JmapUrl`]. The template carries placeholders
/// like `{accountId}` or `{blobId}` that must be expanded via
/// [`expand_url_template`](crate::expand_url_template) before the result
/// can be sent to an HTTP client. Passing the unexpanded template
/// verbatim would produce a request URL containing literal `{...}`
/// braces, which reqwest percent-encodes to `%7B...%7D` and the server
/// rejects.
///
/// Construct via [`JmapUrlTemplate::new`]. See [`JmapUrl`] for the
/// rationale behind not implementing `Deref<Target = str>`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct JmapUrlTemplate(String);

impl JmapUrlTemplate {
    /// Wrap a string as a JMAP URL template.
    pub fn new(template: impl Into<String>) -> Self {
        Self(template.into())
    }

    /// Borrow the inner template string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the inner `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for JmapUrlTemplate {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JmapUrlTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for JmapUrlTemplate {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for JmapUrlTemplate {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<JmapUrlTemplate> for str {
    fn eq(&self, other: &JmapUrlTemplate) -> bool {
        self == other.0
    }
}

impl PartialEq<JmapUrlTemplate> for &str {
    fn eq(&self, other: &JmapUrlTemplate) -> bool {
        *self == other.0
    }
}

// ---------------------------------------------------------------------------
// JmapRequestBuilder (RFC 8620 §3.3)
// ---------------------------------------------------------------------------

/// Fluent builder for multi-method [`JmapRequest`] objects.
///
/// Collects method calls and produces a [`JmapRequest`] ready for dispatch.
///
/// The `using` capability URIs passed to `new` apply to the whole request;
/// callers must include every capability required by the methods they add.
///
/// Spec: RFC 8620 §3.3
#[derive(Debug)]
pub struct JmapRequestBuilder {
    using: Vec<String>,
    method_calls: Vec<Invocation>,
    call_ids: HashSet<String>,
}

impl JmapRequestBuilder {
    /// Create a new builder with the given capability URIs.
    ///
    /// The `using` list MUST include `"urn:ietf:params:jmap:core"` (always
    /// required by RFC 8620 §3.3) plus every capability URI needed by the
    /// methods added via [`add_call`](JmapRequestBuilder::add_call). An
    /// incorrect or empty `using` list will cause the server to return an
    /// `"unknownCapability"` error — the builder does not validate it.
    pub fn new(using: &[&str]) -> Self {
        Self {
            using: using.iter().map(|&s| s.to_owned()).collect(),
            method_calls: Vec::new(),
            call_ids: HashSet::new(),
        }
    }

    /// Add one method call to the request.
    ///
    /// `call_id` must be unique within this request; callers use it to match
    /// responses back to the originating call.
    ///
    /// Returns `Err(ClientError::InvalidArgument)` if `call_id` has already
    /// been used in this builder. Duplicate call IDs violate RFC 8620 §3.5.
    pub fn add_call(
        &mut self,
        method: impl Into<String>,
        args: serde_json::Value,
        call_id: impl Into<String>,
    ) -> Result<&mut Self, ClientError> {
        let call_id = call_id.into();
        if !self.call_ids.insert(call_id.clone()) {
            return Err(ClientError::InvalidArgument(format!(
                "JmapRequestBuilder: duplicate call_id {call_id:?}"
            )));
        }
        self.method_calls.push((method.into(), args, call_id));
        Ok(self)
    }

    /// Consume the builder and produce the [`JmapRequest`].
    ///
    /// Returns `Err(ClientError::InvalidArgument)` if no method calls have
    /// been added. An empty `methodCalls` array is invalid per RFC 8620 §3.3.
    pub fn build(self) -> Result<JmapRequest, ClientError> {
        if self.method_calls.is_empty() {
            return Err(ClientError::InvalidArgument("no method calls added".into()));
        }
        Ok(JmapRequest::new(self.using, self.method_calls, None))
    }
}

// ---------------------------------------------------------------------------
// Session (RFC 8620 §2)
// ---------------------------------------------------------------------------

/// JMAP Session object returned by `GET /.well-known/jmap` (RFC 8620 §2).
///
/// Contains only the base RFC 8620 fields. Extension-specific fields
/// (e.g. JMAP Chat `ownerUserId`) are surfaced by extension crates that
/// parse the `capabilities` and `accounts` maps.
///
/// # `extra` equality is feature-flag-dependent (bd:JMAP-6r7c.43)
///
/// The derived `PartialEq` / `Eq` impl's behaviour on the `extra` field
/// depends on the global `serde_json/preserve_order` feature flag — see
/// the [crate-level note](crate#extra-field-equality-and-the-serde_jsonpreserve_order-feature-bdjmap-6r7c43)
/// for the canonical statement.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Map of capability URI → capability object (RFC 8620 §2).
    ///
    /// Values are kept as raw JSON so callers can extract extension-specific
    /// capability objects without this crate knowing their schema.
    pub capabilities: HashMap<String, serde_json::Value>,

    /// Map of account ID → [`AccountInfo`] (RFC 8620 §2).
    pub accounts: HashMap<String, AccountInfo>,

    /// Map of capability URI → primary account ID (RFC 8620 §2).
    pub primary_accounts: HashMap<String, String>,

    /// Username associated with the current credentials (RFC 8620 §2).
    ///
    /// # ⚠ PII — handle with the same care as a credential (bd:JMAP-6r7c.35)
    ///
    /// This field is typically an email address and is therefore PII under
    /// GDPR / CCPA. The `Session` Debug impl redacts this field to
    /// `"[REDACTED]"`, but the redaction only catches `{:?}`-format paths.
    /// Common ways callers accidentally leak the raw value:
    ///
    /// - `println!("User: {}", session.username)` — `Display`, not redacted.
    /// - `format!("hello {}", session.username)` — `Display`, not redacted.
    /// - `tracing::info!(user = %session.username, ...)` — `%` invokes
    ///   `Display`, not `Debug`.
    /// - `serde_json::to_string(&session)?` — emits the raw value verbatim
    ///   because `Session` derives `Serialize` for wire round-trip.
    /// - `session.username.clone().into_inner()` (no such method exists
    ///   today, but a future newtype migration would expose one) —
    ///   intentional exposure path.
    ///
    /// Do not log this field, do not include it in error messages, do not
    /// serialize it to disk or another network endpoint. If you need a
    /// non-PII session-scoped identifier, prefer
    /// [`primary_accounts`](Session::primary_accounts) account IDs
    /// (RFC 8620 §2's `accountId` is server-opaque and is not PII).
    pub username: String,

    /// URL for JMAP API POST requests (RFC 8620 §2).
    ///
    /// Typed as [`JmapUrl`] (plain URL — no template variables) to
    /// distinguish from the template-shaped URL fields below
    /// (bd:JMAP-6r7c.40). Borrow as `&str` via
    /// [`JmapUrl::as_str`](crate::JmapUrl::as_str) when calling
    /// `&str`-accepting APIs.
    pub api_url: JmapUrl,

    /// URL template for blob downloads (RFC 8620 §2).
    ///
    /// URI Template (level 1) containing variables `accountId`, `blobId`,
    /// `type`, and `name`. Typed as [`JmapUrlTemplate`] so it cannot be
    /// confused with [`api_url`](Self::api_url) at the type level
    /// (bd:JMAP-6r7c.40); expand via
    /// [`expand_url_template`](crate::expand_url_template) before use.
    pub download_url: JmapUrlTemplate,

    /// URL template for blob uploads (RFC 8620 §2).
    ///
    /// URI Template (level 1) containing variable `accountId`. Typed
    /// as [`JmapUrlTemplate`] (bd:JMAP-6r7c.40); see
    /// [`download_url`](Self::download_url) for the type-distinction
    /// rationale.
    pub upload_url: JmapUrlTemplate,

    /// URL template for SSE push event stream (RFC 8620 §2, §7.3).
    ///
    /// URI Template (level 1) containing variables `types`, `closeafter`,
    /// and `ping`. Typed as [`JmapUrlTemplate`] (bd:JMAP-6r7c.40); see
    /// [`download_url`](Self::download_url) for the type-distinction
    /// rationale.
    pub event_source_url: JmapUrlTemplate,

    /// Opaque session state token (RFC 8620 §2).
    ///
    /// Changes whenever any session property changes. Returned in every API
    /// response as `sessionState`; clients compare to detect staleness.
    pub state: State,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Session {
    /// Returns the primary account ID for the given capability URI, if set.
    ///
    /// Example: `session.primary_account_id("urn:ietf:params:jmap:mail")`
    pub fn primary_account_id(&self, capability: &str) -> Option<&str> {
        self.primary_accounts.get(capability).map(String::as_str)
    }

    /// Returns the parsed [`WebSocketCapability`] for the JMAP WebSocket
    /// transport, if advertised (RFC 8887).
    ///
    /// - `Ok(None)` — server does not advertise JMAP WebSocket support.
    /// - `Ok(Some(...))` — WebSocket is supported; use `result.url` to connect.
    /// - `Err` — capability key is present but the value is malformed.
    pub fn websocket_capability(&self) -> Result<Option<WebSocketCapability>, ClientError> {
        self.extension_capability("urn:ietf:params:jmap:websocket")
    }

    /// Returns the parsed extension-capability object for `capability_uri`,
    /// deserialized into the caller-supplied type `T` (bd:JMAP-6r7c.22).
    ///
    /// Use this when an extension defines a typed capability struct (the
    /// way `urn:ietf:params:jmap:websocket` maps to [`WebSocketCapability`])
    /// and you want a typed view instead of poking at the raw
    /// `serde_json::Value` in [`Session::capabilities`]. Each extension
    /// `*-client` crate should expose a typed `XxxCapability` struct and
    /// a thin wrapper like:
    ///
    /// ```rust,ignore
    /// pub fn mail_capability(session: &Session) -> Result<Option<MailCapability>, ClientError> {
    ///     session.extension_capability("urn:ietf:params:jmap:mail")
    /// }
    /// ```
    ///
    /// # Returns
    ///
    /// - `Ok(None)` — server does not advertise this capability.
    /// - `Ok(Some(_))` — capability is advertised AND the value parsed into `T`.
    /// - `Err(ClientError::Parse)` — capability is advertised but the value
    ///   could not be deserialised into `T`. Indicates either a server bug,
    ///   a schema-version mismatch, or a `T` type that does not match the
    ///   spec for `capability_uri`.
    ///
    /// The function only inspects the value when the key is present; an
    /// absent key always returns `Ok(None)` regardless of `T`.
    pub fn extension_capability<T>(&self, capability_uri: &str) -> Result<Option<T>, ClientError>
    where
        T: serde::de::DeserializeOwned,
    {
        let Some(raw) = self.capabilities.get(capability_uri) else {
            return Ok(None);
        };
        T::deserialize(raw)
            .map(Some)
            .map_err(ClientError::from_parse)
    }

    /// Returns `true` if the server advertises the JMAP Blob Content
    /// Identifiers extension (draft-atwood-jmap-cid-00).
    ///
    /// Checks for presence of `capabilities["urn:ietf:params:jmap:cid"]`.
    /// The capability value object is empty per the draft (§2: "no
    /// capability fields defined at this time"), so the presence of the
    /// key is sufficient — no value-shape check is required.
    ///
    /// When `true`, the server commits to including a `sha256` field
    /// (the 64-character lowercase-hex SHA-256 digest of the uploaded
    /// content) on Blob upload responses, and on FileNode objects when
    /// the JMAP FileNode extension is also supported. See
    /// [`jmap_cid_types::Sha256`] for the typed wire shape.
    ///
    /// Mirrors the `supports_*` capability-probe pattern established by
    /// `ChatSessionExt::supports_quotas` and
    /// `ChatSessionExt::supports_refplus` in `jmap-chat-client`.
    ///
    /// [`jmap_cid_types::Sha256`]: https://docs.rs/jmap-cid-types
    pub fn supports_cid(&self) -> bool {
        self.capabilities.contains_key("urn:ietf:params:jmap:cid")
    }
}

/// Manual `Debug` impl that redacts privacy-sensitive fields (bd:JMAP-sc1b.99).
///
/// `Session.username` is the authenticated user's identifier — typically a
/// full email address, which is PII under GDPR/CCPA. `Session.state` is the
/// opaque RFC 8620 §2 session-state token; it is not an auth credential, but
/// it uniquely identifies the client's session and is the same shape of leak
/// as logging a session cookie. Both are replaced with `"[REDACTED]"` /
/// `"[opaque]"` in the Debug output.
///
/// All other URL/map fields are surfaced — they are deployment metadata and
/// not credential-grade. `AccountInfo.name` is redacted by `AccountInfo`'s
/// own manual `Debug` impl, so the `accounts` map below does not leak
/// owner emails transitively (bd:JMAP-sc1b.104).
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("capabilities", &self.capabilities)
            .field("accounts", &self.accounts)
            .field("primary_accounts", &self.primary_accounts)
            .field("username", &"[REDACTED]")
            .field("api_url", &self.api_url)
            .field("download_url", &self.download_url)
            .field("upload_url", &self.upload_url)
            .field("event_source_url", &self.event_source_url)
            .field("state", &"[opaque]")
            .field("extra", &self.extra)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// AccountInfo (RFC 8620 §2 Account object)
// ---------------------------------------------------------------------------

/// Per-account metadata in a JMAP Session (RFC 8620 §2).
///
/// `Debug` is hand-written to redact `name` because the field's own
/// definition identifies it as "typically the owner's email address"
/// (PII under GDPR/CCPA). The other fields are non-credential metadata
/// and are surfaced directly. See bd:JMAP-sc1b.104.
///
/// # `extra` equality is feature-flag-dependent (bd:JMAP-6r7c.43)
///
/// The derived `PartialEq` / `Eq` impl's behaviour on the `extra` field
/// depends on the global `serde_json/preserve_order` feature flag — see
/// the [crate-level note](crate#extra-field-equality-and-the-serde_jsonpreserve_order-feature-bdjmap-6r7c43)
/// for the canonical statement.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    /// Human-readable account name (e.g. the owner's email address).
    ///
    /// # ⚠ PII — same handling rules as [`Session::username`] (bd:JMAP-6r7c.35)
    ///
    /// This field is typically an email address and is therefore PII under
    /// GDPR / CCPA. The `AccountInfo` Debug impl redacts this field to
    /// `"[REDACTED]"`, but the redaction only catches `{:?}`-format paths.
    /// `Display`, `format!("{}")`, `serde_json::to_string`, and
    /// `tracing::info!(name = %account.name, ...)` all leak the raw value.
    /// See [`Session::username`] for the full list of accidental-leak paths
    /// and recommended replacement identifiers.
    pub name: String,

    /// `true` if this is the authenticated user's own personal account.
    pub is_personal: bool,

    /// `true` if the entire account is read-only for the current user.
    pub is_read_only: bool,

    /// Map of capability URI → capability object for this account.
    ///
    /// Values are kept as raw JSON so extension crates can extract
    /// their own capability objects.
    pub account_capabilities: HashMap<String, serde_json::Value>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl AccountInfo {
    /// Returns the parsed per-account extension-capability object for
    /// `capability_uri`, deserialized into the caller-supplied type `T`
    /// (bd:JMAP-6r7c.22).
    ///
    /// Per-account counterpart of [`Session::extension_capability`]. Used
    /// when an extension defines an account-scoped capability shape (e.g.
    /// per-account quotas, per-account folder roots) rather than a
    /// server-wide one.
    ///
    /// # Returns
    ///
    /// - `Ok(None)` — this account does not advertise this capability.
    /// - `Ok(Some(_))` — capability is advertised AND the value parsed into `T`.
    /// - `Err(ClientError::Parse)` — capability is advertised but the value
    ///   could not be deserialised into `T`.
    pub fn account_extension_capability<T>(
        &self,
        capability_uri: &str,
    ) -> Result<Option<T>, ClientError>
    where
        T: serde::de::DeserializeOwned,
    {
        let Some(raw) = self.account_capabilities.get(capability_uri) else {
            return Ok(None);
        };
        T::deserialize(raw)
            .map(Some)
            .map_err(ClientError::from_parse)
    }
}

/// Manual `Debug` impl that redacts `name` (bd:JMAP-sc1b.104).
///
/// `AccountInfo.name` is typically the owner's email address, which is
/// PII under GDPR/CCPA. The other fields (`is_personal`, `is_read_only`,
/// `account_capabilities`) are non-credential metadata and are surfaced
/// directly so `{:?}` output remains useful for debugging.
///
/// This redaction closes the transitive leak through `Session.accounts`
/// — `Session`'s own Debug impl (bd:JMAP-sc1b.99) only redacted
/// `username` and `state` directly and was silent about the accounts
/// map. With `AccountInfo` redacting itself, any `{:?}` of a `Session`
/// is now safe with respect to the canonical email-shaped PII.
impl std::fmt::Debug for AccountInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountInfo")
            .field("name", &"[REDACTED]")
            .field("is_personal", &self.is_personal)
            .field("is_read_only", &self.is_read_only)
            .field("account_capabilities", &self.account_capabilities)
            .field("extra", &self.extra)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// WebSocketCapability (RFC 8887)
// ---------------------------------------------------------------------------

/// Capability object for `"urn:ietf:params:jmap:websocket"` (RFC 8887).
///
/// Advertised in `Session.capabilities` when the server supports JMAP over
/// WebSocket. The `url` field is the `wss://` endpoint to connect to.
///
/// # `extra` equality is feature-flag-dependent (bd:JMAP-6r7c.43)
///
/// The derived `PartialEq` / `Eq` impl's behaviour on the `extra` field
/// depends on the global `serde_json/preserve_order` feature flag — see
/// the [crate-level note](crate#extra-field-equality-and-the-serde_jsonpreserve_order-feature-bdjmap-6r7c43)
/// for the canonical statement.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketCapability {
    /// The WebSocket endpoint URL (`wss://`).
    pub url: String,

    /// Whether the server supports push notifications over this WebSocket.
    #[serde(default)]
    pub supports_push: bool,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // JmapRequestBuilder
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.3 — a request with two method calls serializes to
    /// a JSON object with a "methodCalls" array containing two 3-element arrays.
    /// The expected JSON shape is derived directly from the RFC §3.3 example.
    #[test]
    fn builder_two_calls_serializes_correctly() {
        let mut builder =
            JmapRequestBuilder::new(&["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"]);
        builder
            .add_call(
                "Mailbox/get",
                json!({"accountId": "A13824", "ids": null}),
                "r1",
            )
            .expect("add_call r1 must succeed");
        builder
            .add_call(
                "Email/get",
                json!({"accountId": "A13824", "ids": ["e001"]}),
                "r2",
            )
            .expect("add_call r2 must succeed");
        let req = builder.build().expect("build must succeed with two calls");

        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        // Oracle: RFC 8620 §3.3 — "using" must be present
        assert!(v.get("using").is_some(), "must have 'using' field");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2);
        assert!(using.contains(&json!("urn:ietf:params:jmap:core")));
        assert!(using.contains(&json!("urn:ietf:params:jmap:mail")));

        // Oracle: RFC 8620 §3.3 — "methodCalls" must be present
        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 2, "must have exactly 2 method calls");

        // Oracle: RFC 8620 §3.2 — each invocation is [methodName, args, callId]
        assert_eq!(calls[0][0], json!("Mailbox/get"));
        assert_eq!(calls[0][2], json!("r1"));
        assert_eq!(calls[1][0], json!("Email/get"));
        assert_eq!(calls[1][2], json!("r2"));
    }

    /// Oracle: RFC 8620 §3.3 — build() with no method calls is invalid;
    /// must return Err(InvalidArgument) rather than produce an empty batch.
    #[test]
    fn builder_returns_err_on_empty_build() {
        let result = JmapRequestBuilder::new(&["urn:ietf:params:jmap:core"]).build();
        assert!(
            matches!(result, Err(ClientError::InvalidArgument(_))),
            "empty build must return Err(InvalidArgument), got {result:?}"
        );
    }

    /// Oracle: RFC 8620 §3.5 — call IDs must be unique within a request.
    /// Duplicate call ID returns Err(ClientError::InvalidArgument).
    #[test]
    fn builder_returns_err_on_duplicate_call_id() {
        let mut builder = JmapRequestBuilder::new(&["urn:ietf:params:jmap:core"]);
        builder
            .add_call("Foo/get", json!({}), "r1")
            .expect("first add_call must succeed");
        let result = builder.add_call("Bar/get", json!({}), "r1"); // duplicate
        assert!(
            matches!(result, Err(ClientError::InvalidArgument(_))),
            "duplicate call_id must return Err(InvalidArgument), got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Session
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §2.1 example Session JSON, transcribed from the RFC text.
    /// All field names and values come from the RFC, not from the code under test.
    #[test]
    fn session_deserializes_rfc8620_example() {
        // RFC 8620 §2.1 example — hand-transcribed from spec text.
        let raw = r#"{
            "capabilities": {
                "urn:ietf:params:jmap:core": {
                    "maxSizeUpload": 50000000,
                    "maxConcurrentUpload": 8,
                    "maxSizeRequest": 10000000,
                    "maxConcurrentRequest": 8,
                    "maxCallsInRequest": 32,
                    "maxObjectsInGet": 256,
                    "maxObjectsInSet": 128,
                    "collationAlgorithms": [
                        "i;ascii-numeric",
                        "i;ascii-casemap",
                        "i;unicode-casemap"
                    ]
                },
                "urn:ietf:params:jmap:mail": {},
                "urn:ietf:params:jmap:contacts": {},
                "https://example.com/apis/foobar": {
                    "maxFoosFinangled": 42
                }
            },
            "accounts": {
                "A13824": {
                    "name": "john@example.com",
                    "isPersonal": true,
                    "isReadOnly": false,
                    "accountCapabilities": {
                        "urn:ietf:params:jmap:mail": {
                            "maxMailboxesPerEmail": null,
                            "maxMailboxDepth": 10
                        },
                        "urn:ietf:params:jmap:contacts": {}
                    }
                },
                "A97813": {
                    "name": "jane@example.com",
                    "isPersonal": false,
                    "isReadOnly": true,
                    "accountCapabilities": {
                        "urn:ietf:params:jmap:mail": {
                            "maxMailboxesPerEmail": 1,
                            "maxMailboxDepth": 10
                        }
                    }
                }
            },
            "primaryAccounts": {
                "urn:ietf:params:jmap:mail": "A13824",
                "urn:ietf:params:jmap:contacts": "A13824"
            },
            "username": "john@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/upload/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "75128aab4b1b"
        }"#;

        let session: Session =
            serde_json::from_str(raw).expect("RFC 8620 §2.1 example must deserialize");

        // Oracle: RFC 8620 §2.1
        assert_eq!(session.username, "john@example.com");
        assert_eq!(session.api_url, "https://jmap.example.com/api/");
        assert_eq!(
            session.upload_url,
            "https://jmap.example.com/upload/{accountId}/"
        );
        assert_eq!(
            session.download_url,
            "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}"
        );
        assert_eq!(
            session.event_source_url,
            "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}"
        );
        assert_eq!(session.state, "75128aab4b1b");

        // Oracle: RFC 8620 §2.1 — capabilities map
        assert!(
            session
                .capabilities
                .contains_key("urn:ietf:params:jmap:core"),
            "must have core capability"
        );
        assert!(
            session
                .capabilities
                .contains_key("urn:ietf:params:jmap:mail"),
            "must have mail capability"
        );
        assert!(
            session
                .capabilities
                .contains_key("https://example.com/apis/foobar"),
            "must have vendor capability"
        );

        // Oracle: RFC 8620 §2.1 — accounts map
        assert!(
            session.accounts.contains_key("A13824"),
            "must have account A13824"
        );
        assert!(
            session.accounts.contains_key("A97813"),
            "must have account A97813"
        );

        // Oracle: RFC 8620 §2.1 — primaryAccounts
        assert_eq!(
            session.primary_account_id("urn:ietf:params:jmap:mail"),
            Some("A13824")
        );
        assert_eq!(
            session.primary_account_id("urn:ietf:params:jmap:contacts"),
            Some("A13824")
        );
        assert_eq!(
            session.primary_account_id("urn:ietf:params:jmap:core"),
            None
        );
    }

    // -----------------------------------------------------------------------
    // AccountInfo
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §2.1 example — account A13824 (john@example.com).
    /// Field names and values transcribed directly from the RFC.
    #[test]
    fn account_info_deserializes_rfc8620_example() {
        // RFC 8620 §2.1 example account entry
        let raw = r#"{
            "name": "john@example.com",
            "isPersonal": true,
            "isReadOnly": false,
            "accountCapabilities": {
                "urn:ietf:params:jmap:mail": {
                    "maxMailboxesPerEmail": null,
                    "maxMailboxDepth": 10
                },
                "urn:ietf:params:jmap:contacts": {}
            }
        }"#;

        let account: AccountInfo =
            serde_json::from_str(raw).expect("RFC 8620 §2.1 AccountInfo must deserialize");

        // Oracle: RFC 8620 §2 Account object fields
        assert_eq!(account.name, "john@example.com");
        assert!(account.is_personal, "isPersonal must be true");
        assert!(!account.is_read_only, "isReadOnly must be false");
        assert!(
            account
                .account_capabilities
                .contains_key("urn:ietf:params:jmap:mail"),
            "must have mail capability"
        );
        assert!(
            account
                .account_capabilities
                .contains_key("urn:ietf:params:jmap:contacts"),
            "must have contacts capability"
        );

        // Oracle: RFC 8620 §2.1 — read-only account (A97813 / jane@example.com)
        let raw2 = r#"{
            "name": "jane@example.com",
            "isPersonal": false,
            "isReadOnly": true,
            "accountCapabilities": {
                "urn:ietf:params:jmap:mail": {
                    "maxMailboxesPerEmail": 1,
                    "maxMailboxDepth": 10
                }
            }
        }"#;
        let account2: AccountInfo = serde_json::from_str(raw2)
            .expect("RFC 8620 §2.1 read-only AccountInfo must deserialize");

        assert_eq!(account2.name, "jane@example.com");
        assert!(!account2.is_personal, "isPersonal must be false");
        assert!(account2.is_read_only, "isReadOnly must be true");
    }

    // -----------------------------------------------------------------------
    // WebSocketCapability
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8887 §3 — WebSocketCapability has url and supportsPush fields.
    /// Transcribed from the RFC 8887 capability object definition.
    #[test]
    fn websocket_capability_deserializes() {
        let raw = r#"{"url": "wss://jmap.example.com/ws", "supportsPush": true}"#;
        let cap: WebSocketCapability =
            serde_json::from_str(raw).expect("WebSocketCapability must deserialize");
        assert_eq!(cap.url, "wss://jmap.example.com/ws");
        assert!(cap.supports_push);
    }

    /// Oracle: RFC 8887 §3 — supportsPush defaults to false when absent.
    #[test]
    fn websocket_capability_supports_push_defaults_false() {
        let raw = r#"{"url": "wss://jmap.example.com/ws"}"#;
        let cap: WebSocketCapability =
            serde_json::from_str(raw).expect("WebSocketCapability must deserialize");
        assert_eq!(cap.url, "wss://jmap.example.com/ws");
        assert!(!cap.supports_push, "supportsPush must default to false");
    }

    /// Oracle: Session.websocket_capability() returns Ok(None) when key absent.
    #[test]
    fn session_websocket_capability_absent_returns_ok_none() {
        let raw = r#"{
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        }"#;
        let session: Session = serde_json::from_str(raw).expect("Session must deserialize");
        let result = session.websocket_capability();
        assert!(
            matches!(result, Ok(None)),
            "expected Ok(None), got {result:?}"
        );
    }

    /// Oracle: Session.websocket_capability() returns Ok(Some) when key present and valid.
    #[test]
    fn session_websocket_capability_present_and_valid() {
        let raw = r#"{
            "capabilities": {
                "urn:ietf:params:jmap:websocket": {
                    "url": "wss://jmap.example.com/ws",
                    "supportsPush": true
                }
            },
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        }"#;
        let session: Session = serde_json::from_str(raw).expect("Session must deserialize");
        let ws = session
            .websocket_capability()
            .expect("must not error")
            .expect("websocket capability must be present");
        assert_eq!(ws.url, "wss://jmap.example.com/ws");
        assert!(ws.supports_push);
    }

    /// Oracle: `Session::supports_cid()` returns `false` when the JMAP
    /// CID capability URI is not present in the capabilities map
    /// (bd:JMAP-v9py.14).
    ///
    /// Mirrors the absent-key precedent of
    /// `session_websocket_capability_absent_returns_ok_none`. The test
    /// fixture has an empty capabilities map; the negative answer must
    /// be `false`, not `Err` or panic.
    #[test]
    fn supports_cid_returns_false_when_capability_absent() {
        let raw = r#"{
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        }"#;
        let session: Session = serde_json::from_str(raw).expect("Session must deserialize");
        assert!(!session.supports_cid());
    }

    /// Oracle: `Session::supports_cid()` returns `true` when the JMAP
    /// CID capability URI is present in the capabilities map, even
    /// though the value object is empty per draft-atwood-jmap-cid-00
    /// §2 ("no capability fields defined at this time")
    /// (bd:JMAP-v9py.14).
    #[test]
    fn supports_cid_returns_true_when_capability_present_empty_value() {
        let raw = r#"{
            "capabilities": {
                "urn:ietf:params:jmap:cid": {}
            },
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        }"#;
        let session: Session = serde_json::from_str(raw).expect("Session must deserialize");
        assert!(session.supports_cid());
    }

    /// Oracle: `Session::supports_cid()` checks only for the URI key —
    /// presence with a non-empty value object (vendor extras inside the
    /// CID capability) still returns `true`. The draft reserves the
    /// shape of the capability value but does not currently define any
    /// fields; a server that pre-populates vendor fields under the URI
    /// must still be detected as supporting CID.
    #[test]
    fn supports_cid_returns_true_when_capability_present_with_extra_fields() {
        let raw = r#"{
            "capabilities": {
                "urn:ietf:params:jmap:cid": {
                    "x-vendor-flag": "future-shape"
                }
            },
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        }"#;
        let session: Session = serde_json::from_str(raw).expect("Session must deserialize");
        assert!(session.supports_cid());
    }

    /// Oracle: Session's manual Debug impl never reveals the authenticated
    /// `username` or the opaque `state` token (bd:JMAP-sc1b.99), AND the
    /// `accounts` map does not transitively leak `AccountInfo.name`
    /// (bd:JMAP-sc1b.104). Mirrors the canary tripwire pattern used by
    /// `bearer_auth_debug_does_not_leak_token` and
    /// `basic_auth_debug_does_not_leak_credentials` in auth.rs.
    ///
    /// The canary literals are independent of the Session's internal state —
    /// the test is the oracle, not the code under test. A regression that
    /// re-derives `Debug` on `Session` or `AccountInfo`, or that prints the
    /// username/state/name via a manual impl, would fail the assertion.
    ///
    /// We deliberately reuse `CANARY_USER` in two distinct locations
    /// (`username` and `accounts["a1"].name`) so a single negative
    /// `assert!(!dbg.contains(...))` catches a leak from either path —
    /// the same kind of email-shaped PII surfacing through either field
    /// is the failure we want to fail loudly.
    #[test]
    fn session_debug_does_not_leak_username_or_state() {
        const CANARY_USER: &str = "CANARY-USERNAME-DO-NOT-LEAK@example.com";
        const CANARY_STATE: &str = "CANARY-STATE-TOKEN-DO-NOT-LEAK";
        let raw = format!(
            r#"{{
                "capabilities": {{}},
                "accounts": {{
                    "a1": {{
                        "name": "{CANARY_USER}",
                        "isPersonal": true,
                        "isReadOnly": false,
                        "accountCapabilities": {{}}
                    }}
                }},
                "primaryAccounts": {{}},
                "username": "{CANARY_USER}",
                "apiUrl": "https://jmap.example.com/api/",
                "downloadUrl": "https://jmap.example.com/dl/{{accountId}}/",
                "uploadUrl": "https://jmap.example.com/ul/{{accountId}}/",
                "eventSourceUrl": "https://jmap.example.com/sse/",
                "state": "{CANARY_STATE}"
            }}"#
        );
        let session: Session = serde_json::from_str(&raw).expect("Session must deserialize");

        // Sanity-check: the canary really did land in the AccountInfo —
        // otherwise an empty accounts map would silently make the
        // transitive-leak assertion below tautologically pass.
        let account = session
            .accounts
            .get("a1")
            .expect("accounts['a1'] must deserialize");
        assert_eq!(account.name, CANARY_USER);

        let dbg = format!("{session:?}");
        assert!(
            !dbg.contains(CANARY_USER),
            "Session Debug must not contain the raw username or AccountInfo.name; got: {dbg}"
        );
        assert!(
            !dbg.contains(CANARY_STATE),
            "Session Debug must not contain the raw state token; got: {dbg}"
        );
    }

    /// Oracle: AccountInfo's manual Debug impl never reveals the raw
    /// `name` field (bd:JMAP-sc1b.104). Independent of the Session-level
    /// test above: a regression on AccountInfo alone (e.g. re-deriving
    /// `#[derive(Debug)]`) would be caught here without needing the
    /// Session wrapper.
    #[test]
    fn account_info_debug_does_not_leak_name() {
        const CANARY_NAME: &str = "CANARY-ACCOUNT-NAME-DO-NOT-LEAK@example.com";
        let raw = format!(
            r#"{{
                "name": "{CANARY_NAME}",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {{}}
            }}"#
        );
        let account: AccountInfo =
            serde_json::from_str(&raw).expect("AccountInfo must deserialize");
        // Sanity-check that the canary really did populate `name`.
        assert_eq!(account.name, CANARY_NAME);

        let dbg = format!("{account:?}");
        assert!(
            !dbg.contains(CANARY_NAME),
            "AccountInfo Debug must not contain the raw name; got: {dbg}"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // Synthetic `acmeCorp*` vendor keys cannot collide with any RFC 8620 /
    // RFC 8887 typed field, so the tests are independent of the code under
    // test (workspace test-integrity rule).

    /// `Session.extra` captures unknown fields on deserialize.
    #[test]
    fn session_preserves_vendor_extras() {
        let raw = json!({
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1",
            "acmeCorpDeployment": "prod-eu-west-1"
        });
        let obj: Session = serde_json::from_value(raw).expect("Session must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpDeployment").and_then(|v| v.as_str()),
            Some("prod-eu-west-1")
        );
    }

    /// `AccountInfo.extra` captures unknown fields on deserialize.
    #[test]
    fn account_info_preserves_vendor_extras() {
        let raw = json!({
            "name": "u@example.com",
            "isPersonal": true,
            "isReadOnly": false,
            "accountCapabilities": {},
            "acmeCorpQuotaTier": "gold"
        });
        let obj: AccountInfo = serde_json::from_value(raw).expect("AccountInfo must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpQuotaTier").and_then(|v| v.as_str()),
            Some("gold")
        );
    }

    /// `WebSocketCapability.extra` captures unknown fields on deserialize.
    #[test]
    fn websocket_capability_preserves_vendor_extras() {
        let raw = json!({
            "url": "wss://jmap.example.com/ws",
            "supportsPush": true,
            "acmeCorpHeartbeatMs": 30000
        });
        let obj: WebSocketCapability =
            serde_json::from_value(raw).expect("WebSocketCapability must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpHeartbeatMs")
                .and_then(|v| v.as_u64()),
            Some(30000)
        );
    }

    // -----------------------------------------------------------------------
    // Session::extension_capability / AccountInfo::account_extension_capability
    // (bd:JMAP-6r7c.22)
    // -----------------------------------------------------------------------

    /// Hand-written capability struct standing in for any future extension
    /// (e.g. JMAP Mail / Calendars / Tasks). Has the same shape as
    /// `WebSocketCapability` deliberately — the helper is generic, the
    /// caller supplies the schema.
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct FakeMailCapability {
        max_size_upload: u64,
        max_size_request: u64,
    }

    fn build_session_with_capability(uri: &str, value: serde_json::Value) -> Session {
        let raw = json!({
            "capabilities": { uri: value },
            "accounts": {},
            "primaryAccounts": {},
            "username": "u@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1",
        });
        serde_json::from_value(raw).expect("Session must deserialize")
    }

    /// Oracle: `Session::extension_capability::<T>` returns `Ok(None)` when
    /// the capability key is absent, regardless of `T`.
    #[test]
    fn extension_capability_absent_returns_ok_none() {
        let session = build_session_with_capability(
            "urn:ietf:params:jmap:other",
            json!({"unrelated": "value"}),
        );
        let result: Result<Option<FakeMailCapability>, _> =
            session.extension_capability("urn:ietf:params:jmap:mail");
        assert!(
            matches!(result, Ok(None)),
            "absent capability key must return Ok(None), got {result:?}"
        );
    }

    /// Oracle: `Session::extension_capability::<T>` returns `Ok(Some(T))`
    /// when the capability is present and the value matches `T`.
    #[test]
    fn extension_capability_present_and_valid_returns_ok_some() {
        let session = build_session_with_capability(
            "urn:ietf:params:jmap:mail",
            json!({"maxSizeUpload": 50000000, "maxSizeRequest": 10000000}),
        );
        let cap: FakeMailCapability = session
            .extension_capability("urn:ietf:params:jmap:mail")
            .expect("must not error")
            .expect("capability must be present");
        assert_eq!(cap.max_size_upload, 50_000_000);
        assert_eq!(cap.max_size_request, 10_000_000);
    }

    /// Oracle: `Session::extension_capability::<T>` returns
    /// `Err(ClientError::Parse)` when the capability is present but the
    /// value cannot deserialise into `T` (server bug or schema mismatch).
    #[test]
    fn extension_capability_present_but_malformed_returns_parse_err() {
        let session = build_session_with_capability(
            "urn:ietf:params:jmap:mail",
            // Wrong shape — missing required maxSizeRequest field.
            json!({"maxSizeUpload": 50000000}),
        );
        let result: Result<Option<FakeMailCapability>, _> =
            session.extension_capability("urn:ietf:params:jmap:mail");
        assert!(
            matches!(result, Err(ClientError::Parse(_))),
            "malformed capability value must surface as ClientError::Parse, got {result:?}"
        );
    }

    /// Oracle: `Session::websocket_capability()` delegates to
    /// `extension_capability` and the existing semantics are preserved
    /// (regression test for the refactor).
    #[test]
    fn websocket_capability_still_works_after_refactor() {
        let session = build_session_with_capability(
            "urn:ietf:params:jmap:websocket",
            json!({"url": "wss://jmap.example.com/ws", "supportsPush": true}),
        );
        let ws = session
            .websocket_capability()
            .expect("must not error")
            .expect("websocket capability must be present");
        assert_eq!(ws.url, "wss://jmap.example.com/ws");
        assert!(ws.supports_push);
    }

    /// Oracle: `AccountInfo::account_extension_capability::<T>` returns
    /// `Ok(None)` when the per-account capability key is absent.
    #[test]
    fn account_extension_capability_absent_returns_ok_none() {
        let raw = json!({
            "name": "alice@example.com",
            "isPersonal": true,
            "isReadOnly": false,
            "accountCapabilities": {},
        });
        let acct: AccountInfo = serde_json::from_value(raw).expect("AccountInfo must deserialize");
        let result: Result<Option<FakeMailCapability>, _> =
            acct.account_extension_capability("urn:ietf:params:jmap:mail");
        assert!(
            matches!(result, Ok(None)),
            "absent per-account capability must return Ok(None), got {result:?}"
        );
    }

    /// Oracle: `AccountInfo::account_extension_capability::<T>` returns
    /// `Ok(Some(T))` when the per-account capability is present and valid.
    #[test]
    fn account_extension_capability_present_and_valid() {
        let raw = json!({
            "name": "alice@example.com",
            "isPersonal": true,
            "isReadOnly": false,
            "accountCapabilities": {
                "urn:ietf:params:jmap:mail": {
                    "maxSizeUpload": 50000000,
                    "maxSizeRequest": 10000000,
                },
            },
        });
        let acct: AccountInfo = serde_json::from_value(raw).expect("AccountInfo must deserialize");
        let cap: FakeMailCapability = acct
            .account_extension_capability("urn:ietf:params:jmap:mail")
            .expect("must not error")
            .expect("capability must be present");
        assert_eq!(cap.max_size_upload, 50_000_000);
    }

    // bd:JMAP-6r7c.40 — Typed URL wrappers (JmapUrl, JmapUrlTemplate)

    /// `JmapUrl` and `JmapUrlTemplate` are distinct types at the type
    /// level. A function that takes `&JmapUrlTemplate` MUST refuse a
    /// `&JmapUrl` argument and vice versa. This is the compile-time
    /// guard that prevents callers from accidentally passing
    /// `session.api_url` (a plain URL) to a function expecting a
    /// template, or `session.upload_url` (a template) to a function
    /// expecting a plain URL.
    ///
    /// Implemented as compile-time witness: the function bodies do
    /// nothing useful; if either signature compiled with the other
    /// type, the test would break the type-distinction invariant.
    #[test]
    fn jmap_url_and_template_are_distinct_types() {
        fn _takes_plain_url(_u: &JmapUrl) {}
        fn _takes_template(_t: &JmapUrlTemplate) {}

        let plain = JmapUrl::new("https://example.com/api/");
        let template = JmapUrlTemplate::new("https://example.com/upload/{accountId}/");

        _takes_plain_url(&plain);
        _takes_template(&template);

        // The interesting non-compilation cases:
        //   _takes_plain_url(&template);     // FAILS: expected JmapUrl, got JmapUrlTemplate
        //   _takes_template(&plain);          // FAILS: expected JmapUrlTemplate, got JmapUrl
        // These cannot be expressed as runtime assertions; the test's
        // value is locking in the distinct-types invariant so a future
        // refactor that accidentally collapses the wrappers (e.g. a
        // `type JmapUrlTemplate = JmapUrl;` alias) breaks the function
        // signatures above and the build fails.
    }

    /// `JmapUrl` round-trips through serde_json as a transparent
    /// string. Oracle: hand-written JSON containing a quoted string.
    #[test]
    fn jmap_url_serde_round_trip() {
        let original = JmapUrl::new("https://example.com/api/");
        let json = serde_json::to_value(&original).expect("must serialise");
        assert_eq!(json, serde_json::json!("https://example.com/api/"));
        let restored: JmapUrl = serde_json::from_value(json).expect("must deserialise");
        assert_eq!(restored, original);
    }

    /// `JmapUrlTemplate` round-trips through serde_json as a transparent
    /// string.
    #[test]
    fn jmap_url_template_serde_round_trip() {
        let original = JmapUrlTemplate::new("https://example.com/upload/{accountId}/");
        let json = serde_json::to_value(&original).expect("must serialise");
        assert_eq!(
            json,
            serde_json::json!("https://example.com/upload/{accountId}/")
        );
        let restored: JmapUrlTemplate = serde_json::from_value(json).expect("must deserialise");
        assert_eq!(restored, original);
    }

    /// `PartialEq<&str>` and `PartialEq<str>` ergonomics for
    /// `assert_eq!(session.api_url, "...")` style assertions in
    /// downstream tests.
    #[test]
    fn jmap_url_partial_eq_str() {
        let url = JmapUrl::new("https://example.com/api/");
        assert_eq!(url, "https://example.com/api/");
        assert_eq!("https://example.com/api/", url);
        assert_ne!(url, "https://other.example.com/api/");
    }
}

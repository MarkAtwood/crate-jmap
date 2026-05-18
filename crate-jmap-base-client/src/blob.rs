//! Blob upload/download operations and supporting types (RFC 8620 §6.1, §6.2)

use std::borrow::Cow;

use jmap_types::Id;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::client::{read_capped_body, JmapClient};
use crate::error::ClientError;

/// Parameters for [`JmapClient::download_blob`].
///
/// Use a struct literal to avoid confusion between the string-typed fields:
///
/// ```rust,ignore
/// client.download_blob(DownloadBlobParams {
///     download_url_template: &session.download_url,
///     account_id: "A13824",
///     blob_id: "Gbc4c...",
///     name: "attachment.pdf",
///     accept_type: Some("application/pdf"),
///     expected_sha256: None,
/// }).await?;
/// ```
///
/// To request integrity verification, construct a typed
/// [`jmap_cid_types::Sha256`] and pass a borrow:
///
/// ```rust,ignore
/// let expected = jmap_cid_types::Sha256::from_hex(
///     "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
/// )?;
/// client.download_blob(DownloadBlobParams {
///     // ... other fields ...
///     expected_sha256: Some(&expected),
///     # download_url_template: "",
///     # account_id: "",
///     # blob_id: "",
///     # name: "",
///     # accept_type: None,
/// }).await?;
/// ```
#[derive(Debug, Clone, Copy)]
pub struct DownloadBlobParams<'a> {
    /// URL template from `Session.download_url`.
    pub download_url_template: &'a str,
    /// Account ID that owns the blob.
    pub account_id: &'a str,
    /// Server-assigned blob identifier.
    pub blob_id: &'a str,
    /// Human-readable filename for the `{name}` template variable.
    pub name: &'a str,
    /// Optional accept type for the `{type}` template variable (e.g. `"image/png"`).
    /// Pass `None` when no content-type preference is needed; `{type}` expands to an
    /// empty string.
    pub accept_type: Option<&'a str>,
    /// Optional expected SHA-256 digest for integrity verification.
    /// Pass `None` to skip the check.
    ///
    /// The typed [`jmap_cid_types::Sha256`] wrapper guarantees the value is
    /// exactly 64 characters of lowercase hex per draft-atwood-jmap-cid-00 §2 ABNF.
    /// Construction is the only validation point: callers build the
    /// [`Sha256`](jmap_cid_types::Sha256) via [`from_hex`](jmap_cid_types::Sha256::from_hex)
    /// or deserialize and propagate the [`Sha256DigestError`](jmap_cid_types::Sha256DigestError);
    /// `download_blob` itself does not re-validate, so the `&str`-validation
    /// branch and its [`ClientError::InvalidArgument`] mapping no longer exist
    /// (bd:JMAP-6r7c.48, bd:JMAP-6r7c.53).
    pub expected_sha256: Option<&'a jmap_cid_types::Sha256>,
}

/// Response body returned by a successful blob upload (RFC 8620 §6.1).
///
/// # SemVer coupling with `jmap-cid-types` (bd:JMAP-6r7c.30)
///
/// The `sha256` field uses `jmap_cid_types::Sha256` — a workspace-sibling
/// type, not a wrapped opaque type the way `reqwest::Error` is wrapped
/// behind [`HttpError`](crate::HttpError). Consumers that touch
/// `BlobUploadResponse.sha256` transitively depend on `jmap-cid-types` and
/// must pin its major version alongside `jmap-base-client`.
///
/// The coupling is deliberate. `jmap-cid-types` is a workspace sibling of
/// `jmap-base-client` (both live in the `crate-jmap` workspace) and ships
/// in the same release cadence — every `jmap-cid-types` major bump is also
/// a `jmap-base-client` major bump. The SemVer-isolation pattern that
/// hides `reqwest::Error` behind [`HttpError`](crate::HttpError) is
/// designed for *third-party* deps whose release cadence is uncorrelated
/// with this crate's; workspace siblings do not need that isolation
/// because the workspace-level major-version policy already coordinates
/// them.
///
/// Third-party consumers picking up `jmap-base-client` from crates.io
/// should declare both deps with matching majors:
///
/// ```toml
/// [dependencies]
/// jmap-base-client = "0.1"
/// jmap-cid-types   = "0.1"
/// ```
///
/// If you only ever pattern-match on `Option::Some(_)` (without naming the
/// inner type) you can skip the explicit `jmap-cid-types` dep; touching
/// `Sha256`'s methods or `AsRef<str>` impl requires it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobUploadResponse {
    /// The account the blob was uploaded to.
    pub account_id: Id,
    /// Server-assigned opaque blob identifier.
    pub blob_id: Id,
    /// Media type of the uploaded blob as determined by the server.
    #[serde(rename = "type")]
    pub content_type: String,
    /// Size of the uploaded blob in bytes.
    pub size: u64,
    /// SHA-256 digest of the uploaded blob, present only when the
    /// server advertises the `urn:ietf:params:jmap:cid` capability
    /// (draft-atwood-jmap-cid-00 §3).
    ///
    /// The wire format is a 64-character lowercase-hex string per
    /// the draft's ABNF (`%x30-39 / %x61-66`). The typed
    /// [`jmap_cid_types::Sha256`] enforces that shape on
    /// deserialize: a server response carrying a sha256 field that
    /// is not exactly 64 bytes of lowercase hex will fail to parse
    /// and surface as [`ClientError::Parse`]. Servers that do not
    /// implement the CID extension omit the field; the typed
    /// representation here is `None`.
    ///
    /// History: bd:JMAP-v9py.13 promoted this field from a permissive
    /// `Option<String>` to the typed `Option<jmap_cid_types::Sha256>`,
    /// and bd:JMAP-6r7c.48 propagated the same typed shape to the
    /// download-side [`DownloadBlobParams::expected_sha256`](crate::DownloadBlobParams::expected_sha256)
    /// caller-supplied argument. The previous implementation tolerated
    /// uppercase hex via a permissive ad-hoc validator and a normalize-
    /// to-lowercase step before integrity comparison; the typed path is
    /// strict on every construction site. The inter-op question (whether
    /// to recover the uppercase tolerance via a custom Deserialize wrapper)
    /// is tracked by bd:JMAP-noz7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<jmap_cid_types::Sha256>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Expand a RFC 6570 Level-1 URI template by substituting variables.
///
/// For each `(name, value)` pair in `vars`, replaces `{name}` in `template`
/// with the percent-encoded form of `value`. Encoding follows RFC 3986
/// unreserved characters (ALPHA / DIGIT / `-` / `.` / `_` / `~`), which pass
/// through unchanged; all other bytes are encoded as `%XX` with uppercase hex
/// (RFC 3986 §2.1 requires uppercase).
///
/// Entries in `vars` whose name does not appear in the template are silently
/// ignored. A variable that appears in the template but has no entry in `vars`
/// is an error (`ClientError::InvalidSession`) because templates come from the
/// server's Session document — an unexpected variable indicates a server bug.
///
/// # RFC 6570 Level-1
/// Only simple-string expansion is supported. Reserved-expansion (`{+var}`)
/// and other Level-2+ operators are not supported.
///
/// # ⚠ Level-2+ templates silently mis-expand (bd:JMAP-6r7c.32)
///
/// A server that returns a Level-2 or higher template — e.g.
/// `{+blobId}` (reserved-expansion), `{#var}` (fragment-style), or
/// `{var*}` (list-shaped) — will NOT be detected as malformed. The
/// `{+blobId}` form parses as a variable name `+blobId` which is then
/// either replaced verbatim (if a caller happens to pass that exact
/// name in `vars`) or rejected as an unknown variable. The
/// percent-encoding logic treats every non-unreserved byte as
/// something to escape; Level-2 reserved expansion EXPECTS reserved
/// characters like `/` and `:` to pass through unchanged. A server
/// using `{+downloadId}` to embed a slash-bearing identifier directly
/// would have those slashes percent-encoded by this function and the
/// resulting URL would be rejected server-side.
///
/// **JMAP servers SHOULD use Level-1 templates only per RFC 8620 §2.**
/// If your server uses higher levels, you must expand the template
/// yourself with an RFC-6570-compliant external library and pass the
/// already-expanded URL to methods that accept a plain URL — do not
/// pass a Level-2+ template through this function.
///
/// # Usage with `subscribe_events`
///
/// [`Session::event_source_url`] is a URI template with variables `types`,
/// `closeafter`, and `ping`. Expand it before calling
/// [`JmapClient::subscribe_events`]:
///
/// ```rust,ignore
/// let url = expand_url_template(
///     &session.event_source_url,
///     &[
///         ("types", "Email,Mailbox"),
///         ("closeafter", "state"),
///         ("ping", "0"),
///     ],
/// )?;
/// client.subscribe_events(&url, None).await?;
/// ```
///
/// [`Session::event_source_url`]: crate::request::Session::event_source_url
/// [`JmapClient::subscribe_events`]: crate::client::JmapClient::subscribe_events
pub fn expand_url_template(template: &str, vars: &[(&str, &str)]) -> Result<String, ClientError> {
    let mut result = String::with_capacity(template.len() + 64);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        result.push_str(&rest[..open]);
        rest = &rest[open + 1..];
        let close = rest.find('}').ok_or_else(|| {
            ClientError::InvalidSession("URL template has unmatched '{'".to_owned())
        })?;
        let name = &rest[..close];
        rest = &rest[close + 1..];
        if let Some((_, value)) = vars.iter().find(|(n, _)| *n == name) {
            result.push_str(&percent_encode(value));
        } else {
            return Err(ClientError::InvalidSession(format!(
                "URL template variable not supplied: {{{name}}}"
            )));
        }
    }
    result.push_str(rest);
    Ok(result)
}

/// Percent-encode a string value per RFC 3986 §2.3 unreserved character set.
///
/// The return type is [`Cow<'_, str>`](Cow) so the common JMAP case — inputs
/// that contain only unreserved characters (alphanumeric / `-` / `.` / `_` /
/// `~`) such as account ids and blob ids — borrows the input slice and
/// performs no allocation. Inputs that contain any byte requiring percent-
/// escape allocate a fresh string with the encoded form.
fn percent_encode(value: &str) -> Cow<'_, str> {
    // Fast path: scan for the first byte that needs escaping. If none, no
    // allocation is required.
    let first_escape = value.bytes().position(|b| !is_unreserved(b));
    let Some(first) = first_escape else {
        return Cow::Borrowed(value);
    };

    // Allocate only when at least one byte needs escaping. Reserve at least
    // enough capacity for the unchanged prefix plus the three-byte encoding
    // of the first escaped byte; further escapes will grow as needed.
    let mut out = String::with_capacity(value.len() + 2);
    out.push_str(&value[..first]);
    for byte in value.as_bytes()[first..].iter().copied() {
        if is_unreserved(byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(hex_nibble_upper(byte >> 4));
            out.push(hex_nibble_upper(byte & 0x0f));
        }
    }
    Cow::Owned(out)
}

/// Returns `true` if `byte` is in the RFC 3986 §2.3 unreserved set.
fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.' || byte == b'_' || byte == b'~'
}

/// Returns the uppercase hex character for `nibble` (0–15).
/// Used for percent-encoding (RFC 3986 §2.1 requires uppercase).
fn hex_nibble_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + nibble - 10),
        _ => unreachable!("nibble must be 0–15"),
    }
}

/// Returns the lowercase hex character for `nibble` (0–15).
/// Used for SHA-256 hex output (JMAP-CID §1 requires lowercase).
fn hex_nibble_lower(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + nibble - 10),
        _ => unreachable!("nibble must be 0–15"),
    }
}

impl JmapClient {
    /// Upload raw bytes to the JMAP blob store (RFC 8620 §6.1).
    ///
    /// `upload_url_template` is from `Session.upload_url`; `{accountId}` is
    /// substituted before the request. `content_type` is sent as the
    /// `Content-Type` header. If the server returns a `sha256` field
    /// (JMAP-CID capability), it is verified against the locally-computed
    /// digest and `ClientError::BlobIntegrityMismatch` is returned on mismatch.
    pub async fn upload_blob(
        &self,
        upload_url_template: &str,
        account_id: &str,
        data: bytes::Bytes,
        content_type: &str,
    ) -> Result<BlobUploadResponse, ClientError> {
        crate::client::require_http_url(upload_url_template)?;
        let ct_hv =
            HeaderValue::from_str(content_type).map_err(ClientError::from_invalid_header)?;
        let url = expand_url_template(upload_url_template, &[("accountId", account_id)])?;

        // Compute SHA-256 and capture size before handing ownership of data
        // to the request body. local_size is used to cross-check the server's
        // reported `size` after upload (bd:JMAP-6lsm.8) — when the server
        // does not return sha256 (most do not), size is the only signal that
        // the bytes we sent are the bytes the server stored.
        let local_sha256 = compute_sha256_hex(&data);
        let local_size = data.len() as u64;

        let req = self.inject_auth(
            self.http
                .post(&url)
                .header(CONTENT_TYPE, ct_hv)
                .timeout(self.config.request_timeout)
                .body(data),
        );

        let resp = req.send().await.map_err(ClientError::from_reqwest)?;
        let status = resp.status();
        Self::check_auth_status(status)?;
        let resp = resp.error_for_status().map_err(ClientError::from_reqwest)?;

        let upload_limit = self.config.max_upload_response_body;

        // Stream the body chunk-by-chunk with the cap enforced before each
        // accumulation (bd:JMAP-6r7c.1). See `read_capped_body` for the DOS
        // rationale; without per-chunk streaming, a server that under-reports
        // or omits Content-Length can force unbounded allocation here.
        let bytes = read_capped_body(resp, upload_limit).await?;
        let upload_resp: BlobUploadResponse =
            serde_json::from_slice(&bytes).map_err(ClientError::Parse)?;

        // Defense-in-depth: cross-check the server's reported size against the
        // bytes we actually uploaded (bd:JMAP-6lsm.8). When sha256 is present
        // (below) it makes a size mismatch implausible because length is
        // implicit in the digest, but most servers do not advertise the
        // JMAP-CID sha256 capability — in that case `size` is the only
        // signal that the upload was complete and intact. A mismatch
        // surfaces as UnexpectedResponse rather than a typed variant; a
        // future minor release may add ClientError::BlobSizeMismatch for
        // structured matching, but keeping the variant set stable in 0.1.x
        // is the conservative choice.
        if upload_resp.size != local_size {
            return Err(ClientError::UnexpectedResponse(format!(
                "blob upload size mismatch: client uploaded {local_size} bytes, server reports \
                 {server_size} bytes",
                server_size = upload_resp.size,
            )));
        }

        if let Some(ref server_sha256) = upload_resp.sha256 {
            // The typed `jmap_cid_types::Sha256` deserialize already
            // enforces the 64-character lowercase-hex ABNF; reaching
            // this branch implies the wire value is canonical. We
            // can compare the locally-computed digest (also
            // canonical lowercase hex from `compute_sha256_hex`)
            // against the typed value's string form directly.
            //
            // Uppercase-hex from a non-conformant server is rejected
            // at deserialize per draft-atwood-jmap-cid-00 §2 ABNF and
            // surfaces as ClientError::Parse before this branch runs.
            //
            // Constant-time compare via `hex_digest_eq` (bd:JMAP-6r7c.61):
            // both sides are canonical 64-char lowercase hex by construction.
            if !hex_digest_eq(&local_sha256, server_sha256.as_str()) {
                return Err(ClientError::BlobIntegrityMismatch {
                    expected: local_sha256,
                    actual: server_sha256.as_str().to_owned(),
                });
            }
        }

        Ok(upload_resp)
    }

    /// Download a blob by ID (RFC 8620 §6.2).
    ///
    /// Template variables `{accountId}`, `{blobId}`, `{name}`, and `{type}` are
    /// substituted from the corresponding fields of `params` before the GET
    /// request. `{type}` expands to an empty string when `params.accept_type`
    /// is `None`; templates that include `?accept={type}` produce `?accept=`.
    /// If the server does not tolerate an empty `?accept=` parameter, omit
    /// `{type}` from the `download_url` template in the Session document.
    ///
    /// If `params.expected_sha256` is `Some`, the downloaded bytes are verified
    /// against the typed [`jmap_cid_types::Sha256`] digest and
    /// `ClientError::BlobIntegrityMismatch` is returned on mismatch.
    pub async fn download_blob(
        &self,
        params: DownloadBlobParams<'_>,
    ) -> Result<bytes::Bytes, ClientError> {
        let DownloadBlobParams {
            download_url_template,
            account_id,
            blob_id,
            name,
            accept_type,
            expected_sha256,
        } = params;
        crate::client::require_http_url(download_url_template)?;
        let vars = [
            ("accountId", account_id),
            ("blobId", blob_id),
            ("name", name),
            // Always supply {type} — even as empty string — so templates
            // containing `?accept={type}` expand cleanly rather than triggering
            // the unexpanded-placeholder error.
            ("type", accept_type.unwrap_or("")),
        ];
        let url = expand_url_template(download_url_template, &vars)?;

        let req = self.inject_auth(self.http.get(&url).timeout(self.config.request_timeout));

        let resp = req.send().await.map_err(ClientError::from_reqwest)?;
        let status = resp.status();
        Self::check_auth_status(status)?;
        let resp = resp.error_for_status().map_err(ClientError::from_reqwest)?;

        let download_limit = self.config.max_download_body;

        // Stream the body chunk-by-chunk with the cap enforced before each
        // accumulation (bd:JMAP-6r7c.1). See `read_capped_body` for the DOS
        // rationale; without per-chunk streaming, a server that under-reports
        // or omits Content-Length can force unbounded allocation here.
        let bytes = bytes::Bytes::from(read_capped_body(resp, download_limit).await?);

        if let Some(expected) = expected_sha256 {
            // The typed `jmap_cid_types::Sha256` enforces the canonical
            // 64-char lowercase-hex ABNF at construction (bd:JMAP-6r7c.48),
            // so the runtime length/charset check and the
            // `to_ascii_lowercase` normalize step the previous
            // `Option<&str>` shape required (bd:JMAP-6r7c.53) are gone.
            let actual = compute_sha256_hex(&bytes);
            // Constant-time compare via `hex_digest_eq` (bd:JMAP-6r7c.61):
            // both sides are canonical 64-char lowercase hex by construction.
            if !hex_digest_eq(&actual, expected.as_str()) {
                return Err(ClientError::BlobIntegrityMismatch {
                    expected: expected.as_str().to_owned(),
                    actual,
                });
            }
        }

        Ok(bytes)
    }
}

/// Compute SHA-256 of `data` and return as 64-char lowercase hex string.
fn compute_sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    let mut s = String::with_capacity(64);
    for b in hash.iter() {
        s.push(hex_nibble_lower(*b >> 4));
        s.push(hex_nibble_lower(*b & 0x0f));
    }
    s
}

/// Constant-time equality for two 64-character lowercase SHA-256 hex digests
/// (bd:JMAP-6r7c.61).
///
/// Both upload-side and download-side integrity checks compare hex digests
/// that are guaranteed-canonical by construction before reaching this
/// helper: `compute_sha256_hex` always emits 64 lowercase nibbles, and
/// `jmap_cid_types::Sha256` enforces the 64-char lowercase-hex ABNF on
/// every construction path (deserialize, `from_hex`, etc.). Length-
/// discrimination is therefore not an information leak at these call sites.
///
/// Using `subtle::ConstantTimeEq::ct_eq` over `==` is discipline-propagation,
/// not a defense against a concrete JMAP threat: SHA-256 of blob bytes is
/// not a secret (the attacker can fetch the same blob), so a timing oracle
/// on the digest comparison reveals nothing exploitable in the upload-side
/// case. The download-side `expected_sha256` argument is caller-supplied
/// and *could* in principle come from a private channel (signed manifest,
/// end-to-end attestation); the constant-time compare closes that residual
/// channel for callers who want it.
///
/// Matches the workspace's RustCrypto-first stance and the precedent set by
/// `crate-jmap-chat-server` (invite-code lookup) and `crate-jmap-testjig`
/// (bearer-token check).
fn hex_digest_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: RFC 6570 §1.2 — simple substitution with unreserved-only value
    #[test]
    fn expand_upload_url() {
        let result = expand_url_template(
            "https://example.com/upload/{accountId}/",
            &[("accountId", "account1")],
        )
        .expect("must succeed");
        assert_eq!(result, "https://example.com/upload/account1/");
    }

    // Oracle: RFC 3986 §2.1 — space 0x20 encodes as %20
    #[test]
    fn expand_download_url_with_spaces() {
        let result = expand_url_template(
            "/download/{accountId}/{blobId}/{name}",
            &[
                ("accountId", "acc1"),
                ("blobId", "blob-123"),
                ("name", "my file.png"),
            ],
        )
        .expect("must succeed");
        assert_eq!(result, "/download/acc1/blob-123/my%20file.png");
    }

    // Oracle: RFC 3986 §2.1 — slash 0x2F encodes as %2F
    #[test]
    fn expand_with_slash_in_type() {
        let result = expand_url_template(
            "/dl/{accountId}/{blobId}/{name}?accept={type}",
            &[
                ("accountId", "a"),
                ("blobId", "b"),
                ("name", "x.jpg"),
                ("type", "image/png"),
            ],
        )
        .expect("must succeed");
        assert_eq!(result, "/dl/a/b/x.jpg?accept=image%2Fpng");
    }

    // Oracle: expand_url_template must error when a template variable has no
    // entry in vars — the template comes from the server's Session document,
    // so an unrecognized variable name indicates a server-side bug.
    #[test]
    fn expand_unknown_variable_returns_error() {
        let err = expand_url_template(
            "https://example.com/upload/{accountId}/{unknownVar}/",
            &[("accountId", "acc1")],
        )
        .expect_err("must fail when template variable is not supplied");
        assert!(
            matches!(err, crate::error::ClientError::InvalidSession(_)),
            "expected InvalidSession, got {err:?}"
        );
    }

    // Oracle: expand_url_template must error on a template with an unmatched
    // '{' — no corresponding '}' exists. The expected error is InvalidSession
    // because templates come from the server's Session document.
    #[test]
    fn expand_unmatched_open_brace_returns_error() {
        let err = expand_url_template("https://example.com/{unclosed", &[])
            .expect_err("unmatched '{' must return an error");
        assert!(
            matches!(err, crate::error::ClientError::InvalidSession(_)),
            "expected InvalidSession for unmatched brace, got {err:?}"
        );
    }

    // Oracle: vars entries whose name does not appear in the template are
    // silently ignored — extra vars are benign (caller may pass a superset).
    #[test]
    fn expand_unused_var_is_ignored() {
        let result = expand_url_template(
            "https://example.com/upload/{accountId}/",
            &[("accountId", "acc1"), ("extraVar", "value")],
        )
        .expect("extra vars must be silently ignored");
        assert_eq!(result, "https://example.com/upload/acc1/");
    }

    // Oracle: RFC 3986 §2.3 — for inputs containing only unreserved
    // characters (alphanumeric / `-` / `.` / `_` / `~`), percent_encode
    // MUST return Cow::Borrowed without allocating. Verified by the
    // matches! check against the Cow::Borrowed variant.
    #[test]
    fn percent_encode_unreserved_input_borrows() {
        let input = "abc.DEF_123-xyz~tilde";
        let result = percent_encode(input);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result.as_ref(), input);
    }

    // Oracle: RFC 3986 §2.1 — inputs that contain any byte outside the
    // unreserved set MUST be returned as Cow::Owned with each non-unreserved
    // byte percent-escaped using uppercase hex (§2.1 mandates uppercase).
    #[test]
    fn percent_encode_with_space_is_owned_and_uppercase() {
        let input = "hello world";
        let result = percent_encode(input);
        assert!(matches!(result, Cow::Owned(_)));
        assert_eq!(result.as_ref(), "hello%20world");
    }

    // Oracle: RFC 3986 §2.1 — slash 0x2F encodes as %2F (uppercase hex).
    // Mixed-content input MUST preserve the unreserved prefix verbatim
    // and escape only the disallowed bytes.
    #[test]
    fn percent_encode_mixed_input_preserves_prefix() {
        let input = "image/png";
        let result = percent_encode(input);
        assert!(matches!(result, Cow::Owned(_)));
        assert_eq!(result.as_ref(), "image%2Fpng");
    }

    // Oracle: degenerate empty input. An empty string has no bytes requiring
    // escape, so percent_encode MUST borrow it without allocation.
    #[test]
    fn percent_encode_empty_input_borrows() {
        let result = percent_encode("");
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result.as_ref(), "");
    }

    // Oracle: tests/fixtures/blob/upload_response.json — hand-written fixture
    // derived from RFC 8620 §6.1 blob upload response shape; not produced by
    // the code under test.
    #[test]
    fn blob_upload_response_deserializes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/blob/upload_response.json");
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read fixture: {e}"));
        let resp: BlobUploadResponse =
            serde_json::from_str(&text).expect("fixture must deserialize as BlobUploadResponse");

        assert_eq!(resp.account_id, "account1");
        assert_eq!(resp.blob_id, "Gbc4c377-c8c3-4b48-b2bb-8c1e4cfb8b2a");
        assert_eq!(resp.content_type, "image/png");
        assert_eq!(resp.size, 48291);
        // The typed Sha256 wrapper carries the canonical lowercase
        // hex string; compare via `as_str` to keep the assertion
        // independent of the wrapper's Debug shape.
        assert_eq!(
            resp.sha256.as_ref().map(jmap_cid_types::Sha256::as_str),
            Some("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")
        );
    }

    /// `BlobUploadResponse.extra` captures unknown fields on deserialize.
    #[test]
    fn blob_upload_response_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "accountId": "account1",
            "blobId": "Gbc4c377-c8c3-4b48-b2bb-8c1e4cfb8b2a",
            "type": "image/png",
            "size": 48291,
            "acmeCorpScanResult": "clean"
        });
        let obj: BlobUploadResponse =
            serde_json::from_value(raw).expect("BlobUploadResponse must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpScanResult").and_then(|v| v.as_str()),
            Some("clean")
        );
    }

    // bd:JMAP-v9py.13 oracle.
    //
    // The fixture hex digest is hand-typed from RFC 6234 §8.5 ("abc"
    // test vector, formatted lowercase). It is NOT derived from the
    // code under test, satisfying the workspace test-integrity rule
    // that test oracles must be independent.

    /// Deserialize a Blob/upload response carrying a canonical
    /// 64-char lowercase-hex sha256 → field parses as
    /// Some(Sha256(_)) preserving the wire string.
    #[test]
    fn blob_upload_response_deserializes_sha256_typed() {
        let raw = serde_json::json!({
            "accountId": "account1",
            "blobId": "blob1",
            "type": "text/plain",
            "size": 3,
            "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        });
        let obj: BlobUploadResponse = serde_json::from_value(raw).expect("must deserialize");
        let s = obj.sha256.expect("sha256 must be Some");
        assert_eq!(
            s.as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Serialize a BlobUploadResponse with `sha256: None` → the
    /// `sha256` key must be ABSENT from the wire output per
    /// `skip_serializing_if = "Option::is_none"`. This is the
    /// contract for servers not advertising the CID capability.
    #[test]
    fn blob_upload_response_serializes_without_sha256_when_none() {
        let resp = BlobUploadResponse {
            account_id: Id::from("a1"),
            blob_id: Id::from("b1"),
            content_type: "text/plain".to_owned(),
            size: 0,
            sha256: None,
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&resp).expect("must serialize");
        let obj = v.as_object().expect("object");
        assert!(
            !obj.contains_key("sha256"),
            "None must elide the sha256 key: {v:?}"
        );
    }

    /// Round-trip preservation: a server that omits the sha256
    /// field deserializes into `None`, and serializing back
    /// produces an output without a `sha256` key. This is the
    /// shape RFC 8620 §6.1 compliant servers (without the CID
    /// extension) produce; verify we don't accidentally inject a
    /// null or empty sha256 on round-trip.
    #[test]
    fn blob_upload_response_no_sha256_round_trip() {
        let raw = serde_json::json!({
            "accountId": "account1",
            "blobId": "blob1",
            "type": "text/plain",
            "size": 3
        });
        let obj: BlobUploadResponse = serde_json::from_value(raw).expect("must deserialize");
        assert!(obj.sha256.is_none());

        let re = serde_json::to_value(&obj).expect("must serialize");
        let map = re.as_object().expect("object");
        assert!(
            !map.contains_key("sha256"),
            "round-trip must not introduce sha256 key: {re:?}"
        );
    }

    /// A non-conformant server sending uppercase hex now fails to
    /// deserialize (typed `Sha256` is strict lowercase-only per
    /// draft-atwood-jmap-cid-00 §2). Pinning the strict behavior
    /// per bd:JMAP-v9py.13's design; the inter-op question is
    /// tracked separately at bd:JMAP-noz7.
    #[test]
    fn blob_upload_response_rejects_uppercase_sha256() {
        let raw = serde_json::json!({
            "accountId": "account1",
            "blobId": "blob1",
            "type": "text/plain",
            "size": 3,
            "sha256": "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        });
        let err = serde_json::from_value::<BlobUploadResponse>(raw)
            .expect_err("uppercase hex must fail to deserialize");
        let msg = err.to_string();
        assert!(
            msg.contains("non-lowercase-hex") || msg.contains("at") || msg.contains("hex"),
            "error must explain the hex constraint: {msg}"
        );
    }

    // bd:JMAP-6r7c.61 — behavioral round-trip for the constant-time hex
    // digest comparison helper. We cannot assert side-channel resistance
    // from a unit test (only the compiler/CPU pipeline can); these tests
    // assert the semantic equality contract still holds after swapping
    // `String ==` for `subtle::ConstantTimeEq::ct_eq`. Oracle: NIST
    // FIPS 180-4 Appendix A example 1 (SHA-256 of "abc") and the
    // widely-published SHA-256 of the empty string (RFC 6234
    // implementations, NIST CAVS test vectors) — both hand-typed from
    // external sources, not computed by this crate.
    #[test]
    fn hex_digest_eq_matches_identical_canonical_digests() {
        // NIST FIPS 180-4 Appendix A example 1: SHA-256("abc").
        let abc_sha256_nist = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(hex_digest_eq(abc_sha256_nist, abc_sha256_nist));
    }

    #[test]
    fn hex_digest_eq_rejects_one_byte_mismatch() {
        let abc_sha256_nist = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        // Flip the final nibble (d -> c). Distance: 1 hex char, well
        // inside what a byte-by-byte == would have caught.
        let one_byte_off = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ac";
        assert!(!hex_digest_eq(abc_sha256_nist, one_byte_off));
    }

    #[test]
    fn hex_digest_eq_rejects_complete_mismatch() {
        // NIST FIPS 180-4 Appendix A example 1: SHA-256("abc").
        let abc_sha256_nist = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        // SHA-256 of the empty string (widely-published canonical value;
        // RFC 6234, NIST CAVS, etc.).
        let empty_sha256_canonical =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(!hex_digest_eq(abc_sha256_nist, empty_sha256_canonical));
    }
}

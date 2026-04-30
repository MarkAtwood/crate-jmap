//! Blob upload/download operations and supporting types (RFC 8620 §6.1, §6.2)

use jmap_types::Id;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::client::JmapClient;
use crate::error::ClientError;

/// Parameters for [`JmapClient::download_blob`].
///
/// Use a struct literal to avoid confusion between the six string-typed fields:
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
    /// Optional expected SHA-256 hex digest for integrity verification.
    /// Pass `None` to skip the check.
    pub expected_sha256: Option<&'a str>,
}

/// Response body returned by a successful blob upload (RFC 8620 §6.1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    /// SHA-256 hex digest of the uploaded blob as 64 hex characters (uppercase or
    /// lowercase), if the server supports the JMAP-CID draft extension (`draft-atwood-jmap-cid`).
    ///
    /// This field is **not** defined by RFC 8620. It is present only on servers
    /// that advertise the `urn:ietf:params:jmap:cid` capability. Servers that
    /// do not implement the extension omit the field.
    pub sha256: Option<String>,
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
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || byte == b'-'
            || byte == b'.'
            || byte == b'_'
            || byte == b'~'
        {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(hex_nibble_upper(byte >> 4));
            out.push(hex_nibble_upper(byte & 0x0f));
        }
    }
    out
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
        let ct_hv = HeaderValue::from_str(content_type).map_err(ClientError::InvalidHeaderValue)?;
        let url = expand_url_template(upload_url_template, &[("accountId", account_id)])?;

        // Compute SHA-256 before handing ownership of data to the request body.
        let local_sha256 = compute_sha256_hex(&data);

        let req = self.inject_auth(
            self.http
                .post(&url)
                .header(CONTENT_TYPE, ct_hv)
                .timeout(self.config.request_timeout)
                .body(data),
        );

        let resp = req.send().await.map_err(ClientError::Http)?;
        let status = resp.status();
        Self::check_auth_status(status)?;
        let resp = resp.error_for_status().map_err(ClientError::Http)?;

        let upload_limit = self.config.max_upload_body;

        if let Some(len) = resp.content_length() {
            if len > upload_limit {
                return Err(ClientError::ResponseTooLarge {
                    actual: len,
                    limit: upload_limit,
                });
            }
        }
        let bytes = resp.bytes().await.map_err(ClientError::Http)?;
        if bytes.len() as u64 > upload_limit {
            return Err(ClientError::ResponseTooLarge {
                actual: bytes.len() as u64,
                limit: upload_limit,
            });
        }
        let upload_resp: BlobUploadResponse =
            serde_json::from_slice(&bytes).map_err(ClientError::Parse)?;

        if let Some(ref server_sha256) = upload_resp.sha256 {
            if !is_valid_sha256_hex(server_sha256) {
                return Err(ClientError::InvalidSession(format!(
                    "server sha256 field is not 64-char hex: {server_sha256:?}"
                )));
            }
            // Normalize to lowercase before comparison: JMAP-CID specifies lowercase
            // but non-conformant servers may return uppercase hex. Both represent the
            // same digest; rejecting on case alone would cause spurious integrity errors.
            let server_lower = server_sha256.to_ascii_lowercase();
            if local_sha256 != server_lower {
                return Err(ClientError::BlobIntegrityMismatch {
                    expected: local_sha256,
                    actual: server_lower,
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
    /// against the hex digest and `ClientError::BlobIntegrityMismatch` is
    /// returned on mismatch.
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

        let resp = req.send().await.map_err(ClientError::Http)?;
        let status = resp.status();
        Self::check_auth_status(status)?;
        let resp = resp.error_for_status().map_err(ClientError::Http)?;

        let download_limit = self.config.max_download_body;

        // Check Content-Length header as an early-exit optimization.
        if let Some(len) = resp.content_length() {
            if len > download_limit {
                return Err(ClientError::ResponseTooLarge {
                    actual: len,
                    limit: download_limit,
                });
            }
        }
        let bytes = resp.bytes().await.map_err(ClientError::Http)?;
        // Content-Length can lie — enforce cap on actual bytes read.
        if bytes.len() as u64 > download_limit {
            return Err(ClientError::ResponseTooLarge {
                actual: bytes.len() as u64,
                limit: download_limit,
            });
        }

        if let Some(expected) = expected_sha256 {
            if !is_valid_sha256_hex(expected) {
                return Err(ClientError::InvalidArgument(format!(
                    "expected_sha256 is not 64-char hex: {expected:?}"
                )));
            }
            let actual = compute_sha256_hex(&bytes);
            // Normalize expected to lowercase; callers may hold uppercase hex from
            // a server or external source. Both represent the same digest.
            let expected_lower = expected.to_ascii_lowercase();
            if actual != expected_lower {
                return Err(ClientError::BlobIntegrityMismatch {
                    expected: expected_lower,
                    actual,
                });
            }
        }

        Ok(bytes)
    }
}

/// Returns `true` if `s` is exactly 64 hex characters (uppercase or lowercase).
///
/// Callers are responsible for producing the appropriate [`ClientError`] variant:
/// - server-provided digest (upload response `sha256` field) → [`ClientError::InvalidSession`]
/// - caller-supplied expected digest (`download_blob` `expected_sha256` arg) → [`ClientError::InvalidArgument`]
fn is_valid_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Compute SHA-256 of `data` and return as 64-char lowercase hex string.
fn compute_sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().fold(String::with_capacity(64), |mut s, b| {
        s.push(hex_nibble_lower(*b >> 4));
        s.push(hex_nibble_lower(*b & 0x0f));
        s
    })
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
        assert_eq!(
            resp.sha256.as_deref(),
            Some("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")
        );
    }
}

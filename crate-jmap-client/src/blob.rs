// Blob upload/download operations and supporting types (RFC 8620 §6.1, §6.2)

use jmap_types::Id;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Response body returned by a successful blob upload (RFC 8620 §6.1).
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
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
    /// SHA-256 hex digest of the blob, if provided by the server.
    pub sha256: Option<String>,
}

/// Expand a RFC 6570 Level-1 URI template by substituting variables.
///
/// For each `(name, value)` pair in `vars`, replaces `{name}` in `template`
/// with the percent-encoded form of `value`. Encoding follows RFC 3986
/// unreserved characters (ALPHA / DIGIT / `-` / `.` / `_` / `~`), which pass
/// through unchanged; all other bytes are encoded as `%XX` with uppercase hex
/// (RFC 3986 §2.1 requires uppercase).
pub(crate) fn expand_url_template(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_owned();
    for (name, value) in vars {
        let placeholder = format!("{{{name}}}");
        let encoded = percent_encode(value);
        result = result.replace(&placeholder, &encoded);
    }
    result
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
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4, true));
            out.push(hex_digit(byte & 0x0f, true));
        }
    }
    out
}

/// Returns the hex character for `nibble` (0–15).
/// Uppercase for percent-encoding (RFC 3986 §2.1); lowercase for SHA-256 hex
/// output (JMAP-CID §1 requires lowercase).
fn hex_digit(nibble: u8, uppercase: bool) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 if uppercase => (b'A' + nibble - 10) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!("nibble must be 0–15"),
    }
}

impl crate::client::JmapClient {
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
    ) -> Result<BlobUploadResponse, crate::error::ClientError> {
        let ct_hv = HeaderValue::from_str(content_type)
            .map_err(crate::error::ClientError::InvalidHeaderValue)?;
        let url = expand_url_template(upload_url_template, &[("accountId", account_id)]);

        // Compute SHA-256 before handing ownership of data to the request body.
        let local_sha256 = compute_sha256_hex(&data);

        let mut req = self
            .http
            .post(&url)
            .header(CONTENT_TYPE, ct_hv)
            .timeout(self.config.request_timeout)
            .body(data);
        if let Some((name, value)) = self.auth.auth_header() {
            req = req.header(name, value.as_str());
        }

        let resp = req.send().await.map_err(crate::error::ClientError::Http)?;
        let status = resp.status();
        crate::client::JmapClient::check_auth_status(status)?;
        let resp = resp
            .error_for_status()
            .map_err(crate::error::ClientError::Http)?;

        let upload_limit = self.config.max_upload_body;

        if let Some(len) = resp.content_length() {
            if len > upload_limit {
                return Err(crate::error::ClientError::ResponseTooLarge {
                    actual: len,
                    limit: upload_limit,
                });
            }
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(crate::error::ClientError::Http)?;
        if bytes.len() as u64 > upload_limit {
            return Err(crate::error::ClientError::ResponseTooLarge {
                actual: bytes.len() as u64,
                limit: upload_limit,
            });
        }
        let upload_resp: BlobUploadResponse =
            serde_json::from_slice(&bytes).map_err(|e| crate::error::ClientError::Parse(e.to_string()))?;

        if let Some(ref server_sha256) = upload_resp.sha256 {
            validate_sha256_format(server_sha256)?;
            if local_sha256 != *server_sha256 {
                return Err(crate::error::ClientError::BlobIntegrityMismatch {
                    expected: server_sha256.clone(),
                    actual: local_sha256,
                });
            }
        }

        Ok(upload_resp)
    }

    /// Download a blob by ID (RFC 8620 §6.2).
    ///
    /// `download_url_template` is from `Session.download_url`; `{accountId}`,
    /// `{blobId}`, `{name}`, and `{type}` are substituted before the GET request.
    /// Pass `accept_type` (e.g. `"image/png"`) for content-type negotiation; pass
    /// `None` when no preference is needed — `{type}` expands to an empty string
    /// per RFC 6570 Level-1, so templates that include `?accept={type}` produce
    /// `?accept=` when `accept_type` is `None`.
    /// If `expected_sha256` is `Some`, the downloaded bytes are verified
    /// against the hex digest and `ClientError::BlobIntegrityMismatch` is
    /// returned on mismatch.
    pub async fn download_blob(
        &self,
        download_url_template: &str,
        account_id: &str,
        blob_id: &str,
        name: &str,
        accept_type: Option<&str>,
        expected_sha256: Option<&str>,
    ) -> Result<bytes::Bytes, crate::error::ClientError> {
        let vars: Vec<(&str, &str)> = vec![
            ("accountId", account_id),
            ("blobId", blob_id),
            ("name", name),
            // RFC 6570 Level-1: undefined variables expand to empty string.
            // Always substitute {type} so templates with ?accept={type} produce
            // ?accept= (empty, server ignores) rather than the literal "{type}".
            ("type", accept_type.unwrap_or("")),
        ];
        let url = expand_url_template(download_url_template, &vars);

        let mut req = self.http.get(&url).timeout(self.config.request_timeout);
        if let Some((hdr_name, hdr_value)) = self.auth.auth_header() {
            req = req.header(hdr_name, hdr_value.as_str());
        }

        let resp = req.send().await.map_err(crate::error::ClientError::Http)?;
        let status = resp.status();
        crate::client::JmapClient::check_auth_status(status)?;
        let resp = resp
            .error_for_status()
            .map_err(crate::error::ClientError::Http)?;

        let download_limit = self.config.max_download_body;

        // Check Content-Length header as an early-exit optimization.
        if let Some(len) = resp.content_length() {
            if len > download_limit {
                return Err(crate::error::ClientError::ResponseTooLarge {
                    actual: len,
                    limit: download_limit,
                });
            }
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(crate::error::ClientError::Http)?;
        // Content-Length can lie — enforce cap on actual bytes read.
        if bytes.len() as u64 > download_limit {
            return Err(crate::error::ClientError::ResponseTooLarge {
                actual: bytes.len() as u64,
                limit: download_limit,
            });
        }

        if let Some(expected) = expected_sha256 {
            validate_sha256_format(expected)?;
            let actual = compute_sha256_hex(&bytes);
            if actual != expected {
                return Err(crate::error::ClientError::BlobIntegrityMismatch {
                    expected: expected.to_owned(),
                    actual,
                });
            }
        }

        Ok(bytes)
    }
}

/// Validate that `s` is exactly 64 lowercase hex characters (RFC 6570 / JMAP-CID).
fn validate_sha256_format(s: &str) -> Result<(), crate::error::ClientError> {
    if s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        Err(crate::error::ClientError::Parse(format!(
            "sha256 field is not 64-char lowercase hex: {s:?}"
        )))
    }
}

/// Compute SHA-256 of `data` and return as 64-char lowercase hex string.
fn compute_sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().fold(String::with_capacity(64), |mut s, b| {
        s.push(hex_digit(*b >> 4, false));
        s.push(hex_digit(*b & 0x0f, false));
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
        );
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
        );
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
        );
        assert_eq!(result, "/dl/a/b/x.jpg?accept=image%2Fpng");
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

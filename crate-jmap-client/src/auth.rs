use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use reqwest::header::HeaderValue;

use crate::error::ClientError;

// ---------------------------------------------------------------------------
// TransportConfig — HTTP client construction (TLS, timeouts, trust roots)
// ---------------------------------------------------------------------------

/// Controls how the underlying [`reqwest::Client`] is constructed.
///
/// Implementations configure TLS trust roots, client certificates, and
/// connect timeouts. This is separate from credential injection
/// (see [`AuthProvider`]) so transports and credentials compose freely.
///
/// **Implement this trait** when you need custom TLS logic (e.g. a private CA
/// or a client certificate).  For custom per-request credentials only,
/// implement [`AuthProvider`] instead.  [`DefaultTransport`] covers the common
/// case of publicly-trusted TLS with no custom certificates.
pub trait TransportConfig: Send + Sync {
    /// Build the [`reqwest::Client`] for this transport configuration.
    fn build_client(&self) -> Result<reqwest::Client, ClientError>;
}

/// Standard reqwest client with a 10-second connect timeout; no custom TLS.
///
/// Use for servers with publicly-trusted certificates. Pair with any
/// [`AuthProvider`] for credential injection.
#[derive(Debug)]
pub struct DefaultTransport;

impl TransportConfig for DefaultTransport {
    fn build_client(&self) -> Result<reqwest::Client, ClientError> {
        default_reqwest_client()
    }
}

/// Custom CA trust root (DER-encoded). No `Authorization` header is injected.
///
/// Use when the server presents a certificate signed by a private CA.
/// Pair with any [`AuthProvider`] for credential injection — including
/// [`BearerAuth`] or [`BasicAuth`] if the server also requires credentials.
#[derive(Debug)]
pub struct CustomCaTransport {
    der_cert: Vec<u8>,
}

impl CustomCaTransport {
    /// Construct a `CustomCaTransport` from a DER-encoded CA certificate.
    pub fn new(der_cert: Vec<u8>) -> Self {
        Self { der_cert }
    }
}

impl TransportConfig for CustomCaTransport {
    fn build_client(&self) -> Result<reqwest::Client, ClientError> {
        let cert = reqwest::Certificate::from_der(&self.der_cert)?;
        let client = reqwest::ClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(10))
            .add_root_certificate(cert)
            .build()?;
        Ok(client)
    }
}

// ---------------------------------------------------------------------------
// AuthProvider — per-request credential injection (Authorization header)
// ---------------------------------------------------------------------------

/// Injects per-request authentication credentials.
///
/// Separate from transport configuration ([`TransportConfig`]) so any
/// credential scheme can be paired with any transport.
///
/// **Implement this trait** when you need a custom `Authorization` header or
/// other per-request credential scheme.  For custom TLS/trust-root logic
/// implement [`TransportConfig`] instead.  [`NoneAuth`], [`BearerAuth`], and
/// [`BasicAuth`] cover the common cases.
///
/// Implementations **must not** log the return value of [`auth_header`];
/// it contains credentials.
///
/// [`auth_header`]: AuthProvider::auth_header
pub trait AuthProvider: Send + Sync {
    /// Return an optional `(header-name, header-value)` pair to attach to
    /// every request.
    ///
    /// Returns `None` when no `Authorization` header is required.
    ///
    /// # Implementation contract
    ///
    /// The returned strings **must** be valid HTTP field values (RFC 9110 §5):
    /// - Header name: lowercase ASCII token characters only (no spaces, no
    ///   control characters); e.g. `"authorization"`.
    /// - Header value: visible ASCII characters (0x21–0x7E) and horizontal tab
    ///   (0x09) only; no other control characters.
    ///
    /// Implementations that violate this contract will cause a panic in
    /// `connect_ws` (`ws/mod.rs`), which parses the returned value into a
    /// typed [`http::HeaderValue`]. The HTTP code paths use the value as `&str`
    /// directly and do not perform that conversion, so the panic is deferred
    /// until the WebSocket path is exercised — it will not appear in tests that
    /// only exercise HTTP methods.
    fn auth_header(&self) -> Option<(&'static str, String)>;
}

/// No authentication: no `Authorization` header.
#[derive(Debug)]
pub struct NoneAuth;

impl AuthProvider for NoneAuth {
    fn auth_header(&self) -> Option<(&'static str, String)> {
        None
    }
}

/// Bearer-token authentication (`Authorization: Bearer <token>`).
#[derive(Clone)]
pub struct BearerAuth {
    // Pre-validated at construction and stored as String: avoids per-request
    // allocation and ensures invalid credentials fail at construction, not at
    // the first request. Storing as String eliminates the need for a fallible
    // to_str() call in auth_header().
    header_string: String,
}

impl BearerAuth {
    /// Construct a `BearerAuth` from a Bearer token string.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`] if `token` is empty or whitespace-only.
    /// - [`ClientError::InvalidHeaderValue`] if `token` contains characters that
    ///   are not valid in an HTTP header value (non-visible-ASCII octets).
    pub fn new(token: &str) -> Result<Self, ClientError> {
        if token.trim().is_empty() {
            return Err(ClientError::InvalidArgument(
                "BearerAuth token may not be empty or whitespace-only".into(),
            ));
        }
        let header_string = format!("Bearer {token}");
        // Validate the header value is legal (no control characters, etc.).
        HeaderValue::from_str(&header_string)?;
        Ok(Self { header_string })
    }
}

impl std::fmt::Debug for BearerAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BearerAuth")
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl AuthProvider for BearerAuth {
    fn auth_header(&self) -> Option<(&'static str, String)> {
        Some(("authorization", self.header_string.clone()))
    }
}

/// HTTP Basic authentication (`Authorization: Basic <base64(username:password)>`).
///
/// Credentials are encoded per RFC 7617: `base64(username ":" password)`.
#[derive(Clone)]
pub struct BasicAuth {
    // Pre-validated at construction and stored as String: avoids per-request
    // allocation and ensures invalid credentials fail at construction, not at
    // the first request. Storing as String eliminates the need for a fallible
    // to_str() call in auth_header().
    header_string: String,
}

impl BasicAuth {
    /// Construct a `BasicAuth` from a username and password.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`] if `username` contains a colon (`:`),
    ///   which is forbidden by RFC 7617 §2.
    /// - [`ClientError::InvalidHeaderValue`] if the resulting header value
    ///   contains characters that are not valid in an HTTP header value.
    pub fn new(username: &str, password: &str) -> Result<Self, ClientError> {
        if username.contains(':') {
            return Err(ClientError::InvalidArgument(
                "BasicAuth username may not contain ':'".into(),
            ));
        }
        let encoded = BASE64_STANDARD.encode(format!("{username}:{password}").as_bytes());
        let header_string = format!("Basic {encoded}");
        // Validate the header value is legal (base64 is always printable ASCII,
        // but keep the check for correctness).
        HeaderValue::from_str(&header_string)?;
        Ok(Self { header_string })
    }
}

impl std::fmt::Debug for BasicAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicAuth")
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}

impl AuthProvider for BasicAuth {
    fn auth_header(&self) -> Option<(&'static str, String)> {
        Some(("authorization", self.header_string.clone()))
    }
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Build a standard reqwest client with a 10-second connect timeout.
fn default_reqwest_client() -> Result<reqwest::Client, ClientError> {
    reqwest::ClientBuilder::new()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(ClientError::Http)
}

// ---------------------------------------------------------------------------
// Blanket impl for Box<dyn TransportConfig>
// ---------------------------------------------------------------------------
//
// Allows `Box<dyn TransportConfig>` to satisfy `impl TransportConfig`, so
// factory functions (e.g. `Config::transport`) can return a boxed
// trait object and pass it directly to `JmapClient::new`.
//
// There is intentionally NO `Arc<dyn TransportConfig>` blanket here.
// TransportConfig is consumed once at `JmapClient::new` to build the
// reqwest::Client. The resulting Client is stored; the TransportConfig itself
// is not kept. Arc would imply shared ownership of something that is not
// shared after construction.
//
// Maintenance cost: every method added to `TransportConfig` must be mirrored here.
impl TransportConfig for Box<dyn TransportConfig> {
    fn build_client(&self) -> Result<reqwest::Client, ClientError> {
        (**self).build_client()
    }
}

// ---------------------------------------------------------------------------
// Blanket impl for Arc<dyn AuthProvider>
// ---------------------------------------------------------------------------
//
// Allows `Arc<dyn AuthProvider>` to satisfy `impl AuthProvider`, enabling
// `JmapClient` to be `Clone` (Arc is Clone).
//
// Maintenance cost: every method added to `AuthProvider` must be mirrored here.
impl AuthProvider for Arc<dyn AuthProvider> {
    fn auth_header(&self) -> Option<(&'static str, String)> {
        (**self).auth_header()
    }
}

// ---------------------------------------------------------------------------
// Blanket impl for Box<dyn AuthProvider>
// ---------------------------------------------------------------------------
//
// Allows `Box<dyn AuthProvider>` to satisfy `impl AuthProvider + 'static`,
// so factory functions (e.g. `Config::auth`) can return a boxed
// trait object and pass it directly to `JmapClient::new`.
//
// Maintenance cost: every method added to `AuthProvider` must be mirrored here.
impl AuthProvider for Box<dyn AuthProvider> {
    fn auth_header(&self) -> Option<(&'static str, String)> {
        (**self).auth_header()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: NoneAuth has no authentication header — verified by inspection of the spec.
    #[test]
    fn none_auth_no_header() {
        assert!(NoneAuth.auth_header().is_none());
    }

    /// Oracle: BearerAuth constructs successfully with a valid ASCII token.
    #[test]
    fn bearer_auth_valid_constructs() {
        assert!(BearerAuth::new("tok123").is_ok());
    }

    /// Oracle: BearerAuth header value is "Bearer " + the literal token string.
    /// Verified by inspection: the Authorization header MUST be "Bearer tok123".
    #[test]
    fn bearer_auth_header() {
        let auth = BearerAuth::new("tok123").expect("valid ASCII token must construct");
        let (name, value) = auth.auth_header().expect("BearerAuth must return a header");
        assert_eq!(name, "authorization");
        assert_eq!(value, "Bearer tok123");
    }

    /// Oracle: BearerAuth constructor rejects tokens containing C0 control characters.
    /// HeaderValue::from_str rejects bytes 0x00-0x08 and 0x0A-0x1F (C0 controls,
    /// excluding HTAB 0x09) and 0x7F (DEL). '\x01' (SOH) is unconditionally invalid
    /// per RFC 7230 §3.2.6 and the http crate's header validation.
    #[test]
    fn bearer_auth_invalid_token_rejected() {
        let result = BearerAuth::new("tok\x01abc");
        assert!(
            result.is_err(),
            "token with C0 control character must be rejected by constructor"
        );
    }

    /// Oracle: BasicAuth constructs successfully with valid username and password.
    #[test]
    fn basic_auth_valid_constructs() {
        assert!(BasicAuth::new("alice", "s3cr3t").is_ok());
    }

    /// Oracle: BasicAuth constructor rejects usernames containing a colon (RFC 7617 §2).
    #[test]
    fn basic_auth_colon_in_username_rejected() {
        let result = BasicAuth::new("ali:ce", "s3cr3t");
        match result {
            Ok(_) => panic!("username with colon must be rejected by constructor"),
            Err(e) => {
                let err_msg = e.to_string();
                assert!(
                    err_msg.contains("username"),
                    "error message should mention 'username', got: {err_msg}"
                );
            }
        }
    }

    /// Oracle: `echo -n "alice:s3cr3t" | base64` → `YWxpY2U6czNjcjN0`  (RFC 7617 §2)
    /// This expected value is computed independently of the code under test.
    #[test]
    fn basic_auth_header() {
        let auth = BasicAuth::new("alice", "s3cr3t").expect("valid credentials must construct");
        let (name, value) = auth.auth_header().expect("BasicAuth must return a header");
        assert_eq!(name, "authorization");
        assert_eq!(value, "Basic YWxpY2U6czNjcjN0");
    }

    /// Oracle: CustomCaTransport injects no auth header — it is a transport only.
    #[test]
    fn custom_ca_transport_no_build_with_empty_cert() {
        // Empty DER bytes will fail Certificate::from_der; this test confirms
        // CustomCaTransport is constructible and that auth is separate.
        let transport = CustomCaTransport::new(vec![]);
        assert!(transport.build_client().is_err(), "empty DER must fail");
    }

    /// Oracle: BearerAuth constructor rejects an empty token string.
    /// An empty token would produce "Bearer " which is a malformed credential.
    #[test]
    fn bearer_auth_empty_token_rejected() {
        let result = BearerAuth::new("");
        match result {
            Ok(_) => panic!("empty token must be rejected by constructor"),
            Err(ClientError::InvalidArgument(msg)) => {
                assert!(
                    msg.contains("empty"),
                    "error message should mention 'empty', got: {msg}"
                );
            }
            Err(e) => panic!("expected InvalidArgument, got: {e}"),
        }
    }

    /// Oracle: BearerAuth constructor rejects a whitespace-only token string.
    /// A whitespace-only token would produce "Bearer   " which is a malformed credential.
    #[test]
    fn bearer_auth_whitespace_only_token_rejected() {
        let result = BearerAuth::new("   ");
        match result {
            Ok(_) => panic!("whitespace-only token must be rejected by constructor"),
            Err(ClientError::InvalidArgument(msg)) => {
                assert!(
                    msg.contains("whitespace"),
                    "error message should mention 'whitespace', got: {msg}"
                );
            }
            Err(e) => panic!("expected InvalidArgument, got: {e}"),
        }
    }

    /// Oracle: DefaultTransport uses the default reqwest::Client which always builds successfully.
    #[tokio::test]
    async fn default_transport_builds_client() {
        DefaultTransport
            .build_client()
            .expect("DefaultTransport::build_client must succeed");
    }
}

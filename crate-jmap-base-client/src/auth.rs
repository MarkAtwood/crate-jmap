//! Auth traits and credential implementations for JMAP clients.
//!
//! Provides [`TransportConfig`] (TLS/HTTP client construction) and
//! [`AuthProvider`] (per-request credential injection), plus built-in
//! implementations: [`DefaultTransport`], [`CustomCaTransport`],
//! [`NoneAuth`], [`BearerAuth`], and [`BasicAuth`].

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use reqwest::header::HeaderValue;
use zeroize::Zeroizing;

use crate::error::ClientError;

// ---------------------------------------------------------------------------
// TransportConfig — HTTP client construction (TLS, timeouts, trust roots)
// ---------------------------------------------------------------------------

/// Opaque HTTP client returned by [`TransportConfig::build_client`]
/// (bd:JMAP-6r7c.36).
///
/// The inner third-party type is private; the wrapper exists so the JMAP
/// transport identity does not leak through the public trait signature.
/// A future swap of the underlying HTTP library (e.g. `ureq`, `hyper-util`
/// directly, `curl`) replaces the wrapped type without breaking any
/// downstream extension client or custom `TransportConfig` impl that
/// returns `Result<HttpClient, ClientError>` from `build_client`.
///
/// Custom transports construct via [`HttpClient::new`] — that signature
/// still references [`reqwest::Client`] (the only construction path the
/// kit knows how to make HTTP requests against). The partial-wrap
/// argument mirrors [`ParseError`](crate::error::ParseError) /
/// [`SerializeError`](crate::error::SerializeError): the variant
/// payload / return type is opaque, but the construction signature still
/// names the third-party type so callers have a way in. A future
/// transport swap would deprecate this constructor in favor of an
/// analogous one for the new HTTP client; the wrapper type itself
/// stays stable.
#[non_exhaustive]
pub struct HttpClient(reqwest::Client);

impl HttpClient {
    /// Wrap a [`reqwest::Client`] into an opaque [`HttpClient`].
    ///
    /// Custom [`TransportConfig`] impls use this constructor to wrap a
    /// reqwest client they built with their own TLS / proxy / timeout
    /// configuration:
    ///
    /// ```rust,ignore
    /// impl TransportConfig for MyCustomTransport {
    ///     fn build_client(&self) -> Result<HttpClient, ClientError> {
    ///         let client = reqwest::ClientBuilder::new()
    ///             .proxy(...)
    ///             .build()
    ///             .map_err(ClientError::from_reqwest)?;
    ///         Ok(HttpClient::new(client))
    ///     }
    /// }
    /// ```
    pub fn new(client: reqwest::Client) -> Self {
        Self(client)
    }

    /// Consume the wrapper and return the inner [`reqwest::Client`].
    ///
    /// `pub(crate)` so only this crate's [`JmapClient`](crate::JmapClient)
    /// construction path can unwrap — external code cannot reach inside
    /// the opaque wrapper. A future swap of the HTTP transport would
    /// change the return type here without affecting external callers
    /// (who only see the typed `Result<HttpClient, _>` from
    /// [`TransportConfig::build_client`]).
    pub(crate) fn into_inner(self) -> reqwest::Client {
        self.0
    }
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HttpClient").finish()
    }
}

/// Controls how the underlying [`HttpClient`] is constructed.
///
/// Implementations configure TLS trust roots, client certificates, and
/// connect timeouts. This is separate from credential injection
/// (see [`AuthProvider`]) so transports and credentials compose freely.
///
/// **Implement this trait** when you need custom TLS logic (e.g. a private CA
/// or a client certificate).  For custom per-request credentials only,
/// implement [`AuthProvider`] instead.  [`DefaultTransport`] covers the common
/// case of publicly-trusted TLS with no custom certificates.
///
/// **Return type contract (bd:JMAP-6r7c.36).** `build_client` returns an
/// opaque [`HttpClient`] wrapper, not a bare [`reqwest::Client`]. Custom
/// impls construct via [`HttpClient::new`] after building their reqwest
/// client; the wrapper insulates the trait's public surface from a
/// future HTTP-transport swap.
///
/// **Maintainer note (bd:JMAP-6lsm.19):** if you add a new method to this
/// trait, update the manual blanket impl for `Box<dyn TransportConfig>` at
/// the bottom of this file. The crate ships a hand-written forwarding impl
/// for the boxed trait object so callers can store heterogeneous transport
/// configurations behind a single type. Adding a method here without
/// mirroring it on the blanket impl silently breaks the
/// `JmapClient::new(Box::<dyn TransportConfig>::new(...))` call shape.
pub trait TransportConfig: Send + Sync {
    /// Build the [`HttpClient`] for this transport configuration.
    fn build_client(&self) -> Result<HttpClient, ClientError>;
}

/// Standard reqwest client with a 10-second connect timeout; no custom TLS.
///
/// Use for servers with publicly-trusted certificates. Pair with any
/// [`AuthProvider`] for credential injection.
#[derive(Debug, Clone)]
pub struct DefaultTransport;

impl TransportConfig for DefaultTransport {
    fn build_client(&self) -> Result<HttpClient, ClientError> {
        default_reqwest_client().map(HttpClient::new)
    }
}

/// Custom CA trust root (DER-encoded). No `Authorization` header is injected.
///
/// Use when the server presents a certificate signed by a private CA.
/// Pair with any [`AuthProvider`] for credential injection — including
/// [`BearerAuth`] or [`BasicAuth`] if the server also requires credentials.
///
/// # Trust scope (bd:JMAP-6r7c.57)
///
/// **The bundled public webpki-roots are DISABLED in the constructed
/// reqwest client.** This type is intended for private-CA pinning —
/// connecting to a JMAP server identified by a private CA the operator
/// controls, *refusing* certificates signed by any public CA. That is
/// the threat model where this transport matters: a corporate internal
/// JMAP server, a service-mesh deployment, an air-gapped network. A
/// compromised or malicious public CA (DigiNotar 2011, Symantec 2017,
/// etc.) issuing a certificate for the target host name would otherwise
/// bypass the private-CA defense entirely; disabling the public roots
/// closes that gap.
///
/// If you want trust against BOTH the bundled public roots AND a custom
/// CA (a "hybrid" deployment), `CustomCaTransport` is the wrong tool —
/// implement [`TransportConfig`] directly with the additive behaviour
/// (`reqwest::ClientBuilder::add_root_certificate` does NOT call
/// `.tls_built_in_root_certs(false)` by default, so a hand-rolled impl
/// has the additive shape automatically).
#[derive(Clone)]
pub struct CustomCaTransport {
    der_cert: Vec<u8>,
}

impl CustomCaTransport {
    /// Construct a `CustomCaTransport` from a DER-encoded CA certificate.
    pub fn new(der_cert: Vec<u8>) -> Self {
        Self { der_cert }
    }

    /// Construct a `CustomCaTransport` from a PEM-encoded CA certificate
    /// (bd:JMAP-6r7c.37).
    ///
    /// Operators typically distribute private-CA certificates as PEM
    /// files (text-format, `-----BEGIN CERTIFICATE-----` framing).
    /// Without this helper, every caller has to convert PEM to DER
    /// themselves before passing to [`CustomCaTransport::new`]:
    ///
    /// ```rust,ignore
    /// // Without from_pem_bytes (the long way):
    /// let pem_bytes = std::fs::read("ca.pem")?;
    /// let der = rustls_pemfile::certs(&mut pem_bytes.as_slice())
    ///     .next()
    ///     .transpose()?
    ///     .ok_or("no certificate in PEM file")?
    ///     .to_vec();
    /// let transport = CustomCaTransport::new(der);
    ///
    /// // With from_pem_bytes (the short way):
    /// let transport = CustomCaTransport::from_pem_bytes(&std::fs::read("ca.pem")?)?;
    /// ```
    ///
    /// The first PEM-framed certificate in `pem_bytes` is used. To use
    /// a different certificate from a multi-cert bundle, split the
    /// bundle yourself and pass the desired one. Multi-cert chains
    /// (root + intermediate) require constructing a custom
    /// [`TransportConfig`] implementation that adds multiple roots —
    /// `CustomCaTransport` is single-root.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidArgument`] if `pem_bytes` does not
    /// contain a recognisable PEM-framed certificate or if the PEM
    /// body cannot be base64-decoded.
    ///
    /// **DER validity is NOT checked at this stage.** This matches the
    /// existing [`CustomCaTransport::new`] contract — invalid DER
    /// (PEM body that decodes to non-DER bytes) is detected later when
    /// the `JmapClient` is constructed and the underlying transport
    /// tries to load the root, at which point it surfaces as
    /// [`ClientError::Http`]. The PEM helper deliberately matches the
    /// DER helper's behaviour: cheap validation here, full validation
    /// at client-build time.
    pub fn from_pem_bytes(pem_bytes: &[u8]) -> Result<Self, ClientError> {
        // The PEM-to-DER conversion uses a minimal in-line decoder so
        // this crate does not need to depend on rustls_pemfile. DER
        // semantic validity is the underlying transport's
        // responsibility (it happens at build_client time, where
        // reqwest::Certificate::from_der + ClientBuilder do the
        // actual rustls/native-tls parse).
        let cert_bytes = parse_first_pem_cert(pem_bytes).ok_or_else(|| {
            ClientError::InvalidArgument(
                "CustomCaTransport::from_pem_bytes: no PEM-framed certificate found in input"
                    .into(),
            )
        })?;
        Ok(Self {
            der_cert: cert_bytes,
        })
    }
}

/// Extract the DER bytes of the first PEM-framed certificate in `input`.
///
/// PEM (RFC 7468) format: `-----BEGIN <label>-----` / base64 body /
/// `-----END <label>-----`. We accept any label whose payload is a
/// valid DER certificate (the most common label is `CERTIFICATE`;
/// some toolchains emit `X509 CERTIFICATE` or `TRUSTED CERTIFICATE`).
///
/// Returns `None` if no PEM frame is found or if the base64 body
/// cannot be decoded. The DER validity check is the caller's
/// responsibility (do it via `reqwest::Certificate::from_der`).
fn parse_first_pem_cert(input: &[u8]) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let text = std::str::from_utf8(input).ok()?;
    // Find any BEGIN line. RFC 7468 §3 mandates exactly five hyphens
    // and an ASCII-uppercase label; we accept the common shapes.
    let begin_idx = text.find("-----BEGIN ")?;
    let after_begin = &text[begin_idx + "-----BEGIN ".len()..];
    let begin_eol = after_begin.find('\n')?;
    let label = after_begin[..begin_eol].trim().trim_end_matches('-').trim();
    let end_marker = format!("-----END {label}-----");
    let body_start = begin_idx + "-----BEGIN ".len() + begin_eol + 1;
    let end_offset = text[body_start..].find(end_marker.as_str())?;
    let body = &text[body_start..body_start + end_offset];
    // Strip whitespace from the base64 body. PEM allows line wraps
    // every 64 chars per RFC 7468 §3; the base64 standard engine
    // does not accept embedded whitespace.
    let body_no_ws: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(body_no_ws)
        .ok()
}

/// Manual `Debug` impl that redacts the DER-encoded CA bytes
/// (bd:JMAP-6r7c.13).
///
/// The DER bytes are not a credential, but they are deployment-identifying
/// material: a CA certificate uniquely identifies the deployment's PKI
/// (Subject DN, public key, signing algorithm, validity window, X.509
/// extensions). In federated or multi-tenant scenarios, surfacing those
/// bytes in `tracing` output reveals which private-CA-using customer the
/// client is configured to talk to. Print the length only and let the
/// caller obtain the bytes via a constructor-controlled path if they
/// genuinely need them.
///
/// Mirrors the redacting `Debug` impls on `BearerAuth` and `BasicAuth`
/// in this file and on `Session` and `AccountInfo` in `request.rs`.
impl std::fmt::Debug for CustomCaTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomCaTransport")
            .field("der_cert", &format_args!("<{} bytes>", self.der_cert.len()))
            .finish()
    }
}

impl TransportConfig for CustomCaTransport {
    fn build_client(&self) -> Result<HttpClient, ClientError> {
        let cert =
            reqwest::Certificate::from_der(&self.der_cert).map_err(ClientError::from_reqwest)?;
        // Replace (not augment) the trust root set with the configured
        // private CA. tls_built_in_root_certs(false) disables the bundled
        // webpki-roots before add_root_certificate adds the private CA —
        // the order matters because reqwest treats add_root_certificate
        // as additive (bd:JMAP-6r7c.57).
        let client = reqwest::ClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(10))
            .tls_built_in_root_certs(false)
            .add_root_certificate(cert)
            .build()
            .map_err(ClientError::from_reqwest)?;
        Ok(HttpClient::new(client))
    }
}

// ---------------------------------------------------------------------------
// AuthProvider — per-request credential injection (Authorization header)
// ---------------------------------------------------------------------------

/// Single HTTP `(name, value)` header pair, returned by
/// [`AuthProvider::auth_header`] (bd:JMAP-6r7c.62, bd:JMAP-6r7c.20).
///
/// The wrapper exists for two purposes:
///
/// 1. **Compile-time secret-typing.** [`AuthHeader`]'s `Debug` impl
///    redacts the header value to `"[REDACTED]"`. A future
///    [`AuthProvider`] impl that writes
///    `tracing::trace!(?header, "injecting")` cannot leak the credential
///    through that path because the wrapper's `Debug` output never
///    contains the value bytes. The pre-bd:JMAP-6r7c.62 shape
///    (`Option<(&str, &str)>`) had no such guard — a string tuple
///    formats verbatim via `?`-syntax.
/// 2. **Bounded API surface.** The wrapper packages exactly one
///    `(name, value)` pair. The trait's signature does not admit a
///    list, a sequence, or a per-request-computed value. This is the
///    intentional limitation: `AuthProvider` covers "static,
///    per-connection single-header auth schemes" only (Bearer, Basic,
///    mTLS via [`TransportConfig`]). Schemes that need multiple
///    request-dependent headers (AWS SigV4, OAuth request signing) or
///    async credential refresh require a different abstraction —
///    currently, custom [`TransportConfig`] impls that wire per-request
///    middleware (bd:JMAP-6r7c.20).
///
/// Construct via [`AuthHeader::new`] — both `name` and `value` are
/// caller-supplied borrows; the wrapper stashes them as-is. The
/// constructor does not validate HTTP-header-value syntax; downstream
/// consumers (e.g. [`connect_ws`](crate::ws::connect_ws)) validate at
/// the call site and surface [`ClientError::InvalidArgument`] for
/// invalid bytes.
#[non_exhaustive]
#[derive(Clone, Copy)]
pub struct AuthHeader<'a> {
    name: &'a str,
    value: &'a str,
}

impl<'a> AuthHeader<'a> {
    /// Construct an [`AuthHeader`] from a header name and value borrow.
    pub fn new(name: &'a str, value: &'a str) -> Self {
        Self { name, value }
    }

    /// Borrow the header name. Lowercase-ASCII per RFC 9110 §5.1.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Borrow the header value.
    ///
    /// **Do not log this return value.** The value is credential
    /// material; see the type-level rustdoc. The constructor name is
    /// deliberately explicit ([`expose_value`](Self::expose_value)) so a
    /// call site reveals the intent — a `tracing::*` line that
    /// references `header.expose_value()` is visible in code review,
    /// whereas a `?header` formatter is not.
    pub fn expose_value(&self) -> &'a str {
        self.value
    }
}

impl std::fmt::Debug for AuthHeader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthHeader")
            .field("name", &self.name)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

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
/// it contains credentials. The [`AuthHeader`] return type provides a
/// compile-time guard against the most common leak path — its `Debug`
/// impl redacts the value bytes — but the explicit
/// [`expose_value`](AuthHeader::expose_value) accessor must not be fed
/// into a `tracing::*` argument either.
///
/// [`auth_header`]: AuthProvider::auth_header
///
/// # Intentional limitation: static single-header per-connection schemes (bd:JMAP-6r7c.20)
///
/// The trait shape commits the kit to "static, per-connection,
/// single-header auth schemes" — bearer-token, HTTP Basic, mTLS via
/// [`TransportConfig`]. Three constraints follow from the
/// [`AuthHeader`] return type:
///
/// 1. **One header per request.** A scheme that needs to attach
///    multiple headers per request (AWS SigV4 carries
///    `Authorization`, `X-Amz-Date`, and `X-Amz-Security-Token`
///    together) cannot be expressed by this trait.
/// 2. **No per-request signature.** [`auth_header`] takes `&self`
///    only — there is no access to the request URL, method, or body.
///    Schemes that compute an HMAC over the request body (SigV4,
///    OAuth request signing) cannot be expressed.
/// 3. **No async refresh.** [`auth_header`] is sync. A scheme that
///    needs to refresh an expired OAuth token before returning
///    cannot await inside this method.
///
/// Workaround for callers who need any of the three: implement a
/// custom [`TransportConfig`] that wires per-request middleware into
/// the [`HttpClient`] it returns from
/// [`build_client`](TransportConfig::build_client). The middleware can
/// observe the full request, compute signatures, and refresh tokens
/// asynchronously. The cost is the awkward layering inversion — TLS
/// config and credential injection conceptually belong to different
/// traits — but it does compose against the existing
/// [`AuthProvider::auth_header`] trait without breakage.
///
/// A future reshape that supports the three constraints (likely a
/// new trait, not a backward-compatible widening of this one) would
/// not deprecate `AuthProvider`. The current trait stays as the
/// "fast path for the common case" alongside any richer abstraction.
///
/// # Credential lifetime
///
/// Implementations that cache header bytes (e.g. [`BearerAuth`],
/// [`BasicAuth`]) SHOULD wrap the cached buffer in [`zeroize::Zeroizing`]
/// or equivalent so the credential is overwritten on drop rather than
/// left in freed heap until the allocator re-uses the slab. Callers that
/// build a credential string before passing it into a constructor (e.g.
/// `BearerAuth::new(token)`) SHOULD likewise store that string in a
/// `Zeroizing<String>` — the zeroization done by the auth-type is bounded
/// by what the type owns and cannot reach back into the caller's buffer
/// (bd:JMAP-6r7c.59).
///
/// **Maintainer note (bd:JMAP-6lsm.19):** if you add a new method to this
/// trait, update BOTH manual blanket impls — `Box<dyn AuthProvider>` and
/// `Arc<dyn AuthProvider>` — at the bottom of this file. The crate
/// supports both Box and Arc trait-object call shapes (e.g. for sharing
/// one credential source across multiple `JmapClient`s), and a missing
/// blanket method silently breaks one of those shapes without breaking
/// the other.
pub trait AuthProvider: Send + Sync {
    /// Return an optional [`AuthHeader`] to attach to every request.
    ///
    /// Returns `None` when no `Authorization` header is required.
    ///
    /// The header name and value both borrow from `self` and must live
    /// at least as long as the `&self` borrow. Implementations that
    /// pre-compute the values at construction time can return
    /// `AuthHeader::new("authorization", &self.field)` directly,
    /// avoiding any per-request allocation.
    ///
    /// # Implementation contract
    ///
    /// The returned strings **must** be valid HTTP field values (RFC 9110 §5):
    /// - Header name: lowercase ASCII token characters only (no spaces, no
    ///   control characters); e.g. `"authorization"`.
    /// - Header value: visible ASCII characters (0x21–0x7E) and horizontal tab
    ///   (0x09) only; no other control characters.
    ///
    /// Implementations that violate this contract will cause
    /// [`ClientError::InvalidArgument`] in `connect_ws` (`ws/mod.rs`), which
    /// parses the value into a typed [`http::HeaderValue`]. On HTTP code paths
    /// reqwest returns the error from `.send()` as a builder error rather than
    /// an `InvalidArgument` — the error type differs between the two paths.
    /// Test all custom `AuthProvider` implementations against both HTTP and
    /// WebSocket call paths.
    fn auth_header(&self) -> Option<AuthHeader<'_>>;
}

/// No authentication: no `Authorization` header.
#[derive(Debug, Clone)]
pub struct NoneAuth;

impl AuthProvider for NoneAuth {
    fn auth_header(&self) -> Option<AuthHeader<'_>> {
        None
    }
}

/// Bearer-token authentication (`Authorization: Bearer <token>`).
///
/// # Drop-path zeroization
///
/// The cached header string is wrapped in [`zeroize::Zeroizing`] so its
/// buffer is overwritten with zeros before being returned to the allocator
/// on drop. This defends against credential recovery from process core
/// dumps, `/proc/PID/mem` inspection, and post-drop heap re-use across
/// tenants in long-running multi-user JMAP clients (bd:JMAP-6r7c.59).
/// Callers that hold the original token string SHOULD also store it in a
/// `Zeroizing<String>` or equivalent — the zeroization here is bounded by
/// what this type owns.
///
/// # Do not move validation from construction to per-request (bd:JMAP-6r7c.18)
///
/// A future contributor may suggest "just store the token field and call
/// `HeaderValue::from_str` in `auth_header` on each request". This is the
/// wrong simplification for both `BearerAuth` and `BasicAuth`. Five
/// reasons:
///
/// 1. **Fail-fast at auth setup.** Validation at construction means
///    invalid credentials surface at `BearerAuth::new()` return value —
///    the caller fails near the bug source (their auth-setup code).
///    Per-request validation pushes failures to the first
///    `JmapClient::call()` or `fetch_session()`, far from the bug and
///    harder to debug.
/// 2. **Hot-path performance.** `auth_header` is called on every HTTP
///    request and every WebSocket connection. `HeaderValue::from_str`
///    walks the string and rejects on the first non-VCHAR/SP/HTAB
///    octet (RFC 7230 §3.2.6) — non-trivial work for a hot path.
///    Pre-validation moves that work out of every request.
/// 3. **Infallible accessor signature.** Pre-validation lets
///    `auth_header` keep the signature
///    `fn auth_header(&self) -> Option<AuthHeader<'_>>` — infallible.
///    Per-request validation would require
///    `Result<Option<(&str, &str)>, ClientError>`, propagating an
///    extra error layer through every call site (HTTP `call`, blob
///    upload/download, WebSocket connect, session fetch).
/// 4. **Borrow simplicity.** Storing as `Zeroizing<String>` lets
///    `auth_header` return borrows directly without ownership tricks
///    (`Cow`, `Box<str>`, etc.). The borrow checker stays simple, the
///    call sites stay readable.
/// 5. **Debug-redaction tripwire compatibility.** The manual `Debug`
///    impls on `BearerAuth` and `BasicAuth` (auth.rs further below)
///    target the stored field. A future contributor adding
///    `#[derive(Debug)]` instead of the manual impl is caught
///    immediately by the existing canary tests
///    `bearer_auth_debug_does_not_leak_token` and
///    `basic_auth_debug_does_not_leak_credentials` (bd:JMAP-sc1b.79).
///    Moving to per-request validation requires the field shape to
///    change in a way that re-derives the canary contract — extra
///    surface area for review without buying anything.
///
/// This is the same pre-validate-at-construction pattern `rustls` and
/// `reqwest` use for their own type designs. It is not over-engineering.
#[derive(Clone)]
pub struct BearerAuth {
    // Pre-validated at construction and stored as String: avoids per-request
    // allocation and ensures invalid credentials fail at construction, not at
    // the first request. Storing as String eliminates the need for a fallible
    // to_str() call in auth_header().
    //
    // Wrapped in Zeroizing<String> so the buffer is overwritten on drop
    // (see type-level doc). Zeroizing<String> Derefs to String, which Derefs
    // to &str, so `&self.header_string` in auth_header() coerces cleanly.
    header_string: Zeroizing<String>,
}

impl BearerAuth {
    /// Construct a `BearerAuth` from a Bearer token string.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`] if `token` is empty or contains
    ///   whitespace (RFC 6750 §2.1 bearer tokens must not contain whitespace).
    /// - [`ClientError::InvalidHeaderValue`] if `token` contains characters that
    ///   are not valid in an HTTP header value (non-visible-ASCII octets).
    pub fn new(token: &str) -> Result<Self, ClientError> {
        if token.is_empty() || token.chars().any(|c| c.is_ascii_whitespace()) {
            return Err(ClientError::InvalidArgument(
                "BearerAuth token may not be empty or contain whitespace (RFC 6750 §2.1)".into(),
            ));
        }
        let header_string = Zeroizing::new(format!("Bearer {token}"));
        // Validate the header value is legal (no control characters, etc.).
        HeaderValue::from_str(&header_string).map_err(ClientError::from_invalid_header)?;
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
    fn auth_header(&self) -> Option<AuthHeader<'_>> {
        Some(AuthHeader::new("authorization", &self.header_string))
    }
}

/// HTTP Basic authentication (`Authorization: Basic <base64(username:password)>`).
///
/// Credentials are encoded per RFC 7617: `base64(username ":" password)`.
///
/// # Drop-path zeroization
///
/// The cached header string is wrapped in [`zeroize::Zeroizing`] so its
/// buffer is overwritten with zeros before being returned to the allocator
/// on drop. The intermediate `username:password` plaintext built during
/// base64 encoding is ALSO zeroized — that buffer is the most
/// attack-relevant artifact because it carries the raw password rather
/// than the base64-encoded form. See [`BearerAuth`] for the threat model.
/// (bd:JMAP-6r7c.59)
#[derive(Clone)]
pub struct BasicAuth {
    // Pre-validated at construction and stored as String: avoids per-request
    // allocation and ensures invalid credentials fail at construction, not at
    // the first request. Storing as String eliminates the need for a fallible
    // to_str() call in auth_header().
    //
    // Wrapped in Zeroizing<String> so the buffer is overwritten on drop
    // (see type-level doc).
    header_string: Zeroizing<String>,
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
        // The intermediate plaintext buffer is the most sensitive artifact
        // — it carries the raw password, whereas the base64-encoded form is
        // one step further from a credential a replay attacker can use.
        // Wrap it in Zeroizing so the buffer is overwritten when the local
        // goes out of scope at the end of this function.
        let plaintext = Zeroizing::new(format!("{username}:{password}"));
        let encoded = BASE64_STANDARD.encode(plaintext.as_bytes());
        let header_string = Zeroizing::new(format!("Basic {encoded}"));
        // Validate the header value is legal (base64 is always printable ASCII,
        // but keep the check for correctness).
        HeaderValue::from_str(&header_string).map_err(ClientError::from_invalid_header)?;
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
    fn auth_header(&self) -> Option<AuthHeader<'_>> {
        Some(AuthHeader::new("authorization", &self.header_string))
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
        .map_err(ClientError::from_reqwest)
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
    fn build_client(&self) -> Result<HttpClient, ClientError> {
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
    fn auth_header(&self) -> Option<AuthHeader<'_>> {
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
    fn auth_header(&self) -> Option<AuthHeader<'_>> {
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
        let header = auth.auth_header().expect("BearerAuth must return a header");
        assert_eq!(header.name(), "authorization");
        assert_eq!(header.expose_value(), "Bearer tok123");
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
        let header = auth.auth_header().expect("BasicAuth must return a header");
        assert_eq!(header.name(), "authorization");
        assert_eq!(header.expose_value(), "Basic YWxpY2U6czNjcjN0");
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

    /// bd:JMAP-6r7c.36 — `TransportConfig::build_client` now returns
    /// `Result<HttpClient, _>`, not `Result<reqwest::Client, _>`. The
    /// wrapper exists so the trait's public signature does not name the
    /// underlying HTTP library, insulating extension clients and custom
    /// transport impls from a future transport swap.
    ///
    /// The compile-time witness below pins the new shape; if a future
    /// refactor accidentally widens the return type back to
    /// `reqwest::Client`, the explicit typed `let` binding here breaks
    /// the build.
    #[tokio::test]
    async fn build_client_returns_opaque_http_client() {
        let result: Result<HttpClient, ClientError> = DefaultTransport.build_client();
        let http = result.expect("DefaultTransport::build_client must succeed");
        // Debug output is opaque — no inner reqwest::Client representation.
        let dbg = format!("{http:?}");
        assert_eq!(
            dbg, "HttpClient",
            "HttpClient Debug must be opaque; the wrapper is the only public surface"
        );
    }

    /// bd:JMAP-6r7c.36 — A custom `TransportConfig` impl constructs the
    /// returned `HttpClient` via `HttpClient::new(reqwest::Client)`. This
    /// pins the public construction path; if the constructor signature
    /// changes, the custom-impl pattern below fails to compile and
    /// downstream consumers will pick up the same migration signal at
    /// build time.
    #[test]
    fn http_client_new_is_callable_from_custom_transport_impl() {
        struct StubTransport;
        impl TransportConfig for StubTransport {
            fn build_client(&self) -> Result<HttpClient, ClientError> {
                let client = reqwest::ClientBuilder::new()
                    .build()
                    .map_err(ClientError::from_reqwest)?;
                Ok(HttpClient::new(client))
            }
        }

        StubTransport
            .build_client()
            .expect("custom transport must build the opaque HttpClient");
    }

    /// bd:JMAP-6r7c.62 — `AuthHeader`'s `Debug` impl MUST redact the value
    /// bytes to "[REDACTED]". This is the compile-time guard against a
    /// future `AuthProvider` impl that writes `tracing::trace!(?header,
    /// ...)`. The pre-bd:JMAP-6r7c.62 shape `Option<(&str, &str)>` would
    /// have rendered the value verbatim via `?`-formatter. The canary
    /// literal is the test's independent oracle, never derived from
    /// `AuthHeader`'s internal state.
    #[test]
    fn auth_header_debug_redacts_value() {
        const CANARY: &str = "CANARY-AUTH-VALUE-DO-NOT-LEAK-456";
        let header = AuthHeader::new("authorization", CANARY);
        let dbg = format!("{header:?}");
        assert!(
            !dbg.contains(CANARY),
            "AuthHeader Debug must not contain the canary value: {dbg}"
        );
        assert!(
            dbg.contains("[REDACTED]"),
            "AuthHeader Debug must render '[REDACTED]' for the value field: {dbg}"
        );
        // The name is non-sensitive and may surface to aid diagnostics.
        assert!(
            dbg.contains("authorization"),
            "AuthHeader Debug should include the header name for diagnostic value: {dbg}"
        );
    }

    /// bd:JMAP-6r7c.62 — `expose_value` is the only path to the credential
    /// bytes, so the call-site name (`expose_value`) is the visible
    /// signal in code review. This test pins the accessor name + return
    /// value, so a future rename of the accessor breaks the test loudly.
    #[test]
    fn auth_header_expose_value_returns_credential_bytes() {
        const VALUE: &str = "Bearer some-token-123";
        let header = AuthHeader::new("authorization", VALUE);
        assert_eq!(header.name(), "authorization");
        assert_eq!(header.expose_value(), VALUE);
    }

    /// Oracle: BearerAuth's Debug impl never reveals the underlying token.
    ///
    /// Tripwire against a future refactor that adds `#[derive(Debug)]` to
    /// BearerAuth (clearing the manual redacting impl), or that prints the
    /// inner `header_string`. The canary literal is the independent
    /// oracle — it is under the test's control, never derived from
    /// BearerAuth's internal state.
    #[test]
    fn bearer_auth_debug_does_not_leak_token() {
        const CANARY: &str = "CANARY-TOKEN-DO-NOT-LEAK-123";
        let auth = BearerAuth::new(CANARY).expect("valid ASCII token must construct");
        let dbg = format!("{auth:?}");
        assert!(
            !dbg.contains(CANARY),
            "BearerAuth Debug must not contain the raw token; got: {dbg}"
        );
    }

    /// Oracle: BasicAuth's Debug impl never reveals the underlying credentials.
    ///
    /// Same tripwire shape as `bearer_auth_debug_does_not_leak_token`.
    /// The canary username and password are independent literals; the
    /// assertion verifies neither, nor the base64 encoding of their
    /// concatenation, appears in the Debug output.
    #[test]
    fn basic_auth_debug_does_not_leak_credentials() {
        const CANARY_USER: &str = "CANARY-USER-DO-NOT-LEAK";
        const CANARY_PASS: &str = "CANARY-PASS-DO-NOT-LEAK";
        let auth =
            BasicAuth::new(CANARY_USER, CANARY_PASS).expect("valid credentials must construct");
        let dbg = format!("{auth:?}");
        assert!(
            !dbg.contains(CANARY_USER),
            "BasicAuth Debug must not contain the raw username; got: {dbg}"
        );
        assert!(
            !dbg.contains(CANARY_PASS),
            "BasicAuth Debug must not contain the raw password; got: {dbg}"
        );
        // Also catch a regression that prints the pre-validated header_string,
        // which would surface the base64-encoded credentials.
        let base64_pair = BASE64_STANDARD.encode(format!("{CANARY_USER}:{CANARY_PASS}"));
        assert!(
            !dbg.contains(&base64_pair),
            "BasicAuth Debug must not contain the base64-encoded credentials; got: {dbg}"
        );
    }

    /// Oracle: `CustomCaTransport`'s Debug impl never prints the raw DER
    /// certificate bytes (bd:JMAP-6r7c.13).
    ///
    /// CA DER bytes are not a credential, but they are deployment-identifying
    /// material — Subject DN, public key, signing algorithm, X.509
    /// extensions. Surfacing them in `tracing` output reveals which private-
    /// CA-using customer the client is configured for. The canary byte
    /// sequence is an unmistakable repeating literal `0xCA` 32 times — the
    /// test asserts neither the lower-hex nor the upper-hex nor the
    /// Rust-debug `[202, 202, ...]` rendering of those bytes appears in the
    /// Debug output. Same tripwire shape as the BearerAuth and BasicAuth
    /// tests above.
    #[test]
    fn custom_ca_transport_debug_does_not_leak_der_bytes() {
        // 32 copies of 0xCA — an unmistakable sentinel byte. No conformant
        // DER encoder produces a run like this, so any leakage path
        // surfaces it intact.
        let canary_der = vec![0xCA_u8; 32];
        let transport = CustomCaTransport::new(canary_der);
        let dbg = format!("{transport:?}");
        // Lowercase hex rendering of the canary.
        assert!(
            !dbg.contains("cacacacacacacacacacacacacacacacacacacacacacacacacacacacacacacaca"),
            "CustomCaTransport Debug must not contain lowercase-hex DER bytes; got: {dbg}"
        );
        // Uppercase hex rendering — in case a future fmt::Debug uses {:X}.
        assert!(
            !dbg.contains("CACACACACACACACACACACACACACACACACACACACACACACACACACACACACACACACA"),
            "CustomCaTransport Debug must not contain uppercase-hex DER bytes; got: {dbg}"
        );
        // Rust `[u8]` default Debug rendering — `[202, 202, ...]`. A
        // derive(Debug) regression on the field would emit this shape.
        assert!(
            !dbg.contains("202, 202, 202"),
            "CustomCaTransport Debug must not contain decimal-byte DER bytes; got: {dbg}"
        );
        // Positive assertion: the redacted form mentions the length, so a
        // reader of `tracing` output still knows the field is non-empty.
        assert!(
            dbg.contains("32 bytes"),
            "CustomCaTransport Debug should record the DER byte length; got: {dbg}"
        );
    }

    // bd:JMAP-6r7c.37 — PEM constructor tests.
    //
    // Oracle: a hand-generated self-signed certificate produced by
    // `openssl req -x509 -newkey rsa:2048 -nodes -days 36500
    // -subj "/CN=JMAP-6r7c.37 test CA"`. The PEM and DER forms of the
    // same certificate are committed under tests/fixtures/tls/. The PEM
    // → DER conversion ran via `openssl x509 -outform DER`. Both files
    // are oracles independent of the code under test: the PEM was not
    // produced by `parse_first_pem_cert` and the DER was not produced
    // by reqwest. The test asserts the round-trip matches OpenSSL's
    // canonical bytes.
    const TEST_CA_PEM: &[u8] = include_bytes!("../tests/fixtures/tls/test-ca.pem");
    const TEST_CA_DER: &[u8] = include_bytes!("../tests/fixtures/tls/test-ca.der");

    #[test]
    fn from_pem_bytes_extracts_der_matching_openssl_oracle() {
        let transport = CustomCaTransport::from_pem_bytes(TEST_CA_PEM)
            .expect("test-ca.pem fixture must parse as a valid CA");
        assert_eq!(
            transport.der_cert.as_slice(),
            TEST_CA_DER,
            "PEM-decoded DER must match the openssl-produced reference DER fixture"
        );
    }

    #[test]
    fn from_pem_bytes_rejects_empty_input() {
        let err = CustomCaTransport::from_pem_bytes(b"").expect_err("empty input must be rejected");
        assert!(
            matches!(err, ClientError::InvalidArgument(_)),
            "empty input must surface as InvalidArgument; got {err:?}"
        );
    }

    #[test]
    fn from_pem_bytes_rejects_input_with_no_pem_framing() {
        let err = CustomCaTransport::from_pem_bytes(b"this is not a PEM file")
            .expect_err("non-PEM input must be rejected");
        assert!(
            matches!(err, ClientError::InvalidArgument(_)),
            "non-PEM input must surface as InvalidArgument; got {err:?}"
        );
    }

    #[test]
    fn from_pem_bytes_rejects_pem_with_invalid_base64() {
        // PEM framing with junk inside — should fail base64 decode.
        let bad =
            b"-----BEGIN CERTIFICATE-----\nNOT VALID BASE64 @#$%\n-----END CERTIFICATE-----\n";
        let err =
            CustomCaTransport::from_pem_bytes(bad).expect_err("invalid base64 must be rejected");
        assert!(
            matches!(err, ClientError::InvalidArgument(_)),
            "invalid-base64 PEM must surface as InvalidArgument; got {err:?}"
        );
    }

    #[test]
    fn from_pem_bytes_accepts_garbage_der_payload_deferring_validation_to_build() {
        use base64::Engine as _;
        // Properly-PEM-framed garbage bytes: PEM framing is correct,
        // base64 decodes OK, but the inner bytes are not a DER
        // certificate. By design (matching CustomCaTransport::new's
        // contract), from_pem_bytes accepts these bytes — DER validity
        // is checked at build_client() time, where it surfaces as
        // ClientError::Http through reqwest. This test documents that
        // contract.
        let garbage_der = [0u8; 16];
        let body = base64::engine::general_purpose::STANDARD.encode(garbage_der);
        let pem = format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n");
        let transport = CustomCaTransport::from_pem_bytes(pem.as_bytes())
            .expect("PEM framing OK + base64 OK = constructor accepts");
        assert_eq!(
            transport.der_cert.as_slice(),
            &garbage_der,
            "PEM helper must extract the exact base64-decoded bytes"
        );
        // build_client() is where rustls/native-tls actually parses the
        // DER and would reject the garbage. Exercising that here would
        // require constructing a real ClientBuilder, which is covered
        // by the broader test suite's integration tests.
    }

    // Note: a dyn-AuthProvider Debug test (bead JMAP-sc1b.79 item #4) is
    // intentionally omitted. The AuthProvider trait does not have
    // `std::fmt::Debug` as a supertrait, so `Box<dyn AuthProvider>` is
    // not `Debug`-formattable. Adding `Debug` to the trait bound would
    // be a foundation-crate public API change far outside the scope of
    // a regression-test bead. The concrete-type tests above already
    // catch the hygiene contract for every shipped AuthProvider
    // implementation; the only way a new AuthProvider leaks credentials
    // via Debug is if its own concrete impl does so, and that is
    // caught by the new-impl reviewer (cookie-cutter rule).
}

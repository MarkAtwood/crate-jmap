//! The unsigned HTTPS GET of a peer's `/.well-known/jmap`
//! (draft-atwood-jmap-chat-federation-00 §Key Publication) and the RFC
//! 9111 freshness lifetime of its response.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use http::HeaderMap;
use http::header::{AGE, CACHE_CONTROL, DATE, EXPIRES};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect;

/// The largest response body [`HttpsFetcher`] accepts, in bytes.
///
/// ponytail: one fixed cap. A Session object is a few kilobytes.
pub const MAX_BODY_BYTES: usize = 1 << 20;

/// The request timeout of [`HttpsFetcher`].
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The most redirects [`HttpsFetcher`] follows (reqwest's default).
pub const MAX_REDIRECTS: usize = 10;

/// A 2xx response to [`Fetch::get`].
#[derive(Debug, Clone, Default)]
pub struct Response {
    /// The response header fields; [`freshness_lifetime`] reads them.
    pub headers: HeaderMap,
    /// The response body.
    pub body: Vec<u8>,
}

/// An unsigned HTTPS GET.
///
/// §Key Publication: the key fetch "is an unsigned GET: the fetching
/// server MUST NOT sign it". Implementations return a 2xx response, and
/// [`FetchError::Status`] for any other status.
pub trait Fetch: Send + Sync {
    /// GETs `url`.
    fn get(&self, url: &str) -> impl Future<Output = Result<Response, FetchError>> + Send;
}

/// Error from [`Fetch::get`].
///
/// `#[non_exhaustive]`, so new variants stay semver-additive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The response status was not 2xx.
    #[error("HTTP status {0}")]
    Status(u16),
    /// The response body is longer than [`MAX_BODY_BYTES`].
    #[error("response body exceeds {MAX_BODY_BYTES} bytes")]
    TooLarge,
    /// The request failed before a complete response arrived: DNS, an
    /// address the policy refuses, TLS (including a certificate that
    /// does not validate for the host), a redirect to a non-`https` URI,
    /// connection, timeout.
    #[error("HTTP request failed")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Decides whether the fetcher may connect to an address.
type AddressPolicy = Arc<dyn Fn(IpAddr) -> bool + Send + Sync>;

/// A [`Fetch`] over reqwest that follows the §Key Publication transport
/// rules.
///
/// - HTTPS only, with the server certificate validated against the host
///   of each URI (reqwest's rustls verifier, RFC 9110 §4.3.4 and RFC
///   9525).
/// - "The receiving server MAY follow redirects, provided each one is to
///   an `https` URI whose certificate it validates the same way": a
///   redirect to any other scheme fails, and each hop's certificate is
///   checked like the first.
/// - "the fetch is subject to {{ssrf}}": every address the fetcher would
///   connect to, resolved or an IP-literal redirect target, goes through
///   the caller's address policy. The policy sees only addresses this
///   process resolves; a proxy configured on the builder resolves names
///   itself and bypasses it.
#[derive(Debug, Clone)]
pub struct HttpsFetcher {
    client: reqwest::Client,
}

impl HttpsFetcher {
    /// A fetcher over reqwest's default client that may connect to any
    /// address (no SSRF restriction; see [`with_policy`](Self::with_policy)).
    ///
    /// # Errors
    ///
    /// [`FetchError::Transport`] when reqwest cannot build the client.
    pub fn new() -> Result<Self, FetchError> {
        Self::with_policy(reqwest::Client::builder(), |_| true)
    }

    /// A fetcher over `builder` that connects only to addresses for which
    /// `allow` returns `true`.
    ///
    /// The draft's §Blob Fetch and SSRF: servers "MUST NOT connect to
    /// link-local, loopback, or private-network addresses in response to
    /// a peer-supplied URL". Which ranges that covers in a deployment is
    /// the caller's decision, so `allow` is the caller's.
    ///
    /// HTTPS-only, the redirect rule, [`FETCH_TIMEOUT`] and the address
    /// policy are set on `builder`, replacing any value it had for them.
    /// Root certificates and other TLS settings come from `builder`.
    ///
    /// # Errors
    ///
    /// [`FetchError::Transport`] when reqwest cannot build the client.
    pub fn with_policy(
        builder: reqwest::ClientBuilder,
        allow: impl Fn(IpAddr) -> bool + Send + Sync + 'static,
    ) -> Result<Self, FetchError> {
        let allow: AddressPolicy = Arc::new(allow);
        let redirect_allow = Arc::clone(&allow);
        let redirects = redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            if attempt.url().scheme() != "https" {
                return attempt.error("redirect to a non-https URI");
            }
            // An IP-literal host is connected to without name resolution,
            // so the resolver below never sees it.
            let literal = match attempt.url().host() {
                Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
                Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
                _ => None,
            };
            match literal {
                Some(ip) if !redirect_allow(ip) => {
                    attempt.error("redirect to an address the policy refuses")
                }
                _ => attempt.follow(),
            }
        });
        builder
            .https_only(true)
            .timeout(FETCH_TIMEOUT)
            .redirect(redirects)
            .dns_resolver(Arc::new(PolicyResolver { allow }))
            .build()
            .map(|client| Self { client })
            .map_err(|e| FetchError::Transport(Box::new(e)))
    }
}

impl Fetch for HttpsFetcher {
    async fn get(&self, url: &str) -> Result<Response, FetchError> {
        let transport = |e: reqwest::Error| FetchError::Transport(Box::new(e));
        let mut response = self.client.get(url).send().await.map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let headers = response.headers().clone();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response { headers, body })
    }
}

/// Resolves with the system resolver and drops the addresses the policy
/// refuses.
struct PolicyResolver {
    allow: AddressPolicy,
}

impl Resolve for PolicyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let allow = Arc::clone(&self.allow);
        let host = name.as_str().to_owned();
        Box::pin(async move {
            // The port is a placeholder: hyper sets the URI's port on
            // each resolved address.
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| allow(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("no address of {host} that the policy allows").into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// How long a response stays fresh after it was received: its RFC 9111
/// §4.2.1 freshness lifetime less its §4.2.3 current age.
///
/// - `no-store` or `no-cache` in `Cache-Control`: zero (the response may
///   not be stored, or must be revalidated before each reuse).
/// - Otherwise `max-age`, or else `Expires` minus `Date` (`received` when
///   there is no `Date`, RFC 9110 §6.6.1). An `Expires` that is not an
///   HTTP-date "MUST" be read as "a time in the past" (RFC 9111 §5.3),
///   and an invalid `max-age` makes the response stale (§4.2.1).
/// - Neither: zero.
///
/// The `Age` value, if any, is subtracted.
///
/// ponytail: no heuristic freshness (§4.2.2), `s-maxage` is ignored
/// (the fetching server caches for itself, not as a shared cache), and
/// the current age is the `Age` value alone, without the `Date`-based
/// apparent age or response delay of §4.2.3. A response without
/// explicit freshness is therefore never reused.
pub fn freshness_lifetime(headers: &HeaderMap, received: SystemTime) -> Duration {
    let directives: Vec<(String, Option<String>)> = headers
        .get_all(CACHE_CONTROL)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|d| {
            let d = d.trim();
            if d.is_empty() {
                return None;
            }
            let (name, value) = match d.split_once('=') {
                Some((n, v)) => (n.trim(), Some(v.trim().trim_matches('"').to_owned())),
                None => (d, None),
            };
            Some((name.to_ascii_lowercase(), value))
        })
        .collect();
    if directives
        .iter()
        .any(|(name, _)| name == "no-store" || name == "no-cache")
    {
        return Duration::ZERO;
    }
    let lifetime = match directives.iter().find(|(name, _)| name == "max-age") {
        Some((_, value)) => value
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(Duration::ZERO, Duration::from_secs),
        None => match headers.get(EXPIRES) {
            Some(expires) => {
                let date = headers
                    .get(DATE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| httpdate::parse_http_date(v).ok())
                    .unwrap_or(received);
                expires
                    .to_str()
                    .ok()
                    .and_then(|v| httpdate::parse_http_date(v).ok())
                    .and_then(|expires| expires.duration_since(date).ok())
                    .unwrap_or(Duration::ZERO)
            }
            None => Duration::ZERO,
        },
    };
    let age = headers
        .get(AGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(Duration::ZERO, Duration::from_secs);
    lifetime.saturating_sub(age)
}

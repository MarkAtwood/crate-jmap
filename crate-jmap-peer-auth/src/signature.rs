//! Request signing and verification
//! (draft-atwood-jmap-chat-federation-00 §Request Signing, RFC 9421).

use std::fmt::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use http::{HeaderMap, HeaderValue, Method, Uri};
use sfv::{
    key_ref, string_ref, BareItem, DictSerializer, Dictionary, InnerList, Integer, ListEntry,
    ListSerializer, Parameters, Parser, StringRef, Version,
};
use sha2::{Digest, Sha256};

use crate::{PeerDomain, PeerSigningKey};

/// The `tag` parameter value of a peer request signature.
pub const SIGNATURE_TAG: &str = "jmap-chat-peer";

/// Longest allowed signature lifetime: "`expires` MUST NOT be more than
/// 300 seconds after `created`". [`sign`] uses exactly this lifetime.
pub const MAX_LIFETIME_SECS: u64 = 300;

/// Allowed clock skew between signer and receiver (check 4).
pub const CLOCK_SKEW_SECS: u64 = 60;

/// How long after `expires` an accepted nonce must be remembered
/// (check 5).
pub const REPLAY_MARGIN_SECS: u64 = 60;

/// The signature label [`sign`] uses; the draft leaves it to the signer.
const LABEL: &str = "peer";

/// Nonce length: "at least 128 bits of output from a cryptographically
/// secure random number generator".
const NONCE_BYTES: usize = 16;

/// The parts of an HTTP request that peer authentication reads.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The request method.
    pub method: &'a Method,
    /// The full target URI in absolute form (RFC 9421 §2.2.2), for
    /// example `https://bob.example/jmap/api/`. A receiver behind a
    /// proxy must rebuild the URI the signer used.
    pub target_uri: &'a str,
    /// The request header fields.
    pub headers: &'a HeaderMap,
    /// The request content. Empty means the request has no content.
    pub content: &'a [u8],
}

/// Header field values to add to a signed request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHeaders {
    /// The `Content-Digest` value (RFC 9530, `sha-256`), present when the
    /// request has content.
    pub content_digest: Option<String>,
    /// The `Signature-Input` value.
    pub signature_input: String,
    /// The `Signature` value.
    pub signature: String,
}

/// Why [`sign`] could not sign a request.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SignError {
    /// The target URI is not an absolute URI with a scheme and authority.
    #[error("target URI is not absolute")]
    TargetUri,
    /// The signing time is before 1970 or too far in the future to
    /// express as a structured-field integer.
    #[error("signing time out of range")]
    Clock,
    /// The operating system random number generator failed.
    #[error("random number generator failed")]
    Random(#[source] getrandom::Error),
}

/// Why a received request is not authenticated.
///
/// "The receiving server rejects a request that fails any of these checks
/// with HTTP status 403 (Forbidden)", so every variant maps to 403. The
/// variant names the check for logs and for the caller's key-refetch
/// logic ([`Rejection::UnknownKey`]).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Rejection {
    /// Check 1: no signature tagged `jmap-chat-peer`.
    #[error("no jmap-chat-peer signature")]
    NoSignature,
    /// Check 1: more than one signature tagged `jmap-chat-peer`.
    #[error("more than one jmap-chat-peer signature")]
    MultipleSignatures,
    /// The `Signature-Input`, `Signature` or a signature parameter is
    /// malformed or missing.
    #[error("malformed signature: {0}")]
    Malformed(&'static str),
    /// Check 1: `alg` is missing or is not `ed25519`.
    #[error("alg is not ed25519")]
    WrongAlgorithm,
    /// Check 1: a required component is not covered.
    #[error("signature does not cover {0}")]
    MissingComponent(&'static str),
    /// A covered component cannot be derived: an unsupported derived
    /// component, a component identifier with parameters, or a field
    /// the request does not carry.
    #[error("cannot derive covered component {0}")]
    UnderivableComponent(String),
    /// Check 2: `keyid` is not `<peer-domain>#<kid>`.
    #[error("keyid is not <peer-domain>#<kid>")]
    InvalidKeyId,
    /// Check 2: the `kid` names no usable key of the peer domain. The
    /// caller refetches the keys (at most once per 60 seconds per peer
    /// domain) and verifies again before rejecting.
    #[error("kid names no usable key")]
    UnknownKey,
    /// Check 3: the signature does not verify under the key.
    #[error("signature does not verify")]
    BadSignature,
    /// Check 4: `created` is more than 60 seconds after the current time.
    #[error("created is in the future")]
    CreatedInFuture,
    /// Check 4: `expires` is more than 300 seconds after `created`.
    #[error("signature lifetime exceeds 300 seconds")]
    LifetimeTooLong,
    /// Check 4: the current time is more than 60 seconds after `expires`.
    #[error("signature expired")]
    Expired,
    /// Check 5: the (peer domain, nonce) pair was accepted before.
    /// [`SignedRequest::verify`] never returns it; the caller's replay
    /// store does.
    #[error("nonce replayed")]
    Replayed,
    /// Check 6: `Content-Digest` has no `sha-256` member matching the
    /// content.
    #[error("content-digest does not match the content")]
    ContentDigestMismatch,
    /// §DNSSEC Key Binding: the DNSSEC answer for the peer domain's keys
    /// is bogus.
    #[error("DNSSEC answer is bogus")]
    DnssecBogus,
    /// §Replay: "A receiving server whose nonce cache is lost, for
    /// example on restart, SHOULD reject requests whose `created`
    /// precedes the restart, since it can no longer detect their
    /// replay."
    #[error("created precedes the replay store's memory")]
    PrecedesRestart,
}

/// Signs a request to a peer (§Request Signing).
///
/// The signature is tagged `jmap-chat-peer`, uses `ed25519`, covers
/// `@method`, `@target-uri` and, when `content` is not empty,
/// `content-digest`; its `keyid` is `<peer_domain>#<kid>` where `kid` is
/// the [`PeerSigningKey`] thumbprint of `key`; `expires` is `created`
/// plus [`MAX_LIFETIME_SECS`]; the nonce is 128 bits from the operating
/// system CSPRNG, base64url-encoded.
///
/// Add the returned values as the `Content-Digest`, `Signature-Input`
/// and `Signature` header fields, and send `target_uri` and `content`
/// unchanged.
///
/// # Errors
///
/// [`SignError::TargetUri`] when `target_uri` is not absolute,
/// [`SignError::Clock`] when `created` is out of range, and
/// [`SignError::Random`] when the random number generator fails.
pub fn sign(
    key: &SigningKey,
    peer_domain: &PeerDomain,
    method: &Method,
    target_uri: &str,
    content: &[u8],
    created: SystemTime,
) -> Result<SignatureHeaders, SignError> {
    target_uri
        .parse::<Uri>()
        .ok()
        .filter(|uri| uri.scheme().is_some() && uri.authority().is_some())
        .ok_or(SignError::TargetUri)?;
    let created = created
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .ok_or(SignError::Clock)?;
    let lifetime = MAX_LIFETIME_SECS as i64;
    let (created_param, expires_param) = created
        .checked_add(lifetime)
        .and_then(|expires| {
            Some((
                Integer::try_from(created).ok()?,
                Integer::try_from(expires).ok()?,
            ))
        })
        .ok_or(SignError::Clock)?;
    let mut nonce = [0; NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(SignError::Random)?;
    let nonce = URL_SAFE_NO_PAD.encode(nonce);
    let keyid = format!(
        "{peer_domain}#{}",
        PeerSigningKey::from(key.verifying_key()).kid()
    );

    let mut covered = vec!["@method".to_owned(), "@target-uri".to_owned()];
    let mut headers = HeaderMap::new();
    let content_digest = (!content.is_empty()).then(|| sha256_content_digest(content));
    if let Some(digest) = &content_digest {
        covered.push("content-digest".to_owned());
        headers.insert(
            "content-digest",
            HeaderValue::from_str(digest).expect("a serialized dictionary is a valid field value"),
        );
    }

    let mut params = ListSerializer::new();
    {
        let mut list = params.inner_list();
        for name in &covered {
            let _ = list.bare_item(StringRef::from_str(name).expect("component names are ASCII"));
        }
        let _ = list
            .finish()
            .parameter(key_ref("created"), created_param)
            .parameter(key_ref("expires"), expires_param)
            .parameter(
                key_ref("nonce"),
                StringRef::from_str(&nonce).expect("base64url is ASCII"),
            )
            .parameter(
                key_ref("keyid"),
                StringRef::from_str(&keyid).expect("A-labels and base64url are ASCII"),
            )
            .parameter(key_ref("alg"), string_ref("ed25519"))
            .parameter(key_ref("tag"), string_ref(SIGNATURE_TAG));
    }
    let params = params.finish().expect("the list has one member");

    let request = Request {
        method,
        target_uri,
        headers: &headers,
        content,
    };
    let base = signature_base(&covered, &params, &request)
        .expect("@method, @target-uri and content-digest are derivable");
    let signature = key.sign(base.as_bytes()).to_bytes();
    let mut signature_field = DictSerializer::new();
    let _ = signature_field.bare_item(key_ref(LABEL), &signature[..]);

    Ok(SignatureHeaders {
        content_digest,
        signature_input: format!("{LABEL}={params}"),
        signature: signature_field
            .finish()
            .expect("the dictionary has one member"),
    })
}

/// A received request whose `jmap-chat-peer` signature has been parsed
/// but not yet verified.
///
/// [`SignedRequest::parse`] runs check 1 and parses the `keyid` (check 2),
/// so the caller learns [`peer_domain`](Self::peer_domain) and
/// [`kid`](Self::kid) and can find that domain's usable keys before
/// calling [`verify`](Self::verify).
#[derive(Debug, Clone)]
pub struct SignedRequest<'a> {
    request: Request<'a>,
    covered: Vec<String>,
    params: String,
    created: i64,
    expires: i64,
    nonce: String,
    peer_domain: PeerDomain,
    kid: String,
    signature: Vec<u8>,
}

/// A verified request: its authenticated peer domain, the key that
/// verified it, and the replay-store entry to record.
///
/// `#[non_exhaustive]`, so new fields stay semver-additive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Accepted {
    /// The request's authenticated peer domain.
    pub peer_domain: PeerDomain,
    /// The entry of the `keys` passed to [`SignedRequest::verify`] whose
    /// `kid` the signature's `keyid` names and that verified the signature
    /// (checks 2 and 3). draft-atwood-jmap-chat-did-00 §Federation
    /// Authentication step 2 compares this key with the DID Document's
    /// keys.
    pub key: PeerSigningKey,
    /// The signature's nonce. Record (`peer_domain`, `nonce`) as accepted.
    pub nonce: String,
    /// Keep the (`peer_domain`, `nonce`) record at least until this time:
    /// 60 seconds after the signature's `expires` (check 5).
    pub forget_after: SystemTime,
    /// The signature's `created`, for the §Replay restart rule
    /// ([`Rejection::PrecedesRestart`]).
    pub created: SystemTime,
}

impl<'a> SignedRequest<'a> {
    /// Finds and parses the request's `jmap-chat-peer` signature.
    ///
    /// Check 1: "The request carries exactly one signature tagged
    /// `"jmap-chat-peer"`, its `alg` is `ed25519`, and it covers the
    /// components listed above" (`@method`, `@target-uri`, and
    /// `content-digest` when the request has content). The `keyid` must
    /// parse as `<peer-domain>#<kid>` (check 2), and `created`, `expires`
    /// and `nonce` must be present.
    ///
    /// # Errors
    ///
    /// A [`Rejection`] for the first check that fails.
    pub fn parse(request: Request<'a>) -> Result<Self, Rejection> {
        let inputs = dictionary(request.headers, "signature-input")?;
        let mut tagged = inputs
            .iter()
            .filter(|(_, entry)| string_param(entry_params(entry), "tag") == Some(SIGNATURE_TAG));
        let (label, entry) = tagged.next().ok_or(Rejection::NoSignature)?;
        if tagged.next().is_some() {
            return Err(Rejection::MultipleSignatures);
        }
        let ListEntry::InnerList(list) = entry else {
            return Err(Rejection::Malformed(
                "Signature-Input member is not an inner list",
            ));
        };
        if string_param(&list.params, "alg") != Some("ed25519") {
            return Err(Rejection::WrongAlgorithm);
        }
        let covered = covered_components(list)?;
        let covers = |name: &str| covered.iter().any(|c| c == name);
        for required in ["@method", "@target-uri"] {
            if !covers(required) {
                return Err(Rejection::MissingComponent(required));
            }
        }
        if !request.content.is_empty() && !covers("content-digest") {
            return Err(Rejection::MissingComponent("content-digest"));
        }

        let keyid = string_param(&list.params, "keyid").ok_or(Rejection::Malformed("no keyid"))?;
        let (domain, kid) = keyid.split_once('#').ok_or(Rejection::InvalidKeyId)?;
        let peer_domain = PeerDomain::parse(domain).map_err(|_| Rejection::InvalidKeyId)?;
        if kid.is_empty() {
            return Err(Rejection::InvalidKeyId);
        }
        let created = integer_param(&list.params, "created")
            .ok_or(Rejection::Malformed("no integer created"))?;
        let expires = integer_param(&list.params, "expires")
            .ok_or(Rejection::Malformed("no integer expires"))?;
        let nonce = string_param(&list.params, "nonce")
            .ok_or(Rejection::Malformed("no nonce"))?
            .to_owned();

        let signature = match dictionary(request.headers, "signature")?.get(label) {
            Some(ListEntry::Item(item)) => item.bare_item.as_byte_sequence().map(<[u8]>::to_vec),
            _ => None,
        }
        .ok_or(Rejection::Malformed(
            "no Signature byte sequence for the label",
        ))?;

        Ok(Self {
            request,
            covered,
            params: serialize_inner_list(list),
            created,
            expires,
            nonce,
            peer_domain,
            kid: kid.to_owned(),
            signature,
        })
    }

    /// The peer domain named in the `keyid`, not yet authenticated.
    pub fn peer_domain(&self) -> &PeerDomain {
        &self.peer_domain
    }

    /// The `kid` named in the `keyid`.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Verifies the signature (checks 2, 3, 4 and 6).
    ///
    /// `keys` are the usable keys of [`peer_domain`](Self::peer_domain),
    /// after the DNSSEC precedence rule ([`crate::usable_keys`]). `now` is
    /// the receiver's current time.
    ///
    /// The checks run in this order: 4 (clock window), 6 (content
    /// digest), 2 (usable key), 3 (signature).
    ///
    /// Check 5 (replay) is the caller's: on success, record
    /// ([`Accepted::peer_domain`], [`Accepted::nonce`]) until
    /// [`Accepted::forget_after`] in one atomic test-and-insert, and
    /// reject with [`Rejection::Replayed`] when the pair is already
    /// recorded. A lookup followed by a separate record would let two
    /// concurrent requests with the same nonce both pass.
    /// `jmap_peer_auth_io::authenticate` does this through its
    /// `ReplayStore::insert`.
    ///
    /// # Errors
    ///
    /// A [`Rejection`] for the first check that fails.
    pub fn verify(&self, keys: &[PeerSigningKey], now: SystemTime) -> Result<Accepted, Rejection> {
        let now = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        if self.created > now.saturating_add(CLOCK_SKEW_SECS as i64) {
            return Err(Rejection::CreatedInFuture);
        }
        if self.expires > self.created.saturating_add(MAX_LIFETIME_SECS as i64) {
            return Err(Rejection::LifetimeTooLong);
        }
        let forget_after = self.expires.saturating_add(REPLAY_MARGIN_SECS as i64);
        if now > forget_after {
            return Err(Rejection::Expired);
        }
        if self.covered.iter().any(|c| c == "content-digest") {
            check_content_digest(self.request.headers, self.request.content)?;
        }
        let key = keys
            .iter()
            .find(|k| k.kid() == self.kid)
            .ok_or(Rejection::UnknownKey)?;
        let base = signature_base(&self.covered, &self.params, &self.request)?;
        verify_ed25519(key.verifying_key(), &base, &self.signature)?;
        Ok(Accepted {
            peer_domain: self.peer_domain.clone(),
            key: key.clone(),
            nonce: self.nonce.clone(),
            forget_after: UNIX_EPOCH
                + Duration::from_secs(u64::try_from(forget_after).unwrap_or(0)),
            created: UNIX_EPOCH + Duration::from_secs(u64::try_from(self.created).unwrap_or(0)),
        })
    }
}

/// RFC 9421 §3.3.6: Ed25519 (RFC 8032) over the signature base octets.
fn verify_ed25519(key: &VerifyingKey, base: &str, signature: &[u8]) -> Result<(), Rejection> {
    let signature = Signature::from_slice(signature).map_err(|_| Rejection::BadSignature)?;
    key.verify_strict(base.as_bytes(), &signature)
        .map_err(|_| Rejection::BadSignature)
}

/// The RFC 9530 `Content-Digest` value with one `sha-256` member.
fn sha256_content_digest(content: &[u8]) -> String {
    let mut field = DictSerializer::new();
    let _ = field.bare_item(key_ref("sha-256"), &Sha256::digest(content)[..]);
    field.finish().expect("the dictionary has one member")
}

/// Check 6: "the `content-digest` field contains a `sha-256` digest and
/// that digest matches the content".
fn check_content_digest(headers: &HeaderMap, content: &[u8]) -> Result<(), Rejection> {
    let digest = match dictionary(headers, "content-digest")?.get(key_ref("sha-256")) {
        Some(ListEntry::Item(item)) => item.bare_item.as_byte_sequence().map(<[u8]>::to_vec),
        _ => None,
    };
    if digest.as_deref() == Some(&Sha256::digest(content)[..]) {
        Ok(())
    } else {
        Err(Rejection::ContentDigestMismatch)
    }
}

/// Parses a Dictionary structured field (RFC 8941 §3.2); an absent field
/// is an empty dictionary.
fn dictionary(headers: &HeaderMap, name: &str) -> Result<Dictionary, Rejection> {
    let value = field_value(headers, name).ok_or(Rejection::Malformed("field is not ASCII"))?;
    match value {
        None => Ok(Dictionary::new()),
        Some(value) => Parser::new(&value)
            .with_version(Version::Rfc8941)
            .parse()
            .map_err(|_| Rejection::Malformed("field is not a structured dictionary")),
    }
}

/// RFC 9421 §2.1: the field's values with surrounding whitespace removed,
/// joined by `", "`. `Ok(None)` when the field is absent; `None` when a
/// value is not visible ASCII.
fn field_value(headers: &HeaderMap, name: &str) -> Option<Option<String>> {
    let values = headers
        .get_all(name)
        .iter()
        .map(|v| v.to_str().ok().map(|s| s.trim_matches([' ', '\t'])))
        .collect::<Option<Vec<_>>>()?;
    Some((!values.is_empty()).then(|| values.join(", ")))
}

fn entry_params(entry: &ListEntry) -> &Parameters {
    match entry {
        ListEntry::Item(item) => &item.params,
        ListEntry::InnerList(list) => &list.params,
    }
}

fn string_param<'p>(params: &'p Parameters, name: &str) -> Option<&'p str> {
    params
        .get(key_ref(name))?
        .as_string()
        .map(StringRef::as_str)
}

fn integer_param(params: &Parameters, name: &str) -> Option<i64> {
    params.get(key_ref(name))?.as_integer().map(i64::from)
}

/// The covered component names of a `Signature-Input` inner list.
///
/// Component identifiers with parameters (RFC 9421 §2.1.1 to §2.1.5) are
/// not supported. RFC 9421 §2.5 forbids repeated identifiers.
fn covered_components(list: &InnerList) -> Result<Vec<String>, Rejection> {
    let mut covered: Vec<String> = Vec::with_capacity(list.items.len());
    for item in &list.items {
        let BareItem::String(name) = &item.bare_item else {
            return Err(Rejection::Malformed("component identifier is not a string"));
        };
        let name = name.as_str().to_owned();
        if !item.params.is_empty() {
            return Err(Rejection::UnderivableComponent(name));
        }
        if covered.contains(&name) {
            return Err(Rejection::Malformed("component identifier repeated"));
        }
        covered.push(name);
    }
    Ok(covered)
}

/// The `@signature-params` value: the inner list serialized as a
/// structured field (RFC 9421 §2.3).
fn serialize_inner_list(list: &InnerList) -> String {
    let mut field = ListSerializer::new();
    field.members(std::iter::once(&ListEntry::InnerList(list.clone())));
    field.finish().expect("the list has one member")
}

/// Builds the signature base (RFC 9421 §2.5).
fn signature_base(
    covered: &[String],
    params: &str,
    request: &Request<'_>,
) -> Result<String, Rejection> {
    let mut base = String::new();
    for name in covered {
        let value = component_value(name, request)
            .ok_or_else(|| Rejection::UnderivableComponent(name.clone()))?;
        writeln!(base, "\"{name}\": {value}").expect("writing to a String cannot fail");
    }
    write!(base, "\"@signature-params\": {params}").expect("writing to a String cannot fail");
    Ok(base)
}

// ponytail: derives only the request components computable from the
// method, the target URI and plain header fields (RFC 9421 §2.1, §2.2.1
// to §2.2.7 without @request-target and @query-param). A covered
// @request-target, @query-param, @status or parameterized identifier is
// rejected; add them if a peer implementation covers them.
fn component_value(name: &str, request: &Request<'_>) -> Option<String> {
    let uri = || request.target_uri.parse::<Uri>().ok();
    match name {
        "@method" => Some(request.method.as_str().to_owned()),
        "@target-uri" => Some(request.target_uri.to_owned()),
        "@scheme" => uri()?.scheme_str().map(str::to_ascii_lowercase),
        "@authority" => {
            let uri = uri()?;
            let authority = uri.authority()?.as_str().to_ascii_lowercase();
            let default_port = match uri.scheme_str()? {
                s if s.eq_ignore_ascii_case("https") => ":443",
                s if s.eq_ignore_ascii_case("http") => ":80",
                _ => "",
            };
            match authority.strip_suffix(default_port) {
                Some(host) if !default_port.is_empty() => Some(host.to_owned()),
                _ => Some(authority),
            }
        }
        "@path" => {
            let uri = uri()?;
            Some(
                if uri.path().is_empty() {
                    "/"
                } else {
                    uri.path()
                }
                .to_owned(),
            )
        }
        "@query" => Some(format!("?{}", uri()?.query().unwrap_or(""))),
        _ if name.starts_with('@') || name.bytes().any(|b| b.is_ascii_uppercase()) => None,
        _ => field_value(request.headers, name)?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9421 Appendix B.2.6 "Signing a Request Using ed25519": the
    /// test-request (Appendix B.2), the sig-b26 Signature-Input and
    /// Signature fields, and the signature base, all copied from the RFC
    /// with its RFC 8792 line wrapping undone.
    #[test]
    fn rfc9421_b26_signature_base_and_signature() {
        let signature_input = r#"sig-b26=("date" "@method" "@path" "@authority" "content-type" "content-length");created=1618884473;keyid="test-key-ed25519""#;
        let signature = "sig-b26=:wqcAqbmYJ2ji2glfAMaRy4gruYYnx2nEFN2HN6jrnDnQCK1u02Gb04v9EDgwUPiu4A0w6vuQv5lIp5WPpBKRCw==:";
        let expected_base = concat!(
            "\"date\": Tue, 20 Apr 2021 02:07:55 GMT\n",
            "\"@method\": POST\n",
            "\"@path\": /foo\n",
            "\"@authority\": example.com\n",
            "\"content-type\": application/json\n",
            "\"content-length\": 18\n",
            "\"@signature-params\": (\"date\" \"@method\" \"@path\" \"@authority\" ",
            "\"content-type\" \"content-length\");created=1618884473",
            ";keyid=\"test-key-ed25519\"",
        );
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("example.com"));
        headers.insert(
            "date",
            HeaderValue::from_static("Tue, 20 Apr 2021 02:07:55 GMT"),
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert(
            "content-digest",
            HeaderValue::from_static(
                "sha-512=:WZDPaVn/7XgHaAy8pmojAkGWoRx2UFChF41A2svX+TaPm+AbwAgBWnrIiYllu7BNNyealdVLvRwEmTHWXvJwew==:",
            ),
        );
        headers.insert("content-length", HeaderValue::from_static("18"));
        headers.insert("signature-input", HeaderValue::from_static(signature_input));
        headers.insert("signature", HeaderValue::from_static(signature));
        let request = Request {
            method: &Method::POST,
            target_uri: "https://example.com/foo?param=Value&Pet=dog",
            headers: &headers,
            content: br#"{"hello": "world"}"#,
        };

        let inputs = dictionary(&headers, "signature-input").unwrap();
        let Some(ListEntry::InnerList(list)) = inputs.get(key_ref("sig-b26")) else {
            panic!("sig-b26 is an inner list");
        };
        let base = signature_base(
            &covered_components(list).unwrap(),
            &serialize_inner_list(list),
            &request,
        )
        .unwrap();
        assert_eq!(base, expected_base);

        // RFC 9421 Appendix B.1.4 test-key-ed25519, JWK "x" member.
        let x: [u8; 32] = URL_SAFE_NO_PAD
            .decode("JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs")
            .unwrap()
            .try_into()
            .unwrap();
        let key = VerifyingKey::from_bytes(&x).unwrap();
        let Some(ListEntry::Item(item)) = dictionary(&headers, "signature")
            .unwrap()
            .get(key_ref("sig-b26"))
            .cloned()
        else {
            panic!("sig-b26 signature is an item");
        };
        let signature = item.bare_item.as_byte_sequence().unwrap().to_vec();
        assert_eq!(verify_ed25519(&key, &base, &signature), Ok(()));
        let mut tampered = base.clone();
        tampered.replace_range(
            base.find("/foo").unwrap()..base.find("/foo").unwrap() + 4,
            "/bar",
        );
        assert_eq!(
            verify_ed25519(&key, &tampered, &signature),
            Err(Rejection::BadSignature)
        );
    }

    /// RFC 9530 Appendix D: the sha-256 digest of `{"hello": "world"}`.
    #[test]
    fn rfc9530_appendix_d_sha256() {
        assert_eq!(
            sha256_content_digest(br#"{"hello": "world"}"#),
            "sha-256=:X48E9qOokqqrvdts8nOJRJN3OWDUoyWxBf7kbu9DBPE=:"
        );
    }
}

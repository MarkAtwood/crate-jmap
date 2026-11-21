//! AWS Signature Version 4, header form.
//!
//! Oracle: "Create a signed AWS API request",
//! <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html>
//! (§Create a canonical request, §Create a string to sign, §Derive a
//! signing key, §Calculate the signature, §Add the signature to the
//! request), and "Elements of an AWS API request signature",
//! <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-signing-elements.html>.

use std::fmt::Write as _;
use std::time::SystemTime;

use ring::{digest, hmac};
use url::Url;

use crate::Credentials;
use crate::time::Utc;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// What to sign.
pub(crate) struct Request<'a> {
    pub(crate) method: &'a str,
    pub(crate) url: &'a Url,
    /// Headers to send and sign besides `host`, `x-amz-date` and
    /// `x-amz-security-token`, e.g. `content-type`.
    pub(crate) headers: &'a [(&'a str, &'a str)],
    pub(crate) body: &'a [u8],
}

/// The scope a signature is valid for.
pub(crate) struct Scope<'a> {
    pub(crate) region: &'a str,
    pub(crate) service: &'a str,
    pub(crate) time: SystemTime,
}

/// The headers to add to `request`: `x-amz-date`, `x-amz-security-token`
/// for temporary credentials, and `authorization`.
pub(crate) fn sign(
    credentials: &Credentials,
    request: &Request<'_>,
    scope: &Scope<'_>,
) -> Vec<(&'static str, String)> {
    let now = Utc::from_system_time(scope.time);
    let amz_date = now.amz_date();
    let date = now.scope_date();

    // "You must include the host header ... and any x-amz-* headers in
    // the signature."
    let mut headers: Vec<(String, String)> = vec![
        ("host".to_owned(), host(request.url)),
        ("x-amz-date".to_owned(), amz_date.clone()),
    ];
    if let Some(token) = &credentials.session_token {
        headers.push(("x-amz-security-token".to_owned(), token.clone()));
    }
    headers.extend(
        request
            .headers
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), (*v).to_owned())),
    );
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", trim_value(v)))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method,
        canonical_uri(request.url),
        canonical_query(request.url),
        canonical_headers,
        signed_headers,
        hex_sha256(request.body),
    );
    let credential_scope = format!("{date}/{}/{}/aws4_request", scope.region, scope.service);
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{credential_scope}\n{}",
        hex_sha256(canonical_request.as_bytes())
    );
    let key = signing_key(
        &credentials.secret_access_key,
        &date,
        scope.region,
        scope.service,
    );
    let signature = hex(hmac::sign(&key, string_to_sign.as_bytes()).as_ref());

    let mut out = vec![("x-amz-date", amz_date)];
    if let Some(token) = &credentials.session_token {
        out.push(("x-amz-security-token", token.clone()));
    }
    out.push((
        "authorization",
        format!(
            "{ALGORITHM} Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            credentials.access_key_id
        ),
    ));
    out
}

/// §Derive a signing key: `HMAC(HMAC(HMAC(HMAC("AWS4" + secret, date),
/// region), service), "aws4_request")`.
fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> hmac::Key {
    let step = |key: &[u8], data: &str| {
        hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data.as_bytes())
    };
    let k_date = step(format!("AWS4{secret}").as_bytes(), date);
    let k_region = step(k_date.as_ref(), region);
    let k_service = step(k_region.as_ref(), service);
    let k_signing = step(k_service.as_ref(), "aws4_request");
    hmac::Key::new(hmac::HMAC_SHA256, k_signing.as_ref())
}

/// The `host` header value: the host, with the port when it is not the
/// scheme's default (what reqwest sends).
fn host(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// "The URI-encoded version of the absolute path component". `Url` keeps
/// the path percent-encoded once; services other than S3 want each segment
/// encoded again ("Each path segment must be URI-encoded twice", signing
/// elements §Canonical URI), which `uri_encode` of the encoded form does.
fn canonical_uri(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() {
        return "/".to_owned();
    }
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// Names and values URI-encoded one by one, sorted by name (then value),
/// joined with `&`; empty without a query.
fn canonical_query(url: &Url) -> String {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (uri_encode(&k), uri_encode(&v)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// §`UriEncode`: every byte but `A-Z a-z 0-9 - . _ ~` as `%XX` with upper-case
/// hex; a space is `%20`.
fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// "trim any leading or trailing spaces" and "convert sequential spaces to
/// a single space".
fn trim_value(v: &str) -> String {
    v.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn hex_sha256(data: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, data).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// The example credentials of the AWS signing test suite.
    fn example() -> Credentials {
        Credentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY")
    }

    /// 20150830T123600Z, the time of every test-suite vector.
    fn example_time() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_440_938_160)
    }

    fn authorization(headers: &[(&str, String)]) -> String {
        headers
            .iter()
            .find(|(k, _)| *k == "authorization")
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    /// Oracle: the AWS SigV4 test suite vector `get-vanilla`
    /// (aws-c-auth tests/aws-signing-test-suite/v4/get-vanilla,
    /// <https://github.com/awslabs/aws-c-auth/tree/main/tests/aws-signing-test-suite/v4/get-vanilla>),
    /// `GET /` to `example.amazonaws.com`, service `service`, region
    /// `us-east-1`. Cross-checked with Python's `hmac`/`hashlib`.
    #[test]
    fn get_vanilla() {
        let url = Url::parse("https://example.amazonaws.com/").unwrap();
        let headers = sign(
            &example(),
            &Request {
                method: "GET",
                url: &url,
                headers: &[],
                body: b"",
            },
            &Scope {
                region: "us-east-1",
                service: "service",
                time: example_time(),
            },
        );
        assert_eq!(
            authorization(&headers),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert!(headers.contains(&("x-amz-date", "20150830T123600Z".to_owned())));
    }

    /// Oracle: the signing-key example of the SigV4 documentation
    /// (secret `wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY`, 20150830,
    /// us-east-1, iam), <https://docs.aws.amazon.com/general/latest/gr/signature-v4-examples.html>.
    /// Cross-checked with Python's `hmac`.
    #[test]
    fn signing_key_example() {
        let key = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830",
            "us-east-1",
            "iam",
        );
        // A ring key is opaque, so compare what it signs with what the
        // documented key bytes
        // c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9
        // sign.
        let documented = hmac::Key::new(
            hmac::HMAC_SHA256,
            &[
                0xc4, 0xaf, 0xb1, 0xcc, 0x57, 0x71, 0xd8, 0x71, 0x76, 0x3a, 0x39, 0x3e, 0x44, 0xb7,
                0x03, 0x57, 0x1b, 0x55, 0xcc, 0x28, 0x42, 0x4d, 0x1a, 0x5e, 0x86, 0xda, 0x6e, 0xd3,
                0xc1, 0x54, 0xa4, 0xb9,
            ],
        );
        assert_eq!(
            hmac::sign(&key, b"probe").as_ref(),
            hmac::sign(&documented, b"probe").as_ref()
        );
    }

    /// An IVS-shaped request: POST JSON with `content-type` signed and a
    /// session token. Oracle: the signature computed by Python 3
    /// `hmac`/`hashlib` from the canonical request below, written out by
    /// hand per §Create a canonical request.
    #[test]
    fn post_json_with_session_token() {
        let url = Url::parse("https://ivs.us-west-2.amazonaws.com/GetStream").unwrap();
        let body = br#"{"channelArn":"arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh"}"#;
        let creds = example().with_session_token("AQoDYXdzEPT//////////wEXAMPLE");
        let headers = sign(
            &creds,
            &Request {
                method: "POST",
                url: &url,
                headers: &[("Content-Type", "application/json")],
                body,
            },
            &Scope {
                region: "us-west-2",
                service: "ivs",
                time: example_time(),
            },
        );
        assert_eq!(
            authorization(&headers),
            format!(
                "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-west-2/ivs/aws4_request, \
                 SignedHeaders=content-type;host;x-amz-date;x-amz-security-token, \
                 Signature={POST_JSON_SIGNATURE}"
            )
        );
        assert!(headers.contains(&(
            "x-amz-security-token",
            "AQoDYXdzEPT//////////wEXAMPLE".to_owned()
        )));
    }

    /// See [`post_json_with_session_token`].
    const POST_JSON_SIGNATURE: &str =
        "a79231400b1d1ff1942d7973146d8a4ee41f2b3ca825bb2d7ecb62a5669d502e";

    /// §UriEncode, with the documentation's examples: space is `%20`,
    /// hex is upper case, `/` is encoded in a value.
    #[test]
    fn uri_encode_rules() {
        assert_eq!(uri_encode("a b"), "a%20b");
        assert_eq!(uri_encode("A-Z_a.z~0"), "A-Z_a.z~0");
        assert_eq!(uri_encode("a/b+c="), "a%2Fb%2Bc%3D");
        assert_eq!(uri_encode("\u{e9}"), "%C3%A9");
    }

    /// §CanonicalQueryString: sorted by encoded name; an empty value
    /// stays as `name=`.
    #[test]
    fn canonical_query_sorts() {
        let url =
            Url::parse("https://h.example/?prefix=somePrefix&marker=someMarker&max-keys=20&acl")
                .unwrap();
        assert_eq!(
            canonical_query(&url),
            "acl=&marker=someMarker&max-keys=20&prefix=somePrefix"
        );
    }
}

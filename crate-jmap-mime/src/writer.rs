//! Generic RFC 5322 / MIME message writer.
//!
//! The inverse of `mime_tree::parse`: [`write_message`] turns header fields
//! and a tree of MIME entities into message bytes. No JMAP type appears in
//! this module's API, so it can move into `mime-tree` unchanged; the JMAP
//! `Email` mapping is [`crate::compose_email`].
//!
//! Output rules:
//!
//! - Lines end in CRLF (RFC 5322 §2.1). Header fields fold at whitespace
//!   before a line would pass 78 characters (§2.1.1); one token longer than
//!   that stays on its own line.
//! - Non-ASCII `unstructured` text and display names become RFC 2047
//!   `B`-encoded words of UTF-8, each at most 75 characters (RFC 2047 §2).
//! - Non-ASCII parameter values use RFC 2231 `attr*=utf-8''…` encoding.
//! - Text bodies are quoted-printable (RFC 2045 §6.7), binary bodies
//!   base64 (§6.8), and unencoded bodies 7bit or 8bit (§6.2).
//! - Multipart boundaries are `=_0`, `=_1`, …: the first one that occurs
//!   nowhere in the encoded children (RFC 2046 §5.1.1 "the boundary
//!   delimiter MUST NOT appear inside any of the encapsulated parts").

use std::fmt;

/// Lines fold before passing this length (RFC 5322 §2.1.1 "SHOULD be no
/// more than 78 characters").
const MAX_LINE: usize = 78;

/// Raw bytes per RFC 2047 encoded word: base64 of 45 bytes is 60
/// characters, and `=?UTF-8?B?` plus `?=` adds 12, inside the 75-character
/// limit of RFC 2047 §2.
const ENCODED_WORD_BYTES: usize = 45;

/// One RFC 5322 §3.4 `mailbox`: an optional display name and an addr-spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    /// Display name, written as atoms, a quoted-string or RFC 2047 encoded
    /// words as its characters require. `None` or empty writes the bare
    /// addr-spec.
    pub name: Option<String>,
    /// The addr-spec, written as given.
    pub address: String,
}

/// One entry of an RFC 5322 §3.4 `address-list` that may hold groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The group's display name. `None` writes the mailboxes ungrouped.
    pub name: Option<String>,
    /// The group's mailboxes.
    pub mailboxes: Vec<Mailbox>,
}

/// The value of a header field, by the RFC 5322 syntax it is written in.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderValue {
    /// `unstructured` text (RFC 5322 §3.2.5), RFC 2047 encoded unless it
    /// is printable ASCII.
    Text(String),
    /// A `mailbox-list` or `address-list` without groups (RFC 5322 §3.4).
    Mailboxes(Vec<Mailbox>),
    /// An `address-list` with groups (RFC 5322 §3.4): a named [`Group`]
    /// is written `display-name: mailbox, ...;`.
    Groups(Vec<Group>),
    /// A list of `msg-id`s (RFC 5322 §3.6.4), each given without the angle
    /// brackets.
    MessageIds(Vec<String>),
    /// Written verbatim; the caller is responsible for its syntax.
    Raw(String),
}

/// One header field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Field name, e.g. `Subject`.
    pub name: String,
    /// Field value.
    pub value: HeaderValue,
}

impl Header {
    /// Build a header field from its name and value.
    pub fn new(name: impl Into<String>, value: HeaderValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// One MIME entity (RFC 2045 §2.4): its content headers and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// Media type as `type/subtype` (RFC 2045 §5.1).
    pub content_type: String,
    /// Content-Type parameters. The writer adds `boundary` to a multipart.
    pub params: Vec<(String, String)>,
    /// Content-Disposition type and parameters (RFC 2183), if any.
    pub disposition: Option<(String, Vec<(String, String)>)>,
    /// Further header fields of the entity, such as `Content-ID`.
    pub headers: Vec<Header>,
    /// The body.
    pub body: Body,
}

/// The body of a [`Part`] and how it is encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Text written quoted-printable as UTF-8. Each LF or CRLF in the text
    /// becomes a CRLF line break.
    Text(String),
    /// Bytes written base64.
    Binary(Vec<u8>),
    /// Bytes written as is and labelled `7bit` when every byte is ASCII,
    /// else `8bit`, as `message/*` bodies must be (RFC 2046 §5.2.1).
    Unencoded(Vec<u8>),
    /// The child entities of a `multipart/*` part (RFC 2046 §5.1).
    Multipart(Vec<Part>),
}

/// A value the writer cannot put in a header field without changing the
/// message's structure.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// A header field name is empty or has a character outside RFC 5322
    /// §2.2 `ftext` (printable ASCII except `:`).
    InvalidFieldName(String),
    /// A value the writer emits without encoding contains CR or LF, which
    /// would end the header field early. Holds the field name.
    LineBreak(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFieldName(name) => write!(f, "invalid header field name {name:?}"),
            Self::LineBreak(name) => {
                write!(f, "value of header field {name} contains a line break")
            }
        }
    }
}

impl std::error::Error for WriteError {}

/// Write a message: `headers`, then `MIME-Version: 1.0`, then the content
/// headers and body of `body`.
///
/// # Errors
///
/// [`WriteError`] when a field name is invalid or a value written without
/// encoding (a [`HeaderValue::Raw`] or [`HeaderValue::MessageIds`] value, an
/// addr-spec, a media or disposition type) contains CR or LF.
pub fn write_message(headers: &[Header], body: &Part) -> Result<Vec<u8>, WriteError> {
    let mut out = Vec::new();
    for header in headers {
        write_header(&mut out, header)?;
    }
    write_header(
        &mut out,
        &Header::new("MIME-Version", HeaderValue::Raw("1.0".into())),
    )?;
    write_part(&mut out, body)?;
    Ok(out)
}

fn write_part(out: &mut Vec<u8>, part: &Part) -> Result<(), WriteError> {
    let mut params = part.params.clone();
    let (encoding, body) = match &part.body {
        Body::Text(text) => (Some("quoted-printable"), quoted_printable(text)),
        Body::Binary(bytes) => (Some("base64"), base64(bytes)),
        Body::Unencoded(bytes) => {
            // ponytail: never labels `binary`; a body with NUL or lines past
            // 998 bytes is mislabelled 8bit. Upgrade: scan and pick binary.
            let label = if bytes.is_ascii() { "7bit" } else { "8bit" };
            (Some(label), bytes.clone())
        }
        Body::Multipart(children) => {
            let encoded = children
                .iter()
                .map(|child| {
                    let mut buf = Vec::new();
                    write_part(&mut buf, child).map(|()| buf)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let boundary = (0..)
                .map(|n| format!("=_{n}"))
                .find(|b| !encoded.iter().any(|c| contains(c, b.as_bytes())))
                .expect("an unbounded counter yields a boundary absent from finite input");
            let mut buf = Vec::new();
            for child in &encoded {
                buf.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                buf.extend_from_slice(child);
                buf.extend_from_slice(b"\r\n");
            }
            buf.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            params.push(("boundary".into(), boundary));
            (None, buf)
        }
    };

    let content_type = with_params("Content-Type", &part.content_type, &params)?;
    write_header(
        out,
        &Header::new("Content-Type", HeaderValue::Raw(content_type)),
    )?;
    if let Some((kind, dparams)) = &part.disposition {
        let value = with_params("Content-Disposition", kind, dparams)?;
        write_header(
            out,
            &Header::new("Content-Disposition", HeaderValue::Raw(value)),
        )?;
    }
    for header in &part.headers {
        write_header(out, header)?;
    }
    if let Some(encoding) = encoding {
        write_header(
            out,
            &Header::new(
                "Content-Transfer-Encoding",
                HeaderValue::Raw(encoding.into()),
            ),
        )?;
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&body);
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Render `value; p1=v1; p2=v2` with RFC 2045 §5.1 / RFC 2231 parameter
/// syntax. Tokens are separated by `\u{0}` so [`write_header`] folds only
/// between parameters.
fn with_params(
    field: &str,
    value: &str,
    params: &[(String, String)],
) -> Result<String, WriteError> {
    no_line_break(field, value)?;
    let mut tokens = vec![value.to_owned()];
    for (name, val) in params {
        no_line_break(field, name)?;
        tokens.push(param(name, val));
    }
    Ok(tokens.join(";\u{0}"))
}

fn param(name: &str, value: &str) -> String {
    const TSPECIALS: &[u8] = b"()<>@,;:\\\"/[]?=";
    let is_token = !value.is_empty()
        && value
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && !TSPECIALS.contains(&b));
    if is_token {
        format!("{name}={value}")
    } else if value.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        format!("{name}={}", quoted_string(value))
    } else {
        // RFC 2231 §4 extended-value; attribute-char excludes "*", "'"
        // and "%" as well as tspecials.
        let encoded: String = value
            .bytes()
            .map(|b| {
                if (0x21..=0x7e).contains(&b) && !TSPECIALS.contains(&b) && !b"*'%".contains(&b) {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        format!("{name}*=utf-8''{encoded}")
    }
}

fn quoted_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn no_line_break(field: &str, value: &str) -> Result<(), WriteError> {
    if value.contains(['\r', '\n']) {
        Err(WriteError::LineBreak(field.to_owned()))
    } else {
        Ok(())
    }
}

fn write_header(out: &mut Vec<u8>, header: &Header) -> Result<(), WriteError> {
    let name = &header.name;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b':')
    {
        return Err(WriteError::InvalidFieldName(name.clone()));
    }
    let tokens: Vec<String> = match &header.value {
        HeaderValue::Text(text) => text_tokens(text),
        HeaderValue::Mailboxes(list) => mailbox_tokens(name, list)?,
        HeaderValue::Groups(groups) => {
            let mut tokens: Vec<String> = Vec::new();
            for group in groups {
                if group.name.is_none() && group.mailboxes.is_empty() {
                    continue;
                }
                if let Some(last) = tokens.last_mut() {
                    last.push(',');
                }
                let mut members = mailbox_tokens(name, &group.mailboxes)?;
                match &group.name {
                    // RFC 5322 §3.4 `group = display-name ":" [group-list] ";"`.
                    // `phrase_tokens` never returns an empty list.
                    Some(display) => {
                        let mut words = phrase_tokens(display);
                        if let Some(last) = words.last_mut() {
                            last.push(':');
                        }
                        words.append(&mut members);
                        if let Some(last) = words.last_mut() {
                            last.push(';');
                        }
                        tokens.append(&mut words);
                    }
                    None => tokens.append(&mut members),
                }
            }
            tokens
        }
        HeaderValue::MessageIds(ids) => ids
            .iter()
            .map(|id| no_line_break(name, id).map(|()| format!("<{id}>")))
            .collect::<Result<_, _>>()?,
        HeaderValue::Raw(raw) => {
            // `with_params` separates parameters with NUL; a NUL from the
            // caller is not printable and has no place in a header either.
            no_line_break(name, raw)?;
            raw.split('\u{0}').map(str::to_owned).collect()
        }
    };

    out.extend_from_slice(name.as_bytes());
    out.push(b':');
    let mut line_len = name.len() + 1;
    for (i, token) in tokens.iter().enumerate() {
        // RFC 5322 §3.2.2: never fold so that a line holds only whitespace.
        if i > 0 && !token.is_empty() && line_len + 1 + token.len() > MAX_LINE {
            out.extend_from_slice(b"\r\n ");
            line_len = 1;
        } else {
            out.push(b' ');
            line_len += 1;
        }
        out.extend_from_slice(token.as_bytes());
        line_len += token.len();
    }
    out.extend_from_slice(b"\r\n");
    Ok(())
}

/// A comma-separated `mailbox-list` (RFC 5322 §3.4) as fold tokens.
fn mailbox_tokens(field: &str, list: &[Mailbox]) -> Result<Vec<String>, WriteError> {
    let mut tokens = Vec::new();
    for (i, mailbox) in list.iter().enumerate() {
        no_line_break(field, &mailbox.address)?;
        let mut words = match mailbox.name.as_deref().filter(|n| !n.is_empty()) {
            Some(display) => {
                let mut w = phrase_tokens(display);
                w.push(format!("<{}>", mailbox.address));
                w
            }
            None => vec![mailbox.address.clone()],
        };
        if i + 1 < list.len()
            && let Some(last) = words.last_mut()
        {
            last.push(',');
        }
        tokens.append(&mut words);
    }
    Ok(tokens)
}

/// Split `unstructured` text at spaces, or encode it whole as RFC 2047
/// words unless it is printable ASCII free of `=?`.
fn text_tokens(text: &str) -> Vec<String> {
    let plain = text.bytes().all(|b| (0x20..=0x7e).contains(&b)) && !text.contains("=?");
    if plain {
        text.split(' ').map(str::to_owned).collect()
    } else {
        encoded_words(text)
    }
}

/// A display name as atoms when that round-trips, else a quoted-string,
/// else RFC 2047 encoded words (allowed in a `phrase`, RFC 2047 §5(3)).
fn phrase_tokens(name: &str) -> Vec<String> {
    const ATEXT_SPECIALS: &[u8] = b"!#$%&'*+-/=?^_`{|}~";
    let words: Vec<&str> = name.split(' ').collect();
    let atoms = !words.iter().any(|w| w.is_empty())
        && words.iter().all(|w| {
            w.bytes()
                .all(|b| b.is_ascii_alphanumeric() || ATEXT_SPECIALS.contains(&b))
        })
        && !name.contains("=?");
    if atoms {
        words.into_iter().map(str::to_owned).collect()
    } else if name.bytes().all(|b| (0x20..=0x7e).contains(&b)) && !name.contains("=?") {
        vec![quoted_string(name)]
    } else {
        encoded_words(name)
    }
}

fn encoded_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + ENCODED_WORD_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        words.push(format!(
            "=?UTF-8?B?{}?=",
            base64_line(&text.as_bytes()[start..end])
        ));
        start = end;
    }
    words
}

/// Quoted-printable (RFC 2045 §6.7): encoded lines are at most 76
/// characters including the `=` of a soft line break, and whitespace at
/// the end of a line is encoded.
fn quoted_printable(text: &str) -> Vec<u8> {
    const MAX_QP_LINE: usize = 76;
    let mut out = Vec::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.extend_from_slice(b"\r\n");
        }
        let line = line.strip_suffix('\r').unwrap_or(line).as_bytes();
        let mut col = 0;
        for (j, &b) in line.iter().enumerate() {
            let last = j + 1 == line.len();
            let literal =
                (matches!(b, b' ' | b'\t') && !last) || ((0x21..=0x7e).contains(&b) && b != b'=');
            let width = if literal { 1 } else { 3 };
            let room = if last { MAX_QP_LINE } else { MAX_QP_LINE - 1 };
            if col + width > room {
                out.extend_from_slice(b"=\r\n");
                col = 0;
            }
            if literal {
                out.push(b);
            } else {
                out.extend_from_slice(format!("={b:02X}").as_bytes());
            }
            col += width;
        }
    }
    out
}

/// Base64 (RFC 4648 §4) in CRLF-separated lines of 76 characters
/// (RFC 2045 §6.8).
fn base64(bytes: &[u8]) -> Vec<u8> {
    bytes
        .chunks(57)
        .map(base64_line)
        .collect::<Vec<_>>()
        .join("\r\n")
        .into_bytes()
}

fn base64_line(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_part(content_type: &str, text: &str) -> Part {
        Part {
            content_type: content_type.into(),
            params: vec![("charset".into(), "utf-8".into())],
            disposition: None,
            headers: Vec::new(),
            body: Body::Text(text.into()),
        }
    }

    fn written(headers: &[Header], body: &Part) -> String {
        String::from_utf8(write_message(headers, body).unwrap()).unwrap()
    }

    /// The group fields of RFC 5322 Appendix A.1.3, with the writer's
    /// single space after each `:` and `,` (FWS, RFC 5322 §3.2.2).
    #[test]
    fn groups_match_rfc5322_appendix_a13() {
        let mailbox = |name: Option<&str>, address: &str| Mailbox {
            name: name.map(str::to_owned),
            address: address.into(),
        };
        let headers = [
            Header::new(
                "To",
                HeaderValue::Groups(vec![Group {
                    name: Some("A Group".into()),
                    mailboxes: vec![
                        mailbox(Some("Ed Jones"), "c@a.test"),
                        mailbox(None, "joe@where.test"),
                        mailbox(Some("John"), "jdoe@one.test"),
                    ],
                }]),
            ),
            Header::new(
                "Cc",
                HeaderValue::Groups(vec![Group {
                    name: Some("Undisclosed recipients".into()),
                    mailboxes: Vec::new(),
                }]),
            ),
            Header::new(
                "Bcc",
                HeaderValue::Groups(vec![
                    Group {
                        name: None,
                        mailboxes: vec![mailbox(None, "a@x.test")],
                    },
                    Group {
                        name: None,
                        mailboxes: Vec::new(),
                    },
                    Group {
                        name: Some("G".into()),
                        mailboxes: vec![mailbox(None, "b@x.test")],
                    },
                ]),
            ),
        ];
        let message = written(&headers, &text_part("text/plain", ""));
        assert!(message.starts_with(
            "To: A Group: Ed Jones <c@a.test>, joe@where.test, John <jdoe@one.test>;\r\n\
             Cc: Undisclosed recipients:;\r\n\
             Bcc: a@x.test, G: b@x.test;\r\n"
        ));
    }

    /// RFC 4648 §10 test vectors: the base64 of each prefix of "foobar".
    #[test]
    fn base64_matches_rfc4648_vectors() {
        let expected = [
            "", "Zg==", "Zm8=", "Zm9v", "Zm9vYg==", "Zm9vYmE=", "Zm9vYmFy",
        ];
        for (len, expected) in expected.into_iter().enumerate() {
            assert_eq!(
                base64_line(&b"foobar"[..len]),
                expected,
                "prefix length {len}"
            );
        }
    }

    /// Expected output from Python `base64.b64encode(bytes(range(60)))`,
    /// cut into a 76-character line as RFC 2045 §6.8 requires.
    #[test]
    fn base64_body_wraps_at_76_characters() {
        let bytes: Vec<u8> = (0..60).collect();
        assert_eq!(
            String::from_utf8(base64(&bytes)).unwrap(),
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4\r\nOTo7"
        );
    }

    /// Expected output from Python `quopri.encodestring`, with CRLF line
    /// breaks.
    #[test]
    fn quoted_printable_matches_python_quopri() {
        assert_eq!(
            String::from_utf8(quoted_printable("naïve = 1 \n")).unwrap(),
            "na=C3=AFve =3D 1=20\r\n"
        );
        let long = "a".repeat(80);
        assert_eq!(
            String::from_utf8(quoted_printable(&long)).unwrap(),
            format!("{}=\r\n{}", "a".repeat(75), "a".repeat(5))
        );
    }

    #[test]
    fn quoted_printable_turns_crlf_and_lf_into_crlf() {
        assert_eq!(
            String::from_utf8(quoted_printable("a\r\nb\nc")).unwrap(),
            "a\r\nb\r\nc"
        );
    }

    #[test]
    fn single_text_part_message() {
        let headers = [
            Header::new(
                "From",
                HeaderValue::Mailboxes(vec![Mailbox {
                    name: Some("Alice".into()),
                    address: "alice@example.com".into(),
                }]),
            ),
            Header::new("Subject", HeaderValue::Text("Hello".into())),
        ];
        assert_eq!(
            written(&headers, &text_part("text/plain", "Hi there\n")),
            "From: Alice <alice@example.com>\r\n\
             Subject: Hello\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             Hi there\r\n"
        );
    }

    /// `Q2Fmw6k=` is Python `base64.b64encode("Café".encode())`.
    #[test]
    fn non_ascii_subject_is_rfc2047_encoded() {
        let headers = [Header::new("Subject", HeaderValue::Text("Café".into()))];
        assert!(
            written(&headers, &text_part("text/plain", ""))
                .starts_with("Subject: =?UTF-8?B?Q2Fmw6k=?=\r\n")
        );
    }

    /// Thirty "é" are 60 UTF-8 bytes: one 44-byte word (a 45-byte cut would
    /// split a character) and one 16-byte word. Payloads from Python
    /// `base64.b64encode`.
    #[test]
    fn long_encoded_subject_splits_into_words_and_folds() {
        let headers = [Header::new("Subject", HeaderValue::Text("é".repeat(30)))];
        assert!(written(&headers, &text_part("text/plain", "")).starts_with(
            "Subject: =?UTF-8?B?w6nDqcOpw6nDqcOpw6nDqcOpw6nDqcOpw6nDqcOpw6nDqcOpw6nDqcOpw6k=?=\r\n \
             =?UTF-8?B?w6nDqcOpw6nDqcOpw6nDqQ==?=\r\n"
        ));
    }

    #[test]
    fn display_names_quote_or_encode_as_needed() {
        let headers = [Header::new(
            "To",
            HeaderValue::Mailboxes(vec![
                Mailbox {
                    name: Some("Smith, John".into()),
                    address: "john@example.com".into(),
                },
                Mailbox {
                    name: Some("José".into()),
                    address: "jose@example.com".into(),
                },
                Mailbox {
                    name: None,
                    address: "bare@example.com".into(),
                },
            ]),
        )];
        assert!(written(&headers, &text_part("text/plain", "")).starts_with(
            "To: \"Smith, John\" <john@example.com>, =?UTF-8?B?Sm9zw6k=?= <jose@example.com>,\r\n \
             bare@example.com\r\n"
        ));
    }

    /// Five 17-character addresses: the fourth would end at column 79.
    #[test]
    fn address_list_folds_before_column_78() {
        let list = (1..=5)
            .map(|i| Mailbox {
                name: None,
                address: format!("user{i}@example.com"),
            })
            .collect();
        let headers = [Header::new("To", HeaderValue::Mailboxes(list))];
        assert!(written(&headers, &text_part("text/plain", "")).starts_with(
            "To: user1@example.com, user2@example.com, user3@example.com,\r\n \
             user4@example.com, user5@example.com\r\n"
        ));
    }

    #[test]
    fn message_ids_get_angle_brackets() {
        let headers = [Header::new(
            "References",
            HeaderValue::MessageIds(vec!["a@example.com".into(), "b@example.com".into()]),
        )];
        assert!(
            written(&headers, &text_part("text/plain", ""))
                .starts_with("References: <a@example.com> <b@example.com>\r\n")
        );
    }

    #[test]
    fn multipart_alternative_message() {
        let body = Part {
            content_type: "multipart/alternative".into(),
            params: Vec::new(),
            disposition: None,
            headers: Vec::new(),
            body: Body::Multipart(vec![
                text_part("text/plain", "Hi"),
                text_part("text/html", "<p>Hi</p>"),
            ]),
        };
        assert_eq!(
            written(&[], &body),
            "MIME-Version: 1.0\r\n\
             Content-Type: multipart/alternative; boundary=\"=_0\"\r\n\
             \r\n\
             --=_0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             Hi\r\n\
             --=_0\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             <p>Hi</p>\r\n\
             --=_0--\r\n"
        );
    }

    /// The nested multipart takes `=_0`, so the outer one must pick `=_1`.
    /// `cmVwb3J0` is Python `base64.b64encode(b"report")`; `r%C3%A9sum%C3%A9.pdf`
    /// is Python `urllib.parse.quote("résumé.pdf")`.
    #[test]
    fn nested_multipart_with_attachment() {
        let attachment = Part {
            content_type: "application/pdf".into(),
            params: vec![("name".into(), "résumé.pdf".into())],
            disposition: Some((
                "attachment".into(),
                vec![("filename".into(), "résumé.pdf".into())],
            )),
            headers: vec![Header::new(
                "Content-ID",
                HeaderValue::Raw("<c1@example.com>".into()),
            )],
            body: Body::Binary(b"report".to_vec()),
        };
        let alternative = Part {
            content_type: "multipart/alternative".into(),
            params: Vec::new(),
            disposition: None,
            headers: Vec::new(),
            body: Body::Multipart(vec![text_part("text/plain", "Hi")]),
        };
        let body = Part {
            content_type: "multipart/mixed".into(),
            params: Vec::new(),
            disposition: None,
            headers: Vec::new(),
            body: Body::Multipart(vec![alternative, attachment]),
        };
        assert_eq!(
            written(&[], &body),
            "MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"=_1\"\r\n\
             \r\n\
             --=_1\r\n\
             Content-Type: multipart/alternative; boundary=\"=_0\"\r\n\
             \r\n\
             --=_0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             Hi\r\n\
             --=_0--\r\n\
             \r\n\
             --=_1\r\n\
             Content-Type: application/pdf; name*=utf-8''r%C3%A9sum%C3%A9.pdf\r\n\
             Content-Disposition: attachment; filename*=utf-8''r%C3%A9sum%C3%A9.pdf\r\n\
             Content-ID: <c1@example.com>\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             cmVwb3J0\r\n\
             --=_1--\r\n"
        );
    }

    #[test]
    fn unencoded_body_is_labelled_by_content() {
        let part = |bytes: &[u8]| Part {
            content_type: "message/rfc822".into(),
            params: Vec::new(),
            disposition: None,
            headers: Vec::new(),
            body: Body::Unencoded(bytes.to_vec()),
        };
        assert!(
            written(&[], &part(b"Subject: x\r\n\r\ny"))
                .contains("Content-Transfer-Encoding: 7bit\r\n\r\nSubject: x\r\n\r\ny")
        );
        assert!(
            written(&[], &part("Subject: é\r\n\r\n".as_bytes()))
                .contains("Content-Transfer-Encoding: 8bit\r\n")
        );
    }

    #[test]
    fn line_breaks_in_unencoded_values_are_rejected() {
        let headers = [Header::new(
            "To",
            HeaderValue::Mailboxes(vec![Mailbox {
                name: None,
                address: "a@example.com\r\nBcc: evil@example.com".into(),
            }]),
        )];
        assert_eq!(
            write_message(&headers, &text_part("text/plain", "")),
            Err(WriteError::LineBreak("To".into()))
        );
        let headers = [Header::new(
            "Message-ID",
            HeaderValue::MessageIds(vec!["a\nb".into()]),
        )];
        assert_eq!(
            write_message(&headers, &text_part("text/plain", "")),
            Err(WriteError::LineBreak("Message-ID".into()))
        );
        let mut part = text_part("text/plain", "");
        part.content_type = "text/plain\r\nX: y".into();
        assert_eq!(
            write_message(&[], &part),
            Err(WriteError::LineBreak("Content-Type".into()))
        );
    }

    /// A line break in text is carried inside an encoded word, not emitted.
    #[test]
    fn line_breaks_in_text_are_encoded() {
        let headers = [Header::new(
            "Subject",
            HeaderValue::Text("a\r\nBcc: x".into()),
        )];
        let out = written(&headers, &text_part("text/plain", ""));
        assert!(!out.contains("\r\nBcc"), "{out}");
    }

    #[test]
    fn invalid_field_names_are_rejected() {
        for name in ["", "Bad Name", "Bad:Name"] {
            let headers = [Header::new(name, HeaderValue::Raw("x".into()))];
            assert_eq!(
                write_message(&headers, &text_part("text/plain", "")),
                Err(WriteError::InvalidFieldName(name.into()))
            );
        }
    }

    #[test]
    fn ascii_parameter_values_are_quoted_when_not_tokens() {
        assert_eq!(param("name", "my file.txt"), "name=\"my file.txt\"");
        assert_eq!(param("name", "a\"b"), "name=\"a\\\"b\"");
        assert_eq!(param("charset", "utf-8"), "charset=utf-8");
    }
}

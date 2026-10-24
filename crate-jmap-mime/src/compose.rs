//! JMAP `Email` creation → RFC 5322 message, through [`crate::writer`].
//!
//! [`compose_email`] writes the message that an RFC 8621 §4.6 `Email/set`
//! create describes. It writes exactly what the [`EmailDraft`] holds; the
//! caller adds what the server must set, such as the `Message-ID` and
//! `Date` header fields RFC 8621 §4.6 requires when the client gave none.

use std::collections::BTreeMap;
use std::fmt;

use jmap_mail_types::{EmailAddress, EmailAddressGroup, EmailBodyPart, EmailBodyValue};
use jmap_types::{Date, Id};
use serde_json::{Map, Value};

use crate::writer::{self, Body, Group, Header, HeaderValue, Mailbox, Part, WriteError};

/// The parts of an `Email` create that determine the message bytes.
///
/// Field meanings follow RFC 8621 §4.1. Either `body_structure` or the
/// `text_body` / `html_body` / `attachments` lists describe the body
/// (RFC 8621 §4.6 forbids both). With neither, the body is an empty
/// `text/plain` part.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmailDraft {
    /// `From` header field.
    pub from: Option<Vec<EmailAddress>>,
    /// `Sender` header field.
    pub sender: Option<Vec<EmailAddress>>,
    /// `Reply-To` header field.
    pub reply_to: Option<Vec<EmailAddress>>,
    /// `To` header field.
    pub to: Option<Vec<EmailAddress>>,
    /// `Cc` header field.
    pub cc: Option<Vec<EmailAddress>>,
    /// `Bcc` header field.
    pub bcc: Option<Vec<EmailAddress>>,
    /// `Subject` header field.
    pub subject: Option<String>,
    /// `Date` header field, from an RFC 3339 date-time.
    pub sent_at: Option<Date>,
    /// `Message-ID` header field.
    pub message_id: Option<Vec<String>>,
    /// `In-Reply-To` header field.
    pub in_reply_to: Option<Vec<String>>,
    /// `References` header field.
    pub references: Option<Vec<String>>,
    /// The `text/plain` body part.
    pub text_body: Vec<EmailBodyPart>,
    /// The `text/html` body part.
    pub html_body: Vec<EmailBodyPart>,
    /// Parts placed after the text in a `multipart/mixed`.
    pub attachments: Vec<EmailBodyPart>,
    /// The whole MIME tree.
    pub body_structure: Option<EmailBodyPart>,
    /// Content of the parts that give a `partId`.
    pub body_values: BTreeMap<String, EmailBodyValue>,
    /// The Email's `header:{name}[:{form}][:all]` properties (RFC 8621
    /// §4.1.3), keyed by property name. Keys without the `header:` prefix
    /// are ignored. A body part's own `header:` properties are read from
    /// its `extra` map.
    pub header_properties: Map<String, Value>,
}

impl EmailDraft {
    /// Every `blobId` that a body part of the draft references, in tree
    /// order, without duplicates. The caller fetches these for
    /// [`compose_email`].
    pub fn blob_ids(&self) -> Vec<&Id> {
        fn walk<'a>(part: &'a EmailBodyPart, out: &mut Vec<&'a Id>) {
            if let Some(id) = &part.blob_id
                && !out.contains(&id)
            {
                out.push(id);
            }
            for sub in part.sub_parts.iter().flatten() {
                walk(sub, out);
            }
        }
        let mut out = Vec::new();
        self.body_structure
            .iter()
            .chain(&self.text_body)
            .chain(&self.html_body)
            .chain(&self.attachments)
            .for_each(|part| walk(part, &mut out));
        out
    }
}

/// Why [`compose_email`] could not write a draft.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// Body parts reference these blob ids, which `blobs` lacks.
    BlobNotFound(Vec<Id>),
    /// A body part's `partId` has no entry in `body_values`.
    MissingBodyValue(String),
    /// A body part has no `partId`, `blobId` or `subParts`.
    EmptyPart,
    /// `sent_at` is not an RFC 3339 date-time.
    InvalidDate(String),
    /// A `header:` property is malformed, sets a header field that
    /// another property of the same object also sets, or sets a field its
    /// object must not carry (RFC 8621 §4.6).
    InvalidHeaderProperty {
        /// The property name, e.g. `header:Content-Type`.
        property: String,
        /// Why it was rejected.
        reason: String,
    },
    /// The writer rejected a header field.
    Write(WriteError),
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlobNotFound(ids) => {
                let ids: Vec<&str> = ids.iter().map(AsRef::as_ref).collect();
                write!(f, "blobs not found: {}", ids.join(", "))
            }
            Self::MissingBodyValue(part_id) => {
                write!(f, "bodyValues has no entry for partId {part_id:?}")
            }
            Self::EmptyPart => f.write_str("body part has no partId, blobId or subParts"),
            Self::InvalidDate(date) => write!(f, "sentAt {date:?} is not an RFC 3339 date-time"),
            Self::InvalidHeaderProperty { property, reason } => write!(f, "{property}: {reason}"),
            Self::Write(e) => write!(f, "cannot write message: {e}"),
        }
    }
}

impl std::error::Error for ComposeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write(e) => Some(e),
            _ => None,
        }
    }
}

impl From<WriteError> for ComposeError {
    fn from(e: WriteError) -> Self {
        Self::Write(e)
    }
}

/// Write the RFC 5322 message `draft` describes.
///
/// `blobs` maps each id from [`EmailDraft::blob_ids`] to the blob's bytes.
///
/// Header fields come in the order `Date`, `From`, `Sender`, `Reply-To`,
/// `To`, `Cc`, `Bcc`, `Message-ID`, `In-Reply-To`, `References`,
/// `Subject`; absent or empty ones are left out. The fields of
/// `header_properties` follow, one per value (an `:all` property's array
/// gives one field per non-null element; a null value gives none). Body
/// parts map as follows:
///
/// - A part with `subParts` is a multipart of its `type` (default
///   `multipart/mixed`).
/// - A `partId` part holds its `bodyValues` text, quoted-printable; a
///   `text/*` one gets `charset=utf-8` (RFC 8621 §4.6: "the server may
///   choose any appropriate encoding").
/// - A `blobId` part holds the blob, base64, or unencoded for `message/*`
///   (RFC 2046 §5.2.1), with the part's `charset`.
/// - `type` defaults to `text/plain` (RFC 2045 §5.2). `name` is the
///   Content-Type `name` and, with a `disposition`, the Content-Disposition
///   `filename`; `cid`, `language` and `location` become `Content-ID`,
///   `Content-Language` and `Content-Location`.
///
/// - A part's `header:` properties add header fields to it, except that
///   `header:Content-Type` and `header:Content-Disposition` (one value, in
///   the Raw or `asText` form) replace the fields built from `type`,
///   `charset`, `name` and `disposition`.
///
/// Without `body_structure`, a text and an HTML part form a
/// `multipart/alternative`, and attachments follow it in a
/// `multipart/mixed`.
///
/// Each `header:` property value takes the JSON shape of its form (RFC
/// 8621 §4.1.2): a string for Raw, `asText` and `asDate`, an
/// `EmailAddress` list for `asAddresses`, an `EmailAddressGroup` list for
/// `asGroupedAddresses`, and a string list for `asMessageIds` and
/// `asURLs`. Which forms a field allows is not checked here.
///
/// # Errors
///
/// [`ComposeError::BlobNotFound`] lists every referenced blob missing from
/// `blobs`. [`ComposeError::InvalidHeaderProperty`] reports a `header:`
/// property that breaks an RFC 8621 §4.6 rule: two properties of one
/// object set the same field (e.g. `from` and `header:From`); a
/// `Content-*` field or `MIME-Version` is set on the Email; a
/// `Content-Transfer-Encoding` is set on a part; the message's root part
/// sets a field the Email sets; or a replaced `Content-Type` gives the
/// `charset` of a `partId` part or a multipart `boundary`, which the
/// server picks. The other variants report a malformed draft.
pub fn compose_email(
    draft: &EmailDraft,
    blobs: &BTreeMap<Id, Vec<u8>>,
) -> Result<Vec<u8>, ComposeError> {
    let missing: Vec<Id> = draft
        .blob_ids()
        .into_iter()
        .filter(|id| !blobs.contains_key(*id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(ComposeError::BlobNotFound(missing));
    }

    let mut headers = Vec::new();
    if let Some(date) = &draft.sent_at {
        let value = rfc5322_date(date.as_ref())
            .ok_or_else(|| ComposeError::InvalidDate(date.as_ref().to_owned()))?;
        headers.push(Header::new("Date", HeaderValue::Raw(value)));
    }
    for (name, list) in [
        ("From", &draft.from),
        ("Sender", &draft.sender),
        ("Reply-To", &draft.reply_to),
        ("To", &draft.to),
        ("Cc", &draft.cc),
        ("Bcc", &draft.bcc),
    ] {
        if let Some(list) = list.as_ref().filter(|l| !l.is_empty()) {
            let mailboxes = list
                .iter()
                .map(|a| Mailbox {
                    name: a.name.clone(),
                    address: a.email.clone(),
                })
                .collect();
            headers.push(Header::new(name, HeaderValue::Mailboxes(mailboxes)));
        }
    }
    for (name, ids) in [
        ("Message-ID", &draft.message_id),
        ("In-Reply-To", &draft.in_reply_to),
        ("References", &draft.references),
    ] {
        if let Some(ids) = ids.as_ref().filter(|l| !l.is_empty()) {
            headers.push(Header::new(name, HeaderValue::MessageIds(ids.clone())));
        }
    }
    if let Some(subject) = &draft.subject {
        headers.push(Header::new("Subject", HeaderValue::Text(subject.clone())));
    }

    let taken = [
        ("date", "sentAt", draft.sent_at.is_some()),
        ("from", "from", draft.from.is_some()),
        ("sender", "sender", draft.sender.is_some()),
        ("reply-to", "replyTo", draft.reply_to.is_some()),
        ("to", "to", draft.to.is_some()),
        ("cc", "cc", draft.cc.is_some()),
        ("bcc", "bcc", draft.bcc.is_some()),
        ("message-id", "messageId", draft.message_id.is_some()),
        ("in-reply-to", "inReplyTo", draft.in_reply_to.is_some()),
        ("references", "references", draft.references.is_some()),
        ("subject", "subject", draft.subject.is_some()),
    ];
    // The fields the Email sets, which the root part must not set again
    // (RFC 8621 §4.6).
    let mut outer: Vec<String> = taken
        .iter()
        .filter(|(_, _, set)| *set)
        .map(|(field, _, _)| (*field).to_owned())
        .collect();
    for (property, field, fields) in header_fields(&draft.header_properties, &taken)? {
        // RFC 8621 §4.6: "Header fields beginning with "Content-" MUST
        // NOT be specified on the Email object, only on EmailBodyPart
        // objects." The writer emits MIME-Version itself.
        if field.starts_with("content-") || field == "mime-version" {
            return Err(invalid_header(
                &property,
                "this header field is set on body parts, not on the Email",
            ));
        }
        headers.extend(fields);
        outer.push(field);
    }

    // The root part is `body_structure`, or the only text part when there
    // are no attachments.
    let lone = draft.text_body.len() + draft.html_body.len() == 1 && draft.attachments.is_empty();
    let to_part = |p: &EmailBodyPart| body_part(p, &draft.body_values, blobs, &[]);
    let body = match &draft.body_structure {
        Some(structure) => body_part(structure, &draft.body_values, blobs, &outer)?,
        None => {
            let texts = draft
                .text_body
                .iter()
                .chain(&draft.html_body)
                .map(|p| {
                    body_part(
                        p,
                        &draft.body_values,
                        blobs,
                        if lone { &outer } else { &[] },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let main = match texts.len() {
                0 => None,
                1 => texts.into_iter().next(),
                _ => Some(multipart("multipart/alternative", texts)),
            };
            if draft.attachments.is_empty() {
                main.unwrap_or_else(|| Part {
                    params: vec![("charset".to_owned(), "utf-8".to_owned())],
                    ..leaf("text/plain", Body::Text(String::new()))
                })
            } else {
                let attachments = draft
                    .attachments
                    .iter()
                    .map(to_part)
                    .collect::<Result<Vec<_>, _>>()?;
                multipart(
                    "multipart/mixed",
                    main.into_iter().chain(attachments).collect(),
                )
            }
        }
    };

    Ok(writer::write_message(&headers, &body)?)
}

fn invalid_header(property: &str, reason: &str) -> ComposeError {
    ComposeError::InvalidHeaderProperty {
        property: property.to_owned(),
        reason: reason.to_owned(),
    }
}

/// The `header:` properties of one object as `(property, lower-case
/// field name, fields)`, after checking that no two of them, and no
/// convenience property in `taken` (`(field, property, given)`), set the
/// same field (RFC 8621 §4.6).
fn header_fields(
    props: &Map<String, Value>,
    taken: &[(&str, &str, bool)],
) -> Result<Vec<(String, String, Vec<Header>)>, ComposeError> {
    let mut out: Vec<(String, String, Vec<Header>)> = Vec::new();
    for (property, value) in props {
        let Some(rest) = property.strip_prefix("header:") else {
            continue;
        };
        let err = |reason: String| ComposeError::InvalidHeaderProperty {
            property: property.clone(),
            reason,
        };
        let (name, form, all) = match rest.split(':').collect::<Vec<_>>().as_slice() {
            [name] => (*name, "", false),
            [name, "all"] => (*name, "", true),
            [name, form] => (*name, *form, false),
            [name, form, "all"] => (*name, *form, true),
            _ => return Err(err("expected header:{name}[:{form}][:all]".into())),
        };
        if name.is_empty() {
            return Err(err("empty header field name".into()));
        }
        let field = name.to_ascii_lowercase();
        if let Some((_, other, _)) = taken.iter().find(|(f, _, set)| *set && *f == field) {
            return Err(err(format!("{other} sets the same header field")));
        }
        if let Some((other, _, _)) = out.iter().find(|(_, f, _)| *f == field) {
            return Err(err(format!("{other} sets the same header field")));
        }
        let values: Vec<&Value> = match value {
            Value::Null => Vec::new(),
            Value::Array(items) if all => items.iter().filter(|v| !v.is_null()).collect(),
            _ if all => return Err(err("an :all property takes an array".into())),
            v => vec![v],
        };
        let fields = values
            .into_iter()
            .map(|v| header_value(form, v).map(|hv| Header::new(name, hv)))
            .collect::<Result<_, _>>()
            .map_err(err)?;
        out.push((property.clone(), field, fields));
    }
    Ok(out)
}

/// One header field value given in `form` (RFC 8621 §4.1.2; `""` is Raw).
fn header_value(form: &str, v: &Value) -> Result<HeaderValue, String> {
    fn mailbox(a: &EmailAddress) -> Mailbox {
        Mailbox {
            name: a.name.clone(),
            address: a.email.clone(),
        }
    }
    let text = || v.as_str().ok_or_else(|| "expected a string".to_owned());
    let strings = || serde_json::from_value::<Vec<String>>(v.clone()).map_err(|e| e.to_string());
    Ok(match form {
        // RFC 8621 §4.1.2.1: the raw value runs from after the colon, so
        // it may start with whitespace and be folded; RFC 5322 §2.2.3
        // unfolds by removing each CRLF that precedes WSP. Email/get
        // returns the value with LF for CRLF, so a bare LF unfolds too.
        "" => HeaderValue::Raw(
            text()?
                .replace("\r\n ", " ")
                .replace("\r\n\t", "\t")
                .replace("\n ", " ")
                .replace("\n\t", "\t")
                .trim_start_matches([' ', '\t'])
                .to_owned(),
        ),
        "asText" => HeaderValue::Text(text()?.to_owned()),
        "asAddresses" => HeaderValue::Mailboxes(
            serde_json::from_value::<Vec<EmailAddress>>(v.clone())
                .map_err(|e| e.to_string())?
                .iter()
                .map(mailbox)
                .collect(),
        ),
        "asGroupedAddresses" => HeaderValue::Groups(
            serde_json::from_value::<Vec<EmailAddressGroup>>(v.clone())
                .map_err(|e| e.to_string())?
                .iter()
                .map(|g| Group {
                    name: g.name.clone(),
                    mailboxes: g.addresses.iter().map(mailbox).collect(),
                })
                .collect(),
        ),
        "asMessageIds" => HeaderValue::MessageIds(strings()?),
        "asDate" => HeaderValue::Raw(
            rfc5322_date(text()?).ok_or_else(|| "expected an RFC 3339 date-time".to_owned())?,
        ),
        // RFC 2369 §2: each URL in angle brackets, separated by commas.
        // NUL marks the fold points of a Raw value.
        "asURLs" => HeaderValue::Raw(
            strings()?
                .iter()
                .map(|u| format!("<{u}>"))
                .collect::<Vec<_>>()
                .join(",\u{0}"),
        ),
        other => return Err(format!("unknown header form {other:?}")),
    })
}

/// Whether the parameters of a Content-Type value name `param`.
fn has_param(value: &str, param: &str) -> bool {
    value.split(';').skip(1).any(|p| {
        let p = p.trim_start();
        p.get(..param.len())
            .is_some_and(|n| n.eq_ignore_ascii_case(param))
            && matches!(
                p.as_bytes().get(param.len()),
                Some(b'=' | b'*' | b' ' | b'\t')
            )
    })
}

fn leaf(content_type: &str, body: Body) -> Part {
    Part {
        content_type: content_type.to_owned(),
        params: Vec::new(),
        disposition: None,
        headers: Vec::new(),
        body,
    }
}

fn multipart(content_type: &str, parts: Vec<Part>) -> Part {
    leaf(content_type, Body::Multipart(parts))
}

/// `outer` lists the lower-case header fields that the Email sets, which
/// this part must not set again; it is empty below the root.
fn body_part(
    part: &EmailBodyPart,
    values: &BTreeMap<String, EmailBodyValue>,
    blobs: &BTreeMap<Id, Vec<u8>>,
    outer: &[String],
) -> Result<Part, ComposeError> {
    let taken = [
        ("content-type", "type", part.type_.is_some()),
        ("content-type", "charset", part.charset.is_some()),
        ("content-type", "name", part.name.is_some()),
        (
            "content-disposition",
            "disposition",
            part.disposition.is_some(),
        ),
        ("content-disposition", "name", part.name.is_some()),
        ("content-id", "cid", part.cid.is_some()),
        ("content-language", "language", part.language.is_some()),
        ("content-location", "location", part.location.is_some()),
    ];
    let mut type_override: Option<(String, String)> = None;
    let mut disposition_override = None;
    let mut extra_headers = Vec::new();
    for (property, field, fields) in header_fields(&part.extra, &taken)? {
        if outer.contains(&field) {
            return Err(invalid_header(
                &property,
                "the Email already sets this header field",
            ));
        }
        match field.as_str() {
            "content-transfer-encoding" => {
                return Err(invalid_header(
                    &property,
                    "the server picks the Content-Transfer-Encoding",
                ));
            }
            "content-type" | "content-disposition" => {
                let value = match fields.as_slice() {
                    [] => continue,
                    [
                        Header {
                            value: HeaderValue::Raw(v) | HeaderValue::Text(v),
                            ..
                        },
                    ] => v.clone(),
                    _ => {
                        return Err(invalid_header(
                            &property,
                            "expected one value in the Raw or asText form",
                        ));
                    }
                };
                if field == "content-type" {
                    type_override = Some((property, value));
                } else {
                    disposition_override = Some(value);
                }
            }
            _ => extra_headers.extend(fields),
        }
    }
    // The media type, without parameters, that decides the body encoding.
    let media = type_override
        .as_ref()
        .map(|(_, v)| v.split(';').next().unwrap_or_default().trim())
        .or(part.type_.as_deref());

    let mut params = Vec::new();
    let (default_type, body) = if let Some(subs) = &part.sub_parts {
        let children = subs
            .iter()
            .map(|p| body_part(p, values, blobs, &[]))
            .collect::<Result<_, _>>()?;
        ("multipart/mixed", Body::Multipart(children))
    } else if let Some(part_id) = &part.part_id {
        let value = values
            .get(part_id)
            .ok_or_else(|| ComposeError::MissingBodyValue(part_id.clone()))?;
        ("text/plain", Body::Text(value.value.clone()))
    } else if let Some(blob_id) = &part.blob_id {
        let bytes = blobs
            .get(blob_id)
            .ok_or_else(|| ComposeError::BlobNotFound(vec![blob_id.clone()]))?
            .clone();
        let is_message = media.is_some_and(|t| {
            t.get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("message/"))
        });
        if let Some(charset) = &part.charset {
            params.push(("charset".to_owned(), charset.clone()));
        }
        let body = if is_message {
            Body::Unencoded(bytes)
        } else {
            Body::Binary(bytes)
        };
        ("text/plain", body)
    } else {
        return Err(ComposeError::EmptyPart);
    };
    let media = media.unwrap_or(default_type);
    let is_text = matches!(body, Body::Text(_))
        && media
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case("text/"));
    if let Some((property, value)) = &type_override {
        // RFC 8621 §4.6: a `partId` part's charset is the server's choice,
        // and so is a multipart's boundary.
        if (is_text && has_param(value, "charset"))
            || (matches!(body, Body::Multipart(_)) && has_param(value, "boundary"))
        {
            return Err(invalid_header(
                property,
                "the server picks the charset of a partId part and a multipart boundary",
            ));
        }
    }
    if is_text {
        params.push(("charset".to_owned(), "utf-8".to_owned()));
    }
    if let Some(name) = &part.name {
        params.push(("name".to_owned(), name.clone()));
    }
    let disposition = part
        .disposition
        .as_ref()
        .map(|d| {
            let dparams = part
                .name
                .iter()
                .map(|n| ("filename".to_owned(), n.clone()))
                .collect();
            (d.clone(), dparams)
        })
        .or(disposition_override.map(|d| (d, Vec::new())));
    let mut headers = Vec::new();
    if let Some(cid) = &part.cid {
        headers.push(Header::new(
            "Content-ID",
            HeaderValue::Raw(format!("<{cid}>")),
        ));
    }
    if let Some(language) = part.language.as_ref().filter(|l| !l.is_empty()) {
        headers.push(Header::new(
            "Content-Language",
            HeaderValue::Raw(language.join(", ")),
        ));
    }
    if let Some(location) = &part.location {
        headers.push(Header::new(
            "Content-Location",
            HeaderValue::Raw(location.clone()),
        ));
    }
    headers.extend(extra_headers);
    let content_type = type_override.as_ref().map_or(media, |(_, v)| v).to_owned();
    Ok(Part {
        content_type,
        params,
        disposition,
        headers,
        body,
    })
}

/// `2014-10-24T09:10:00.5+02:00` → `24 Oct 2014 09:10:00 +0200`, the
/// RFC 5322 §3.3 `date-time` without the optional day-of-week. Fractional
/// seconds are dropped; RFC 5322 has none.
fn rfc5322_date(rfc3339: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let b = rfc3339.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        b.get(r.clone())
            .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
    };
    let shape = digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10)
        && matches!(b.get(10), Some(b'T' | b't'))
        && digits(11..13)
        && b.get(13) == Some(&b':')
        && digits(14..16)
        && b.get(16) == Some(&b':')
        && digits(17..19);
    if !shape {
        return None;
    }
    let month: usize = rfc3339[5..7].parse().ok()?;
    let month = MONTHS.get(month.checked_sub(1)?)?;
    let mut rest = &rfc3339[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let end = frac
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(frac.len());
        if end == 0 {
            return None;
        }
        rest = &frac[end..];
    }
    let zone = match rest.as_bytes() {
        [b'Z' | b'z'] => "+0000".to_owned(),
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]
            if [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()) =>
        {
            String::from_utf8(vec![*sign, *h1, *h2, *m1, *m2]).ok()?
        }
        _ => return None,
    };
    Some(format!(
        "{} {month} {} {} {zone}",
        &rfc3339[8..10],
        &rfc3339[0..4],
        &rfc3339[11..19]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn addr(name: Option<&str>, email: &str) -> EmailAddress {
        let mut a = EmailAddress::new(email);
        a.name = name.map(str::to_owned);
        a
    }

    fn text_part(part_id: &str, type_: &str) -> EmailBodyPart {
        let mut p = EmailBodyPart::default();
        p.part_id = Some(part_id.into());
        p.type_ = Some(type_.into());
        p
    }

    fn composed(draft: &EmailDraft, blobs: &BTreeMap<Id, Vec<u8>>) -> String {
        String::from_utf8(compose_email(draft, blobs).unwrap()).unwrap()
    }

    /// RFC 5322 §3.3 date-time, from the RFC 8621 §4.1.3 `sentAt` example
    /// shape and RFC 3339 §5.8 examples.
    #[test]
    fn dates_convert_to_rfc5322() {
        assert_eq!(
            rfc5322_date("2014-10-24T09:10:00+02:00").as_deref(),
            Some("24 Oct 2014 09:10:00 +0200")
        );
        assert_eq!(
            rfc5322_date("1985-04-12T23:20:50.52Z").as_deref(),
            Some("12 Apr 1985 23:20:50 +0000")
        );
        assert_eq!(
            rfc5322_date("1996-12-19T16:39:57-08:00").as_deref(),
            Some("19 Dec 1996 16:39:57 -0800")
        );
        for bad in [
            "",
            "2014-13-01T00:00:00Z",
            "2014-10-24 09:10:00Z",
            "2014-10-24T09:10:00",
            "2014-10-24T09:10:00.Z",
        ] {
            assert_eq!(rfc5322_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn text_only_draft() {
        let draft = EmailDraft {
            from: Some(vec![addr(Some("Alice"), "alice@example.com")]),
            to: Some(vec![addr(None, "bob@example.com")]),
            subject: Some("Lunch".into()),
            sent_at: Some(Date::from("2026-10-23T12:00:00Z")),
            message_id: Some(vec!["m1@example.com".into()]),
            in_reply_to: Some(vec!["m0@example.com".into()]),
            text_body: vec![text_part("1", "text/plain")],
            body_values: BTreeMap::from([("1".into(), EmailBodyValue::new("Noon?\n"))]),
            ..Default::default()
        };
        assert_eq!(
            composed(&draft, &BTreeMap::new()),
            "Date: 23 Oct 2026 12:00:00 +0000\r\n\
             From: Alice <alice@example.com>\r\n\
             To: bob@example.com\r\n\
             Message-ID: <m1@example.com>\r\n\
             In-Reply-To: <m0@example.com>\r\n\
             Subject: Lunch\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             Noon?\r\n"
        );
    }

    #[test]
    fn empty_draft_has_empty_text_body() {
        assert_eq!(
            composed(&EmailDraft::default(), &BTreeMap::new()),
            "MIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n"
        );
    }

    /// `aGVsbG8=` is Python `base64.b64encode(b"hello")`.
    #[test]
    fn text_html_and_attachment_draft() {
        let mut attachment = EmailBodyPart::default();
        attachment.blob_id = Some(Id::from("B1"));
        attachment.type_ = Some("application/octet-stream".into());
        attachment.name = Some("a.bin".into());
        attachment.disposition = Some("attachment".into());
        attachment.cid = Some("c1@example.com".into());
        let draft = EmailDraft {
            text_body: vec![text_part("t", "text/plain")],
            html_body: vec![text_part("h", "text/html")],
            attachments: vec![attachment],
            body_values: BTreeMap::from([
                ("t".into(), EmailBodyValue::new("Hi")),
                ("h".into(), EmailBodyValue::new("<b>Hi</b>")),
            ]),
            ..Default::default()
        };
        let blobs = BTreeMap::from([(Id::from("B1"), b"hello".to_vec())]);
        assert_eq!(
            composed(&draft, &blobs),
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
             --=_0\r\n\
             Content-Type: text/html; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             <b>Hi</b>\r\n\
             --=_0--\r\n\
             \r\n\
             --=_1\r\n\
             Content-Type: application/octet-stream; name=a.bin\r\n\
             Content-Disposition: attachment; filename=a.bin\r\n\
             Content-ID: <c1@example.com>\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             aGVsbG8=\r\n\
             --=_1--\r\n"
        );
    }

    #[test]
    fn body_structure_draft_with_message_attachment() {
        let mut inner = EmailBodyPart::default();
        inner.blob_id = Some(Id::from("M"));
        inner.type_ = Some("message/rfc822".into());
        let mut root = EmailBodyPart::default();
        root.type_ = Some("multipart/mixed".into());
        root.sub_parts = Some(vec![text_part("1", "text/plain"), inner]);
        let draft = EmailDraft {
            body_structure: Some(root),
            body_values: BTreeMap::from([("1".into(), EmailBodyValue::new("fwd"))]),
            ..Default::default()
        };
        let blobs = BTreeMap::from([(Id::from("M"), b"Subject: old\r\n\r\nold".to_vec())]);
        assert_eq!(
            composed(&draft, &blobs),
            "MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"=_0\"\r\n\
             \r\n\
             --=_0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n\
             fwd\r\n\
             --=_0\r\n\
             Content-Type: message/rfc822\r\n\
             Content-Transfer-Encoding: 7bit\r\n\
             \r\n\
             Subject: old\r\n\r\nold\r\n\
             --=_0--\r\n"
        );
    }

    #[test]
    fn missing_blobs_are_all_reported() {
        let blob_part = |id: &str| {
            let mut p = EmailBodyPart::default();
            p.blob_id = Some(Id::from(id));
            p
        };
        let draft = EmailDraft {
            attachments: vec![
                blob_part("x"),
                blob_part("present"),
                blob_part("y"),
                blob_part("x"),
            ],
            ..Default::default()
        };
        let blobs = BTreeMap::from([(Id::from("present"), Vec::new())]);
        assert_eq!(
            compose_email(&draft, &blobs),
            Err(ComposeError::BlobNotFound(vec![
                Id::from("x"),
                Id::from("y")
            ]))
        );
    }

    #[test]
    fn malformed_drafts_are_rejected() {
        let mut draft = EmailDraft {
            text_body: vec![text_part("1", "text/plain")],
            ..Default::default()
        };
        assert_eq!(
            compose_email(&draft, &BTreeMap::new()),
            Err(ComposeError::MissingBodyValue("1".into()))
        );
        draft.text_body = vec![EmailBodyPart::default()];
        assert_eq!(
            compose_email(&draft, &BTreeMap::new()),
            Err(ComposeError::EmptyPart)
        );
        let draft = EmailDraft {
            sent_at: Some(Date::from("yesterday")),
            ..Default::default()
        };
        assert_eq!(
            compose_email(&draft, &BTreeMap::new()),
            Err(ComposeError::InvalidDate("yesterday".into()))
        );
    }

    /// Cross-check against mime-tree, an independent parser: the composed
    /// message parses back to the draft's values.
    #[test]
    fn composed_message_parses_back_with_mime_tree() {
        let draft = EmailDraft {
            from: Some(vec![addr(Some("José Núñez"), "jose@example.com")]),
            subject: Some("Grüße aus Köln".into()),
            sent_at: Some(Date::from("2014-10-24T09:10:00+02:00")),
            text_body: vec![text_part("1", "text/plain")],
            body_values: BTreeMap::from([(
                "1".into(),
                EmailBodyValue::new("Ünïcödé body\nline two"),
            )]),
            ..Default::default()
        };
        let raw = compose_email(&draft, &BTreeMap::new()).unwrap();

        let msg = mime_tree::parse(&raw).unwrap();
        let header = |name: &str| {
            msg.headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap()
        };
        assert_eq!(
            mime_tree::parse_text(header("Subject").as_bytes()),
            "Grüße aus Köln"
        );
        let from = mime_tree::parse_addresses(header("From").as_bytes());
        assert_eq!(from[0].name.as_deref(), Some("José Núñez"));
        assert_eq!(from[0].address.as_deref(), Some("jose@example.com"));
        assert_eq!(
            mime_tree::parse_date(header("Date").as_bytes())
                .map(|d| d.to_rfc3339())
                .as_deref(),
            Some("2014-10-24T09:10:00+02:00")
        );
        let text_id = &msg.text_body[0];
        let part = msg.part_index.find_by_id(text_id).unwrap();
        let decoded = mime_tree::decode_body_value(&raw, part, None).unwrap();
        assert_eq!(decoded.value, "Ünïcödé body\r\nline two");
    }

    fn props(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn header_error(draft: &EmailDraft, blobs: &BTreeMap<Id, Vec<u8>>) -> String {
        match compose_email(draft, blobs) {
            Err(ComposeError::InvalidHeaderProperty { property, .. }) => property,
            other => panic!("expected InvalidHeaderProperty, got {other:?}"),
        }
    }

    /// RFC 8621 §4.6: "the client must set each header field as an
    /// individual property". Each form takes the JSON shape of RFC 8621
    /// §4.1.2; the expected fields are hand-written RFC 5322 / RFC 2369.
    /// Keys are in sorted order, the order the map iterates.
    #[test]
    fn email_header_properties_in_every_form() {
        let draft = EmailDraft {
            header_properties: props(&[
                ("header:Comments", json!(" folded\r\n value")),
                (
                    "header:List-Unsubscribe:asURLs",
                    json!(["mailto:u@example.com", "https://example.com/u"]),
                ),
                (
                    "header:Resent-Date:asDate",
                    json!("2026-10-23T12:00:00-07:00"),
                ),
                (
                    "header:Resent-Message-ID:asMessageIds",
                    json!(["r1@example.com"]),
                ),
                (
                    "header:Resent-To:asGroupedAddresses",
                    json!([
                        {"name": null, "addresses": [{"email": "a@example.com"}]},
                        {"name": "Team", "addresses": [{"name": "Bo", "email": "b@example.com"}]}
                    ]),
                ),
                (
                    "header:Sender:asAddresses",
                    json!([{"name": "Al", "email": "al@example.com"}]),
                ),
                ("header:X-Absent", Value::Null),
                ("header:X-Tag:all", json!(["one", null, "two"])),
                ("header:X-Text:asText", json!("Grüße")),
            ]),
            ..Default::default()
        };
        // `R3LDvMOfZQ==` is Python `base64.b64encode("Grüße".encode())`.
        assert_eq!(
            composed(&draft, &BTreeMap::new()),
            "Comments: folded value\r\n\
             List-Unsubscribe: <mailto:u@example.com>, <https://example.com/u>\r\n\
             Resent-Date: 23 Oct 2026 12:00:00 -0700\r\n\
             Resent-Message-ID: <r1@example.com>\r\n\
             Resent-To: a@example.com, Team: Bo <b@example.com>;\r\n\
             Sender: Al <al@example.com>\r\n\
             X-Tag: one\r\n\
             X-Tag: two\r\n\
             X-Text: =?UTF-8?B?R3LDvMOfZQ==?=\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: quoted-printable\r\n\
             \r\n"
        );
    }

    /// A part's `header:` properties: Content-Type and Content-Disposition
    /// replace the built fields, others are added after them. The lone
    /// text part is the root, and may set fields the Email does not.
    #[test]
    fn part_header_properties() {
        let mut attachment = EmailBodyPart::default();
        attachment.blob_id = Some(Id::from("B1"));
        attachment.extra = props(&[
            ("header:Content-Disposition", json!("inline")),
            ("header:Content-Type", json!("application/x-foo; x=1")),
            ("header:X-Part:asText", json!("p")),
        ]);
        let draft = EmailDraft {
            text_body: vec![text_part("t", "text/plain")],
            attachments: vec![attachment],
            body_values: BTreeMap::from([("t".into(), EmailBodyValue::new("Hi"))]),
            ..Default::default()
        };
        let blobs = BTreeMap::from([(Id::from("B1"), b"hello".to_vec())]);
        assert!(composed(&draft, &blobs).contains(
            "Content-Type: application/x-foo; x=1\r\n\
             Content-Disposition: inline\r\n\
             X-Part: p\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             aGVsbG8=\r\n"
        ));

        let mut text = text_part("t", "text/plain");
        text.extra = props(&[("header:X-Root", json!("r"))]);
        let draft = EmailDraft {
            subject: Some("S".into()),
            text_body: vec![text],
            body_values: BTreeMap::from([("t".into(), EmailBodyValue::new("Hi"))]),
            ..Default::default()
        };
        assert!(composed(&draft, &BTreeMap::new()).contains(
            "Content-Type: text/plain; charset=utf-8\r\n\
             X-Root: r\r\n"
        ));
    }

    /// Each case breaks one RFC 8621 §4.6 constraint, quoted beside it.
    #[test]
    fn header_properties_breaking_rfc8621_4_6_are_rejected() {
        let blobs = BTreeMap::new();
        let email = |pairs: &[(&str, Value)]| EmailDraft {
            header_properties: props(pairs),
            ..Default::default()
        };

        // "There MUST NOT be two properties that represent the same header
        // field (e.g., "header:from" and "from")".
        let mut draft = email(&[("header:from", json!(" a@example.com"))]);
        draft.from = Some(vec![addr(None, "b@example.com")]);
        assert_eq!(header_error(&draft, &blobs), "header:from");
        let draft = email(&[
            ("header:Subject", json!("a")),
            ("header:subject:asText", json!("b")),
        ]);
        assert_eq!(header_error(&draft, &blobs), "header:subject:asText");

        // "Header fields beginning with "Content-" MUST NOT be specified
        // on the Email object, only on EmailBodyPart objects."
        let draft = email(&[("header:Content-Language", json!("en"))]);
        assert_eq!(header_error(&draft, &blobs), "header:Content-Language");

        // "A Content-Transfer-Encoding header field MUST NOT be given."
        let mut part = text_part("1", "text/plain");
        part.extra = props(&[("header:Content-Transfer-Encoding", json!("7bit"))]);
        let draft = EmailDraft {
            body_structure: Some(part.clone()),
            body_values: BTreeMap::from([("1".into(), EmailBodyValue::new("x"))]),
            ..Default::default()
        };
        assert_eq!(
            header_error(&draft, &blobs),
            "header:Content-Transfer-Encoding"
        );

        // "the "bodyStructure" EmailBodyPart MUST NOT contain a property
        // representing a header field that is already defined on the
        // top-level Email object."
        part.extra = props(&[("header:Subject:asText", json!("again"))]);
        let mut draft = EmailDraft {
            subject: Some("S".into()),
            body_structure: Some(part.clone()),
            body_values: BTreeMap::from([("1".into(), EmailBodyValue::new("x"))]),
            ..Default::default()
        };
        assert_eq!(header_error(&draft, &blobs), "header:Subject:asText");
        draft.subject = None;
        draft.header_properties = props(&[("header:subject", json!("S"))]);
        assert_eq!(header_error(&draft, &blobs), "header:Subject:asText");

        // Within a part, `type` and header:Content-Type are the same field.
        part.extra = props(&[("header:Content-Type", json!("text/html"))]);
        let draft = EmailDraft {
            body_structure: Some(part.clone()),
            body_values: BTreeMap::from([("1".into(), EmailBodyValue::new("x"))]),
            ..Default::default()
        };
        assert_eq!(header_error(&draft, &blobs), "header:Content-Type");

        // "The "charset" property MUST be omitted if a partId is given
        // (... the server may choose any appropriate encoding)."
        part.type_ = None;
        part.extra = props(&[("header:Content-Type", json!("text/plain; charset=latin1"))]);
        let draft = EmailDraft {
            body_structure: Some(part),
            body_values: BTreeMap::from([("1".into(), EmailBodyValue::new("x"))]),
            ..Default::default()
        };
        assert_eq!(header_error(&draft, &blobs), "header:Content-Type");

        // Malformed properties.
        for (property, value) in [
            ("header:X-Tag:all", json!("not an array")),
            ("header:X-Tag:asBogus", json!("x")),
            ("header:X-Tag:asText:all:extra", json!(["x"])),
            ("header:Resent-Date:asDate", json!("yesterday")),
            ("header:Sender:asAddresses", json!("al@example.com")),
        ] {
            assert_eq!(header_error(&email(&[(property, value)]), &blobs), property);
        }
    }
}

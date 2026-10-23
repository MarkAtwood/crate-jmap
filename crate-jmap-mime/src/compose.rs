//! JMAP `Email` creation → RFC 5322 message, through [`crate::writer`].
//!
//! [`compose_email`] writes the message that an RFC 8621 §4.6 `Email/set`
//! create describes. It writes exactly what the [`EmailDraft`] holds; the
//! caller adds what the server must set, such as the `Message-ID` and
//! `Date` header fields RFC 8621 §4.6 requires when the client gave none.

use std::collections::BTreeMap;
use std::fmt;

use jmap_mail_types::{EmailAddress, EmailBodyPart, EmailBodyValue};
use jmap_types::{Date, Id};

use crate::writer::{self, Body, Header, HeaderValue, Mailbox, Part, WriteError};

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
/// `Subject`; absent or empty ones are left out. Body parts map as
/// follows:
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
/// Without `body_structure`, a text and an HTML part form a
/// `multipart/alternative`, and attachments follow it in a
/// `multipart/mixed`.
///
/// # Errors
///
/// [`ComposeError::BlobNotFound`] lists every referenced blob missing from
/// `blobs`. The other variants report a malformed draft.
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

    let to_part = |p: &EmailBodyPart| body_part(p, &draft.body_values, blobs);
    let body = match &draft.body_structure {
        Some(structure) => to_part(structure)?,
        None => {
            let texts = draft
                .text_body
                .iter()
                .chain(&draft.html_body)
                .map(to_part)
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

fn body_part(
    part: &EmailBodyPart,
    values: &BTreeMap<String, EmailBodyValue>,
    blobs: &BTreeMap<Id, Vec<u8>>,
) -> Result<Part, ComposeError> {
    let mut params = Vec::new();
    let (default_type, body) = if let Some(subs) = &part.sub_parts {
        let children = subs
            .iter()
            .map(|p| body_part(p, values, blobs))
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
        let is_message = part.type_.as_deref().is_some_and(|t| {
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
    let content_type = part.type_.as_deref().unwrap_or(default_type);
    if matches!(body, Body::Text(_))
        && content_type
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case("text/"))
    {
        params.push(("charset".to_owned(), "utf-8".to_owned()));
    }
    if let Some(name) = &part.name {
        params.push(("name".to_owned(), name.clone()));
    }
    let disposition = part.disposition.as_ref().map(|d| {
        let dparams = part
            .name
            .iter()
            .map(|n| ("filename".to_owned(), n.clone()))
            .collect();
        (d.clone(), dparams)
    });
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
    Ok(Part {
        content_type: content_type.to_owned(),
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
}

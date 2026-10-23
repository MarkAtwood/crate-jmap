//! Raw RFC 5322 message → [`Email`] fields, shared by `Email/set` create,
//! `Email/import` and `Email/parse`.

use jmap_mail_types::{Email, EmailAddress, EmailHeader};
use jmap_mime::message_to_jmap_body;
use jmap_types::{Date, Id};

/// Header fields parsed from an RFC 5322 message header block.
pub(crate) struct ParsedHeaders {
    pub(crate) subject: Option<String>,
    pub(crate) message_id: Option<Vec<String>>,
    pub(crate) in_reply_to: Vec<String>,
    pub(crate) references: Vec<String>,
    pub(crate) from: Option<Vec<EmailAddress>>,
    pub(crate) sender: Option<Vec<EmailAddress>>,
    pub(crate) reply_to: Option<Vec<EmailAddress>>,
    pub(crate) to: Option<Vec<EmailAddress>>,
    pub(crate) cc: Option<Vec<EmailAddress>>,
    pub(crate) bcc: Option<Vec<EmailAddress>>,
    /// `Date` header field as RFC 3339 (RFC 8621 §4.1.3 `sentAt`).
    pub(crate) sent_at: Option<Date>,
    /// Short preview of the raw body (first 256 characters), used only when
    /// mime-tree cannot parse the message.
    pub(crate) preview: Option<String>,
    /// Raw header fields in order, for `Email.headers` (RFC 8621 §4.1.3).
    pub(crate) raw_headers: Vec<EmailHeader>,
}

/// Bare-minimum RFC 5322 header parser.
///
/// Reads raw bytes as UTF-8 (lossy), splits on the blank line that separates
/// headers from the body, and extracts the fields RFC 8621 §4.1.3 defines
/// convenience properties for. Folded header lines (CRLF + whitespace) are
/// unfolded. Structured values go through mime-tree, the workspace's RFC
/// 5322 parsing gateway.
pub(crate) fn parse_rfc5322_headers(bytes: &[u8]) -> ParsedHeaders {
    let text = String::from_utf8_lossy(bytes);

    // Split headers from body at the first blank line.
    let (header_block, body_block) = if let Some(idx) = text.find("\r\n\r\n") {
        (&text[..idx], &text[idx + 4..])
    } else if let Some(idx) = text.find("\n\n") {
        (&text[..idx], &text[idx + 2..])
    } else {
        (text.as_ref(), "")
    };

    // Build raw_headers: fold continuation lines back into the preceding header.
    // A line beginning with whitespace is a continuation of the previous header value.
    let mut raw_headers: Vec<EmailHeader> = Vec::new();
    for line in header_block.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line — append to previous header value.
            if let Some(last) = raw_headers.last_mut() {
                last.value.push('\n');
                last.value.push_str(line);
            }
        } else if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].to_owned();
            let value = line[colon_pos + 1..].to_owned();
            raw_headers.push(EmailHeader::new(name, value));
        }
        // Lines with no colon and no leading whitespace (malformed) are skipped.
    }

    // Unfold header lines for the structured field extraction below.
    let unfolded = header_block
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ")
        .replace("\n ", " ")
        .replace("\n\t", " ");

    let mut parsed = ParsedHeaders {
        subject: None,
        message_id: None,
        in_reply_to: Vec::new(),
        references: Vec::new(),
        from: None,
        sender: None,
        reply_to: None,
        to: None,
        cc: None,
        bcc: None,
        sent_at: None,
        preview: None,
        raw_headers,
    };

    for line in unfolded.lines() {
        // RFC 5322 §2.2: header field names are case-insensitive. Split on the
        // first ':' and compare the field name case-insensitively.
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let addresses = || Some(parse_address_list_via_mime_tree(rest));
        match name.to_ascii_lowercase().as_str() {
            // RFC 8621 §4.1.3 `subject` is the header "parsed as Text",
            // which decodes RFC 2047 encoded words.
            "subject" => parsed.subject = Some(mime_tree::parse_text(rest.as_bytes())),
            "message-id" => {
                let ids = parse_msg_ids_via_mime_tree(rest);
                if !ids.is_empty() {
                    parsed.message_id = Some(ids);
                }
            }
            "in-reply-to" => parsed.in_reply_to = parse_msg_ids_via_mime_tree(rest),
            "references" => parsed.references = parse_msg_ids_via_mime_tree(rest),
            "from" => parsed.from = addresses(),
            "sender" => parsed.sender = addresses(),
            "reply-to" => parsed.reply_to = addresses(),
            "to" => parsed.to = addresses(),
            "cc" => parsed.cc = addresses(),
            "bcc" => parsed.bcc = addresses(),
            "date" => {
                parsed.sent_at = mime_tree::parse_date(rest.as_bytes())
                    .map(|d| Date::from(d.to_rfc3339().as_str()));
            }
            _ => {}
        }
    }

    let trimmed = body_block.trim();
    if !trimmed.is_empty() {
        let end = trimmed
            .char_indices()
            .take(256)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(trimmed.len());
        parsed.preview = Some(trimmed[..end].to_owned());
    }

    parsed
}

/// Fill `email`'s RFC 8621 §4.1.3 header properties from `parsed` and its
/// §4.1.4 body properties from a MIME parse of `bytes`.
///
/// Leaf parts get the blob ids `{blob_id}-part-{n}`. The preview comes from
/// the MIME parse, or from the raw body when mime-tree cannot parse the
/// message.
pub(crate) fn fill_from_message(
    email: &mut Email,
    parsed: ParsedHeaders,
    bytes: &[u8],
    blob_id: &Id,
) {
    email.subject = parsed.subject;
    email.message_id = parsed.message_id;
    email.in_reply_to = (!parsed.in_reply_to.is_empty()).then_some(parsed.in_reply_to);
    email.references = (!parsed.references.is_empty()).then_some(parsed.references);
    email.from = parsed.from;
    email.sender = parsed.sender;
    email.reply_to = parsed.reply_to;
    email.to = parsed.to;
    email.cc = parsed.cc;
    email.bcc = parsed.bcc;
    email.sent_at = parsed.sent_at;
    email.headers = parsed.raw_headers;
    email.preview = parsed.preview;

    if let Ok(parsed_msg) = mime_tree::parse(bytes) {
        let part_counter = std::cell::Cell::new(0usize);
        let body_fields = message_to_jmap_body(&parsed_msg, |_part| {
            let i = part_counter.get();
            part_counter.set(i + 1);
            Id::from(format!("{blob_id}-part-{i}"))
        });
        email.text_body = body_fields.text_body;
        email.html_body = body_fields.html_body;
        email.has_attachment = !body_fields.attachments.is_empty();
        email.attachments = body_fields.attachments;
        email.body_structure = Some(body_fields.body_structure);
        if body_fields.preview.is_some() {
            email.preview = body_fields.preview;
        }
    }
}

/// Parse `<id>` tokens from a Message-ID / In-Reply-To / References
/// header value via mime-tree.
///
/// This shares the workspace's single RFC 5322 parsing gateway with
/// `email.rs::apply_header_form` so the Email/get `messageId` /
/// `inReplyTo` / `references` properties stay consistent with
/// `header:<name>:asMessageIds` results on identical input
/// (`bd:JMAP-j7pa.10`).
fn parse_msg_ids_via_mime_tree(s: &str) -> Vec<String> {
    match mime_tree::parse_header_typed(mime_tree::HeaderForm::MessageIds, s.as_bytes()) {
        mime_tree::HeaderValueTyped::MessageIds(ids) => ids,
        _ => Vec::new(),
    }
}

/// Parse an RFC 5322 address-list header value via mime-tree.
///
/// Handles RFC 2047 encoded-words, RFC 5322 groups, and quoted-string
/// display names with embedded commas. Shares the workspace's single RFC
/// 5322 parsing gateway with `email.rs::apply_header_form` so the
/// Email/get address properties stay consistent with
/// `header:<name>:asAddresses` results on identical input
/// (`bd:JMAP-j7pa.10`).
fn parse_address_list_via_mime_tree(s: &str) -> Vec<EmailAddress> {
    match mime_tree::parse_header_typed(mime_tree::HeaderForm::Addresses, s.as_bytes()) {
        mime_tree::HeaderValueTyped::Addresses(list) => list
            .into_iter()
            .map(|addr| {
                let mut ea = EmailAddress::new(addr.address.unwrap_or_default());
                ea.name = addr.name;
                ea
            })
            .collect(),
        _ => Vec::new(),
    }
}

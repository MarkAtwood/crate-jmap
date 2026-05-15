//! Thin adapter: [`mime_tree`] types → [`jmap_mail_types`] types.
//!
//! All MIME parsing lives in `mime-tree`. This crate only maps field names.
//!
//! # Usage
//!
//! A real `MailBackend::parse_email` implementation MUST surface
//! `mime_tree::parse` and `decode_body_value` errors as JMAP method
//! errors per RFC 8620 §3.6.2 — never panic. This example uses `?` and
//! returns `Result` so the pattern reads correctly when copied.
//!
//! ```rust
//! use jmap_mime::{message_to_jmap_body, body_value_to_jmap};
//! use jmap_types::Id;
//! use mime_tree::{parse, decode_body_value};
//!
//! # fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let raw = b"From: alice@example.com\r\n\
//!             Content-Type: text/plain; charset=utf-8\r\n\
//!             \r\n\
//!             Hello, world!\r\n";
//!
//! let msg = parse(raw)?;
//!
//! // Assign blob IDs for each leaf part (storage layer decides how).
//! let fields = message_to_jmap_body(&msg, |part| {
//!     Id::from(format!("blob-{}", part.part_id))
//! });
//!
//! assert_eq!(fields.text_body.len(), 1);
//! assert_eq!(fields.text_body[0].type_.as_deref(), Some("text/plain"));
//!
//! // Decode body values on demand and map them into EmailBodyValue.
//! for part_id in &fields.body_value_part_ids {
//!     if let Some(part) = msg.part_index.find_by_id(part_id) {
//!         let decoded = decode_body_value(raw, part, Some(8192))?;
//!         let _jmap_val = body_value_to_jmap(decoded);
//!     }
//! }
//! # Ok(())
//! # }
//! # demo().unwrap();
//! ```

#![forbid(unsafe_code)]

use jmap_mail_types::email::{EmailBodyPart, EmailBodyValue};
use jmap_types::Id;
use mime_tree::{DecodedBodyValue, ParsedMessage, ParsedPart};

/// Maximum multipart nesting depth this adapter will recurse into.
///
/// A multipart part whose depth in the tree (root = 0) exceeds this bound
/// is converted as an opaque leaf: its [`EmailBodyPart`] is emitted with
/// the same type / disposition / cid / charset / name as the multipart,
/// but `sub_parts` is set to `None` instead of recursing further. The
/// resulting JMAP wire response is a structurally well-formed truncation
/// rather than a stack-overflow crash.
///
/// The bound exists as defense-in-depth against deeply-nested
/// `multipart/*` framing supplied by a hostile SMTP sender. Mainstream
/// MIME parsers cap recursion at roughly the same depth (64 is the
/// commonly-cited industry value). The bound is intentionally far below
/// the system thread stack frame limit so that a few thousand bytes of
/// per-frame state stays well clear of overflow.
///
/// Note: this constant bounds only `jmap-mime`'s own walk. The upstream
/// [`mime_tree::parse`] / [`mime_tree::ParsedPart::find_by_id`] paths
/// have their own (currently unbounded — see crate `Gotchas`) recursion
/// posture. Consumers MUST also bound raw message size upstream of
/// `mime_tree::parse` to obtain total-message safety.
pub const MAX_PART_DEPTH: usize = 64;

/// The JMAP body fields derived from a parsed MIME message.
///
/// Returned by [`message_to_jmap_body`]. Each list mirrors the RFC 8621
/// §4.1.4 definitions. `body_value_part_ids` lists the part IDs the caller
/// should decode via [`mime_tree::decode_body_value`] and insert into the
/// `bodyValues` map of the JMAP `Email` response.
#[derive(Debug, Clone, PartialEq)]
pub struct JmapBodyFields {
    /// Full MIME tree (RFC 8621 §4.1.4 `bodyStructure`).
    pub body_structure: EmailBodyPart,
    /// Text/plain display parts (RFC 8621 §4.1.4 `textBody`).
    pub text_body: Vec<EmailBodyPart>,
    /// Text/html display parts (RFC 8621 §4.1.4 `htmlBody`).
    pub html_body: Vec<EmailBodyPart>,
    /// Attachment and non-inline parts (RFC 8621 §4.1.4 `attachments`).
    pub attachments: Vec<EmailBodyPart>,
    /// Short preview of the message body (RFC 8621 §4.1.4 `preview`).
    pub preview: Option<String>,
    /// Part IDs whose body content should be decoded and surfaced in
    /// `bodyValues`. Typically the union of `textBody` and `htmlBody` part IDs.
    pub body_value_part_ids: Vec<String>,
}

/// Convert a [`ParsedPart`] (and its children) into an [`EmailBodyPart`] tree.
///
/// `blob_id_for` is called once per non-multipart leaf to assign a `blobId`.
/// Multipart parts receive `None` for `partId`, `blobId`, and `size`.
///
/// The `headers` and `language`/`location` fields are not populated here
/// because they require access to the raw message bytes. Callers that need
/// per-part raw headers can extract them from `part.header_range` and the
/// original `&[u8]`.
///
/// # Depth bound
///
/// The recursion is bounded by [`MAX_PART_DEPTH`]. A multipart subtree
/// nested deeper than the bound is converted as an opaque leaf with
/// `sub_parts = None`. See the [`MAX_PART_DEPTH`] doc for rationale.
pub fn part_to_jmap(part: &ParsedPart, blob_id_for: impl Fn(&ParsedPart) -> Id) -> EmailBodyPart {
    part_to_jmap_inner(part, &blob_id_for, 0)
}

fn part_to_jmap_inner(
    part: &ParsedPart,
    blob_id_for: &dyn Fn(&ParsedPart) -> Id,
    depth: usize,
) -> EmailBodyPart {
    let is_multipart = !part.children.is_empty();

    // Defense-in-depth: stop recursing once the multipart nesting exceeds
    // MAX_PART_DEPTH. The over-deep subtree is emitted as a structurally
    // typed-but-leaf EmailBodyPart so the JMAP wire response stays valid.
    let truncate_here = is_multipart && depth >= MAX_PART_DEPTH;

    let sub_parts = if is_multipart && !truncate_here {
        Some(
            part.children
                .iter()
                .map(|c| part_to_jmap_inner(c, blob_id_for, depth + 1))
                .collect(),
        )
    } else {
        None
    };

    // EmailBodyPart is #[non_exhaustive]; use Default + field mutation.
    let mut out = EmailBodyPart::default();
    out.part_id = (!is_multipart).then(|| part.part_id.clone());
    out.blob_id = (!is_multipart).then(|| blob_id_for(part));
    // size: pre-decoded byte length of the body (encoded size; exact for
    // identity/7bit/8bit, approximate for base64/QP).
    out.size = (!is_multipart).then_some(part.body_range.1 as u64);
    out.name = part.filename.clone();
    out.type_ = Some(part.content_type.clone());
    out.charset = part.charset.clone();
    out.disposition = part.disposition.clone();
    out.cid = part.cid.clone();
    out.sub_parts = sub_parts;
    // headers, language, location: require raw bytes; left as None/empty.
    out
}

/// Convert a [`DecodedBodyValue`] into an [`EmailBodyValue`].
///
/// This is a direct field rename; no logic.
pub fn body_value_to_jmap(val: DecodedBodyValue) -> EmailBodyValue {
    // EmailBodyValue is #[non_exhaustive]; use the provided constructor.
    let mut out = EmailBodyValue::new(val.value);
    out.is_encoding_problem = val.is_encoding_problem;
    out.is_truncated = val.is_truncated;
    out
}

/// Build the full JMAP body fields from a [`ParsedMessage`].
///
/// Converts the entire part tree and all RFC 8621 §4.1.4 body lists.
/// The `blob_id_for` closure is called once per non-multipart leaf part.
///
/// The returned [`JmapBodyFields::body_value_part_ids`] lists the part IDs
/// the caller must decode via [`mime_tree::decode_body_value`] to populate
/// `bodyValues` in the JMAP response.
///
/// # Depth bound
///
/// The recursion is bounded by [`MAX_PART_DEPTH`]. The bound applies to
/// `body_structure` and to each entry in `text_body` / `html_body` /
/// `attachments`. See the [`MAX_PART_DEPTH`] doc for rationale.
pub fn message_to_jmap_body(
    msg: &ParsedMessage,
    blob_id_for: impl Fn(&ParsedPart) -> Id,
) -> JmapBodyFields {
    let blob_id_for = &blob_id_for;

    let body_structure = part_to_jmap_inner(&msg.part_index, blob_id_for, 0);

    let text_body = msg
        .text_body
        .iter()
        .filter_map(|id| msg.part_index.find_by_id(id))
        .map(|p| part_to_jmap_inner(p, blob_id_for, 0))
        .collect();

    let html_body = msg
        .html_body
        .iter()
        .filter_map(|id| msg.part_index.find_by_id(id))
        .map(|p| part_to_jmap_inner(p, blob_id_for, 0))
        .collect();

    let attachments = msg
        .attachments
        .iter()
        .filter_map(|id| msg.part_index.find_by_id(id))
        .map(|p| part_to_jmap_inner(p, blob_id_for, 0))
        .collect();

    let mut body_value_part_ids = Vec::with_capacity(msg.text_body.len() + msg.html_body.len());
    body_value_part_ids.extend(msg.text_body.iter().cloned());
    body_value_part_ids.extend(msg.html_body.iter().cloned());

    JmapBodyFields {
        body_structure,
        text_body,
        html_body,
        attachments,
        preview: msg.preview.clone(),
        body_value_part_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mime_tree::{decode_body_value, parse};

    // Oracle: mime_tree::parse() on known-good RFC 5322 bytes.

    // --- body_value_to_jmap ---

    #[test]
    fn body_value_plain_maps_fields() {
        let dv = DecodedBodyValue {
            value: "hello".into(),
            is_truncated: false,
            is_encoding_problem: false,
        };
        let jv = body_value_to_jmap(dv);
        assert_eq!(jv.value, "hello");
        assert!(!jv.is_truncated);
        assert!(!jv.is_encoding_problem);
    }

    #[test]
    fn body_value_flags_preserved() {
        let dv = DecodedBodyValue {
            value: "x".into(),
            is_truncated: true,
            is_encoding_problem: true,
        };
        let jv = body_value_to_jmap(dv);
        assert!(jv.is_truncated);
        assert!(jv.is_encoding_problem);
    }

    // --- part_to_jmap: single text/plain message ---

    // Oracle: RFC 5322 minimal message with one text/plain part.
    const PLAIN_MSG: &[u8] = b"From: alice@example.com\r\n\
        Content-Type: text/plain; charset=utf-8\r\n\
        \r\n\
        Hello, world!\r\n";

    #[test]
    fn part_to_jmap_plain_part_id_and_type() {
        let msg = parse(PLAIN_MSG).unwrap();
        let root = &msg.part_index;
        let jpart = part_to_jmap(root, |p| Id::from(format!("b-{}", p.part_id)));

        assert_eq!(jpart.part_id.as_deref(), Some("1"));
        assert_eq!(jpart.type_.as_deref(), Some("text/plain"));
        assert_eq!(jpart.charset.as_deref(), Some("utf-8"));
        assert!(jpart.sub_parts.is_none());
    }

    #[test]
    fn part_to_jmap_plain_blob_id_assigned() {
        let msg = parse(PLAIN_MSG).unwrap();
        let jpart = part_to_jmap(&msg.part_index, |p| Id::from(format!("b-{}", p.part_id)));
        assert_eq!(jpart.blob_id.as_ref().map(|id| id.as_ref()), Some("b-1"));
    }

    #[test]
    fn part_to_jmap_plain_size_nonzero() {
        let msg = parse(PLAIN_MSG).unwrap();
        let jpart = part_to_jmap(&msg.part_index, |p| Id::from(p.part_id.clone()));
        assert!(
            jpart.size.unwrap_or(0) > 0,
            "size must be nonzero for non-empty body"
        );
    }

    // --- part_to_jmap: multipart message ---

    // Oracle: RFC 5322 multipart/alternative with text + html parts.
    const ALT_MSG: &[u8] = b"From: alice@example.com\r\n\
        Content-Type: multipart/alternative; boundary=\"b\"\r\n\
        \r\n\
        --b\r\n\
        Content-Type: text/plain; charset=utf-8\r\n\
        \r\n\
        Plain text\r\n\
        --b\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        \r\n\
        <p>HTML</p>\r\n\
        --b--\r\n";

    #[test]
    fn part_to_jmap_multipart_has_no_part_id() {
        let msg = parse(ALT_MSG).unwrap();
        let jpart = part_to_jmap(&msg.part_index, |p| Id::from(p.part_id.clone()));

        // Root is multipart — no partId, no blobId, no size.
        assert!(
            jpart.part_id.is_none(),
            "multipart root must have no partId"
        );
        assert!(
            jpart.blob_id.is_none(),
            "multipart root must have no blobId"
        );
        assert!(jpart.size.is_none(), "multipart root must have no size");
    }

    #[test]
    fn part_to_jmap_multipart_sub_parts_present() {
        let msg = parse(ALT_MSG).unwrap();
        let jpart = part_to_jmap(&msg.part_index, |p| Id::from(p.part_id.clone()));

        let subs = jpart
            .sub_parts
            .as_ref()
            .expect("multipart must have sub_parts");
        assert_eq!(subs.len(), 2, "two child parts expected");
        assert_eq!(subs[0].type_.as_deref(), Some("text/plain"));
        assert_eq!(subs[1].type_.as_deref(), Some("text/html"));
        // Children are leaves: they have partIds and no sub_parts.
        assert!(subs[0].part_id.is_some());
        assert!(subs[1].part_id.is_some());
        assert!(subs[0].sub_parts.is_none());
        assert!(subs[1].sub_parts.is_none());
    }

    // --- message_to_jmap_body: plain text ---

    #[test]
    fn message_to_jmap_body_plain_text_body() {
        let msg = parse(PLAIN_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        assert_eq!(fields.text_body.len(), 1);
        assert_eq!(fields.text_body[0].type_.as_deref(), Some("text/plain"));
        // RFC 8621 §4.1.4: when no HTML part exists, html_body mirrors text_body.
        assert_eq!(
            fields.html_body.len(),
            1,
            "html_body must mirror text_body when no HTML part present"
        );
        assert!(fields.attachments.is_empty());
    }

    #[test]
    fn message_to_jmap_body_plain_preview() {
        let msg = parse(PLAIN_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        // mime-tree computes the preview; we just pass it through.
        assert!(
            fields.preview.is_some(),
            "plain text message should have a preview"
        );
        let preview = fields.preview.unwrap();
        assert!(
            preview.contains("Hello"),
            "preview should contain body text"
        );
    }

    #[test]
    fn message_to_jmap_body_plain_body_value_part_ids() {
        let msg = parse(PLAIN_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        // text_body part IDs should be in body_value_part_ids.
        assert!(
            !fields.body_value_part_ids.is_empty(),
            "plain text must have at least one body value part ID"
        );
        for id in &fields.text_body {
            let pid = id
                .part_id
                .as_deref()
                .expect("text_body part must have partId");
            assert!(
                fields.body_value_part_ids.contains(&pid.to_owned()),
                "text_body partId {pid} must appear in body_value_part_ids"
            );
        }
    }

    // --- message_to_jmap_body: multipart/alternative ---

    #[test]
    fn message_to_jmap_body_alt_text_and_html() {
        let msg = parse(ALT_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        assert_eq!(fields.text_body.len(), 1);
        assert_eq!(fields.html_body.len(), 1);
        assert!(fields.attachments.is_empty());
        assert_eq!(fields.text_body[0].type_.as_deref(), Some("text/plain"));
        assert_eq!(fields.html_body[0].type_.as_deref(), Some("text/html"));
    }

    #[test]
    fn message_to_jmap_body_alt_body_value_part_ids_both() {
        let msg = parse(ALT_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        // Both text and html part IDs must be present.
        for part in fields.text_body.iter().chain(fields.html_body.iter()) {
            let pid = part.part_id.as_deref().expect("must have partId");
            assert!(
                fields.body_value_part_ids.contains(&pid.to_owned()),
                "partId {pid} must be in body_value_part_ids"
            );
        }
    }

    // --- decode round-trip via body_value_to_jmap ---

    #[test]
    fn decode_roundtrip_plain() {
        let msg = parse(PLAIN_MSG).unwrap();
        let part = msg.part_index.find_by_id("1").unwrap();
        let decoded = decode_body_value(PLAIN_MSG, part, None).unwrap();
        let jval = body_value_to_jmap(decoded);

        assert!(jval.value.contains("Hello, world!"));
        assert!(!jval.is_truncated);
        assert!(!jval.is_encoding_problem);
    }

    #[test]
    fn decode_roundtrip_with_truncation() {
        let msg = parse(PLAIN_MSG).unwrap();
        let part = msg.part_index.find_by_id("1").unwrap();
        // Truncate at 5 bytes — body is longer, so is_truncated must be true.
        let decoded = decode_body_value(PLAIN_MSG, part, Some(5)).unwrap();
        let jval = body_value_to_jmap(decoded);

        assert!(
            jval.is_truncated,
            "value must be marked truncated when max_bytes hit"
        );
        assert!(jval.value.len() <= 5);
    }

    // --- attachment message ---

    // Oracle: RFC 5322 message with text/plain body and an attachment.
    const ATTACH_MSG: &[u8] = b"From: alice@example.com\r\n\
        Content-Type: multipart/mixed; boundary=\"m\"\r\n\
        \r\n\
        --m\r\n\
        Content-Type: text/plain; charset=utf-8\r\n\
        \r\n\
        See attached\r\n\
        --m\r\n\
        Content-Type: application/octet-stream\r\n\
        Content-Disposition: attachment; filename=\"file.bin\"\r\n\
        \r\n\
        BINARYDATA\r\n\
        --m--\r\n";

    #[test]
    fn message_to_jmap_body_attachment_classified() {
        let msg = parse(ATTACH_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        assert_eq!(fields.text_body.len(), 1);
        assert_eq!(fields.attachments.len(), 1);
        assert_eq!(
            fields.attachments[0].type_.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(
            fields.attachments[0].disposition.as_deref(),
            Some("attachment")
        );
        assert_eq!(fields.attachments[0].name.as_deref(), Some("file.bin"));
    }

    #[test]
    fn message_to_jmap_body_attachment_not_in_body_values() {
        let msg = parse(ATTACH_MSG).unwrap();
        let fields = message_to_jmap_body(&msg, |p| Id::from(p.part_id.clone()));

        // Attachments should NOT be in body_value_part_ids.
        for attach in &fields.attachments {
            if let Some(pid) = &attach.part_id {
                assert!(
                    !fields.body_value_part_ids.contains(pid),
                    "attachment partId {pid} must not be in body_value_part_ids"
                );
            }
        }
    }

    // --- depth-bound (DoS defense) tests ---

    use mime_tree::TransferEncoding;

    // Build a deeply-nested multipart `ParsedPart` chain of length `depth`,
    // terminated by a single text/plain leaf at the bottom. This bypasses
    // `mime_tree::parse` so the test can exercise depths that the upstream
    // parser cannot itself handle (its recursion is currently unbounded).
    fn make_deep_multipart_chain(depth: usize) -> ParsedPart {
        // Innermost leaf.
        let mut node = ParsedPart {
            part_id: "1".to_owned(),
            content_type: "text/plain".to_owned(),
            charset: Some("utf-8".to_owned()),
            transfer_encoding: TransferEncoding::Identity,
            disposition: None,
            filename: None,
            cid: None,
            header_range: (0, 0),
            body_range: (0, 0),
            children: Vec::new(),
            is_encoding_problem: false,
        };
        for level in (0..depth).rev() {
            node = ParsedPart {
                part_id: format!("L{level}"),
                content_type: "multipart/mixed".to_owned(),
                charset: None,
                transfer_encoding: TransferEncoding::Identity,
                disposition: None,
                filename: None,
                cid: None,
                header_range: (0, 0),
                body_range: (0, 0),
                children: vec![node],
                is_encoding_problem: false,
            };
        }
        node
    }

    // Walk an EmailBodyPart sub_parts chain and return its depth, counting
    // multipart wrappers. Stops when sub_parts is None or empty.
    fn email_body_chain_depth(root: &EmailBodyPart) -> usize {
        let mut depth = 0usize;
        let mut cur = root;
        while let Some(subs) = cur.sub_parts.as_ref().filter(|s| !s.is_empty()) {
            depth += 1;
            cur = &subs[0];
        }
        depth
    }

    #[test]
    fn part_to_jmap_truncates_over_deep_multipart() {
        // Build a chain MAX_PART_DEPTH + 4 multiparts deep. The adapter
        // must convert it without recursing past MAX_PART_DEPTH and must
        // not stack-overflow.
        let part = make_deep_multipart_chain(MAX_PART_DEPTH + 4);
        let jpart = part_to_jmap(&part, |p| Id::from(p.part_id.clone()));
        // The emitted EmailBodyPart chain reflects the multipart wrappers
        // walked before the bound kicks in. The wrapper at depth
        // MAX_PART_DEPTH is emitted as an opaque leaf (sub_parts = None),
        // so the visible chain length is exactly MAX_PART_DEPTH.
        let observed = email_body_chain_depth(&jpart);
        assert_eq!(
            observed, MAX_PART_DEPTH,
            "adapter should stop recursing at MAX_PART_DEPTH",
        );
    }

    #[test]
    fn part_to_jmap_preserves_full_tree_below_bound() {
        // A chain shallower than the bound must round-trip without
        // truncation.
        let depth = 5usize;
        assert!(depth < MAX_PART_DEPTH);
        let part = make_deep_multipart_chain(depth);
        let jpart = part_to_jmap(&part, |p| Id::from(p.part_id.clone()));
        let observed = email_body_chain_depth(&jpart);
        assert_eq!(
            observed, depth,
            "shallow trees must be preserved without truncation",
        );
    }

    #[test]
    fn part_to_jmap_handles_one_thousand_levels_without_panic() {
        // Worst-case adversary: 1000-level-deep tree. Adapter must return
        // in bounded stack regardless. The visible-chain length is
        // MAX_PART_DEPTH; depths beyond that are collapsed to an opaque
        // leaf.
        let part = make_deep_multipart_chain(1000);
        let jpart = part_to_jmap(&part, |p| Id::from(p.part_id.clone()));
        assert_eq!(email_body_chain_depth(&jpart), MAX_PART_DEPTH);
    }

    #[test]
    fn truncated_multipart_emits_opaque_leaf() {
        // The first part whose depth equals MAX_PART_DEPTH must come back
        // as a multipart-typed EmailBodyPart with sub_parts == None — the
        // structural signal that the tree was truncated.
        let part = make_deep_multipart_chain(MAX_PART_DEPTH + 2);
        let jpart = part_to_jmap(&part, |p| Id::from(p.part_id.clone()));

        // Walk down to the deepest emitted level.
        let mut cur = &jpart;
        for _ in 0..MAX_PART_DEPTH - 1 {
            cur = &cur
                .sub_parts
                .as_ref()
                .expect("interior nodes must have sub_parts")[0];
        }
        // `cur` is the multipart at depth MAX_PART_DEPTH - 1. Its single
        // child is the one at depth MAX_PART_DEPTH, which should be the
        // truncation marker: multipart type, sub_parts None.
        let truncated = &cur
            .sub_parts
            .as_ref()
            .expect("level MAX_PART_DEPTH - 1 still has sub_parts")[0];
        assert_eq!(truncated.type_.as_deref(), Some("multipart/mixed"));
        assert!(
            truncated.sub_parts.is_none(),
            "over-depth multipart must be emitted as opaque leaf (sub_parts = None)",
        );
        // Multipart parts carry no partId / blobId / size, even when
        // they are the truncation marker.
        assert!(truncated.part_id.is_none());
        assert!(truncated.blob_id.is_none());
        assert!(truncated.size.is_none());
    }
}

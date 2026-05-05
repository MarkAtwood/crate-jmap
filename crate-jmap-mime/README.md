# jmap-mime

MIME-to-JMAP adapter: converts `mime_tree` parsed output to `jmap-mail-types`
body types. Used by `jmap-mail-server` backends that parse raw RFC 5322
messages.

All MIME parsing lives in `mime_tree`. This crate only maps field names and
assembles the RFC 8621 §4.1.4 body structure lists. No parsing logic,
content-type matching, or encoding/decoding logic belongs here.

## What it does

Three public functions cover the full conversion surface:

### `part_to_jmap(part, blob_id_for) -> EmailBodyPart`

Converts a `mime_tree::ParsedPart` (and all its children, recursively) to a
`jmap_mail_types::email::EmailBodyPart` tree.

- The `blob_id_for` closure is called once per non-multipart leaf to assign a
  `blobId`; the storage layer decides how to construct these IDs.
- Multipart parts receive `None` for `part_id`, `blob_id`, and `size`.
- The `headers`, `language`, and `location` fields are not populated because
  they require access to the raw message bytes; callers that need per-part raw
  headers can extract them from `part.header_range` and the original `&[u8]`.

### `body_value_to_jmap(val) -> EmailBodyValue`

Converts a `mime_tree::DecodedBodyValue` to a
`jmap_mail_types::email::EmailBodyValue`. This is a direct field rename with no
logic.

### `message_to_jmap_body(msg, blob_id_for) -> JmapBodyFields`

Builds the complete RFC 8621 §4.1.4 body structure from a
`mime_tree::ParsedMessage`. Returns a `JmapBodyFields` struct with:

- `body_structure` — full MIME part tree (`bodyStructure`)
- `text_body` — `Vec<EmailBodyPart>` of `text/plain` display parts (`textBody`)
- `html_body` — `Vec<EmailBodyPart>` of `text/html` display parts (`htmlBody`)
- `attachments` — non-inline, non-display parts (`attachments`)
- `preview` — short plaintext preview, as computed by `mime_tree` (`preview`)
- `body_value_part_ids` — union of `textBody` and `htmlBody` part IDs; the
  caller should decode each via `mime_tree::decode_body_value` and insert the
  resulting `EmailBodyValue`s into the `bodyValues` map of the JMAP `Email`
  response.

## Usage

A typical `MailBackend::parse_email` implementation:

```rust
use jmap_mime::{message_to_jmap_body, body_value_to_jmap};
use jmap_types::Id;
use mime_tree::{parse, decode_body_value};

fn parse_email(raw: &[u8]) -> jmap_mail_types::Email {
    let msg = parse(raw).expect("parse failed");

    // Build body structure. The closure assigns blob IDs to leaf parts.
    let fields = message_to_jmap_body(&msg, |part| {
        Id::from(format!("blob-{}", part.part_id))
    });

    // Decode body values on demand.
    let mut body_values = std::collections::HashMap::new();
    for part_id in &fields.body_value_part_ids {
        if let Some(part) = msg.part_index.find_by_id(part_id) {
            if let Ok(decoded) = decode_body_value(raw, part, Some(8192)) {
                body_values.insert(part_id.clone(), body_value_to_jmap(decoded));
            }
        }
    }

    // Populate the Email struct with body fields.
    let mut email = jmap_mail_types::Email::default();
    email.body_structure = Some(fields.body_structure);
    email.text_body = Some(fields.text_body);
    email.html_body = Some(fields.html_body);
    email.attachments = Some(fields.attachments);
    email.preview = fields.preview;
    email.body_values = Some(body_values);
    email
}
```

## Known Limitations

- **RFC 2047 encoded-word decoding is `mime_tree`'s responsibility.** Headers
  containing `=?UTF-8?B?...?=` or similar encoded-word sequences (e.g. encoded
  subject lines) are decoded by `mime_tree`, not this crate. Ensure the
  `mime_tree` version in use supports the encodings your server receives.
- **Body value decoding (quoted-printable, base64) is `mime_tree`'s
  responsibility.** This crate receives already-decoded `DecodedBodyValue`
  strings from `mime_tree::decode_body_value` and maps them to `EmailBodyValue`
  fields. Encoding errors surface as `is_encoding_problem: true` flags set by
  `mime_tree` before this crate sees them.
- **No multipart/alternative part selection.** `message_to_jmap_body` returns
  all `text/plain` and `text/html` parts in `text_body` and `html_body`
  respectively; it does not choose between alternatives. RFC 8621 §4.1.4
  specifies that the server returns all parts and the client selects. Callers
  that must present a single body to a user must pick themselves.
- **No S/MIME or PGP/MIME signature verification.** Signed and encrypted
  multipart types (`multipart/signed`, `multipart/encrypted`) are treated as
  ordinary multipart containers. Signature verification and decryption are out
  of scope for this crate.

## Crate family

```
jmap-types
    └── jmap-mail-types      EmailBodyPart, EmailBodyValue, etc.
            └── jmap-mime    ← this crate
```

`jmap-mime` also depends on `mime_tree` (external) for the source types.

## References

- **[RFC 8621]** — JMAP for Mail — body structure specification (§4.1.4)
- **[RFC 5322]** — Internet Message Format — overall message structure
- **[RFC 2045]** — MIME Part One — body structure and encoding types
- **[RFC 2047]** — MIME Part Three — encoded words in headers

[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 5322]: https://www.rfc-editor.org/rfc/rfc5322
[RFC 2045]: https://www.rfc-editor.org/rfc/rfc2045
[RFC 2047]: https://www.rfc-editor.org/rfc/rfc2047

## License

MIT OR Apache-2.0

# jmap-mime

MIME-to-JMAP adapter: converts `mime_tree` parsed output to `jmap-mail-types`
body types. Used by `jmap-mail-server` backends that parse raw RFC 5322
messages.

All MIME parsing lives in `mime_tree`. This crate only maps field names and
assembles the RFC 8621 §4.1.4 body structure lists. No parsing logic,
content-type matching, or encoding/decoding logic belongs here.

## What it is

Three public functions cover the full conversion surface:

### `part_to_jmap(part, blob_id_for) -> EmailBodyPart`

Converts a `mime_tree::ParsedPart` (and all its children, recursively) to a
`jmap_mail_types::email::EmailBodyPart` tree.

- The `blob_id_for` closure is called once per non-multipart leaf to assign a
  `blobId`; the storage layer decides how to construct these IDs. **The
  closure MUST be idempotent on `part.part_id`** — repeated calls for parts
  with the same `part_id` must return the same `Id`. Counter-based or
  remote-allocate schemes that return a fresh `Id` on every call produce a
  silent wire-format defect (the same logical leaf appears under multiple
  distinct `blobId`s in the response). Pure functions of `part.part_id` or
  of the part bytes satisfy the contract trivially. See the "Gotchas"
  section for the related call-frequency caveat in `message_to_jmap_body`.
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
- `body_value_part_ids` — concatenation of `textBody` and `htmlBody` part
  IDs (in that order, with no dedup); the caller should decode each via
  `mime_tree::decode_body_value` and insert the resulting `EmailBodyValue`s
  into the `bodyValues` map of the JMAP `Email` response. Note: for
  plain-text-only messages where `htmlBody` mirrors `textBody` per RFC 8621
  §4.1.4, the same `part_id` appears twice in this list. A caller that
  builds a `HashMap` keyed by `part_id` silently dedups, but a caller that
  preserves order in a `Vec` or that emits each entry directly to a JSON
  sink must dedup at the call site.

## What it's for

`jmap-mime` is the thin adapter between `mime-tree` (the RFC 5322 / 2045
MIME parser) and `jmap-mail-types` (the RFC 8621 JMAP wire shape). Consumed
by `jmap-mail-server` backends that parse raw `.eml` messages, and by any
other consumer needing to convert a `mime_tree::ParsedMessage` or
`ParsedPart` into JMAP `EmailBodyPart` / `EmailBodyValue` shapes. The crate
exists so that parsing logic stays in `mime-tree` and JMAP wire shapes stay
in `jmap-mail-types`; this crate is pure field-rename glue.

## How to use

A typical `MailBackend::parse_email` implementation. A real backend MUST
surface parse/decode errors as JMAP method errors per RFC 8620 §3.6.2 —
never panic on attacker-controlled input.

```rust
use jmap_mime::{message_to_jmap_body, body_value_to_jmap};
use jmap_types::Id;
use mime_tree::{parse, decode_body_value};

fn parse_email(raw: &[u8]) -> Result<jmap_mail_types::Email, Box<dyn std::error::Error>> {
    let msg = parse(raw)?;

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
    Ok(email)
}
```

## Examples

A runnable end-to-end demo lives in [`examples/parse_eml.rs`](examples/parse_eml.rs):

```sh
cargo run --example parse_eml -p jmap-mime
```

It parses a hand-supplied multipart/alternative .eml fixture, maps it into
`JmapBodyFields`, and prints the `textBody` / `htmlBody` / `attachments`
shape plus a decoded `bodyValues` entry for the first text part.

## How it works

- Adapter-only design — no parsing logic, no content-type matching, no
  encoding/decoding logic lives here. Every function should be obvious
  field mapping; if a future change wants to add a `match content_type`
  or a parsing loop it belongs in `mime-tree` instead.
- The three `*_to_jmap*` entry points cover the full conversion surface:
  `part_to_jmap` for the recursive `ParsedPart` → `EmailBodyPart` walk,
  `body_value_to_jmap` for `DecodedBodyValue` → `EmailBodyValue` field
  renaming, and `message_to_jmap_body` for assembling the full RFC 8621
  §4.1.4 `JmapBodyFields` (bodyStructure / textBody / htmlBody /
  attachments / preview / body_value_part_ids).
- Multipart vs leaf-part handling: leaves get a `blobId` from the
  caller-supplied `blob_id_for` closure; multipart parts get `None` for
  `part_id`, `blob_id`, and `size` per RFC 8621.
- Pure conversion: no I/O, no async, no allocator opinions beyond what
  `mime-tree` and `jmap-mail-types` already pull in.

## Gotchas

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
- **Blob ID construction is the consumer's responsibility — and must be
  unguessable if access control depends on it.** The closure passed to
  `message_to_jmap_body` / `part_to_jmap` receives a `&ParsedPart` whose
  `part_id` is a deterministic dotted IMAP path (`"1"`, `"2.1"`, `"3.4.7"`).
  The illustrative `format!("blob-{part_id}")` form used throughout the
  examples is **for demonstration only** — its output is enumerable across
  all mailboxes, so a consumer that treats blobId existence as authorization
  will leak blobs across users. Production backends should either
  (a) make `blobId` content-addressed (`SHA-256(part_bytes)`, optionally
  surfaced via `urn:ietf:params:jmap:cid` per draft-atwood-jmap-cid-00 /
  `jmap-cid-types`), (b) make `blobId` a random opaque token bound to the
  (account, part) pair in the storage layer, or (c) enforce access control
  on `(caller, blobId)` rather than on `blobId` alone.
- **`blob_id_for` is invoked once per appearance, not once per unique leaf.**
  `message_to_jmap_body` walks `body_structure`, `text_body`, `html_body`,
  and `attachments` independently, calling the closure on every visited
  leaf. A plain-only message with one `text/plain` part triggers three
  invocations on that one part (once from `body_structure`, once from
  `text_body`, once from `html_body` — RFC 8621 §4.1.4 has `html_body`
  mirror `text_body` when no HTML part exists). The closure MUST be
  idempotent on `part.part_id` (see the [`part_to_jmap`](#part_to_jmappart-blob_id_for---emailbodypart)
  section). Non-idempotent closures (counter-based, remote-allocate)
  produce a silent wire-format defect.
- **Recursion in this adapter is bounded by `jmap_mime::MAX_PART_DEPTH`.**
  A multipart subtree deeper than the bound is emitted as an opaque leaf
  (a multipart-typed `EmailBodyPart` with `sub_parts = None`). This is
  defense-in-depth against deeply-nested `multipart/*` framing supplied by
  hostile senders. The bound applies to `part_to_jmap` and to every entry
  point in `message_to_jmap_body` (`bodyStructure`, `textBody`, `htmlBody`,
  `attachments`).
- **Upstream MIME parsing (`mime_tree::parse`) is currently unbounded.**
  The adapter's own depth bound (above) does NOT protect the upstream
  parser from running first on the same hostile input. Consumers MUST
  bound raw message size upstream — typical SMTP MaxMessageSize caps
  (25–50 MB) are not sufficient on their own, because a deeply-nested
  multipart message can pack tens of thousands of nesting levels into
  well under 1 MB. Consumers that accept arbitrary RFC 5322 bytes from
  the public internet should either (a) cap message size below the depth
  at which `mime_tree::parse` stack-overflows on their platform,
  (b) parse on a worker thread with a controlled stack size and a
  supervisor that restarts on overflow, or (c) wait for `mime_tree` to
  add its own recursion bound.

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

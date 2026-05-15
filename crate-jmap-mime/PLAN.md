# jmap-mime — Design Plan

## Role

Thin adapter crate. Converts `mime-tree` output types to `jmap-mail-types` types.

No parsing logic lives here — every function is obvious field mapping. If a function requires
a loop, a match on `Content-Type`, or any RFC-specific decision, that logic belongs in
`mime-tree`, not here.

Used by: `jmap-mail-server` (RFC 8621 method handlers).

## Dependency Chain

```
mime-tree  →  jmap-mime  →  jmap-mail-types  (+ jmap-types for Id)
```

`mime-tree` is pinned to the crates.io registry release `"0.1"` in `Cargo.toml`.
The local workspace contains a newer development checkout of `mime-tree` at
`~/PROJECT/MIME/mime-tree/` which is NOT a workspace member here.

**Known version drift (JMAP-dcgj.4):** The local checkout may expose API surface that
differs from the `0.1` registry release. Before releasing `jmap-mime`, verify that
`cargo test -p jmap-mime` passes against the published `mime-tree 0.1` crate. Do not add
the local path dep to this workspace without checking for API divergence.

## Public API

### `JmapBodyFields` (struct)

Return value of `message_to_jmap_body`. Packages all RFC 8621 §4.1.4 body fields together
so callers receive everything in a single allocation.

| Field | RFC 8621 property | Type |
|---|---|---|
| `body_structure` | `bodyStructure` | `EmailBodyPart` |
| `text_body` | `textBody` | `Vec<EmailBodyPart>` |
| `html_body` | `htmlBody` | `Vec<EmailBodyPart>` |
| `attachments` | `attachments` | `Vec<EmailBodyPart>` |
| `preview` | `preview` | `Option<String>` |
| `body_value_part_ids` | *(caller hint, not a wire field)* | `Vec<String>` |

`body_value_part_ids` is the union of text-body and html-body part IDs. It is not sent to
the client; callers use it to know which part IDs to decode and include in `bodyValues`.

---

### `part_to_jmap(part, blob_id_for) -> EmailBodyPart`

Converts a single `ParsedPart` node (and its entire subtree) to `EmailBodyPart`.

`blob_id_for` is a caller-supplied closure called once per non-multipart leaf; the storage
layer decides the actual ID format, keeping this crate storage-agnostic.

The function accepts `impl Fn` at the public boundary (zero-cost, monomorphised) and
immediately borrows it as `&dyn Fn` before passing to the recursive inner function. This
avoids infinite monomorphisation in the recursion without sacrificing call-site performance.

**Field mapping (leaf parts):**

| `ParsedPart` | `EmailBodyPart` | Notes |
|---|---|---|
| `part.part_id` | `part_id` | Cloned |
| `blob_id_for(part)` | `blob_id` | Caller-assigned |
| `part.body_range.1` | `size` | Encoded byte length; see Known Limitations |
| `part.filename` | `name` | Optional, cloned |
| `part.content_type` | `type_` | Wrapped in `Some` |
| `part.charset` | `charset` | Optional, cloned |
| `part.disposition` | `disposition` | Optional, cloned |
| `part.cid` | `cid` | Optional, cloned |

**For multipart parts:** `part_id`, `blob_id`, and `size` are all `None`. `sub_parts` is
populated with the recursively converted children.

**Fields NOT populated:**

| `EmailBodyPart` field | Reason |
|---|---|
| `headers` | Requires raw message bytes (`part.header_range`), which are not passed in. Callers that need per-part raw headers must extract them from `part.header_range` and the original `&[u8]`. |
| `language` | Same: requires raw bytes. |
| `location` | Same: requires raw bytes. |

This is a deliberate API boundary. These fields cannot be added without changing the function
signature to accept `&[u8]`. Tracked as **JMAP-dcgj.5**.

`EmailBodyPart` is `#[non_exhaustive]`; construction uses `Default::default()` + field
mutation to survive future field additions in `jmap-mail-types`.

---

### `body_value_to_jmap(val: DecodedBodyValue) -> EmailBodyValue`

Direct field rename, no logic:

| `DecodedBodyValue` | `EmailBodyValue` |
|---|---|
| `val.value` | `value` (via constructor) |
| `val.is_encoding_problem` | `is_encoding_problem` |
| `val.is_truncated` | `is_truncated` |

`val` is consumed by value; the `String` is moved without cloning.

`EmailBodyValue` is `#[non_exhaustive]`; construction uses `EmailBodyValue::new(val.value)`
then field mutation.

Note: `ParsedPart` carries `is_encoding_problem` as a structural parse-time flag. It has no
corresponding `EmailBodyPart` field in RFC 8621, so it is dropped at the `part_to_jmap`
boundary. `DecodedBodyValue::is_encoding_problem` is a separate, content-level flag that
is preserved here.

---

### `message_to_jmap_body(msg, blob_id_for) -> JmapBodyFields`

Builds all RFC 8621 §4.1.4 body fields from a fully-parsed `ParsedMessage`.

- `body_structure`: full recursive tree via `part_to_jmap_inner`.
- `text_body`, `html_body`, `attachments`: `mime_tree` pre-classifies parts into these
  lists; this function maps each listed ID back to its `ParsedPart` via `find_by_id` and
  converts it. Parts not found in the index are silently skipped (`filter_map`) — this
  trusts that `mime_tree::parse` produces a consistent `ParsedMessage`.
- `preview`: passed through from `msg.preview` unchanged; computed by `mime_tree`.
- `body_value_part_ids`: text IDs first, then html IDs; `Vec::with_capacity` avoids
  reallocation.

The generic `blob_id_for` is immediately borrowed as `&blob_id_for` so all four
`part_to_jmap_inner` calls share one `&dyn Fn` without cloning.

## Known Limitations

### 1. `size` uses encoded length, not decoded length (JMAP-dcgj.3)

RFC 8621 §4.1.4 specifies:
> "The size, in octets, of the raw data **after content transfer decoding**."

`part_to_jmap` currently sets `size` from `part.body_range.1`, which is the on-wire
(encoded) byte length. For identity/7bit/8bit parts this is exact. For base64-encoded parts
the decoded size is approximately 75 % of the encoded size; for quoted-printable it varies.

This is a known spec deviation. Fixing it requires `mime_tree` to expose the decoded size or
requires re-decoding the body during the mapping step, which violates the "thin adapter"
constraint. Tracked as **JMAP-dcgj.3**.

### 2. `headers`, `language`, `location` not populated (JMAP-dcgj.5)

`part_to_jmap` does not accept raw message bytes, so per-part header extraction is not
possible here. Callers that need these fields must do the raw-byte extraction themselves
using `part.header_range`.

A future signature change (`part_to_jmap(part, raw: &[u8], blob_id_for) -> EmailBodyPart`)
would enable this but is an API break. Tracked as **JMAP-dcgj.5**.

### 3. `mime-tree` version drift (JMAP-dcgj.4)

The pinned registry dep is `mime-tree = "0.1"`. The local workspace checkout at
`~/PROJECT/MIME/mime-tree/` is a development version with a potentially diverged API.
Verify compatibility before release. Tracked as **JMAP-dcgj.4**.

### 4. `is_encoding_problem` on `ParsedPart` is silently dropped

`ParsedPart::is_encoding_problem` flags a structural parse-time error in the MIME tree.
RFC 8621 has no corresponding `EmailBodyPart` field, so the flag is silently discarded at
the `part_to_jmap` boundary. This is correct per spec. The content-level variant
(`DecodedBodyValue::is_encoding_problem`) is preserved by `body_value_to_jmap`.

### 5. `filter_map` on ID lookups silently skips missing parts

If `mime_tree` produces a `ParsedMessage` where a `text_body`, `html_body`, or `attachments`
part ID has no matching entry in `part_index`, that part is silently dropped. This trusts
`mime_tree`'s internal consistency. A corrupted or partially-parsed message would silently
yield shorter-than-expected body lists with no error signal.

### 6. `body_value_part_ids` is concatenation, not union (JMAP-t307.10)

`message_to_jmap_body` returns `body_value_part_ids` as the plain concatenation of
`mime_tree`'s `text_body` and `html_body` ID lists — **no dedup**. The two lists are NOT
disjoint by RFC 8621 §4.1.4 design: when no HTML part exists, `html_body` mirrors
`text_body`, so the same `part_id` appears twice in the output. Callers that build a
`HashMap` keyed by `part_id` silently dedup; callers that preserve order in a `Vec` or that
emit each entry directly to a JSON sink must dedup at the call site. README and rustdoc
document this.

### 7. Recursion depth bound (JMAP-t307.15)

`part_to_jmap_inner` recursion is bounded by the public constant `MAX_PART_DEPTH = 64`. A
multipart part nested deeper than the bound is emitted as an opaque leaf (a multipart-typed
`EmailBodyPart` with `sub_parts = None`) rather than recursing further. This is
defense-in-depth against deeply-nested `multipart/*` framing from hostile SMTP senders;
without the bound the adapter would stack-overflow on inputs with thousands of nesting
levels.

The bound applies to every entry into the recursion: `part_to_jmap` and each list walked by
`message_to_jmap_body` (`bodyStructure`, `text_body`, `html_body`, `attachments`).

Note: `mime_tree::parse` and `mime_tree::ParsedPart::find_by_id` are themselves unbounded
in `mime-tree 0.3.0`. The adapter's bound protects the adapter's own recursion; consumers
must still bound raw message size upstream to obtain total-message safety. README "Gotchas"
documents this.

## Test Coverage

16 unit tests + 1 doc-test in `src/lib.rs`. All tests use RFC 5322 byte literals as
independent oracles — no test uses the code under test as its own oracle.

Three fixtures: `PLAIN_MSG` (single text/plain), `ALT_MSG` (multipart/alternative with
text + html), `ATTACH_MSG` (multipart/mixed with text + attachment).

| # | Test name | Covers |
|---|---|---|
| 1 | `body_value_plain_maps_fields` | All three `body_value_to_jmap` fields; both flags false |
| 2 | `body_value_flags_preserved` | Both booleans preserved when true |
| 3 | `part_to_jmap_plain_part_id_and_type` | Leaf part_id, type_, charset; sub_parts=None |
| 4 | `part_to_jmap_plain_blob_id_assigned` | blob_id_for closure result in blob_id |
| 5 | `part_to_jmap_plain_size_nonzero` | size is Some and non-zero for non-empty body |
| 6 | `part_to_jmap_multipart_has_no_part_id` | Multipart root: part_id/blob_id/size all None |
| 7 | `part_to_jmap_multipart_sub_parts_present` | Multipart root has 2 children; children are leaves |
| 8 | `message_to_jmap_body_plain_text_body` | Plain msg: text_body=1, html_body mirrors text_body, attachments=0 |
| 9 | `message_to_jmap_body_plain_preview` | preview is Some and contains "Hello" |
| 10 | `message_to_jmap_body_plain_body_value_part_ids` | body_value_part_ids non-empty; text_body part_ids present |
| 11 | `message_to_jmap_body_alt_text_and_html` | Alt msg: text_body=1 plain, html_body=1 html, attachments=0 |
| 12 | `message_to_jmap_body_alt_body_value_part_ids_both` | Both text and html part_ids in body_value_part_ids |
| 13 | `decode_roundtrip_plain` | Full mime_tree decode + body_value_to_jmap: value="Hello, world!"; flags false |
| 14 | `decode_roundtrip_with_truncation` | Truncation at 5 bytes: is_truncated=true; value.len()≤5 |
| 15 | `message_to_jmap_body_attachment_classified` | Attachment: text_body=1, attachments=1, correct type/disposition/name |
| 16 | `message_to_jmap_body_attachment_not_in_body_values` | Attachment part_id NOT in body_value_part_ids |

## Status

All three public functions fully implemented. 16 unit tests + 1 doc-test pass.
`cargo clippy -p jmap-mime -- -D warnings` clean. No stubs, no TODO/FIXME, no dead code.

Open gaps tracked as Beads issues and will not be fixed without a separate bead:
- **JMAP-dcgj.3** — size uses encoded length (spec deviation)
- **JMAP-dcgj.4** — mime-tree version drift vs. local workspace copy
- **JMAP-dcgj.5** — headers/language/location API design

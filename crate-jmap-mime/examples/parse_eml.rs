//! Demonstrate the `mime-tree` → `jmap-mime` → `jmap-mail-types` pipeline.
//!
//! Parses a hand-supplied multipart/alternative .eml fixture, maps it into
//! `JmapBodyFields`, and prints the resulting `textBody` / `htmlBody`
//! summary plus a decoded `bodyValues` entry for the first text part.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example parse_eml -p jmap-mime
//! ```

use jmap_mime::{body_value_to_jmap, message_to_jmap_body};
use jmap_types::Id;
use mime_tree::{decode_body_value, parse};

const RAW: &[u8] = b"From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: Hello\r\n\
Content-Type: multipart/alternative; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello, world!\r\n\
--b\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Hello, <b>world</b>!</p>\r\n\
--b--\r\n";

fn main() {
    let msg = parse(RAW).expect("MIME parse failed");

    // Storage layer would assign real blob IDs; the example uses a counter.
    let fields = message_to_jmap_body(&msg, |part| Id::from(format!("blob-{}", part.part_id)));

    println!("textBody parts: {}", fields.text_body.len());
    println!("htmlBody parts: {}", fields.html_body.len());
    println!("attachments:    {}", fields.attachments.len());
    println!("preview:        {:?}", fields.preview);

    // Decode and print the first text body value.
    if let Some(first_text_id) = fields.body_value_part_ids.first() {
        let part = msg
            .part_index
            .find_by_id(first_text_id)
            .expect("part must exist");
        let decoded = decode_body_value(RAW, part, Some(8192)).expect("decode failed");
        let jmap_val = body_value_to_jmap(decoded);
        println!("first textBody value: {:?}", jmap_val.value);
    }
}

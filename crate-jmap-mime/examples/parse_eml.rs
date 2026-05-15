//! Demonstrate the `mime-tree` → `jmap-mime` → `jmap-mail-types` pipeline.
//!
//! Parses a hand-supplied multipart/alternative .eml fixture, maps it into
//! `JmapBodyFields`, and prints the resulting `textBody` / `htmlBody`
//! summary plus a decoded `bodyValues` entry for the first text part.
//!
//! A real `MailBackend::parse_email` implementation MUST surface
//! `mime_tree::parse` and `decode_body_value` errors as JMAP method
//! errors (e.g. `invalidArguments`, `serverFail`) per RFC 8620 §3.6.2 —
//! never panic. This example uses `?` and a `Result` return from `main`
//! so that pattern reads correctly when copied.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example parse_eml -p jmap-mime
//! ```

use std::error::Error;

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

fn main() -> Result<(), Box<dyn Error>> {
    let msg = parse(RAW)?;

    // Storage layer would assign real blob IDs; the example uses a counter.
    let fields = message_to_jmap_body(&msg, |part| Id::from(format!("blob-{}", part.part_id)));

    println!("textBody parts: {}", fields.text_body.len());
    println!("htmlBody parts: {}", fields.html_body.len());
    println!("attachments:    {}", fields.attachments.len());
    println!("preview:        {:?}", fields.preview);

    // Decode and print the first text body value. A real backend would
    // log-and-skip on `find_by_id` returning None or on `decode_body_value`
    // returning Err, rather than aborting the whole Email/get response.
    if let Some(first_text_id) = fields.body_value_part_ids.first() {
        if let Some(part) = msg.part_index.find_by_id(first_text_id) {
            match decode_body_value(RAW, part, Some(8192)) {
                Ok(decoded) => {
                    let jmap_val = body_value_to_jmap(decoded);
                    println!("first textBody value: {:?}", jmap_val.value);
                }
                Err(e) => eprintln!("decode failed for partId {first_text_id}: {e}"),
            }
        } else {
            eprintln!("partId {first_text_id} not found in part_index");
        }
    }
    Ok(())
}

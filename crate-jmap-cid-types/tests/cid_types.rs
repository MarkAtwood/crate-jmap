//! Integration tests for jmap-cid-types.
//!
//! Independent oracles: the capability URI literal is asserted
//! against the value defined in draft-atwood-jmap-cid-00 §3, NOT
//! against the constant's own value.

use jmap_cid_types::JMAP_CID_URI;

// ---------------------------------------------------------------------------
// Capability URI
// ---------------------------------------------------------------------------

#[test]
fn capability_uri_matches_draft_00() {
    // Oracle: draft-atwood-jmap-cid-00 §3 (Capability).
    assert_eq!(JMAP_CID_URI, "urn:ietf:params:jmap:cid");
}

// ---------------------------------------------------------------------------
// CIDv1 mapping
//
// Oracle: draft-atwood-jmap-cid-00 (jmap-chat-spec), §Mapping to CIDv1,
// L346-381. The string vectors are copied verbatim from L373 and L381;
// they were also cross-checked with Python `base64.b32encode`.
// ---------------------------------------------------------------------------

use jmap_cid_types::{CidV1Error, Sha256};

/// L158: the example `sha256` value in the upload section.
const UPLOAD_SHA256: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
/// L373: "For the example `sha256` value in {{upload}}, the string form is:"
const UPLOAD_CID: &str = "bafkreie7q3iidccmpvszul7kudcvvuavuo7u6gzlbobczuk5nqk3b4akba";
/// L377: "For empty content (`sha256` value `e3b0...b855`)"
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// L381: the string form for empty content.
const EMPTY_CID: &str = "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku";

/// L354-362: "The binary CIDv1 is the concatenation of the following bytes:
/// `0x01` ... `0x55` ... `0x12` ... `0x20` ... The 32 bytes obtained by
/// decoding the 64 hex digits of `sha256`." Digest bytes transcribed by
/// hand from `EMPTY_SHA256`.
const EMPTY_CID_BYTES: [u8; 36] = [
    0x01, 0x55, 0x12, 0x20, 0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8,
    0x99, 0x6f, 0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
    0x78, 0x52, 0xb8, 0x55,
];

fn sha(hex: &str) -> Sha256 {
    hex.parse().expect("draft example is a valid sha256-value")
}

#[test]
fn to_cidv1_matches_draft_vectors() {
    assert_eq!(sha(UPLOAD_SHA256).to_cidv1(), UPLOAD_CID);
    assert_eq!(sha(EMPTY_SHA256).to_cidv1(), EMPTY_CID);
}

#[test]
fn from_cidv1_matches_draft_vectors() {
    // L365-366: "The reverse conversion checks the four prefix bytes and
    // hex-encodes the remaining 32 bytes."
    assert_eq!(Sha256::from_cidv1(UPLOAD_CID).unwrap(), UPLOAD_SHA256);
    assert_eq!(Sha256::from_cidv1(EMPTY_CID).unwrap(), EMPTY_SHA256);
}

#[test]
fn to_cidv1_bytes_is_prefix_then_digest() {
    assert_eq!(sha(EMPTY_SHA256).to_cidv1_bytes(), EMPTY_CID_BYTES);
}

#[test]
fn from_cidv1_bytes_strips_prefix() {
    assert_eq!(
        Sha256::from_cidv1_bytes(&EMPTY_CID_BYTES).unwrap(),
        EMPTY_SHA256
    );
}

#[test]
fn from_cidv1_bytes_rejects_other_codec() {
    // L358: "`0x55`: multicodec `raw`". 0x70 is multicodec dag-pb.
    let mut b = EMPTY_CID_BYTES;
    b[1] = 0x70;
    assert!(matches!(
        Sha256::from_cidv1_bytes(&b),
        Err(CidV1Error::UnsupportedPrefix {
            got: [0x01, 0x70, 0x12, 0x20],
            ..
        })
    ));
}

#[test]
fn from_cidv1_bytes_rejects_wrong_length() {
    // L361: digest length 32, so the binary CID is 4 + 32 bytes.
    assert!(matches!(
        Sha256::from_cidv1_bytes(&EMPTY_CID_BYTES[..35]),
        Err(CidV1Error::WrongLength { got: 35, .. })
    ));
    let mut long = EMPTY_CID_BYTES.to_vec();
    long.push(0);
    assert!(matches!(
        Sha256::from_cidv1_bytes(&long),
        Err(CidV1Error::WrongLength { got: 37, .. })
    ));
}

#[test]
fn from_cidv1_rejects_dag_pb_cid() {
    // The well-known `bafybei` CIDv1 dag-pb prefix over the empty digest
    // (Python `base64.b32encode` of 01 70 12 20 || digest). Not `raw`
    // per L358, so the reverse conversion's prefix check rejects it.
    assert!(matches!(
        Sha256::from_cidv1("bafybeihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku"),
        Err(CidV1Error::UnsupportedPrefix {
            got: [0x01, 0x70, 0x12, 0x20],
            ..
        })
    ));
}

#[test]
fn from_cidv1_rejects_other_multibase() {
    // L364-365: "the multibase prefix `b` followed by the binary CIDv1
    // encoded as lowercase base32 without padding."
    let upper = EMPTY_CID.to_ascii_uppercase();
    assert!(matches!(
        Sha256::from_cidv1(&upper),
        Err(CidV1Error::UnsupportedMultibase { .. })
    ));
    assert!(matches!(
        Sha256::from_cidv1(""),
        Err(CidV1Error::UnsupportedMultibase { .. })
    ));
}

#[test]
fn from_cidv1_rejects_uppercase_and_padding() {
    // Lowercase base32 "without padding": an uppercase digit or a `=`
    // pad is outside the alphabet.
    let mixed = EMPTY_CID.replacen('h', "H", 1);
    assert!(matches!(
        Sha256::from_cidv1(&mixed),
        Err(CidV1Error::InvalidBase32 {
            at: 7,
            byte: b'H',
            ..
        })
    ));
    let padded = format!("{EMPTY_CID}======");
    assert!(matches!(
        Sha256::from_cidv1(&padded),
        Err(CidV1Error::InvalidBase32 {
            at: 59,
            byte: b'=',
            ..
        })
    ));
}

#[test]
fn from_cidv1_rejects_nonzero_trailing_bits() {
    // RFC 4648 §3.5: "pad bits MUST be set to zero by conforming
    // encoders". 58 base32 digits carry 290 bits for 288 data bits;
    // the final `a` (0b00000) of L373 becomes `b` (0b00001), setting a
    // pad bit.
    let bad = format!("{}b", &UPLOAD_CID[..UPLOAD_CID.len() - 1]);
    assert!(matches!(
        Sha256::from_cidv1(&bad),
        Err(CidV1Error::NonCanonicalBase32 { .. })
    ));
}

#[test]
fn from_cidv1_rejects_impossible_base32_lengths() {
    // RFC 4648 §6: a final quantum is 8, 16, 24, 32 or 40 bits, i.e.
    // 2, 4, 5 or 7 base32 digits (or a full 8). 1, 3 and 6 digits
    // encode no whole number of bytes.
    for s in ["bq", "bqqq", "bqqqqqq"] {
        assert!(
            matches!(
                Sha256::from_cidv1(s),
                Err(CidV1Error::NonCanonicalBase32 { .. })
            ),
            "{s}"
        );
    }
}

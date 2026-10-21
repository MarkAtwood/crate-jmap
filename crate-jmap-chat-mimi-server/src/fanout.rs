//! FanoutMessages a MIMI provider stores (draft-atwood-jmap-chat-mimi-00
//! §application/mimi-fanout).
//!
//! [`parse_fanout_messages`] splits a `/notify` body (draft-ietf-mimi-protocol-06
//! §5.5: "The hub can include multiple concatenated FanoutMessage objects")
//! into [`Fanout`]s, deriving what [`crate::MimiBackend::store_fanout`]
//! needs: the `senderMsgId`, `sentAt`, the `message` octets the byte
//! duplicate rule compares, and the KeyPackageRefs a Welcome names.
//!
//! An MLSMessage has no length prefix, so finding where each FanoutMessage
//! ends takes an openmls parse of its `message`.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use jmap_chat_mimi_types::{ulid, Base64Url};
use jmap_types::UTCDate;
use openmls::prelude::{ContentType, MlsMessageBodyIn, MlsMessageIn};
use sha2::{Digest, Sha256};
use tls_codec::{Deserialize, Serialize as _, VLBytes};

/// The Frank of draft-ietf-mimi-protocol-06 §5.4: `server_frank[32]`,
/// `franking_signature_ciphersuite`, `franking_integrity_signature<V>`.
pub(crate) type Frank = ([u8; 32], u16, VLBytes);

/// The local accounts a FanoutMessage goes to
/// (draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FanoutRecipients {
    /// Every Chat of a local account bound to the room.
    Room,
    /// A Welcome: "stored only in the account owning a KeyPackageRef named
    /// in the Welcome". Each KeyPackageRef is in the encoding of
    /// [`crate::NewKeyPackage::key_package_ref`], with its length prefix.
    Welcome(Vec<Base64Url>),
}

/// One FanoutMessage of a `/notify` body, ready for
/// [`crate::MimiBackend::store_fanout`].
///
/// Built by [`parse_fanout_messages`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fanout {
    /// The ULID "whose 48-bit time component is the FanoutMessage
    /// `timestamp` and whose 80-bit random component is the first 10
    /// octets of the SHA-256 hash of the FanoutMessage octets".
    pub sender_msg_id: String,
    /// The FanoutMessage `timestamp`, the Message's `sentAt`.
    pub sent_at: UTCDate,
    /// The FanoutMessage octets, verbatim: the Message's `body`.
    pub body: Base64Url,
    /// The octets of the FanoutMessage `message`, which the byte duplicate
    /// rule compares.
    pub message: Vec<u8>,
    /// Where the FanoutMessage goes.
    pub recipients: FanoutRecipients,
}

/// A `/notify` body that is not one or more concatenated FanoutMessages
/// (draft-ietf-mimi-protocol-06 §5.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutParseError {
    description: String,
}

impl FanoutParseError {
    fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Human-readable description of what did not parse.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl fmt::Display for FanoutParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed FanoutMessage: {}", self.description)
    }
}

impl std::error::Error for FanoutParseError {}

fn malformed(what: &str) -> impl FnOnce(tls_codec::Error) -> FanoutParseError + '_ {
    move |e| FanoutParseError::new(format!("{what}: {e}"))
}

/// Read a `T` from the front of `bytes`. The `Read`-based `Deserialize` is
/// used because tls_codec 0.5.0's `DeserializeBytes` for a `<V>` vector
/// longer than its input hits a `debug_assert_eq!` instead of returning an
/// error.
fn read<'a, T: Deserialize>(
    bytes: &'a [u8],
    what: &str,
) -> Result<(T, &'a [u8]), FanoutParseError> {
    let mut rest = bytes;
    let value = T::tls_deserialize(&mut rest).map_err(malformed(what))?;
    Ok((value, rest))
}

/// Split a `/notify` body into its FanoutMessages, in order.
///
/// Each FanoutMessage is a `uint64 timestamp` and an MLSMessage, followed
/// by what its content type selects (draft-ietf-mimi-protocol-06 §5.5):
/// an `optional Frank` for an application message, `moreProposals<V>` for
/// a proposal, `externalProposals<V>` for a commit, and a
/// RatchetTreeOption (draft-mahy-mls-ratchet-tree-options-04 §3) for a
/// Welcome. The proposal lists are kept as opaque octets.
///
/// # Errors
///
/// [`FanoutParseError`] when `body` is empty, when a FanoutMessage does
/// not parse, when its `message` is not a PublicMessage, PrivateMessage or
/// Welcome, or when its `timestamp` does not fit a ULID (48 bits) or a
/// four-digit-year `UTCDate`.
// ponytail: a SemiPrivateMessage (draft-mahy-mls-semiprivatemessage)
// handshake does not parse, because openmls 0.9.1 does not decode that
// wire format; supporting it needs an openmls that does.
pub fn parse_fanout_messages(body: &[u8]) -> Result<Vec<Fanout>, FanoutParseError> {
    if body.is_empty() {
        return Err(FanoutParseError::new("empty body"));
    }
    let mut out = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let (fanout, tail) = parse_one(rest)?;
        out.push(fanout);
        rest = tail;
    }
    Ok(out)
}

fn parse_one(bytes: &[u8]) -> Result<(Fanout, &[u8]), FanoutParseError> {
    let (timestamp, after_timestamp) = read::<u64>(bytes, "timestamp")?;
    let (message, tail) = read::<MlsMessageIn>(after_timestamp, "message")?;
    let message_octets = &after_timestamp[..after_timestamp.len() - tail.len()];
    let (recipients, tail) = match message.extract() {
        MlsMessageBodyIn::PublicMessage(m) => {
            (FanoutRecipients::Room, skip_tail(m.content_type(), tail)?)
        }
        MlsMessageBodyIn::PrivateMessage(m) => {
            (FanoutRecipients::Room, skip_tail(m.content_type(), tail)?)
        }
        MlsMessageBodyIn::Welcome(welcome) => {
            let refs = welcome
                .secrets()
                .iter()
                .map(|s| {
                    s.new_member()
                        .tls_serialize_detached()
                        .map_err(malformed("KeyPackageRef"))
                        .and_then(|r| {
                            Base64Url::try_from(URL_SAFE_NO_PAD.encode(r))
                                .map_err(|e| FanoutParseError::new(format!("KeyPackageRef: {e}")))
                        })
                })
                .collect::<Result<_, _>>()?;
            (
                FanoutRecipients::Welcome(refs),
                skip_ratchet_tree_option(tail)?,
            )
        }
        _ => {
            return Err(FanoutParseError::new(
                "message is not a PublicMessage, PrivateMessage or Welcome",
            ))
        }
    };
    let octets = &bytes[..bytes.len() - tail.len()];
    let hash = Sha256::digest(octets);
    let sender_msg_id = ulid(timestamp, &std::array::from_fn(|i| hash[i]))
        .ok_or_else(|| FanoutParseError::new("timestamp does not fit in 48 bits"))?;
    let sent_at = utc_date_from_millis(timestamp)
        .ok_or_else(|| FanoutParseError::new("timestamp is past year 9999"))?;
    let body = Base64Url::try_from(URL_SAFE_NO_PAD.encode(octets))
        .map_err(|e| FanoutParseError::new(format!("body: {e}")))?;
    let fanout = Fanout {
        sender_msg_id,
        sent_at,
        body,
        message: message_octets.to_vec(),
        recipients,
    };
    Ok((fanout, tail))
}

/// Skip what the content type of a PublicMessage or PrivateMessage
/// selects in a FanoutMessage.
fn skip_tail(content_type: ContentType, bytes: &[u8]) -> Result<&[u8], FanoutParseError> {
    let tail = match content_type {
        ContentType::Application => read::<Option<Frank>>(bytes, "frank")?.1,
        ContentType::Proposal => read::<VLBytes>(bytes, "moreProposals")?.1,
        ContentType::Commit => read::<VLBytes>(bytes, "externalProposals")?.1,
    };
    Ok(tail)
}

/// Skip a RatchetTreeOption: `full(1)` carries `ratchet_tree<V>`,
/// `httpsUri(2)` carries `ratchet_tree_url<V>`, `outOfBand(3)` and
/// `distributionService(4)` carry nothing.
fn skip_ratchet_tree_option(bytes: &[u8]) -> Result<&[u8], FanoutParseError> {
    let (representation, rest) = read::<u8>(bytes, "ratchetTreeOption")?;
    match representation {
        1 | 2 => Ok(read::<VLBytes>(rest, "ratchetTreeOption")?.1),
        3 | 4 => Ok(rest),
        other => Err(FanoutParseError::new(format!(
            "ratchetTreeOption representation {other} is not defined"
        ))),
    }
}

/// `ms` milliseconds since the Unix epoch as a millisecond-precision
/// `UTCDate`, or `None` past year 9999. A zero fraction is omitted, as
/// RFC 8620 §1.4 requires.
pub(crate) fn utc_date_from_millis(ms: u64) -> Option<UTCDate> {
    let seconds = jmap_server::utc_date_from_epoch_seconds(i64::try_from(ms / 1000).ok()?)?;
    if ms.is_multiple_of(1000) {
        return Some(seconds);
    }
    let seconds = seconds.as_ref().strip_suffix('Z')?;
    Some(UTCDate::from(format!("{seconds}.{:03}Z", ms % 1000)))
}

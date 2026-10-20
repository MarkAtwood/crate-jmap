//! draft-atwood-jmap-chat-federation-00 §Session Object Extensions for
//! Federation — the `ownerUserId`, `peerSigningKeys` and
//! `ownerDirectAddress` Session fields, the chat `ownerEndpoints` field
//! as ChatContact `endpoints`, and the anonymous Session object a
//! server answers an unsigned GET of `/.well-known/jmap` with (§Key
//! Publication).
//!
//! The fields are Session extensions, so an RFC 8620 Session parser keeps
//! them in its catch-all map (for example `jmap_base_client::Session::extra`);
//! [`FederationSession::from_session_extra`] reads them from there.

use jmap_chat_types::Endpoint;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The federation fields of a JMAP Session object.
///
/// Spec: draft-atwood-jmap-chat-federation-00 §Session Object Extensions
/// for Federation: "Servers implementing this specification MUST include
/// the following fields in their JMAP Session object (advertised at
/// `/.well-known/jmap`)".
///
/// `peer_signing_keys` holds each entry as raw JSON. The receiving server
/// "MUST ignore any other entry" than a usable key (§Key Publication), so a
/// malformed entry must not make the whole Session unreadable; filter the
/// entries with `jmap_peer_auth::usable_keys`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FederationSession {
    /// `ownerUserId`: "The identity string of the mailbox owner."
    pub owner_user_id: String,
    /// `peerSigningKeys`: "The public keys with which this server signs
    /// peer requests", each a JSON Web Key.
    pub peer_signing_keys: Vec<Value>,
    /// `ownerDirectAddress`: "A deployment-specific address at which the
    /// owner's client may be reachable directly." Optional, advisory and
    /// peer-supplied (§Direct Address Hints).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_direct_address: Option<String>,
}

impl FederationSession {
    /// The fields with `ownerUserId` and `peerSigningKeys`, and no
    /// `ownerDirectAddress`.
    pub fn new(owner_user_id: impl Into<String>, peer_signing_keys: Vec<Value>) -> Self {
        Self {
            owner_user_id: owner_user_id.into(),
            peer_signing_keys,
            owner_direct_address: None,
        }
    }

    /// Reads the federation fields from a Session object's extension map.
    ///
    /// Pass the parsed Session's catch-all map, or the whole Session
    /// object; other members are ignored.
    ///
    /// # Errors
    ///
    /// A [`serde_json::Error`] when `ownerUserId` or `peerSigningKeys` is
    /// absent, either has the wrong JSON type, or `ownerDirectAddress` is
    /// present and not a string.
    pub fn from_session_extra(extra: &Map<String, Value>) -> Result<Self, serde_json::Error> {
        let pick = |k: &str| extra.get(k).map(|v| (k.to_owned(), v.clone()));
        let fields: Map<String, Value> = ["ownerUserId", "peerSigningKeys", "ownerDirectAddress"]
            .into_iter()
            .filter_map(pick)
            .collect();
        serde_json::from_value(Value::Object(fields))
    }
}

/// The `endpoints` value for the ChatContact whose server published a
/// Session with these extension fields.
///
/// Spec: draft-atwood-jmap-chat-00 §Session Object Extensions:
/// "`ownerEndpoints` (Endpoint\[\], optional): The owner's advertised
/// out-of-band capability endpoints ... Peers that probe
/// `/.well-known/jmap` SHOULD merge these into the `endpoints` field of the
/// corresponding ChatContact record." §ChatContact: "Servers populate this
/// field from the ChatContact's `ownerEndpoints` at `/.well-known/jmap`."
///
/// Pass the probed Session's catch-all map, or the whole Session object.
/// Write the result to the ChatContact's `endpoints`, replacing the stored
/// value, so an endpoint the peer stopped advertising goes away:
///
/// - `None` when `ownerEndpoints` is absent or not an array: the
///   ChatContact has no `endpoints`.
/// - Otherwise the entries that parse as an Endpoint, in order. An entry
///   without a string `type` and `uri` is skipped, so one malformed entry
///   does not hide the others. Entries with an unknown `type` are kept:
///   clients ignore them (§Endpoint).
///
/// The values are peer-supplied and untrusted (draft-atwood-jmap-chat-00
/// §Out-of-Band Endpoints and Actions). A server that fetches from one
/// applies draft-atwood-jmap-chat-federation-00 §Blob Fetch and SSRF
/// (`jmap_peer_auth::fetch_address_allowed`).
///
/// # Example
///
/// ```
/// let session = serde_json::json!({
///     "ownerEndpoints": [{ "type": "urn:jmap:chat:cap:vtc", "uri": "https://meet.b.example/bob" }]
/// });
/// let endpoints = jmap_chat_federation_types::owner_endpoints(session.as_object().unwrap());
/// assert_eq!(endpoints.unwrap()[0].uri, "https://meet.b.example/bob");
/// ```
pub fn owner_endpoints(extra: &Map<String, Value>) -> Option<Vec<Endpoint>> {
    let entries = extra.get("ownerEndpoints")?.as_array()?;
    Some(
        entries
            .iter()
            .filter_map(|e| serde_json::from_value(e.clone()).ok())
            .collect(),
    )
}

/// Why [`anonymous_well_known_session`] refused to build a Session.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnonymousSessionError {
    /// `peerSigningKeys` is empty: "Servers MUST advertise this field with
    /// at least one key" (§Session Object Extensions for Federation).
    #[error("peerSigningKeys must contain at least one key")]
    NoPeerSigningKeys,
    /// The `peerSigningKeys` entry at this index carries the private key
    /// member `d` (RFC 8037 §2). "A JWK in `peerSigningKeys` SHOULD NOT
    /// contain private key members" (§Key Publication); publishing one
    /// discloses the signing key, so this builder refuses.
    #[error("peerSigningKeys[{0}] contains the private key member \"d\"")]
    PrivateKeyMember(usize),
}

/// The Session object to answer an unsigned GET of `/.well-known/jmap`.
///
/// Spec: draft-atwood-jmap-chat-federation-00 §Key Publication: "A server
/// MUST answer an unsigned GET of its `/.well-known/jmap` with a Session
/// object that includes `ownerUserId` and `peerSigningKeys`, and MUST NOT
/// require any authentication for that response; in it, `accounts` and
/// `primaryAccounts` MAY be empty objects and `username` MAY be the empty
/// string."
///
/// `session` is the server's Session object (RFC 8620 §2). The result keeps
/// its other members, replaces `accounts` and `primaryAccounts` with empty
/// objects and `username` with the empty string, so the unauthenticated
/// response names no account or user, and sets the federation fields from
/// `fields`.
///
/// # Errors
///
/// [`AnonymousSessionError`] when `fields.peer_signing_keys` is empty or an
/// entry has a `d` member.
///
/// # Example
///
/// ```
/// use jmap_chat_federation_types::{anonymous_well_known_session, FederationSession};
/// use serde_json::json;
///
/// let session = json!({ "username": "alice", "accounts": { "A1": {} } });
/// let key = json!({ "kty": "OKP", "crv": "Ed25519", "x": "x", "kid": "k" });
/// let fields = FederationSession::new("alice@a.example", vec![key]);
/// let anon = anonymous_well_known_session(session.as_object().unwrap().clone(), &fields)
///     .unwrap();
/// assert_eq!(anon["username"], "");
/// assert_eq!(anon["ownerUserId"], "alice@a.example");
/// ```
pub fn anonymous_well_known_session(
    mut session: Map<String, Value>,
    fields: &FederationSession,
) -> Result<Map<String, Value>, AnonymousSessionError> {
    if fields.peer_signing_keys.is_empty() {
        return Err(AnonymousSessionError::NoPeerSigningKeys);
    }
    if let Some(i) = fields
        .peer_signing_keys
        .iter()
        .position(|k| k.get("d").is_some())
    {
        return Err(AnonymousSessionError::PrivateKeyMember(i));
    }
    session.insert("accounts".into(), Value::Object(Map::new()));
    session.insert("primaryAccounts".into(), Value::Object(Map::new()));
    session.insert("username".into(), Value::String(String::new()));
    session.insert("ownerUserId".into(), fields.owner_user_id.clone().into());
    session.insert(
        "peerSigningKeys".into(),
        Value::Array(fields.peer_signing_keys.clone()),
    );
    match &fields.owner_direct_address {
        Some(a) => session.insert("ownerDirectAddress".into(), a.clone().into()),
        None => session.remove("ownerDirectAddress"),
    };
    Ok(session)
}

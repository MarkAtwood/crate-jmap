//! Resolving a JMAP object URI in the context it was received from
//! (draft-atwood-jmap-object-uri-00 §Interpretation).
//!
//! "A client resolves it, in the context from which it was received, by
//! looking up the type name's Data Type in the 'JMAP Object URI Types'
//! registry and calling that data type's `/get` method on the named
//! account, including in `using` the capability that the 'JMAP Data
//! Types' registry (RFC 9404 §6.3) lists for that data type."

use crate::registry::{UriTypeEntry, uri_type};
use jmap_types::data_types::{DataTypeEntry, data_type};
use jmap_types::{Id, JmapUri};
use serde_json::{Value, json};
use std::fmt;

/// The capability and account data of a JMAP Session (RFC 8620 §2) that
/// resolution needs.
///
/// Implemented for [`serde_json::Value`] holding a Session object, so a
/// Session fetched from a server or a federation peer can be used as-is.
/// Client and server crates with a typed Session implement it for that
/// type.
pub trait SessionView {
    /// Whether the Session's `capabilities` object has the key
    /// `capability`.
    fn has_capability(&self, capability: &str) -> bool;

    /// Whether the account `account_id` has `capability` among its
    /// `accountCapabilities`; `None` when the Session has no such account.
    fn account_has_capability(&self, account_id: &Id, capability: &str) -> Option<bool>;
}

impl SessionView for Value {
    fn has_capability(&self, capability: &str) -> bool {
        self.get("capabilities")
            .and_then(Value::as_object)
            .is_some_and(|caps| caps.contains_key(capability))
    }

    fn account_has_capability(&self, account_id: &Id, capability: &str) -> Option<bool> {
        let account = self
            .get("accounts")
            .and_then(Value::as_object)?
            .get(account_id.as_ref())?;
        Some(
            account
                .get("accountCapabilities")
                .and_then(Value::as_object)
                .is_some_and(|caps| caps.contains_key(capability)),
        )
    }
}

/// A JMAP object URI mapped to the call that dereferences it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The "JMAP Object URI Types" row of the URI's type name.
    pub uri_type: &'static UriTypeEntry,
    /// The "JMAP Data Types" row of the data type it stands for.
    pub data_type: &'static DataTypeEntry,
    /// The account holding the object.
    pub account_id: Id,
    /// The object's `id`.
    pub object_id: Id,
}

impl Resolution {
    /// The capability URI to include in `using`, from the "JMAP Data
    /// Types" registry, such as `urn:ietf:params:jmap:calendars`.
    pub fn capability(&self) -> &'static str {
        self.data_type.capability
    }

    /// The `/get` method name, such as `CalendarEvent/get`.
    pub fn get_method(&self) -> String {
        format!("{}/get", self.data_type.type_name)
    }

    /// The `/get` arguments that fetch the one object (RFC 8620 §5.1):
    /// `{"accountId": …, "ids": [objectId]}`.
    pub fn get_arguments(&self) -> Value {
        json!({ "accountId": self.account_id, "ids": [self.object_id] })
    }
}

/// Why a [`JmapUri`] cannot be resolved.
///
/// A receiver treats such a URI as unresolvable. When resolution succeeds
/// but the object does not exist or the user cannot read it, the `/get`
/// call reports it in `notFound` instead (§Security Considerations).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The type name is not in the "JMAP Object URI Types" registry.
    UnknownType {
        /// The URI's type name.
        type_name: String,
    },
    /// The Session does not advertise the data type's capability.
    CapabilityNotSupported {
        /// The capability URI the data type needs.
        capability: &'static str,
    },
    /// The Session has no account with the URI's account id.
    UnknownAccount {
        /// The URI's account id.
        account_id: Id,
    },
    /// The named account does not have the data type's capability.
    AccountLacksCapability {
        /// The URI's account id.
        account_id: Id,
        /// The capability URI the data type needs.
        capability: &'static str,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType { type_name } => {
                write!(f, "jmap: URI type name {type_name:?} is not registered")
            }
            Self::CapabilityNotSupported { capability } => {
                write!(f, "session does not support capability {capability}")
            }
            Self::UnknownAccount { account_id } => {
                write!(f, "session has no account {account_id}")
            }
            Self::AccountLacksCapability {
                account_id,
                capability,
            } => write!(f, "account {account_id} lacks capability {capability}"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Map `uri` to its data type, `/get` method and capability, from the
/// registries alone.
///
/// # Errors
///
/// [`ResolveError::UnknownType`] when the type name is not registered.
pub fn resolve_type(uri: &JmapUri) -> Result<Resolution, ResolveError> {
    let unknown = || ResolveError::UnknownType {
        type_name: uri.data_type().to_owned(),
    };
    let uri_type = uri_type(uri.data_type()).ok_or_else(unknown)?;
    // Every registry row names a registered data type (tested); a miss
    // would be a stale table, which leaves the URI just as unresolvable.
    let data_type = data_type(uri_type.data_type).ok_or_else(unknown)?;
    Ok(Resolution {
        uri_type,
        data_type,
        account_id: uri.account_id().clone(),
        object_id: uri.object_id().clone(),
    })
}

/// Resolve `uri` against the Session it was received with: map it with
/// [`resolve_type`], then check that the Session supports the data type's
/// capability and has the named account with that capability.
///
/// # Errors
///
/// [`ResolveError::UnknownType`] as for [`resolve_type`];
/// [`ResolveError::CapabilityNotSupported`],
/// [`ResolveError::UnknownAccount`] or
/// [`ResolveError::AccountLacksCapability`] when the Session cannot serve
/// the `/get` call.
pub fn resolve(uri: &JmapUri, session: &impl SessionView) -> Result<Resolution, ResolveError> {
    let found = resolve_type(uri)?;
    let capability = found.capability();
    if !session.has_capability(capability) {
        return Err(ResolveError::CapabilityNotSupported { capability });
    }
    match session.account_has_capability(&found.account_id, capability) {
        None => Err(ResolveError::UnknownAccount {
            account_id: found.account_id,
        }),
        Some(false) => Err(ResolveError::AccountLacksCapability {
            account_id: found.account_id,
            capability,
        }),
        Some(true) => Ok(found),
    }
}

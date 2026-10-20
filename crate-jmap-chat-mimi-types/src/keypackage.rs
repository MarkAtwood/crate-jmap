//! draft-atwood-jmap-chat-mimi-00 §KeyPackages — the `KeyPackage` object
//! and the argument shapes of `KeyPackage/get` and `KeyPackage/set`.
//!
//! Both methods are standard RFC 8620 methods, so their responses are
//! [`jmap_types::GetResponse<KeyPackage>`] and
//! [`jmap_types::SetResponse<KeyPackageCreated>`]. `/set` differs from
//! RFC 8620 §5.3 only in that "`update` is not supported: the server MUST
//! reject every update with a SetError of type `forbidden`". That is
//! server behaviour; [`KeyPackage`] still implements
//! [`SetObject`] so a server can route the method.

use jmap_types::{GetObject, Id, JmapObject, PatchObject, SetObject};
use serde::{Deserialize, Serialize};

use crate::Base64Url;

/// One MLS KeyPackage the server publishes for one of the owner's clients
/// and hands out in response to MIMI key material requests.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §KeyPackage Object Fields
///
/// # Strictness
///
/// No struct-level `#[serde(default)]`: the draft marks no field
/// optional, so a missing field is a deserialize error. A `/get` with a
/// `properties` subset returns partial objects; deserialize those as
/// [`serde_json::Value`].
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::KeyPackage;
///
/// let kp: KeyPackage = serde_json::from_str(r#"{
///     "id": "kp42",
///     "keyPackage": "AAEC",
///     "keyPackageRef": "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f",
///     "lastResort": false
/// }"#)?;
/// assert!(!kp.last_resort);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPackage {
    /// The id of the record. Immutable, server-set.
    pub id: Id,
    /// The KeyPackage (RFC 9420 §10). Immutable.
    pub key_package: Base64Url,
    /// The KeyPackageRef of `key_package` (RFC 9420 §5.2). Immutable,
    /// server-set.
    pub key_package_ref: Base64Url,
    /// `true` when `key_package` is the client's last-resort KeyPackage
    /// (RFC 9420 §16.8); `false` otherwise. Immutable; the server derives
    /// it from `key_package`.
    pub last_resort: bool,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A value of the `create` map of a `KeyPackage/set` request.
///
/// Spec: draft-atwood-jmap-chat-mimi-00 §KeyPackage Object Fields. `id`
/// and `keyPackageRef` are server-set and not part of a create.
/// `lastResort` is optional: "A create MAY include it; the server MUST
/// reject a create whose `lastResort` differs from the value derived from
/// `keyPackage`".
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::KeyPackageCreate;
///
/// let create = KeyPackageCreate::new("AAEC".parse()?).with_last_resort(false);
/// assert_eq!(
///     serde_json::to_string(&create).unwrap(),
///     r#"{"keyPackage":"AAEC","lastResort":false}"#
/// );
/// # Ok::<(), jmap_chat_mimi_types::Base64UrlError>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPackageCreate {
    /// The KeyPackage (RFC 9420 §10) to publish.
    pub key_package: Base64Url,
    /// The client's claim that `key_package` is (or is not) its
    /// last-resort KeyPackage. Absent means no claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_resort: Option<bool>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl KeyPackageCreate {
    /// A create carrying `key_package` and no `lastResort` claim.
    pub fn new(key_package: Base64Url) -> Self {
        Self {
            key_package,
            last_resort: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Set the `lastResort` claim.
    #[must_use]
    pub fn with_last_resort(mut self, last_resort: bool) -> Self {
        self.last_resort = Some(last_resort);
        self
    }
}

/// A value of the `created` map of a `KeyPackage/set` response.
///
/// RFC 8620 §5.3: `created` holds "any properties of the created Foo
/// object that were not sent by the client. This includes all server-set
/// properties". `id` and `keyPackageRef` are server-set and so always
/// present. `lastResort` is present only when the client omitted it.
///
/// # Example
///
/// ```
/// use jmap_chat_mimi_types::KeyPackageCreated;
/// use jmap_types::SetResponse;
///
/// let resp: SetResponse<KeyPackageCreated> = serde_json::from_str(r#"{
///     "accountId": "u1",
///     "oldState": "s7",
///     "newState": "s8",
///     "created": {
///         "k1": {
///             "id": "kp42",
///             "keyPackageRef": "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f"
///         }
///     }
/// }"#)?;
/// assert_eq!(resp.created.unwrap()["k1"].id.as_ref(), "kp42");
/// # Ok::<(), serde_json::Error>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPackageCreated {
    /// The id the server assigned to the record.
    pub id: Id,
    /// The KeyPackageRef the server computed from `keyPackage`.
    pub key_package_ref: Base64Url,
    /// The server-derived `lastResort`, present when the create omitted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_resort: Option<bool>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Property selector for [`KeyPackage`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyPackageProperty {
    /// The `id` property (draft-atwood-jmap-chat-mimi-00 §KeyPackage
    /// Object Fields).
    Id,
    /// The `keyPackage` property (draft-atwood-jmap-chat-mimi-00
    /// §KeyPackage Object Fields).
    KeyPackage,
    /// The `keyPackageRef` property (draft-atwood-jmap-chat-mimi-00
    /// §KeyPackage Object Fields).
    KeyPackageRef,
    /// The `lastResort` property (draft-atwood-jmap-chat-mimi-00
    /// §KeyPackage Object Fields).
    LastResort,
}

impl JmapObject for KeyPackage {
    const TYPE_NAME: &'static str = "KeyPackage";
    type Property = KeyPackageProperty;
}

impl GetObject for KeyPackage {}

impl SetObject for KeyPackage {
    type Patch = PatchObject;
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_types::{GetResponse, SetResponse};
    use serde_json::json;

    // The draft's §Examples payloads are shortened with "..." and so are
    // not base64url. These tests keep the example JSON shapes and ids and
    // substitute valid payloads: "AAEC" (octets 0x00 0x01 0x02, hand
    // encoded) and the §Conventions KeyPackageRef example string.
    const KP: &str = "AAEC";
    const KP_REF: &str = "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";

    fn sample() -> serde_json::Value {
        json!({
            "id": "kp42",
            "keyPackage": KP,
            "keyPackageRef": KP_REF,
            "lastResort": true
        })
    }

    #[test]
    fn type_name_matches_draft() {
        // Oracle: draft §KeyPackage/get and §KeyPackage/set method names.
        assert_eq!(KeyPackage::TYPE_NAME, "KeyPackage");
    }

    #[test]
    fn object_round_trips() {
        let raw = sample();
        let kp: KeyPackage = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(kp.id.as_ref(), "kp42");
        assert_eq!(kp.key_package.as_str(), KP);
        assert_eq!(kp.key_package_ref.as_str(), KP_REF);
        assert!(kp.last_resort);
        assert!(kp.extra.is_empty());
        assert_eq!(serde_json::to_value(&kp).unwrap(), raw);
    }

    #[test]
    fn each_object_field_required() {
        // Oracle: draft §KeyPackage Object Fields marks no field optional.
        for field in ["id", "keyPackage", "keyPackageRef", "lastResort"] {
            let mut raw = sample();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<KeyPackage>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn padded_byte_strings_rejected() {
        // Oracle: draft §Conventions: byte strings are the "unpadded
        // base64url encoding".
        for field in ["keyPackage", "keyPackageRef"] {
            let mut raw = sample();
            raw[field] = json!("AA==");
            assert!(serde_json::from_value::<KeyPackage>(raw).is_err());
        }
    }

    #[test]
    fn create_matches_draft_example_shape() {
        // Oracle: draft §Examples request:
        // "k1": { "keyPackage": "AAEAAkBBxu1...", "lastResort": false }
        let raw = json!({ "keyPackage": KP, "lastResort": false });
        let c: KeyPackageCreate = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(c.key_package.as_str(), KP);
        assert_eq!(c.last_resort, Some(false));
        assert_eq!(serde_json::to_value(&c).unwrap(), raw);
        assert_eq!(
            serde_json::to_value(
                KeyPackageCreate::new(KP.parse().unwrap()).with_last_resort(false)
            )
            .unwrap(),
            raw
        );
    }

    #[test]
    fn create_last_resort_optional() {
        // Oracle: draft §KeyPackage Object Fields, `lastResort`: "A create
        // MAY include it".
        let raw = json!({ "keyPackage": KP });
        let c: KeyPackageCreate = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(c.last_resort, None);
        assert_eq!(serde_json::to_value(&c).unwrap(), raw);
    }

    #[test]
    fn create_requires_key_package() {
        assert!(
            serde_json::from_value::<KeyPackageCreate>(json!({ "lastResort": false })).is_err()
        );
    }

    #[test]
    fn set_response_matches_draft_example() {
        // Oracle: draft §Examples response, with the shortened
        // keyPackageRef replaced by the §Conventions example string.
        let raw = json!({
            "accountId": "u1",
            "oldState": "s7",
            "newState": "s8",
            "created": {
                "k1": { "id": "kp42", "keyPackageRef": KP_REF }
            }
        });
        let resp: SetResponse<KeyPackageCreated> = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.account_id.as_ref(), "u1");
        assert_eq!(resp.old_state.unwrap().as_ref(), "s7");
        assert_eq!(resp.new_state.as_ref(), "s8");
        let created = resp.created.unwrap();
        let k1 = &created["k1"];
        assert_eq!(k1.id.as_ref(), "kp42");
        assert_eq!(k1.key_package_ref.as_str(), KP_REF);
        assert_eq!(k1.last_resort, None);
        assert_eq!(
            serde_json::to_value(k1).unwrap(),
            json!({ "id": "kp42", "keyPackageRef": KP_REF })
        );
    }

    #[test]
    fn created_carries_server_derived_last_resort() {
        // Oracle: RFC 8620 §5.3 `created`: "any properties of the created
        // Foo object that were not sent by the client. This includes all
        // server-set properties".
        let raw = json!({ "id": "kp43", "keyPackageRef": KP_REF, "lastResort": true });
        let c: KeyPackageCreated = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(c.last_resort, Some(true));
        assert_eq!(serde_json::to_value(&c).unwrap(), raw);
    }

    #[test]
    fn created_requires_server_set_fields() {
        for field in ["id", "keyPackageRef"] {
            let mut raw = json!({ "id": "kp42", "keyPackageRef": KP_REF });
            raw.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<KeyPackageCreated>(raw).is_err());
        }
    }

    #[test]
    fn get_response_parses() {
        // Oracle: RFC 8620 §5.1 response: accountId, state, list,
        // notFound.
        let raw = json!({
            "accountId": "u1",
            "state": "s8",
            "list": [sample()],
            "notFound": ["kp9"]
        });
        let resp: GetResponse<KeyPackage> = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "kp42");
        assert_eq!(resp.not_found.unwrap()[0].as_ref(), "kp9");
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = sample();
        raw["vendor.example:clientLabel"] = json!("phone");
        let kp: KeyPackage = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            kp.extra.get("vendor.example:clientLabel"),
            Some(&json!("phone"))
        );
        assert_eq!(serde_json::to_value(&kp).unwrap(), raw);
    }
}

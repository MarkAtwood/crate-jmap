//! draft-atwood-jmap-chat-federation-00 §ChatContact Type Extensions for
//! Federation — the `ChatContact.serverUrl` and `ChatContact.directAddress`
//! fields.
//!
//! Both are extension fields, so `jmap_chat_types::ChatContact` keeps them
//! in `extra`. Both are peer-supplied and untrusted (draft §Direct Address
//! Hints); these readers do not validate them.

/// The `serverUrl` of a ChatContact, read from the ChatContact's `extra`
/// map.
///
/// Spec: draft-atwood-jmap-chat-federation-00 §ChatContact Type Extensions
/// for Federation: "`serverUrl` (String): Base HTTPS URL of this contact's
/// mailbox. Used for outbound delivery and for probing `/.well-known/jmap`."
///
/// Pass `&contact.extra`. Returns `None` when `serverUrl` is absent or is
/// not a string. The value is returned verbatim: it is not checked to be
/// an HTTPS URL.
///
/// # Example
///
/// ```
/// let extra = serde_json::json!({ "serverUrl": "https://b.example/jmap/" });
/// assert_eq!(
///     jmap_chat_federation_types::contact_server_url(extra.as_object().unwrap()),
///     Some("https://b.example/jmap/")
/// );
/// ```
pub fn contact_server_url(extra: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
    extra.get("serverUrl")?.as_str()
}

/// The `directAddress` of a ChatContact, read from the ChatContact's
/// `extra` map.
///
/// Spec: draft-atwood-jmap-chat-federation-00 §ChatContact Type Extensions
/// for Federation: "`directAddress` (String, optional): A deployment-specific
/// address at which this contact's client may be reachable directly, without
/// routing through their mailbox. ... This value is peer-supplied and MUST
/// be treated as untrusted". Servers "MUST NOT use `directAddress` ... as
/// `fetchUrl` targets for blob fetches" (draft §Direct Address Hints).
///
/// Pass `&contact.extra`. Returns `None` when `directAddress` is absent or
/// is not a string. The format is deployment-defined, so the value is
/// returned verbatim.
///
/// # Example
///
/// ```
/// let extra = serde_json::json!({ "directAddress": "bob-laptop.tailnet.example" });
/// assert_eq!(
///     jmap_chat_federation_types::contact_direct_address(extra.as_object().unwrap()),
///     Some("bob-laptop.tailnet.example")
/// );
/// ```
pub fn contact_direct_address(extra: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
    extra.get("directAddress")?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_chat_types::ChatContact;

    /// A hand-written federated ChatContact. draft-atwood-jmap-chat-00
    /// L461: "Additional ChatContact fields used in federation deployments
    /// (`serverUrl`, `directAddress`) are defined in {{JMAP-CHAT-FED}}", so
    /// `jmap-chat-types` keeps them in `extra` and they round-trip.
    fn contact(fields: &str) -> ChatContact {
        let json = format!(
            r#"{{
                "id": "c1",
                "identity": "bob@b.example",
                "login": "bob",
                "firstSeenAt": "2026-01-01T00:00:00Z",
                "lastSeenAt": "2026-01-02T00:00:00Z"{fields}
            }}"#
        );
        serde_json::from_str(&json).expect("valid ChatContact")
    }

    /// draft-atwood-jmap-chat-federation-00 L268-273: `serverUrl` (String)
    /// and `directAddress` (String, optional) on ChatContact.
    #[test]
    fn reads_both_fields_from_chat_contact() {
        let c = contact(
            r#", "serverUrl": "https://b.example/jmap/",
                "directAddress": "100.64.0.7:4443""#,
        );
        assert_eq!(
            contact_server_url(&c.extra),
            Some("https://b.example/jmap/")
        );
        assert_eq!(contact_direct_address(&c.extra), Some("100.64.0.7:4443"));

        let wire = serde_json::to_value(&c).expect("serialize");
        assert_eq!(wire["serverUrl"], "https://b.example/jmap/");
        assert_eq!(wire["directAddress"], "100.64.0.7:4443");
    }

    /// L271: `directAddress` is "optional"; a contact without it reads as
    /// `None`.
    #[test]
    fn absent_fields_read_as_none() {
        let c = contact("");
        assert_eq!(contact_server_url(&c.extra), None);
        assert_eq!(contact_direct_address(&c.extra), None);
    }

    /// L268, L271: both fields are Strings; any other JSON type is not a
    /// value of the field.
    #[test]
    fn non_string_values_read_as_none() {
        let c = contact(r#", "serverUrl": 443, "directAddress": ["a", "b"]"#);
        assert_eq!(contact_server_url(&c.extra), None);
        assert_eq!(contact_direct_address(&c.extra), None);
    }
}

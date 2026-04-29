use crate::email::EmailAddress;
use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// An RFC 8621 §6 Identity object.
///
/// Stores information about an email address or domain the user may send from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Identity {
    /// The id of the Identity (immutable; server-set).
    pub id: Id,
    /// The "From" name the client SHOULD use when creating a new Email
    /// from this Identity.  Defaults to `""`.
    pub name: String,
    /// The "From" email address the client MUST use (immutable).
    pub email: String,
    /// The Reply-To value the client SHOULD set.  `null` if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<EmailAddress>>,
    /// The Bcc value the client SHOULD set.  `null` if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bcc: Option<Vec<EmailAddress>>,
    /// Plaintext signature.  Defaults to `""`.
    pub text_signature: String,
    /// HTML snippet signature.  Defaults to `""`.
    pub html_signature: String,
    /// Whether the user may delete this Identity (server-set).
    pub may_delete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: RFC 8621 §6.4 example response — first identity object.
    #[test]
    fn identity_round_trips_from_rfc_example() {
        let json = r#"{
            "id": "XD-3301-222-11_22AAz",
            "name": "Joe Bloggs",
            "email": "joe@example.com",
            "replyTo": null,
            "bcc": [{"name": null, "email": "joe+archive@example.com"}],
            "textSignature": "-- \nJoe Bloggs\nMaster of Email",
            "htmlSignature": "<div><b>Joe Bloggs</b></div><div>Master of Email</div>",
            "mayDelete": false
        }"#;
        let identity: Identity = serde_json::from_str(json).expect("deserialize");
        assert_eq!(identity.id.as_ref(), "XD-3301-222-11_22AAz");
        assert_eq!(identity.name, "Joe Bloggs");
        assert_eq!(identity.email, "joe@example.com");
        assert_eq!(identity.reply_to, None);
        let bcc = identity.bcc.as_ref().expect("bcc present");
        assert_eq!(bcc.len(), 1);
        assert_eq!(bcc[0].email, "joe+archive@example.com");
        assert_eq!(identity.text_signature, "-- \nJoe Bloggs\nMaster of Email");
        assert!(!identity.may_delete);
    }

    // Oracle: RFC 8621 §6.4 example response — second identity object.
    #[test]
    fn identity_round_trips_minimal() {
        let json = r#"{
            "id": "XD-9911312-11_22AAz",
            "name": "Joe B",
            "email": "*@example.com",
            "replyTo": null,
            "bcc": null,
            "textSignature": "",
            "htmlSignature": "",
            "mayDelete": true
        }"#;
        let identity: Identity = serde_json::from_str(json).expect("deserialize");
        assert_eq!(identity.id.as_ref(), "XD-9911312-11_22AAz");
        assert_eq!(identity.reply_to, None);
        assert_eq!(identity.bcc, None);
        assert_eq!(identity.text_signature, "");
        assert_eq!(identity.html_signature, "");
        assert!(identity.may_delete);
    }

    // Oracle: text_signature and html_signature are always present in serialized output
    // (RFC §6 — they have defined defaults but must appear in responses).
    #[test]
    fn identity_serializes_signatures_always() {
        let identity = Identity {
            id: Id::from("test-id"),
            name: String::new(),
            email: "user@example.com".to_owned(),
            reply_to: None,
            bcc: None,
            text_signature: String::new(),
            html_signature: String::new(),
            may_delete: true,
        };
        let json = serde_json::to_string(&identity).expect("serialize");
        assert!(
            json.contains("\"textSignature\":\"\""),
            "textSignature must be present"
        );
        assert!(
            json.contains("\"htmlSignature\":\"\""),
            "htmlSignature must be present"
        );
    }

    // Oracle: null replyTo and bcc are omitted from serialized output
    // (skip_serializing_if = "Option::is_none").
    #[test]
    fn identity_null_optional_fields_omitted_in_serialization() {
        let identity = Identity {
            id: Id::from("test-id"),
            name: String::new(),
            email: "user@example.com".to_owned(),
            reply_to: None,
            bcc: None,
            text_signature: String::new(),
            html_signature: String::new(),
            may_delete: false,
        };
        let json = serde_json::to_string(&identity).expect("serialize");
        assert!(!json.contains("replyTo"), "null replyTo must be omitted");
        assert!(!json.contains("bcc"), "null bcc must be omitted");
    }
}

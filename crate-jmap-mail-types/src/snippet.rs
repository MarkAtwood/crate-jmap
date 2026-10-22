//! RFC 8621 §5 SearchSnippet object.
//!
//! Provides [`SearchSnippet`] — a highlighted text excerpt returned by
//! `SearchSnippet/get` for each matched [`crate::Email`] — and
//! [`SearchSnippetGetResponse`], the `SearchSnippet/get` response.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// A search result snippet for one Email (RFC 8621 §5).
///
/// Returned by `SearchSnippet/get`. Note that `SearchSnippet` has **no `id`
/// field** — it is identified by `email_id` instead.
///
/// The `subject` and `preview` fields are `None` when the server could not
/// determine a snippet for that Email. Per RFC 8620 §5.1, absent and `null`
/// are semantically equivalent for `|null` properties, so these fields are
/// omitted from serialized JSON (rather than emitted as `null`) when `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnippet {
    /// The Email id the snippet applies to.
    pub email_id: Id,
    /// Subject of the Email with matching words wrapped in `<mark>` tags, or
    /// `null` if the subject did not match the filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Relevant section of the body with matching words wrapped in `<mark>`
    /// tags (max 255 octets), or `null` if the body did not match the filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SearchSnippet {
    /// Construct a [`SearchSnippet`] with no subject or preview match.
    pub fn new(email_id: Id) -> Self {
        Self {
            email_id,
            subject: None,
            preview: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// RFC 8621 §5.1 — `SearchSnippet/get` response.
///
/// Not a standard RFC 8620 §5.1 `/get` response: there is no `state`, and
/// `notFound` is `Id[]|null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnippetGetResponse {
    /// The account used for the call.
    pub account_id: Id,
    /// One snippet per requested Email id, in no particular order.
    pub list: Vec<SearchSnippet>,
    /// Requested Email ids the server could not find, or `None` if all
    /// were found.
    pub not_found: Option<Vec<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────

    /// `SearchSnippet.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn search_snippet_preserves_vendor_extras() {
        let raw = json!({
            "emailId": "e1",
            "subject": "Hello <mark>world</mark>",
            "preview": "...the <mark>world</mark>...",
            "acmeCorpRelevanceScore": 0.87
        });
        let snip: SearchSnippet = serde_json::from_value(raw).unwrap();
        assert_eq!(
            snip.extra
                .get("acmeCorpRelevanceScore")
                .and_then(|v| v.as_f64()),
            Some(0.87)
        );
        let back = serde_json::to_value(&snip).unwrap();
        assert_eq!(back["acmeCorpRelevanceScore"], 0.87);
    }

    /// Oracle: the RFC 8621 §5.2 example response (page 71), with the
    /// line break inside the second preview joined and the third snippet,
    /// which the RFC cuts off with `...`, left out.
    #[test]
    fn search_snippet_get_response_parses_rfc8621_example() {
        let raw = json!({
            "accountId": "ue150411c",
            "list": [{
                "emailId": "M44200ec123de277c0c1ce69c",
                "subject": null,
                "preview": null
            }, {
                "emailId": "M7bcbcb0b58d7729686e83d99",
                "subject": "The <mark>Foo</mark>sball competition",
                "preview": "...year the <mark>foo</mark>sball competition will be held in the Stadium de ..."
            }],
            "notFound": null
        });
        let resp: SearchSnippetGetResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.account_id.as_ref(), "ue150411c");
        assert_eq!(resp.list.len(), 2);
        assert_eq!(resp.list[0].subject, None);
        assert_eq!(
            resp.list[1].subject.as_deref(),
            Some("The <mark>Foo</mark>sball competition")
        );
        assert_eq!(resp.not_found, None);
        assert!(resp.extra.is_empty());
    }

    /// RFC 8621 §5.1: `notFound` lists requested ids that were not found;
    /// unknown response properties land in `extra`.
    #[test]
    fn search_snippet_get_response_not_found_and_extras() {
        let raw = json!({
            "accountId": "A1",
            "list": [],
            "notFound": ["M9"],
            "filter": { "text": "foo" }
        });
        let resp: SearchSnippetGetResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(resp.not_found, Some(vec![Id::from("M9")]));
        assert_eq!(resp.extra["filter"], json!({ "text": "foo" }));
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);
    }
}

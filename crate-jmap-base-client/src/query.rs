//! [`QueryParams`]: the standard `/query` window arguments (RFC 8620 §5.5).

use jmap_types::Id;

/// The RFC 8620 §5.5 `/query` arguments shared by every `Foo/query`
/// method: which window of the result list to return and whether to
/// compute the total.
///
/// `accountId`, `filter` and `sort` are passed to each `*_query` method
/// separately because their types differ per object type; the
/// type-specific arguments some methods define (for example
/// `collapseThreads` on `Email/query`, RFC 8621 §4.4) are passed the
/// same way. Every field defaults to `None`, which omits the argument so
/// the server applies the RFC 8620 §5.5 default:
///
/// ```
/// use jmap_base_client::QueryParams;
///
/// let page = QueryParams {
///     limit: Some(50),
///     calculate_total: Some(true),
///     ..QueryParams::default()
/// };
/// # let _ = page;
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct QueryParams {
    /// `position` (RFC 8620 §5.5, `Int`, default `0`): zero-based index of
    /// the first id to return. A negative value is an offset from the end
    /// of the result list. Ignored by the server when [`Self::anchor`] is
    /// set.
    pub position: Option<i64>,
    /// `anchor` (RFC 8620 §5.5, `Id|null`): an id in the result list; the
    /// returned window starts at its index plus [`Self::anchor_offset`].
    /// The server returns `anchorNotFound` if the id is not in the
    /// results.
    pub anchor: Option<Id>,
    /// `anchorOffset` (RFC 8620 §5.5, `Int`, default `0`): index of the
    /// first result relative to [`Self::anchor`]; may be negative.
    pub anchor_offset: Option<i64>,
    /// `limit` (RFC 8620 §5.5, `UnsignedInt|null`): maximum number of ids
    /// to return. The server may clamp it to its own maximum and report
    /// the clamped value in the response.
    pub limit: Option<u64>,
    /// `calculateTotal` (RFC 8620 §5.5, `Boolean`, default `false`): ask
    /// the server to return `total` in the response.
    pub calculate_total: Option<bool>,
    /// Vendor or extension arguments not covered by the typed fields.
    ///
    /// Typed fields and the method's own `accountId` / `filter` / `sort`
    /// and type-specific arguments win on a key collision; place vendor
    /// extensions under vendor-prefixed keys (e.g. `"acmeCorpFoo"`).
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl QueryParams {
    /// Build the `/query` arguments object: [`Self::extra`], then
    /// `accountId` and every typed field that is `Some`.
    ///
    /// Callers add `filter`, `sort` and type-specific arguments to the
    /// returned object.
    pub fn to_arguments(&self, account_id: &str) -> serde_json::Value {
        let mut args = self.extra.clone();
        args.insert("accountId".into(), account_id.into());
        if let Some(p) = self.position {
            args.insert("position".into(), p.into());
        }
        if let Some(a) = &self.anchor {
            args.insert("anchor".into(), a.as_ref().into());
        }
        if let Some(o) = self.anchor_offset {
            args.insert("anchorOffset".into(), o.into());
        }
        if let Some(l) = self.limit {
            args.insert("limit".into(), l.into());
        }
        if let Some(c) = self.calculate_total {
            args.insert("calculateTotal".into(), c.into());
        }
        serde_json::Value::Object(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: RFC 8620 §5.5 argument names, hand-written JSON.
    #[test]
    fn all_fields_use_rfc8620_wire_names() {
        let params = QueryParams {
            position: Some(-5),
            anchor: Some(Id::from("m1")),
            anchor_offset: Some(-1),
            limit: Some(20),
            calculate_total: Some(true),
            extra: serde_json::Map::new(),
        };
        assert_eq!(
            params.to_arguments("acc1"),
            json!({
                "accountId": "acc1",
                "position": -5,
                "anchor": "m1",
                "anchorOffset": -1,
                "limit": 20,
                "calculateTotal": true,
            })
        );
    }

    /// Oracle: RFC 8620 §5.5 — every window argument has a default, so an
    /// all-`None` value sends only `accountId`.
    #[test]
    fn default_sends_only_account_id() {
        assert_eq!(
            QueryParams::default().to_arguments("acc1"),
            json!({ "accountId": "acc1" })
        );
    }

    /// Oracle: hand-written JSON — vendor keys pass through; a colliding
    /// key loses to the typed field and to `accountId`.
    #[test]
    fn extra_passes_through_and_typed_fields_win() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpFoo".into(), json!(1));
        extra.insert("limit".into(), json!(999));
        extra.insert("accountId".into(), json!("evil"));
        let params = QueryParams {
            limit: Some(10),
            extra,
            ..QueryParams::default()
        };
        assert_eq!(
            params.to_arguments("acc1"),
            json!({ "accountId": "acc1", "limit": 10, "acmeCorpFoo": 1 })
        );
    }
}

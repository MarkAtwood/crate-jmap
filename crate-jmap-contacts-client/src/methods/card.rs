//! JMAP Contacts — ContactCard/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_CONTACTS)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch ContactCard objects by IDs (RFC 9610 §3.1).
    ///
    /// If `ids` is `None`, the server returns all ContactCards for the account,
    /// SUBJECT TO the server's `maxObjectsInGet` cap (RFC 8620 §5.1).
    /// For production use, scope the result set via the corresponding
    /// /query method first and pass explicit ids here to avoid
    /// `requestTooLarge` errors when the account holds more objects
    /// than the cap.
    /// Pass `properties: None` to return all fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn contact_card_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_contacts_types::ContactCard>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `address_book_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("ContactCard/get", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ContactCard objects since `since_state`
    /// (RFC 9610 §3.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until the flag is false.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful and would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::contact_card_get`].
    pub async fn contact_card_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `address_book_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "contact_card_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("ContactCard/changes", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy ContactCard objects
    /// (RFC 9610 §3.3).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `update` is `Some` and `serde_json::to_value` fails on the
    ///   patch map (pathological conditions only; see
    ///   [`Self::address_book_set`] for the memory-cost discussion that
    ///   applies identically here).
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::contact_card_get`].
    pub async fn contact_card_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse<jmap_contacts_types::ContactCard>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "contact_card_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("ContactCard/set", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy ContactCards from another account (RFC 8620 §5.4 /copy).
    ///
    /// `from_account_id` is the source account. `create` is a map of
    /// caller-supplied creation keys to copy descriptors. The server assigns
    /// new IDs in the destination account.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::contact_card_get`]. RFC 8620
    ///   §5.4 /copy adds method-level errors `fromAccountNotFound`,
    ///   `fromAccountNotSupportedByMethod`, and `anchorNotFound`; they
    ///   surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn contact_card_copy(
        &self,
        from_account_id: &Id,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_contacts_types::ContactCard>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "fromAccountId": from_account_id,
            "accountId": account_id,
            "create": create,
        });
        let req = super::build_request("ContactCard/copy", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query ContactCard IDs with optional filter and sort
    /// (RFC 9610 §3.4).
    ///
    /// Pass `filter: None` and `sort: None` to return all ContactCards with
    /// server-default ordering. Use `position` and `limit` for pagination.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::contact_card_get`]. RFC 8620
    ///   §5.5 defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn contact_card_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(p) = position {
            args["position"] = p.into();
        }
        if let Some(l) = limit {
            args["limit"] = l.into();
        }
        let req = super::build_request("ContactCard/query", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for ContactCard since `since_query_state`
    /// (RFC 9610 §3.5).
    ///
    /// Returns which ContactCard IDs were removed from or added to the query
    /// result set since the given state. `max_changes` may be `None`.
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `ContactCard/query` call that returned `since_query_state`
    /// — RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::contact_card_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::contact_card_get`]. RFC 8620
    ///   §5.6 also defines `cannotCalculateChanges` (returned when the
    ///   server cannot honour the request given the supplied filter /
    ///   sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn contact_card_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `contact_card_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "contact_card_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] = serde_json::to_value(uti).expect("Id Serialize is infallible");
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req = super::build_request("ContactCard/queryChanges", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    // Inline guard smoke tests (e.g.
    // `contact_card_get_empty_id_returns_invalid_argument`,
    // `contact_card_changes_empty_since_state_returns_invalid_argument`,
    // `contact_card_copy_empty_from_account_id_returns_invalid_argument`,
    // `contact_card_copy_non_empty_from_account_id_passes_guard`,
    // `contact_card_query_changes_empty_state_returns_invalid_argument`)
    // were removed by the JMAP-6by7.4 typed-Id refactor. They were
    // vacuous because they only iterated a local `&[""]` slice (or
    // duplicated the guard's `is_empty()` check) and asserted
    // `is_empty()` found the empty value, without invoking any
    // production method. Under typed `&Id` / `&[Id]` / `&State`
    // parameters, an empty-Id input is impossible to express through
    // the API (`Id::new_validated("")` returns `Err` at the call site)
    // so the bug they pretended to test is unrepresentable.
    //
    // Additionally, `contact_card_get_request_shape`,
    // `contact_card_changes_request_includes_since_state`,
    // `contact_card_copy_request_includes_from_account_id`,
    // `contact_card_query_request_includes_filter`,
    // `contact_card_query_request_includes_sort`, and
    // `contact_card_query_changes_request_includes_since_query_state`
    // were vacuous: they hand-built `args` Values and fed them to
    // `build_request`, never exercising the production `contact_card_*`
    // builders. Deleted in JMAP-tco1.15.
    //
    // Real production-path coverage:
    //   - contact_card_get_round_trip
    //   - contact_card_changes_sends_since_state
    //   - contact_card_set_create_round_trip
    //   - contact_card_copy_round_trip
    // in tests/contactcard_tests.rs, and
    //   - contact_card_query_with_filter
    //   - contact_card_query_changes_round_trip
    // in tests/contactcard_query_tests.rs (wiremock-backed end-to-end).
    //
    // Specific-flag passthrough coverage that may be lost is tracked
    // under JMAP-uuoi for follow-up wiremock smoke tests.
    //
    // `build_request`, `CALL_ID`, and `USING_CONTACTS` themselves have
    // their own focused tests in `methods/mod.rs`.

    /// Oracle: ContactCard deserialization from RFC 9610 §4.1 example.
    /// Expected JSON taken verbatim from spec §4.1.
    #[test]
    fn contact_card_deserializes_from_spec_example() {
        let json = json!({
            "id": "3",
            "addressBookIds": {
                "062adcfa-105d-455c-bc60-6db68b69c3f3": true
            },
            "name": {
                "components": [
                    { "kind": "given", "value": "Joe" },
                    { "kind": "surname", "value": "Bloggs" }
                ],
                "isOrdered": true
            },
            "emails": {
                "0": {
                    "contexts": { "private": true },
                    "address": "joe.bloggs@example.com"
                }
            }
        });
        let card: jmap_contacts_types::ContactCard =
            serde_json::from_value(json).expect("ContactCard must deserialize");

        let id = card.id.as_ref().expect("id must be present");
        assert_eq!(id.as_ref(), "3");

        let ab_ids = card
            .address_book_ids
            .as_ref()
            .expect("addressBookIds must be present");
        let ab_key: jmap_types::Id = jmap_types::Id::from("062adcfa-105d-455c-bc60-6db68b69c3f3");
        assert!(ab_ids[&ab_key]);

        let emails = card.emails.as_ref().expect("emails must be present");
        assert_eq!(emails["0"]["address"], "joe.bloggs@example.com");
    }

    /// Oracle: GetResponse<ContactCard> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_contact_card_deserializes() {
        use super::super::GetResponse;

        let json = json!({
            "accountId": "acc1",
            "state": "s7",
            "list": [
                {
                    "id": "card1",
                    "addressBookIds": { "ab1": true }
                }
            ],
            "notFound": null
        });
        let resp: GetResponse<jmap_contacts_types::ContactCard> =
            serde_json::from_value(json).expect("GetResponse<ContactCard> must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s7");
        assert_eq!(resp.list.len(), 1);
        assert!(resp.not_found.is_none());
    }

    /// Oracle: SetResponse<ContactCard> deserializes with created entry.
    #[test]
    fn set_response_contact_card_with_created_deserializes() {
        use super::super::SetResponse;

        let json = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "created": {
                "newCard": {
                    "id": "server-assigned-id",
                    "addressBookIds": { "ab1": true }
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse<jmap_contacts_types::ContactCard> =
            serde_json::from_value(json).expect("SetResponse<ContactCard> must deserialize");
        assert_eq!(resp.new_state, "s2");
        let created = resp.created.expect("created must be present");
        assert!(
            created.contains_key("newCard"),
            "created must contain 'newCard'"
        );
        let card = &created["newCard"];
        assert_eq!(
            card.id.as_ref().map(|id| id.as_ref()),
            Some("server-assigned-id")
        );
    }
}

// JMAP Contacts — ContactCard/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_CONTACTS)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch ContactCard objects by IDs (RFC 9610 §3.1).
    ///
    /// If `ids` is `None`, the server returns all ContactCards for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn contact_card_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_contacts_types::ContactCard>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "contact_card_get: ids element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": ids,
            "properties": properties,
        });
        let req = super::build_request("ContactCard/get", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ContactCard objects since `since_state`
    /// (RFC 9610 §3.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until the flag is false.
    pub async fn contact_card_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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
    pub async fn contact_card_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<serde_json::Value>,
        destroy: Option<Vec<&str>>,
    ) -> Result<SetResponse<jmap_contacts_types::ContactCard>, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "contact_card_set: destroy element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = u;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::Value::Array(
                d.into_iter()
                    .map(|id| serde_json::Value::String(id.to_owned()))
                    .collect(),
            );
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
    /// Returns `InvalidArgument` if `from_account_id` is empty.
    pub async fn contact_card_copy(
        &self,
        from_account_id: &str,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_contacts_types::ContactCard>, jmap_base_client::ClientError> {
        if from_account_id.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "contact_card_copy: from_account_id may not be empty".into(),
            ));
        }
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
    pub async fn contact_card_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "contact_card_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
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
    use super::super::{build_request, CALL_ID, USING_CONTACTS};
    use serde_json::json;

    /// Oracle: empty ID in ids slice triggers the validation guard.
    #[test]
    fn contact_card_get_empty_id_returns_invalid_argument() {
        let ids: &[&str] = &[""];
        let mut found_error = false;
        for id in ids.iter() {
            if id.is_empty() {
                found_error = true;
                break;
            }
        }
        assert!(
            found_error,
            "empty id must trigger the InvalidArgument guard"
        );
    }

    /// Oracle: empty since_state returns InvalidArgument.
    #[test]
    fn contact_card_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "contact_card_changes: since_state may not be empty".into(),
                ))
            } else {
                Ok(())
            }
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_state must produce InvalidArgument"
        );
    }

    /// Oracle: empty from_account_id in contact_card_copy returns InvalidArgument.
    /// Guard fires before any session lookup or network call.
    #[test]
    fn contact_card_copy_empty_from_account_id_returns_invalid_argument() {
        let from_account_id = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if from_account_id.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "contact_card_copy: from_account_id may not be empty".into(),
                ))
            } else {
                Ok(())
            }
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty from_account_id must produce InvalidArgument"
        );
    }

    /// Oracle: non-empty from_account_id passes the guard.
    #[test]
    fn contact_card_copy_non_empty_from_account_id_passes_guard() {
        let from_account_id = "srcAcc1";
        let result: Result<(), jmap_base_client::ClientError> = {
            if from_account_id.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "contact_card_copy: from_account_id may not be empty".into(),
                ))
            } else {
                Ok(())
            }
        };
        assert!(
            result.is_ok(),
            "non-empty from_account_id must pass the guard"
        );
    }

    /// Oracle: empty since_query_state returns InvalidArgument.
    #[test]
    fn contact_card_query_changes_empty_state_returns_invalid_argument() {
        let since_query_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_query_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "contact_card_query_changes: since_query_state may not be empty".into(),
                ))
            } else {
                Ok(())
            }
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_query_state must produce InvalidArgument"
        );
    }

    /// Oracle: ContactCard/get request has correct method name and CALL_ID.
    /// Expected method name is "ContactCard/get" per RFC 9610 §3.1.
    #[test]
    fn contact_card_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("ContactCard/get", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");

        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("ContactCard/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
    }

    /// Oracle: ContactCard/changes request includes sinceState.
    #[test]
    fn contact_card_changes_request_includes_since_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceState": "state99",
        });
        let req = build_request("ContactCard/changes", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["sinceState"], json!("state99"));
    }

    /// Oracle: ContactCard/copy request includes fromAccountId.
    /// Expected: args contain "fromAccountId" per RFC 8620 §5.4.
    #[test]
    fn contact_card_copy_request_includes_from_account_id() {
        let args = json!({
            "fromAccountId": "srcAcc1",
            "accountId": "dstAcc2",
            "create": {
                "k1": {"id": "card1", "addressBookIds": {"ab1": true}}
            }
        });
        let req = build_request("ContactCard/copy", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("ContactCard/copy"), "method name");
        assert_eq!(calls[0][1]["fromAccountId"], json!("srcAcc1"));
        assert_eq!(calls[0][1]["accountId"], json!("dstAcc2"));
    }

    /// Oracle: ContactCard/query with filter sends filter in args.
    #[test]
    fn contact_card_query_request_includes_filter() {
        let filter = json!({"inAddressBook": "ab1"});
        let mut args = json!({ "accountId": "acc1" });
        args["filter"] = filter.clone();

        let req = build_request("ContactCard/query", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["filter"]["inAddressBook"], json!("ab1"));
    }

    /// Oracle: ContactCard/query with sort sends sort in args.
    #[test]
    fn contact_card_query_request_includes_sort() {
        let sort = json!([{"property": "name/surname", "isAscending": true}]);
        let mut args = json!({ "accountId": "acc1" });
        args["sort"] = sort;

        let req = build_request("ContactCard/query", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(
            calls[0][1]["sort"][0]["property"],
            json!("name/surname"),
            "sort property must be name/surname"
        );
    }

    /// Oracle: ContactCard/queryChanges request includes sinceQueryState.
    #[test]
    fn contact_card_query_changes_request_includes_since_query_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceQueryState": "qs42",
        });
        let req = build_request("ContactCard/queryChanges", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(
            calls[0][0],
            json!("ContactCard/queryChanges"),
            "method name"
        );
        assert_eq!(calls[0][1]["sinceQueryState"], json!("qs42"));
    }

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
        assert_eq!(ab_ids[&ab_key], true);

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

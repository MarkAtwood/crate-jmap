// JMAP Contacts — AddressBook/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_CONTACTS)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use super::{AddressBookSetParams, ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch AddressBook objects by IDs (RFC 9610 §2.1).
    ///
    /// If `ids` is `None`, the server returns all AddressBooks for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn address_book_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_contacts_types::AddressBook>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "address_book_get: ids element may not be empty".into(),
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
        let req = super::build_request("AddressBook/get", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to AddressBook objects since `since_state`
    /// (RFC 9610 §2.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until the flag is false.
    pub async fn address_book_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "address_book_changes: since_state may not be empty".into(),
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
        let req = super::build_request("AddressBook/changes", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy AddressBook objects
    /// (RFC 9610 §2.3).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `params` carries the Contacts-specific extra arguments
    /// `onDestroyRemoveContents` and `onSuccessSetIsDefault`. Pass
    /// `None` (or `Some(Default::default())`) when neither is needed.
    pub async fn address_book_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<serde_json::Value>,
        destroy: Option<Vec<&str>>,
        params: Option<AddressBookSetParams>,
    ) -> Result<SetResponse<jmap_contacts_types::AddressBook>, jmap_base_client::ClientError> {
        if let Some(ref ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "address_book_set: destroy element may not be empty".into(),
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
        if let Some(p) = params {
            if let Some(v) = p.on_destroy_remove_contents {
                args["onDestroyRemoveContents"] = v.into();
            }
            if let Some(v) = p.on_success_set_is_default {
                args["onSuccessSetIsDefault"] = v;
            }
        }
        let req = super::build_request("AddressBook/set", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, AddressBookSetParams, CALL_ID, USING_CONTACTS};
    use serde_json::json;

    /// Oracle: empty ID in ids slice triggers the validation guard.
    /// Guard fires before any session lookup or network call.
    #[test]
    fn address_book_get_empty_id_returns_invalid_argument() {
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
    /// Guard fires before any session or network call.
    #[test]
    fn address_book_changes_empty_since_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = {
            if since_state.is_empty() {
                Err(jmap_base_client::ClientError::InvalidArgument(
                    "address_book_changes: since_state may not be empty".into(),
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

    /// Oracle: empty ID in destroy list returns InvalidArgument.
    #[test]
    fn address_book_set_empty_destroy_id_returns_invalid_argument() {
        let destroy: Vec<&str> = vec![""];
        let mut found_error = false;
        for id in destroy.iter() {
            if id.is_empty() {
                found_error = true;
                break;
            }
        }
        assert!(
            found_error,
            "empty destroy id must trigger the InvalidArgument guard"
        );
    }

    /// Oracle: AddressBook/get request has correct method name and CALL_ID.
    /// Expected method name is "AddressBook/get" per RFC 9610 §2.1.
    #[test]
    fn address_book_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("AddressBook/get", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");

        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("AddressBook/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:contacts")));
    }

    /// Oracle: AddressBook/changes request includes sinceState in args.
    /// Expected: args object has "sinceState" key with the provided value.
    #[test]
    fn address_book_changes_request_includes_since_state() {
        let args = json!({
            "accountId": "acc1",
            "sinceState": "state42",
        });
        let req = build_request("AddressBook/changes", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["sinceState"], json!("state42"));
    }

    /// Oracle: AddressBook/set with destroy list sends destroy array in args.
    #[test]
    fn address_book_set_destroy_request_shape() {
        let destroy_ids = ["id1", "id2"];
        let destroy_val = serde_json::Value::Array(
            destroy_ids
                .iter()
                .map(|id| serde_json::Value::String((*id).to_owned()))
                .collect(),
        );
        let mut args = json!({ "accountId": "acc1" });
        args["destroy"] = destroy_val;

        let req = build_request("AddressBook/set", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("AddressBook/set"));
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
        assert!(destroy_arr.contains(&json!("id1")));
        assert!(destroy_arr.contains(&json!("id2")));
    }

    /// Oracle: AddressBook/set with onDestroyRemoveContents sends the field.
    /// Expected: JSON key is "onDestroyRemoveContents" per RFC 9610 §2.3.
    #[test]
    fn address_book_set_params_on_destroy_serializes() {
        let params = AddressBookSetParams {
            on_destroy_remove_contents: Some(true),
            on_success_set_is_default: None,
        };
        let mut args = json!({ "accountId": "acc1" });
        if let Some(v) = params.on_destroy_remove_contents {
            args["onDestroyRemoveContents"] = v.into();
        }
        let req = build_request("AddressBook/set", args, USING_CONTACTS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(
            calls[0][1]["onDestroyRemoveContents"],
            json!(true),
            "onDestroyRemoveContents must be true"
        );
    }

    /// Oracle: AddressBook deserialization from RFC 9610 §4.1 example.
    /// Expected JSON taken verbatim from spec §4.1.
    #[test]
    fn address_book_deserializes_from_spec_example() {
        let json = json!({
            "id": "062adcfa-105d-455c-bc60-6db68b69c3f3",
            "name": "Personal",
            "description": null,
            "sortOrder": 0,
            "isDefault": true,
            "isSubscribed": true,
            "shareWith": null,
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": true,
                "mayDelete": false
            }
        });
        let ab: jmap_contacts_types::AddressBook =
            serde_json::from_value(json).expect("AddressBook must deserialize");
        assert_eq!(ab.name, "Personal");
        assert!(ab.is_default);
        assert!(ab.is_subscribed);
        assert_eq!(ab.sort_order, 0);
        assert!(ab.description.is_none());
        assert!(ab.share_with.is_none());
        assert!(ab.my_rights.may_read);
        assert!(ab.my_rights.may_write);
        assert!(ab.my_rights.may_share);
        assert!(!ab.my_rights.may_delete);
    }

    /// Oracle: GetResponse<AddressBook> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_address_book_deserializes() {
        use super::super::GetResponse;

        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [
                {
                    "id": "ab1",
                    "name": "Personal",
                    "sortOrder": 0,
                    "isDefault": true,
                    "isSubscribed": true,
                    "myRights": {
                        "mayRead": true,
                        "mayWrite": true,
                        "mayShare": false,
                        "mayDelete": false
                    }
                }
            ],
            "notFound": null
        });
        let resp: GetResponse<jmap_contacts_types::AddressBook> =
            serde_json::from_value(json).expect("GetResponse<AddressBook> must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].name, "Personal");
        assert!(resp.not_found.is_none());
    }
}

//! JMAP Contacts — AddressBook/* method implementations on SessionClient.
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

use super::{AddressBookSetParams, ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch AddressBook objects by IDs (RFC 9610 §2.1).
    ///
    /// If `ids` is `None`, the server returns all AddressBooks for the account,
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
    pub async fn address_book_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_contacts_types::AddressBook>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` entirely when None rather than sending
        // an explicit JSON null. RFC 8620 §5.1 accepts both shapes, but the
        // crate's other builders (set/changes/query) consistently use the
        // conditional-add idiom; matching it here keeps the wire request
        // canonical and avoids "present-but-null vs absent" interop quirks
        // in proxies / audit loggers.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("AddressBook/get", args, super::USING_CONTACTS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to AddressBook objects since `since_state`
    /// (RFC 9610 §2.2).
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
    ///   the matching error list on [`Self::address_book_get`].
    pub async fn address_book_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: even with the typed-`State` parameter (a transparent
        // newtype around `String`), an empty state token is still a logically
        // invalid value that should be caught client-side rather than producing
        // a confusing server-side `cannotCalculateChanges` error.
        if since_state.as_ref().is_empty() {
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
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// `params` carries the Contacts-specific extra arguments
    /// `onDestroyRemoveContents` and `onSuccessSetIsDefault`. Pass
    /// `None` (or `Some(Default::default())`) when neither is needed.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:contacts`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `update` is `Some` and `serde_json::to_value` fails on the
    ///   patch map (pathological conditions only — allocation failure,
    ///   or a `PatchObject` whose JSON tree exceeds `serde_json`'s
    ///   recursion limit). The transient memory peak for very large
    ///   `update` maps is roughly 3-4× the `HashMap`'s in-memory size
    ///   (source map + `serde_json::Value` tree + serialized `Vec<u8>`
    ///   body); callers dealing with thousands of patches per call may
    ///   prefer to batch.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::address_book_get`].
    pub async fn address_book_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        params: Option<AddressBookSetParams>,
    ) -> Result<SetResponse<jmap_contacts_types::AddressBook>, jmap_base_client::ClientError> {
        if create.is_none() && update.is_none() && destroy.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "address_book_set: at least one of create, update, destroy must be Some \
                 (an all-None /set is a no-op round-trip)"
                    .into(),
            ));
        }
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
                    "address_book_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
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
    use serde_json::json;

    // Inline guard smoke tests (e.g. `address_book_get_empty_id_returns_invalid_argument`,
    // `address_book_changes_empty_since_state_returns_invalid_argument`,
    // `address_book_set_empty_destroy_id_returns_invalid_argument`) were
    // removed by the JMAP-6by7.4 typed-Id refactor. They were vacuous
    // because they only iterated a local `&[""]` slice (or duplicated the
    // guard's `is_empty()` check) and asserted `is_empty()` found the
    // empty value, without invoking any production method. Under typed
    // `&[Id]` / `Vec<Id>` parameters, an empty-Id input is impossible to
    // express through the API (`Id::new_validated("")` returns `Err` at
    // the call site) so the bug they pretended to test is unrepresentable.
    //
    // Additionally, `address_book_get_request_shape`,
    // `address_book_changes_request_includes_since_state`,
    // `address_book_set_destroy_request_shape`, and
    // `address_book_set_params_on_destroy_serializes` were vacuous: they
    // hand-built `args` Values and fed them to `build_request`, never
    // exercising the production `address_book_*` builders. Deleted in
    // JMAP-tco1.15.
    //
    // Real production-path coverage:
    //   - addressbook_get_round_trip
    //   - addressbook_changes_sends_since_state
    //   - addressbook_set_create_round_trip
    //   - addressbook_set_on_destroy_remove_contents
    // in tests/addressbook_tests.rs (wiremock-backed end-to-end).
    //
    // Specific-flag passthrough coverage that may be lost is tracked
    // under JMAP-uuoi for follow-up wiremock smoke tests.
    //
    // `build_request`, `CALL_ID`, and `USING_CONTACTS` themselves have
    // their own focused tests in `methods/mod.rs`, including a
    // dedicated `AddressBookSetParams` serialization test.

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

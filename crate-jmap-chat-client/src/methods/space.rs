// JMAP Chat — Space/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards, empty-slice guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_CHAT)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use jmap_types::PatchObject;

use super::{
    ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse,
    SpaceAddChannelInput, SpaceAddMemberInput, SpaceCreateInput, SpaceJoinInput, SpaceJoinResponse,
    SpacePatch, SpaceQueryInput, SpaceUpdateMemberInput,
};

impl super::SessionClient {
    /// Fetch Space objects by IDs (RFC 8620 §5.1 / JMAP Chat §Space/get).
    ///
    /// If `ids` is `None`, the server returns all Spaces for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn space_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_chat_types::Space>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "space_get: ids element may not be empty".into(),
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
        let req = super::build_request("Space/get", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Space objects since `since_state` (RFC 8620 §5.2 / Space/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    pub async fn space_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Space/changes", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy Space objects (RFC 8620 §5.3 / Space/set destroy).
    ///
    /// Permanently removes the listed Space IDs from the account.
    /// `ids` must be non-empty; the guard fires before any network call.
    pub async fn space_destroy(
        &self,
        ids: &[&str],
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_destroy: ids may not be empty".into(),
            ));
        }
        for id in ids.iter() {
            if id.is_empty() {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "space_destroy: ids element may not be empty".into(),
                ));
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": ids,
        });
        let req = super::build_request("Space/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Space IDs with optional filter (RFC 8620 §5.5 / JMAP Chat §Space/query).
    ///
    /// Only keys that are `Some` in `input` are included in the filter object;
    /// an empty filter is sent as JSON `null`.
    pub async fn space_query(
        &self,
        input: &SpaceQueryInput<'_>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut filter = serde_json::Map::new();
        if let Some(n) = input.filter_name {
            filter.insert("name".into(), n.into());
        }
        if let Some(p) = input.filter_is_public {
            filter.insert("isPublic".into(), p.into());
        }
        let filter_val = if filter.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(filter)
        };
        let mut args = serde_json::json!({
            "accountId": account_id,
            "filter": filter_val,
        });
        if let Some(p) = input.position {
            args["position"] = p.into();
        }
        if let Some(l) = input.limit {
            args["limit"] = l.into();
        }
        let req = super::build_request("Space/query", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Space since `since_query_state`
    /// (RFC 8620 §5.6 / Space/queryChanges).
    ///
    /// Returns which Space IDs were removed from or added to the query result set
    /// since the given state. `max_changes` may be `None`.
    pub async fn space_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("Space/queryChanges", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create a new Space (JMAP Chat §Space/set create).
    ///
    /// When `input.client_id` is `None`, a ULID is generated automatically.
    /// The server maps the creation key to the server-assigned Space id in
    /// `SetResponse.created`.
    pub async fn space_create(
        &self,
        input: &SpaceCreateInput<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if input.name.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_create: name may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let client_id = super::resolve_client_id(input.client_id);
        let mut create_obj = serde_json::json!({ "name": input.name });
        if let Some(d) = input.description {
            create_obj["description"] = d.into();
        }
        if let Some(b) = input.icon_blob_id {
            if b.is_empty() {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "space_create: icon_blob_id may not be empty".into(),
                ));
            }
            create_obj["iconBlobId"] = b.into();
        }
        let args = serde_json::json!({
            "accountId": account_id,
            "create": { client_id: create_obj },
        });
        let req = super::build_request("Space/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Join a Space via invite code or direct ID (JMAP Chat §Space/join).
    ///
    /// `input` selects exactly one join path; the enum makes invalid inputs
    /// unrepresentable at the type level.
    pub async fn space_join(
        &self,
        input: &SpaceJoinInput<'_>,
    ) -> Result<SpaceJoinResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({ "accountId": account_id });
        match input {
            SpaceJoinInput::InviteCode(ic) => {
                if ic.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "space_join: invite_code may not be empty".into(),
                    ));
                }
                args["inviteCode"] = (*ic).into();
            }
            SpaceJoinInput::SpaceId(sid) => {
                if sid.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "space_join: space_id may not be empty".into(),
                    ));
                }
                args["spaceId"] = (*sid).into();
            }
        }
        let req = super::build_request("Space/join", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update Space properties (JMAP Chat §Space/set update).
    ///
    /// Issues an `update` operation patching only the fields present in `patch`.
    /// Use `Patch::Set(v)` to set nullable fields, `Patch::Clear` to null-clear
    /// them, and `Patch::Keep` (default) to leave them unchanged. Slice fields
    /// default to `None` for no-change.
    ///
    /// **Out of scope**: `addRoles`, `removeRoles`, `updateRoles`,
    /// `updateChannels`, `addCategories`, `removeCategories`, `updateCategories`
    /// are not included. Role and category management will be added in a future
    /// iteration of this API.
    pub async fn space_update(
        &self,
        id: &str,
        patch: &SpacePatch<'_>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if id.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "space_update: id may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut patch_map = serde_json::Map::new();

        if let Some(n) = patch.name {
            patch_map.insert("name".into(), n.into());
        }
        if let Some(entry) = patch
            .description
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("description".into(), entry);
        }
        if let Some(entry) = patch
            .icon_blob_id
            .map_entry()
            .map_err(jmap_base_client::ClientError::Parse)?
        {
            patch_map.insert("iconBlobId".into(), entry);
        }
        if let Some(ip) = patch.is_public {
            patch_map.insert("isPublic".into(), ip.into());
        }
        if let Some(ipp) = patch.is_publicly_previewable {
            patch_map.insert("isPubliclyPreviewable".into(), ipp.into());
        }
        if let Some(members) = patch.add_members {
            if !members.is_empty() {
                let arr: Vec<serde_json::Value> = members
                    .iter()
                    .map(
                        |m: &SpaceAddMemberInput<'_>| -> Result<
                            serde_json::Value,
                            jmap_base_client::ClientError,
                        > {
                            if m.id.is_empty() {
                                return Err(jmap_base_client::ClientError::InvalidArgument(
                                    "space_update: addMembers member id may not be empty".into(),
                                ));
                            }
                            let mut obj = serde_json::json!({ "id": m.id });
                            if let Some(role_ids) = m.role_ids {
                                for rid in role_ids.iter() {
                                    if rid.is_empty() {
                                        return Err(
                                            jmap_base_client::ClientError::InvalidArgument(
                                                "space_update: addMembers role_ids element may \
                                                 not be empty"
                                                    .into(),
                                            ),
                                        );
                                    }
                                }
                                obj["roleIds"] = serde_json::Value::Array(
                                    role_ids
                                        .iter().copied().map(serde_json::Value::from).collect(),
                                );
                            }
                            Ok(obj)
                        },
                    )
                    .collect::<Result<Vec<_>, _>>()?;
                patch_map.insert("addMembers".into(), serde_json::Value::Array(arr));
            }
        }
        if let Some(rm) = patch.remove_members {
            if !rm.is_empty() {
                for rid in rm.iter() {
                    if rid.is_empty() {
                        return Err(jmap_base_client::ClientError::InvalidArgument(
                            "space_update: removeMembers id may not be empty".into(),
                        ));
                    }
                }
                patch_map.insert(
                    "removeMembers".into(),
                    serde_json::Value::Array(
                        rm.iter().copied().map(serde_json::Value::from).collect(),
                    ),
                );
            }
        }
        if let Some(um) = patch.update_members {
            if !um.is_empty() {
                let arr: Vec<serde_json::Value> = um
                    .iter()
                    .map(
                        |u: &SpaceUpdateMemberInput<'_>| -> Result<
                            serde_json::Value,
                            jmap_base_client::ClientError,
                        > {
                            if u.id.is_empty() {
                                return Err(jmap_base_client::ClientError::InvalidArgument(
                                    "space_update: updateMembers member id may not be empty"
                                        .into(),
                                ));
                            }
                            let mut obj = serde_json::json!({ "id": u.id });
                            if let Some(role_ids) = u.role_ids {
                                for rid in role_ids.iter() {
                                    if rid.is_empty() {
                                        return Err(
                                            jmap_base_client::ClientError::InvalidArgument(
                                                "space_update: updateMembers role_ids element \
                                                 may not be empty"
                                                    .into(),
                                            ),
                                        );
                                    }
                                }
                                obj["roleIds"] = serde_json::Value::Array(
                                    role_ids
                                        .iter().copied().map(serde_json::Value::from).collect(),
                                );
                            }
                            if let Some(entry) = u
                                .nick
                                .map_entry()
                                .map_err(jmap_base_client::ClientError::Parse)?
                            {
                                obj["nick"] = entry;
                            }
                            Ok(obj)
                        },
                    )
                    .collect::<Result<Vec<_>, _>>()?;
                patch_map.insert("updateMembers".into(), serde_json::Value::Array(arr));
            }
        }
        if let Some(ac) = patch.add_channels {
            if !ac.is_empty() {
                let arr = ac
                    .iter()
                    .map(|c: &SpaceAddChannelInput<'_>| {
                        if c.name.is_empty() {
                            return Err(jmap_base_client::ClientError::InvalidArgument(
                                "space_update: channel name may not be empty".into(),
                            ));
                        }
                        let mut obj = serde_json::json!({ "name": c.name });
                        if let Some(cat) = c.category_id {
                            if cat.is_empty() {
                                return Err(jmap_base_client::ClientError::InvalidArgument(
                                    "space_update: category_id may not be empty".into(),
                                ));
                            }
                            obj["categoryId"] = cat.into();
                        }
                        if let Some(pos) = c.position {
                            obj["position"] = pos.into();
                        }
                        if let Some(t) = c.topic {
                            obj["topic"] = t.into();
                        }
                        Ok(obj)
                    })
                    .collect::<Result<Vec<serde_json::Value>, jmap_base_client::ClientError>>()?;
                patch_map.insert("addChannels".into(), serde_json::Value::Array(arr));
            }
        }
        if let Some(rc) = patch.remove_channels {
            if !rc.is_empty() {
                for cid in rc.iter() {
                    if cid.is_empty() {
                        return Err(jmap_base_client::ClientError::InvalidArgument(
                            "space_update: removeChannels id may not be empty".into(),
                        ));
                    }
                }
                patch_map.insert(
                    "removeChannels".into(),
                    serde_json::Value::Array(
                        rc.iter().copied().map(serde_json::Value::from).collect(),
                    ),
                );
            }
        }

        // Wrap the constructed map in a PatchObject (RFC 8620 §5.3) before
        // serializing. Wire bytes are unchanged because PatchObject is
        // #[serde(transparent)]; the typed boundary documents the contract.
        let patch_value = serde_json::Value::Object(PatchObject::from_map(patch_map).into_inner());
        let args = serde_json::json!({
            "accountId": account_id,
            "update": { id: patch_value },
        });
        let req = super::build_request("Space/set", args, super::USING_CHAT);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

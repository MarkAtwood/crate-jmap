//! [`SessionClient`] and its `Quota/*` methods (RFC 9425 §4).

use jmap_quotas_types::{JMAP_QUOTA_URI, Quota, QuotaComparator, QuotaFilter};
use jmap_types::{Id, State};
use serde_json::{Map, Value, json};

/// RFC 8620 §5 response shapes, shared by every `jmap-*-client` crate.
pub use jmap_types::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse,
};

const CALL_ID: &str = "r1";
const CORE: &str = "urn:ietf:params:jmap:core";

/// A [`jmap_base_client::JmapClient`] bound to a Session, for `Quota/*`
/// calls on the primary account for `urn:ietf:params:jmap:quota`.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
    /// `None`: every capability in the Session.
    pub(crate) type_capabilities: Option<Vec<String>>,
    pub(crate) i18n: Option<jmap_base_client::I18nSetting>,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .field("type_capabilities", &self.type_capabilities)
            .finish()
    }
}

impl SessionClient {
    /// The underlying client.
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// The bound Session.
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// Send only `capabilities`, besides `core` and `quota`, in `using`, so
    /// that Quotas show only the data types those capabilities cover
    /// (RFC 9425 §4.1). For example, `["urn:ietf:params:jmap:mail"]` shows
    /// mail quotas with `types` holding only mail types.
    ///
    /// Without this call `using` holds every capability the Session
    /// advertises.
    #[must_use]
    pub fn with_type_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.type_capabilities = Some(capabilities.into_iter().map(Into::into).collect());
        self
    }

    /// Use `urn:ietf:params:jmap:i18n` (draft-atwood-jmap-updates-00) on
    /// every call, with `language` as each call's `language` argument.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// when the Session does not advertise the capability.
    pub fn with_i18n(
        mut self,
        language: Option<Vec<jmap_types::i18n::LanguageRange>>,
    ) -> Result<Self, jmap_base_client::ClientError> {
        self.i18n = Some(jmap_base_client::I18nSetting::new(&self.session, language)?);
        Ok(self)
    }

    /// The primary account for `urn:ietf:params:jmap:quota`.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    /// when the Session names none.
    pub fn quota_account_id(&self) -> Result<&Id, jmap_base_client::ClientError> {
        self.session
            .primary_account_id(JMAP_QUOTA_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(format!(
                    "no primary account for {JMAP_QUOTA_URI}"
                ))
            })
    }

    /// The `using` list: `core`, `quota`, then the type capabilities.
    fn using(&self) -> Vec<String> {
        let mut using = vec![CORE.to_owned(), JMAP_QUOTA_URI.to_owned()];
        let extra: Vec<String> = match &self.type_capabilities {
            Some(caps) => caps.clone(),
            None => self.session.capabilities.keys().cloned().collect(),
        };
        for cap in extra {
            if !using.contains(&cap) {
                using.push(cap);
            }
        }
        using
    }

    /// Send one method call with `args` (plus `accountId`) and decode its
    /// response.
    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        mut args: Value,
    ) -> Result<T, jmap_base_client::ClientError> {
        let account_id = self.quota_account_id()?;
        args["accountId"] = json!(account_id);
        if let Some(i18n) = &self.i18n {
            i18n.add_language(&mut args);
        }
        let mut req = jmap_types::JmapRequest::new(
            self.using(),
            vec![jmap_types::Invocation::new(method, args, CALL_ID)],
            None,
        );
        if let Some(i18n) = &self.i18n {
            i18n.add_to_using(&mut req);
        }
        let resp = self
            .client
            .call(self.session.api_url.as_str(), &req)
            .await?;
        jmap_base_client::extract_response(&resp, CALL_ID)
    }

    /// `Quota/get` (RFC 9425 §4.2) for `ids`, or every Quota when `None`,
    /// with every property.
    ///
    /// # Errors
    ///
    /// Transport, session and method errors as [`jmap_base_client::ClientError`].
    pub async fn quota_get(
        &self,
        ids: Option<&[Id]>,
    ) -> Result<GetResponse<Quota>, jmap_base_client::ClientError> {
        self.call("Quota/get", json!({ "ids": ids })).await
    }

    /// `Quota/get` returning only `properties` (plus `id`) of each Quota, as
    /// JSON objects: for example the `updatedProperties` of
    /// [`quota_changes`](Self::quota_changes), as in RFC 9425 §5.2.
    ///
    /// # Errors
    ///
    /// Transport, session and method errors as [`jmap_base_client::ClientError`].
    pub async fn quota_get_properties(
        &self,
        ids: Option<&[Id]>,
        properties: &[&str],
    ) -> Result<GetResponse<Map<String, Value>>, jmap_base_client::ClientError> {
        self.call("Quota/get", json!({ "ids": ids, "properties": properties }))
            .await
    }

    /// `Quota/changes` (RFC 9425 §4.3). The response's `updated_properties`
    /// is `Some(Some(["used"]))` when only usage changed.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    /// for an empty `since_state`; otherwise transport, session and method
    /// errors.
    pub async fn quota_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "quota_changes: since_state may not be empty".into(),
            ));
        }
        let mut args = json!({ "sinceState": since_state });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        self.call("Quota/changes", args).await
    }

    /// `Quota/query` (RFC 9425 §4.4).
    ///
    /// # Errors
    ///
    /// Transport, session and method errors as [`jmap_base_client::ClientError`].
    pub async fn quota_query(
        &self,
        filter: Option<&QuotaFilter>,
        sort: Option<&[QuotaComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let account_id = self.quota_account_id()?;
        let mut args = params.to_arguments(account_id.as_ref());
        if let Some(f) = filter {
            args["filter"] = jmap_base_client::to_args(f)?;
        }
        if let Some(s) = sort {
            args["sort"] = jmap_base_client::to_args(s)?;
        }
        self.call("Quota/query", args).await
    }

    /// `Quota/queryChanges` (RFC 9425 §4.5).
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    /// for an empty `since_query_state`; otherwise transport, session and
    /// method errors.
    pub async fn quota_query_changes(
        &self,
        since_query_state: &State,
        filter: Option<&QuotaFilter>,
        sort: Option<&[QuotaComparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        calculate_total: bool,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "quota_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let mut args = json!({ "sinceQueryState": since_query_state });
        if let Some(f) = filter {
            args["filter"] = jmap_base_client::to_args(f)?;
        }
        if let Some(s) = sort {
            args["sort"] = jmap_base_client::to_args(s)?;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(id) = up_to_id {
            args["upToId"] = json!(id);
        }
        if calculate_total {
            args["calculateTotal"] = true.into();
        }
        self.call("Quota/queryChanges", args).await
    }
}

#[allow(dead_code)]
fn _assert_session_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionClient>();
}

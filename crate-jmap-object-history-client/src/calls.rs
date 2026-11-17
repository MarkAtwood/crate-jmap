//! The [`SessionClient`] calls and their parameter structs.

use jmap_base_client::{
    ClientError, JmapClient, JmapRequestBuilder, Session, SetCall, extract_response, to_args,
};
use jmap_object_history_types::{
    DestroyedQueryArguments, HistoryEntry, HistoryGetArguments, JMAP_OBJECT_HISTORY_URI,
    ObjectHistoryAccountCapability, RestoreOutcome,
};
use jmap_types::{
    GetResponse, Id, PartialObject, QueryChangesResponse, QueryResponse, SetResponse, UTCDate,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// A history `Foo/get` (§3): the account, the data type's capability URI,
/// `ids`, `properties`, and the history arguments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryGetCall {
    /// `accountId`.
    pub account_id: Id,
    /// The data type's capability URI (e.g. `urn:ietf:params:jmap:mail`);
    /// added to `using` with the object-history URI.
    pub type_capability: String,
    /// `ids`; `None` for every object (and, with `includeDestroyed`, every
    /// destroyed one).
    pub ids: Option<Vec<Id>>,
    /// `properties`; `None` for the type's default. `objectHistory` is
    /// returned whatever this says.
    pub properties: Option<Vec<String>>,
    /// The §3 arguments.
    pub arguments: HistoryGetArguments,
}

impl HistoryGetCall {
    /// A history `/get` of every object in `account_id`.
    pub fn new(
        account_id: Id,
        type_capability: impl Into<String>,
        arguments: HistoryGetArguments,
    ) -> Self {
        Self {
            account_id,
            type_capability: type_capability.into(),
            ids: None,
            properties: None,
            arguments,
        }
    }

    /// Set [`ids`](Self::ids).
    #[must_use]
    pub fn ids(mut self, ids: Option<Vec<Id>>) -> Self {
        self.ids = ids;
        self
    }

    /// Set [`properties`](Self::properties).
    #[must_use]
    pub fn properties(mut self, properties: Option<Vec<String>>) -> Self {
        self.properties = properties;
        self
    }
}

/// A destroyed `Foo/query` (§4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct DestroyedQueryCall {
    /// `accountId`.
    pub account_id: Id,
    /// The data type's capability URI.
    pub type_capability: String,
    /// `filter`, applied to each object's final version; `None` for none.
    pub filter: Option<Value>,
    /// `sort`; `None` orders by destruction time, most recent first.
    pub sort: Option<Value>,
    /// `position`.
    pub position: Option<i64>,
    /// `limit`.
    pub limit: Option<u64>,
    /// `calculateTotal`.
    pub calculate_total: bool,
    /// `destroyedAfter` and `destroyedBefore`; `onlyDestroyed` is always
    /// sent as true.
    pub arguments: DestroyedQueryArguments,
}

impl DestroyedQueryCall {
    /// A destroyed query of every destroyed object in `account_id`.
    pub fn new(account_id: Id, type_capability: impl Into<String>) -> Self {
        Self {
            account_id,
            type_capability: type_capability.into(),
            filter: None,
            sort: None,
            position: None,
            limit: None,
            calculate_total: false,
            arguments: DestroyedQueryArguments::new(),
        }
    }

    /// Set the window (`destroyedAfter`, `destroyedBefore`).
    #[must_use]
    pub fn window(mut self, after: Option<UTCDate>, before: Option<UTCDate>) -> Self {
        self.arguments.destroyed_after = after;
        self.arguments.destroyed_before = before;
        self
    }

    /// Set [`limit`](Self::limit).
    #[must_use]
    pub fn limit(mut self, limit: Option<u64>) -> Self {
        self.limit = limit;
        self
    }

    /// Set [`filter`](Self::filter).
    #[must_use]
    pub fn filter(mut self, filter: Option<Value>) -> Self {
        self.filter = filter;
        self
    }
}

/// A destroyed `Foo/queryChanges` (-04 §4): the window and filter of the
/// `Foo/query` it follows, which the client "MUST pass".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct DestroyedQueryChangesCall {
    /// The `Foo/query` this continues: account, type, filter, sort and
    /// window.
    pub query: DestroyedQueryCall,
    /// `sinceQueryState`.
    pub since_query_state: String,
    /// `maxChanges`.
    pub max_changes: Option<u64>,
}

impl DestroyedQueryChangesCall {
    /// Changes to `query`'s results since `since_query_state`.
    pub fn new(query: DestroyedQueryCall, since_query_state: impl Into<String>) -> Self {
        Self {
            query,
            since_query_state: since_query_state.into(),
            max_changes: None,
        }
    }
}

/// The object-history calls bound to a session, from
/// [`JmapObjectHistoryExt::with_object_history_session`](crate::JmapObjectHistoryExt::with_object_history_session).
#[derive(Clone)]
pub struct SessionClient {
    client: JmapClient,
    session: Session,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            .field("api_url", &self.session.api_url.as_str())
            .finish_non_exhaustive()
    }
}

impl SessionClient {
    pub(crate) fn new(client: JmapClient, session: Session) -> Self {
        Self { client, session }
    }

    /// The `urn:ietf:params:jmap:object-history` capability of
    /// `account_id` (§2.1), or `None` when the account has none.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] when the object does not parse.
    pub fn capability(
        &self,
        account_id: &Id,
    ) -> Result<Option<ObjectHistoryAccountCapability>, ClientError> {
        let Some(value) = self
            .session
            .accounts
            .get(account_id)
            .and_then(|a| a.account_capabilities.get(JMAP_OBJECT_HISTORY_URI))
        else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| ClientError::InvalidSession(format!("{JMAP_OBJECT_HISTORY_URI}: {e}")))
    }

    /// Whether `account_id` keeps history for `type_name`. A client "MUST
    /// ignore type names [it does] not recognise" (§2.1): this is a string
    /// lookup only.
    ///
    /// # Errors
    ///
    /// As [`capability`](Self::capability).
    pub fn supports(&self, account_id: &Id, type_name: &str) -> Result<bool, ClientError> {
        Ok(self
            .capability(account_id)?
            .is_some_and(|c| c.supports_type(type_name)))
    }

    async fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        type_capability: &str,
        args: Value,
    ) -> Result<R, ClientError> {
        let using = [
            "urn:ietf:params:jmap:core",
            type_capability,
            JMAP_OBJECT_HISTORY_URI,
        ];
        let mut builder = JmapRequestBuilder::new(&using);
        builder.add_call(method, args, "0")?;
        let response = self
            .client
            .call_session(&self.session, &builder.build()?)
            .await?;
        extract_response(&response, "0")
    }

    /// `<type_name>/get` with the §3 history arguments. Each list entry is
    /// a `T` (a typed object, or a JSON map) with its `objectHistory`;
    /// entries of one id ascend by version.
    ///
    /// # Errors
    ///
    /// Transport and parse errors; [`ClientError::MethodError`] for a
    /// method error (`invalidArguments` when the account does not list the
    /// type).
    pub async fn get_history<T: DeserializeOwned>(
        &self,
        type_name: &str,
        call: &HistoryGetCall,
    ) -> Result<GetResponse<HistoryEntry<T>>, ClientError> {
        let mut args = match to_args(&call.arguments)? {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        args.insert("accountId".to_owned(), to_args(&call.account_id)?);
        args.insert("ids".to_owned(), to_args(&call.ids)?);
        if let Some(props) = &call.properties {
            args.insert("properties".to_owned(), to_args(props)?);
        }
        self.call(
            &format!("{type_name}/get"),
            &call.type_capability,
            Value::Object(args),
        )
        .await
    }

    fn query_args(call: &DestroyedQueryCall) -> Result<Map<String, Value>, ClientError> {
        let mut args = match to_args(&call.arguments)? {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        args.insert("accountId".to_owned(), to_args(&call.account_id)?);
        args.insert("onlyDestroyed".to_owned(), Value::Bool(true));
        if let Some(filter) = &call.filter {
            args.insert("filter".to_owned(), filter.clone());
        }
        if let Some(sort) = &call.sort {
            args.insert("sort".to_owned(), sort.clone());
        }
        Ok(args)
    }

    /// `<type_name>/query` over destroyed objects (§4).
    ///
    /// # Errors
    ///
    /// As [`get_history`](Self::get_history), plus `unsupportedFilter` and
    /// `unsupportedSort` from a server that does not evaluate them for
    /// destroyed objects.
    pub async fn query_destroyed(
        &self,
        type_name: &str,
        call: &DestroyedQueryCall,
    ) -> Result<QueryResponse, ClientError> {
        let mut args = Self::query_args(call)?;
        if let Some(position) = call.position {
            args.insert("position".to_owned(), Value::from(position));
        }
        if let Some(limit) = call.limit {
            args.insert("limit".to_owned(), Value::from(limit));
        }
        if call.calculate_total {
            args.insert("calculateTotal".to_owned(), Value::Bool(true));
        }
        self.call(
            &format!("{type_name}/query"),
            &call.type_capability,
            Value::Object(args),
        )
        .await
    }

    /// `<type_name>/queryChanges` for a destroyed query (-04 §4), with the
    /// window and filter the query used.
    ///
    /// # Errors
    ///
    /// As [`query_destroyed`](Self::query_destroyed), plus
    /// `cannotCalculateChanges`.
    pub async fn query_destroyed_changes(
        &self,
        type_name: &str,
        call: &DestroyedQueryChangesCall,
    ) -> Result<QueryChangesResponse, ClientError> {
        let mut args = Self::query_args(&call.query)?;
        args.insert(
            "sinceQueryState".to_owned(),
            Value::String(call.since_query_state.clone()),
        );
        if let Some(max) = call.max_changes {
            args.insert("maxChanges".to_owned(), Value::from(max));
        }
        self.call(
            &format!("{type_name}/queryChanges"),
            &call.query.type_capability,
            Value::Object(args),
        )
        .await
    }

    /// `<type_name>/set` with `call.restore` (§5.1), and any creates,
    /// updates, destroys and conditional options `call` carries: restores
    /// run first. `using` gets `extra_using` (the type's capability) and
    /// the object-history URI. Returns the `/set` response and its
    /// `restored`/`notRestored`.
    ///
    /// Under conditional `atomic`, a failed restore fails the whole call
    /// with `atomicFailure` (decode it with
    /// `jmap_base_client::ConditionalError::from_method_error`; its
    /// `AtomicFailure::not_restored` lists the failed restores).
    ///
    /// # Errors
    ///
    /// Those of `JmapClient::set_call`, and a parse error for a malformed
    /// `restored`/`notRestored`.
    pub async fn restore(
        &self,
        type_name: &str,
        call: &SetCall,
        extra_using: &[&str],
    ) -> Result<(SetResponse<PartialObject>, RestoreOutcome), ClientError> {
        let mut using: Vec<&str> = extra_using.to_vec();
        if !using.contains(&JMAP_OBJECT_HISTORY_URI) {
            using.push(JMAP_OBJECT_HISTORY_URI);
        }
        let response: SetResponse<PartialObject> = self
            .client
            .set_call(&self.session, &format!("{type_name}/set"), &using, call)
            .await?;
        let outcome = RestoreOutcome::from_set_extra(&response.extra)
            .map_err(|e| ClientError::UnexpectedResponse(format!("restored/notRestored: {e}")))?;
        Ok((response, outcome))
    }
}

/// The `replaced` of the oldest entry in `entries`, exactly as received,
/// to pass as `historyBefore` for the next older page. `entries` are one
/// id's: §3 has "a client paging through the history of several ids makes
/// a separate Foo/get call for each id", and clients "SHOULD pass the
/// replaced value exactly as received". `None` when no entry was
/// replaced.
pub fn older_page_before<T>(entries: &[HistoryEntry<T>]) -> Option<&UTCDate> {
    entries
        .iter()
        .min_by_key(|e| e.object_history.version)
        .and_then(|e| e.object_history.replaced.as_ref())
}

/// Whether the server said it retains a version older than the oldest of
/// `entries` (one id's): `hasOlder` on that entry (§3.1). `false` promises
/// nothing ("The server MAY return false even if older versions exist"),
/// and `true` does not promise the next request finds them.
pub fn has_older<T>(entries: &[HistoryEntry<T>]) -> bool {
    entries
        .iter()
        .min_by_key(|e| e.object_history.version)
        .is_some_and(|e| e.object_history.has_older)
}

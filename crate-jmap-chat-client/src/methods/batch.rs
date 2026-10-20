//! Typed batching: several typed method calls in one JMAP request, with
//! RFC 8620 §3.7 result references for the query-then-get pattern.

use std::marker::PhantomData;

use jmap_types::{Invocation, JmapRequest, JmapResponse, ResultReference};

use jmap_types::query::Filter;
use serde::Serialize;

use super::{GetResponse, QueryResponse};

/// Several typed method calls sent as one JMAP request (RFC 8620 §3.3).
///
/// Obtain with [`SessionClient::batch`](super::SessionClient::batch), add
/// calls, then send with
/// [`SessionClient::send_batch`](super::SessionClient::send_batch). Each
/// adder returns [`CallHandle`]s; read each typed result from the
/// [`BatchResponse`] with [`BatchResponse::get`].
///
/// ```rust,no_run
/// # use jmap_chat_client::SessionClient;
/// # use jmap_chat_types::query::{ChatFilter, MessageFilter, MessageFilterCondition};
/// # async fn example(sc: SessionClient, chat_id: jmap_types::Id) -> Result<(), jmap_base_client::ClientError> {
/// let mut batch = sc.batch()?;
/// let (_, chats) =
///     batch.chat_query_get(None::<&ChatFilter>, None, None, None, None, None, None, None)?;
/// let mut cond = MessageFilterCondition::default();
/// cond.chat_id = Some(chat_id);
/// let filter = MessageFilter::Condition(cond);
/// let (_, messages) =
///     batch.message_query_get(Some(&filter), None, None, Some(50), None, None, None, None)?;
/// let resp = sc.send_batch(batch).await?;
/// let chats = resp.get(&chats)?;
/// let messages = resp.get(&messages)?;
/// # let _ = (chats, messages);
/// # Ok(())
/// # }
/// ```
///
/// # Limits
///
/// - Only the `Chat/query` → `Chat/get` and `Message/query` →
///   `Message/get` pairs are offered. For other methods, build a
///   [`JmapRequest`] by hand and send it with
///   [`JmapClient::call`](jmap_base_client::JmapClient::call).
/// - Every call targets the account the [`SessionClient`](super::SessionClient)
///   targeted when [`batch`](super::SessionClient::batch) was called.
/// - The /get is not paged: it fetches every id the /query returns, so
///   pass a `limit` to stay within the server's
///   `maxObjectsInGet` (RFC 8620 §2).
/// - The server's `maxCallsInRequest` (RFC 8620 §2) is not checked on the
///   client. Each pair is two calls; exceeding the cap fails the whole
///   request with the `urn:ietf:params:jmap:error:limit` request-level
///   error (RFC 8620 §3.6.1), returned by `send_batch`.
/// - Call ids are positional (`t0`, `t1`, …), so a [`CallHandle`] is
///   meaningful only for the response to the batch that issued it.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Batch {
    account_id: String,
    calls: Vec<Invocation>,
}

/// Reference to one call in a [`Batch`]; `T` is that call's response type.
#[derive(Debug)]
pub struct CallHandle<T> {
    call_id: String,
    _response: PhantomData<fn() -> T>,
}

impl<T> CallHandle<T> {
    fn new(call_id: String) -> Self {
        Self {
            call_id,
            _response: PhantomData,
        }
    }
}

/// The JMAP response to a sent [`Batch`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct BatchResponse {
    response: JmapResponse,
}

impl BatchResponse {
    /// Extract the typed result of the call `handle` refers to.
    ///
    /// # Errors
    ///
    /// - [`ClientError::MethodError`](jmap_base_client::ClientError::MethodError)
    ///   if the server answered that call with an error. When a /query
    ///   fails, its paired /get fails with `invalidResultReference`
    ///   (RFC 8620 §3.7); calls in other pairs are unaffected.
    /// - Any other error from
    ///   [`extract_response`](jmap_base_client::extract_response), such
    ///   as [`Parse`](jmap_base_client::ClientError::Parse) when the
    ///   result does not match `T`.
    pub fn get<T: serde::de::DeserializeOwned>(
        &self,
        handle: &CallHandle<T>,
    ) -> Result<T, jmap_base_client::ClientError> {
        jmap_base_client::extract_response(&self.response, &handle.call_id)
    }
}

impl Batch {
    /// Add `Chat/query` (built as by
    /// [`SessionClient::chat_query`](super::SessionClient::chat_query))
    /// and a `Chat/get` whose `ids` is a `#ids` result reference to the
    /// query's `/ids` (RFC 8620 §3.7). `properties: None` returns all
    /// fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::Parse`](jmap_base_client::ClientError::Parse) if
    ///   serializing `filter` or `sort` fails, as for `chat_query`. Nothing is
    ///   added on error.
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    pub fn chat_query_get<C: Serialize>(
        &mut self,
        filter: Option<&Filter<C>>,
        sort: Option<&[jmap_chat_types::query::ChatComparator]>,
        position: Option<u64>,
        limit: Option<u64>,
        anchor: Option<&jmap_types::Id>,
        anchor_offset: Option<i64>,
        calculate_total: Option<bool>,
        properties: Option<&[&str]>,
    ) -> Result<
        (
            CallHandle<QueryResponse>,
            CallHandle<GetResponse<jmap_chat_types::Chat>>,
        ),
        jmap_base_client::ClientError,
    > {
        let args = super::chat::chat_query_args(
            &self.account_id,
            filter,
            sort,
            position,
            limit,
            anchor,
            anchor_offset,
            calculate_total,
        )?;
        Ok(self.query_get("Chat/query", args, "Chat/get", properties))
    }

    /// Add `Message/query` (built as by
    /// [`SessionClient::message_query`](super::SessionClient::message_query))
    /// and a `Message/get` whose `ids` is a `#ids` result reference to the
    /// query's `/ids` (RFC 8620 §3.7). `properties: None` returns all
    /// fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   or [`ClientError::Parse`](jmap_base_client::ClientError::Parse)
    ///   for the same inputs `message_query` rejects. Nothing is added on
    ///   error.
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    pub fn message_query_get<C: Serialize>(
        &mut self,
        filter: Option<&Filter<C>>,
        sort: Option<&[jmap_chat_types::query::MessageComparator]>,
        position: Option<u64>,
        limit: Option<u64>,
        anchor: Option<&jmap_types::Id>,
        anchor_offset: Option<i64>,
        calculate_total: Option<bool>,
        properties: Option<&[&str]>,
    ) -> Result<
        (
            CallHandle<QueryResponse>,
            CallHandle<GetResponse<jmap_chat_types::Message>>,
        ),
        jmap_base_client::ClientError,
    > {
        let args = super::message::message_query_args(
            &self.account_id,
            filter,
            sort,
            position,
            limit,
            anchor,
            anchor_offset,
            calculate_total,
        )?;
        Ok(self.query_get("Message/query", args, "Message/get", properties))
    }

    fn query_get<G>(
        &mut self,
        query_method: &str,
        query_args: serde_json::Value,
        get_method: &str,
        properties: Option<&[&str]>,
    ) -> (CallHandle<QueryResponse>, CallHandle<G>) {
        let query_id = self.push(query_method, query_args);
        let mut get_args = serde_json::json!({
            "accountId": self.account_id,
            "#ids": ResultReference::new(query_id.as_str(), query_method, "/ids"),
        });
        if let Some(props) = properties {
            get_args["properties"] = props.into();
        }
        let get_id = self.push(get_method, get_args);
        (CallHandle::new(query_id), CallHandle::new(get_id))
    }

    fn push(&mut self, method: &str, args: serde_json::Value) -> String {
        let call_id = format!("t{}", self.calls.len());
        self.calls
            .push(Invocation::new(method.to_owned(), args, call_id.clone()));
        call_id
    }
}

impl super::SessionClient {
    /// Start an empty [`Batch`] targeting this handle's chat account (see
    /// [`Self::chat_account_id`]).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   under the same condition as [`Self::chat_account_id`].
    pub fn batch(&self) -> Result<Batch, jmap_base_client::ClientError> {
        Ok(Batch {
            account_id: self.chat_account_id()?.to_owned(),
            calls: Vec::new(),
        })
    }

    /// Send every call in `batch` as one JMAP request.
    ///
    /// A method-level error in one call does not fail this method; it
    /// surfaces from [`BatchResponse::get`] for that call's handle.
    ///
    /// # Errors
    ///
    /// Transport and request-level errors from
    /// [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    /// a request-level `limit` error when the batch exceeds the server's
    /// `maxCallsInRequest`.
    pub async fn send_batch(
        &self,
        batch: Batch,
    ) -> Result<BatchResponse, jmap_base_client::ClientError> {
        let using = super::USING_CHAT.iter().map(|&s| s.to_owned()).collect();
        let req = JmapRequest::new(using, batch.calls, None);
        let response = self.call_internal(self.api_url(), &req).await?;
        Ok(BatchResponse { response })
    }
}

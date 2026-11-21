//! A SigV4-signed JSON request path shared by the service modules.
//!
//! IVS is a REST-JSON service ("POST /`CreateChannel` HTTP/1.1 /
//! Content-type: application/json", e.g.
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_CreateChannel.html#API_CreateChannel_RequestSyntax>);
//! MediaLive and MediaPackage are REST-JSON too, with other methods and
//! paths, so this client takes both.

use std::time::SystemTime;

use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use url::Url;

use crate::sigv4::{self, Request, Scope};
use crate::{AwsError, AwsErrorCode, Credentials};

/// One AWS service endpoint with its signing scope.
#[derive(Debug, Clone)]
pub(crate) struct JsonClient {
    http: reqwest::Client,
    credentials: Credentials,
    region: String,
    service: &'static str,
    endpoint: Url,
}

impl JsonClient {
    pub(crate) fn new(
        http: reqwest::Client,
        credentials: Credentials,
        region: String,
        service: &'static str,
        endpoint: Url,
    ) -> Self {
        Self {
            http,
            credentials,
            region,
            service,
            endpoint,
        }
    }

    pub(crate) fn with_endpoint(self, endpoint: Url) -> Self {
        Self { endpoint, ..self }
    }

    pub(crate) fn region(&self) -> &str {
        &self.region
    }

    /// `POST {endpoint}{path}` with `body`, signed; the response body as
    /// JSON (`Value::Null` for an empty body, e.g. a 204).
    ///
    /// # Errors
    ///
    /// [`AwsError::Service`] for a non-2xx status, [`AwsError::Transport`]
    /// without a response, [`AwsError::Response`] for a body that is not
    /// JSON.
    pub(crate) async fn post(&self, path: &str, body: &Value) -> Result<Value, AwsError> {
        let url = self
            .endpoint
            .join(path)
            .map_err(|e| AwsError::Response(format!("bad endpoint path {path}: {e}")))?;
        let body = serde_json::to_vec(body)
            .map_err(|e| AwsError::Response(format!("request body: {e}")))?;
        let signed = sigv4::sign(
            &self.credentials,
            &Request {
                method: "POST",
                url: &url,
                headers: &[("content-type", "application/json")],
                body: &body,
            },
            &Scope {
                region: &self.region,
                service: self.service,
                time: SystemTime::now(),
            },
        );
        let mut request = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(body);
        for (name, value) in signed {
            request = request.header(name, value);
        }
        let response = request.send().await?;
        let status = response.status();
        let error_type = response
            .headers()
            .get("x-amzn-errortype")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = response.bytes().await?;
        let json: Value = if bytes.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) if !status.is_success() => Value::Null,
                Err(e) => return Err(AwsError::Response(format!("body is not JSON: {e}"))),
            }
        };
        if status.is_success() {
            return Ok(json);
        }
        Err(AwsError::Service {
            status: status.as_u16(),
            code: error_code(error_type.as_deref(), &json),
            message: ["exceptionMessage", "message", "Message"]
                .iter()
                .find_map(|k| json.get(*k).and_then(Value::as_str))
                .map(str::to_owned),
        })
    }
}

/// The error code from `x-amzn-ErrorType` (`Code` or `Code:detail-url`),
/// else the body's `__type` (`Code` or `namespace#Code`) or `code`.
fn error_code(header: Option<&str>, body: &Value) -> AwsErrorCode {
    let from_header = header.map(|h| h.split(':').next().unwrap_or(h));
    let from_body = || {
        body.get("__type")
            .or_else(|| body.get("code"))
            .and_then(Value::as_str)
            .map(|t| t.rsplit('#').next().unwrap_or(t))
    };
    AwsErrorCode::from(from_header.or_else(from_body).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_code_sources() {
        assert_eq!(
            error_code(
                Some("ValidationException:http://internal.amazon.com/coral/com.amazonaws.ivs/"),
                &Value::Null
            ),
            AwsErrorCode::Validation
        );
        assert_eq!(
            error_code(
                None,
                &json!({"__type": "com.amazonaws.ivs#ConflictException"})
            ),
            AwsErrorCode::Conflict
        );
        assert_eq!(
            error_code(None, &Value::Null),
            AwsErrorCode::Other(String::new())
        );
    }
}

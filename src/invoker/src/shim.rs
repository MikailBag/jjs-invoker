//! Shim client

use anyhow::Context;

struct HttpShim {
    client: reqwest::Client,
    base: String,
}

/// Preprocesses requests using the shim, if required.
pub struct ShimClient {
    // if None, shim is not configured
    http: Option<HttpShim>,
}

/// Represents shim response
pub enum ShimResponse {
    /// Shim accepted the request and modified it
    Accept(invoker_api::invoke::InvokeRequest),
    /// Shim rejected the request, and provided value
    /// that should be returned to the user
    Reject(serde_json::Value),
}

impl ShimClient {
    pub fn new(base: Option<&str>) -> anyhow::Result<Self> {
        let base = match base {
            Some(a) => a,
            None => return Ok(ShimClient { http: None }),
        };
        let client = reqwest::Client::new();
        if base.ends_with('/') {
            anyhow::bail!("shim address must not contain trailing slash")
        }
        Ok(ShimClient {
            http: Some(HttpShim {
                client,
                base: base.to_string(),
            }),
        })
    }

    #[tracing::instrument(skip(self, val))]
    pub async fn call(&self, val: serde_json::Value) -> anyhow::Result<ShimResponse> {
        let h = match self.http.as_ref() {
            Some(h) => h,
            None => {
                tracing::info!("Shim not configured");
                return Ok(ShimResponse::Accept(serde_json::from_value(val).context(
                    "no shim enabled, but incoming request is not valid InvokeRequest",
                )?));
            }
        };
        let uri = format!("{}/on-request", h.base);
        tracing::info!(uri = uri.as_str(), "Requesting shim");
        let mut req = h.client.post(uri);
        req = req.body(serde_json::to_vec(&val).context("failed to serialize request")?);
        let resp = req.send().await.context("transport error")?;
        let status = resp.status().as_u16();

        let body: invoker_api::shims::ShimResponse = resp
            .json()
            .await
            .context("failed to parse shim response body")?;
        if body.http_status() != status {
            anyhow::bail!(
                "unexpected shim status: got {}, but {} expected",
                status,
                body.http_status()
            );
        }
        match body {
            invoker_api::shims::ShimResponse::Result(res) => Ok(ShimResponse::Accept(res)),
            invoker_api::shims::ShimResponse::Error(rej) => Ok(ShimResponse::Reject(rej)),
        }
    }
}

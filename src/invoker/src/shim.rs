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
    Accept(serde_json::Value),
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

    pub async fn call(&self, val: serde_json::Value) -> anyhow::Result<ShimResponse> {
        let h = match self.http.as_ref() {
            Some(h) => h,
            None => return Ok(ShimResponse::Accept(val)),
        };
        let mut req = h.client.post(&format!("{}/on-request", h.base));
        req = req.body(serde_json::to_vec(&val).context("failed to serialize request")?);
        let resp = req.send().await.context("transport error")?;
        let status = resp.status().as_u16();

        if status != 200 && status != 400 {
            anyhow::bail!("unexpected shim status: {}", status);
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .context("failed to parse shim response body")?;
        let body = body.as_object().context("shim response is not object")?;
        if status == 200 {
            let processed = body
                .get("result")
                .context("'result' key missing in shim response")?;
            Ok(ShimResponse::Accept(processed.clone()))
        } else if status == 400 {
            let rejection = body
                .get("error")
                .context("'rejection' key missing in shim response")?;
            Ok(ShimResponse::Reject(rejection.clone()))
        } else {
            anyhow::bail!("Unexpected shim response status: {}", status);
        }
    }
}

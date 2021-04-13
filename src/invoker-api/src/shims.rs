//! Defines shim <-> invoker API

use serde::{Deserialize, Serialize};

/// Represents shim response
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShimResponse {
    /// Shim accepted the request and modified it
    Result(crate::invoke::InvokeRequest),
    /// Shim rejected the request, and provided value
    /// that should be returned to the user
    Error(serde_json::Value),
}

impl ShimResponse {
    pub fn http_status(&self) -> u16 {
        match self {
            ShimResponse::Result(_) => 200,
            ShimResponse::Error(_) => 400,
        }
    }
}

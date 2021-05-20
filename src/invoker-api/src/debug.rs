//! Invoker interactive debugging APIs
use serde::{Deserialize, Serialize};

/// Sent as POST payload after sandbox creation
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachRequest {
    /// Raw backend-specific details
    pub raw: serde_json::Value,
    /// Request id
    pub request_id: uuid::Uuid,
    /// Sandbox name
    pub sandbox_name: String,
}

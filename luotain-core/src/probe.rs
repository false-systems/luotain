use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A probe request — what to send to the target system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeRequest {
    /// HTTP method, CLI command, etc.
    pub method: String,
    /// Target URL or address
    pub url: String,
    /// Request headers
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub headers: HashMap<String, String>,
    /// Request body
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// What we observed from the probe — structured for agent reasoning.
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    /// HTTP status code (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Response headers
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Raw response body
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Parsed JSON body (if response was JSON)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_json: Option<serde_json::Value>,
    /// Detected content type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Body size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_size: Option<usize>,
    /// Connection or protocol error (if the probe failed to reach the target)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Timing information for the probe.
#[derive(Debug, Clone, Serialize)]
pub struct Timing {
    /// Total round-trip time in milliseconds
    pub total_ms: u64,
}

/// The result of a single probe — the core unit of observation.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    /// Unique probe ID (ULID)
    pub id: String,
    /// When the probe was executed
    pub timestamp: DateTime<Utc>,
    /// Which spec this probe relates to (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_path: Option<String>,
    /// What was sent
    pub request: ProbeRequest,
    /// What came back
    pub observation: Observation,
    /// How long it took
    pub timing: Timing,
}

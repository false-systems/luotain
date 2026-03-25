use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Which probe type generated this observation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    Http,
    Grpc,
    Tcp,
    Cli,
    Sql,
}

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
///
/// Flat struct with optional fields. Each probe type populates only the
/// relevant fields. `skip_serializing_if` ensures the agent sees clean JSON
/// with only what matters for that probe type.
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    /// Which probe type produced this
    pub kind: ProbeKind,

    // --- HTTP fields ---
    /// HTTP status code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Response headers
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Raw response body / stdout
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

    // --- Error ---
    /// Connection or protocol error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    // --- CLI fields ---
    /// Process exit code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Stderr output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,

    // --- gRPC fields ---
    /// gRPC status code string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_status: Option<String>,

    // --- TCP/TLS fields ---
    /// Whether TLS handshake succeeded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_established: Option<bool>,
    /// TLS protocol version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_version: Option<String>,

    // --- SQL fields ---
    /// Query result rows
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<serde_json::Value>>,
    /// Row count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
}

impl Observation {
    /// Create a minimal HTTP observation.
    pub fn http() -> Self {
        Self {
            kind: ProbeKind::Http,
            status: None,
            headers: HashMap::new(),
            body: None,
            body_json: None,
            content_type: None,
            body_size: None,
            error: None,
            exit_code: None,
            stderr: None,
            grpc_status: None,
            tls_established: None,
            tls_version: None,
            rows: None,
            row_count: None,
        }
    }

    /// Create a minimal CLI observation.
    pub fn cli() -> Self {
        Self {
            kind: ProbeKind::Cli,
            ..Self::http()
        }
    }

    /// Create a minimal TCP observation.
    pub fn tcp() -> Self {
        Self {
            kind: ProbeKind::Tcp,
            ..Self::http()
        }
    }
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

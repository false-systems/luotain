use crate::config::{Auth, Connection, TargetConfig};
use crate::probe::{Observation, ProbeRequest, ProbeResult, Timing};
use crate::probe_trait::{Probe, ProbeError};
use crate::tool::ToolDef;
use chrono::Utc;
use std::collections::HashMap;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HttpProbeError {
    #[error("request error: {0}")]
    Request(#[from] reqwest::Error),
    #[error("invalid method: {0}")]
    InvalidMethod(String),
}

/// HTTP probe engine — makes requests and returns structured observations.
///
/// Redirects are NOT followed — the agent sees them as-is.
/// Timeout is 30 seconds by default.
pub struct HttpProbe {
    client: reqwest::Client,
}

impl HttpProbe {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Execute an HTTP probe and return a structured observation.
    pub async fn probe(&self, request: &ProbeRequest) -> Result<ProbeResult, HttpProbeError> {
        let method: reqwest::Method = request
            .method
            .parse()
            .map_err(|_| HttpProbeError::InvalidMethod(request.method.clone()))?;

        let mut req_builder = self.client.request(method, &request.url);

        for (key, value) in &request.headers {
            req_builder = req_builder.header(key.as_str(), value.as_str());
        }

        if let Some(body) = &request.body {
            req_builder = req_builder.body(body.clone());
        }

        let start = Instant::now();
        let probe_id = ulid::Ulid::new().to_string();
        let timestamp = Utc::now();

        let (observation, timing) = match req_builder.send().await {
            Ok(response) => {
                let elapsed = start.elapsed();
                let status = response.status().as_u16();

                let mut headers = HashMap::new();
                for (name, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(name.to_string(), v.to_string());
                    }
                }

                let content_type = headers.get("content-type").cloned();
                let body_bytes = response.bytes().await.ok();
                let body_size = body_bytes.as_ref().map(|b| b.len());
                let body = body_bytes
                    .as_ref()
                    .and_then(|b| String::from_utf8(b.to_vec()).ok());
                let body_json = body.as_ref().and_then(|b| serde_json::from_str(b).ok());

                let mut obs = Observation::http();
                obs.status = Some(status);
                obs.headers = headers;
                obs.body = body;
                obs.body_json = body_json;
                obs.content_type = content_type;
                obs.body_size = body_size;

                (
                    obs,
                    Timing {
                        total_ms: elapsed.as_millis() as u64,
                    },
                )
            }
            Err(e) => {
                let elapsed = start.elapsed();
                let mut obs = Observation::http();
                obs.error = Some(e.to_string());

                (
                    obs,
                    Timing {
                        total_ms: elapsed.as_millis() as u64,
                    },
                )
            }
        };

        Ok(ProbeResult {
            id: probe_id,
            timestamp,
            spec_path: None,
            request: request.clone(),
            observation,
            timing,
        })
    }

    /// Apply auth from TargetConfig to headers.
    fn apply_auth(headers: &mut HashMap<String, String>, auth: &Auth) {
        match auth {
            Auth::Bearer { token } => {
                headers.insert("Authorization".into(), format!("Bearer {}", token));
            }
            Auth::Basic { username, password } => {
                use std::io::Write;
                let mut buf = Vec::new();
                write!(buf, "{}:{}", username, password).ok();
                let encoded = base64_encode(&buf);
                headers.insert("Authorization".into(), format!("Basic {}", encoded));
            }
            Auth::ApiKey { header, key } => {
                headers.insert(header.clone(), key.clone());
            }
            Auth::Mtls { .. } => {
                // mTLS is handled at the client level, not headers
            }
        }
    }
}

/// Simple base64 encoding (avoids adding a dependency).
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

impl Default for HttpProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Probe for HttpProbe {
    fn kind(&self) -> &str {
        "http"
    }

    fn tool_definitions(&self) -> Vec<ToolDef> {
        vec![ToolDef {
            name: "probe_http".into(),
            description: "Send an HTTP request to the target and observe the response. Returns: status code, headers, body (raw + parsed JSON), content type, body size, timing (ms).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "method": {
                        "type": "string",
                        "description": "HTTP method (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS)"
                    },
                    "path": {
                        "type": "string",
                        "description": "URL path relative to target (e.g., '/api/login', '/health')"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Request headers",
                        "additionalProperties": { "type": "string" }
                    },
                    "body": {
                        "type": "string",
                        "description": "Request body (typically JSON string)"
                    }
                },
                "required": ["method", "path"]
            }),
        }]
    }

    async fn execute(
        &self,
        _tool_name: &str,
        input: &serde_json::Value,
        target: &TargetConfig,
    ) -> Result<ProbeResult, ProbeError> {
        let base_url = match &target.connection {
            Connection::Http { base_url } => base_url,
            _ => {
                return Err(ProbeError::InvalidInput(
                    "HTTP probe requires HTTP connection type".into(),
                ))
            }
        };

        let method = input["method"].as_str().unwrap_or("GET");
        let path = input["path"].as_str().unwrap_or("/");

        let full_url = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let mut headers = HashMap::new();
        if let Some(h) = input["headers"].as_object() {
            for (k, v) in h {
                if let Some(vs) = v.as_str() {
                    headers.insert(k.clone(), vs.to_string());
                }
            }
        }

        // Apply auth from config
        if let Some(auth) = &target.auth {
            Self::apply_auth(&mut headers, auth);
        }

        let body = input["body"].as_str().map(String::from);

        let request = ProbeRequest {
            method: method.to_string(),
            url: full_url,
            headers,
            body,
        };

        self.probe(&request)
            .await
            .map_err(|e| ProbeError::Connection(e.to_string()))
    }
}

use crate::probe::{Observation, ProbeRequest, ProbeResult, Timing};
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

                (
                    Observation {
                        status: Some(status),
                        headers,
                        body,
                        body_json,
                        content_type,
                        body_size,
                        error: None,
                    },
                    Timing {
                        total_ms: elapsed.as_millis() as u64,
                    },
                )
            }
            Err(e) => {
                let elapsed = start.elapsed();
                (
                    Observation {
                        status: None,
                        headers: HashMap::new(),
                        body: None,
                        body_json: None,
                        content_type: None,
                        body_size: None,
                        error: Some(e.to_string()),
                    },
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
}

impl Default for HttpProbe {
    fn default() -> Self {
        Self::new()
    }
}

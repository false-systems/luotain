//! gRPC probe — call gRPC services and observe responses.
//!
//! Two tools:
//! - `probe_grpc_health`: Standard health check (no proto needed)
//! - `probe_grpc_call`: Dynamic call with JSON payload (needs proto file)
//!
//! Uses reqwest with HTTP/2 for transport and prost-reflect for
//! JSON-to-protobuf conversion. No tonic dependency — keeps it lean.

#[cfg(feature = "grpc")]
mod inner {
    use crate::config::{Connection, TargetConfig};
    use crate::probe::{Observation, ProbeKind, ProbeRequest, ProbeResult, Timing};
    use crate::probe_trait::{Probe, ProbeError};
    use crate::tool::ToolDef;
    use chrono::Utc;
    use std::time::Instant;

    pub struct GrpcProbe {
        client: reqwest::Client,
    }

    impl GrpcProbe {
        pub fn new() -> Self {
            let client = reqwest::Client::builder()
                .http2_prior_knowledge()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default();
            Self { client }
        }
    }

    impl Default for GrpcProbe {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait::async_trait]
    impl Probe for GrpcProbe {
        fn kind(&self) -> &str {
            "grpc"
        }

        fn tool_definitions(&self) -> Vec<ToolDef> {
            vec![
                ToolDef {
                    name: "probe_grpc_health".into(),
                    description: "Check gRPC service health using the standard grpc.health.v1.Health/Check. Works on any gRPC service. Returns serving status and timing.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "service": {
                                "type": "string",
                                "description": "Service name to check (empty for overall server health)"
                            }
                        }
                    }),
                },
                ToolDef {
                    name: "probe_grpc_call".into(),
                    description: "Call a gRPC method with a JSON payload. Requires proto_path in _config.md. JSON is converted to protobuf, response comes back as JSON.".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "service": {
                                "type": "string",
                                "description": "Full service name (e.g., 'ahti.v1.AhtiService')"
                            },
                            "method": {
                                "type": "string",
                                "description": "Method name (e.g., 'SendEvent')"
                            },
                            "payload": {
                                "type": "object",
                                "description": "Request as JSON (fields match the protobuf message)"
                            }
                        },
                        "required": ["service", "method"]
                    }),
                },
            ]
        }

        async fn execute(
            &self,
            tool_name: &str,
            input: &serde_json::Value,
            target: &TargetConfig,
        ) -> Result<ProbeResult, ProbeError> {
            let (host, port, tls, proto_path) = match &target.connection {
                Connection::Grpc {
                    host,
                    port,
                    tls,
                    proto_path,
                } => (host.clone(), *port, *tls, proto_path.clone()),
                _ => {
                    return Err(ProbeError::InvalidInput(
                        "gRPC probe requires gRPC connection type".into(),
                    ))
                }
            };

            let scheme = if tls { "https" } else { "http" };
            let base = format!("{}://{}:{}", scheme, host, port);

            match tool_name {
                "probe_grpc_health" => self.health_check(input, &base).await,
                "probe_grpc_call" => {
                    let proto = proto_path.ok_or_else(|| {
                        ProbeError::InvalidInput(
                            "probe_grpc_call requires proto_path in _config.md".into(),
                        )
                    })?;
                    self.dynamic_call(input, &base, &proto).await
                }
                _ => Err(ProbeError::NotSupported(format!(
                    "unknown gRPC tool: {}",
                    tool_name
                ))),
            }
        }
    }

    impl GrpcProbe {
        /// Standard gRPC health check.
        async fn health_check(
            &self,
            input: &serde_json::Value,
            base: &str,
        ) -> Result<ProbeResult, ProbeError> {
            let service_name = input["service"].as_str().unwrap_or("");

            let start = Instant::now();
            let probe_id = ulid::Ulid::new().to_string();
            let timestamp = Utc::now();

            // Hand-craft the HealthCheckRequest protobuf:
            // field 1 (string) = service name
            let mut request_bytes = Vec::new();
            if !service_name.is_empty() {
                request_bytes.push(0x0a); // tag: field 1, wire type 2
                request_bytes.push(service_name.len() as u8);
                request_bytes.extend_from_slice(service_name.as_bytes());
            }

            let url = format!("{}/grpc.health.v1.Health/Check", base);

            let observation = match self.raw_grpc_call(&url, &request_bytes).await {
                Ok(response_bytes) => {
                    // Parse HealthCheckResponse: field 1 (enum) = status
                    let status = parse_health_status(&response_bytes);
                    let status_str = match status {
                        0 => "UNKNOWN",
                        1 => "SERVING",
                        2 => "NOT_SERVING",
                        3 => "SERVICE_UNKNOWN",
                        _ => "UNRECOGNIZED",
                    };
                    let mut obs = grpc_observation();
                    obs.grpc_status = Some("OK".into());
                    obs.body = Some(status_str.to_string());
                    obs.body_json = Some(serde_json::json!({ "status": status_str }));
                    obs
                }
                Err(e) => {
                    let mut obs = grpc_observation();
                    obs.error = Some(format!("{}", e));
                    obs
                }
            };

            Ok(ProbeResult {
                id: probe_id,
                timestamp,
                spec_path: None,
                request: ProbeRequest {
                    method: "GRPC_HEALTH".into(),
                    url,
                    headers: Default::default(),
                    body: if service_name.is_empty() {
                        None
                    } else {
                        Some(service_name.to_string())
                    },
                },
                observation,
                timing: Timing {
                    total_ms: start.elapsed().as_millis() as u64,
                },
            })
        }

        /// Dynamic gRPC call using proto descriptor.
        async fn dynamic_call(
            &self,
            input: &serde_json::Value,
            base: &str,
            proto_path: &std::path::Path,
        ) -> Result<ProbeResult, ProbeError> {
            let service = input["service"]
                .as_str()
                .ok_or_else(|| ProbeError::InvalidInput("missing 'service'".into()))?;
            let method = input["method"]
                .as_str()
                .ok_or_else(|| ProbeError::InvalidInput("missing 'method'".into()))?;
            let payload = input.get("payload").cloned().unwrap_or(serde_json::json!({}));

            let grpc_path = format!("/{}/{}", service, method);
            let url = format!("{}{}", base, grpc_path);

            let start = Instant::now();
            let probe_id = ulid::Ulid::new().to_string();
            let timestamp = Utc::now();

            let observation =
                match self
                    .do_dynamic_call(&url, service, method, &payload, proto_path)
                    .await
                {
                    Ok(response_json) => {
                        let body_str =
                            serde_json::to_string_pretty(&response_json).unwrap_or_default();
                        let mut obs = grpc_observation();
                        obs.grpc_status = Some("OK".into());
                        obs.body = Some(body_str);
                        obs.body_json = Some(response_json);
                        obs
                    }
                    Err(e) => {
                        let mut obs = grpc_observation();
                        let err_str = format!("{}", e);
                        obs.grpc_status = Some(extract_grpc_code(&err_str));
                        obs.error = Some(err_str);
                        obs
                    }
                };

            Ok(ProbeResult {
                id: probe_id,
                timestamp,
                spec_path: None,
                request: ProbeRequest {
                    method: "GRPC_CALL".into(),
                    url,
                    headers: Default::default(),
                    body: Some(serde_json::to_string(&payload).unwrap_or_default()),
                },
                observation,
                timing: Timing {
                    total_ms: start.elapsed().as_millis() as u64,
                },
            })
        }

        async fn do_dynamic_call(
            &self,
            url: &str,
            service_name: &str,
            method_name: &str,
            payload: &serde_json::Value,
            proto_path: &std::path::Path,
        ) -> Result<serde_json::Value, ProbeError> {
            // 1. Parse proto → descriptor
            let include_dir = proto_path
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let file_descriptor_set =
                protox::compile([proto_path], [include_dir]).map_err(|e| {
                    ProbeError::Protocol(format!("proto compile failed: {}", e))
                })?;

            let pool =
                prost_reflect::DescriptorPool::from_file_descriptor_set(file_descriptor_set)
                    .map_err(|e| {
                        ProbeError::Protocol(format!("descriptor pool failed: {}", e))
                    })?;

            // 2. Find service and method
            let svc = pool.get_service_by_name(service_name).ok_or_else(|| {
                let available: Vec<String> = pool.services().map(|s| s.full_name().to_string()).collect();
                ProbeError::InvalidInput(format!(
                    "service '{}' not found. available: {:?}",
                    service_name, available
                ))
            })?;

            let mtd = svc.methods().find(|m| m.name() == method_name).ok_or_else(|| {
                let available: Vec<String> = svc.methods().map(|m| m.name().to_string()).collect();
                ProbeError::InvalidInput(format!(
                    "method '{}' not found in '{}'. available: {:?}",
                    method_name, service_name, available
                ))
            })?;

            // 3. JSON → protobuf
            let input_desc = mtd.input();
            let request_msg =
                prost_reflect::DynamicMessage::deserialize(input_desc, payload).map_err(|e| {
                    ProbeError::InvalidInput(format!("JSON to protobuf failed: {}", e))
                })?;

            use prost::Message;
            let request_bytes = request_msg.encode_to_vec();

            // 4. Send via raw gRPC call
            let response_bytes = self.raw_grpc_call(url, &request_bytes).await?;

            // 5. Protobuf → JSON
            let output_desc = mtd.output();
            let response_msg =
                prost_reflect::DynamicMessage::decode(output_desc, response_bytes.as_slice())
                    .map_err(|e| {
                        ProbeError::Protocol(format!("response decode failed: {}", e))
                    })?;

            serde_json::to_value(&response_msg)
                .map_err(|e| ProbeError::Protocol(format!("response to JSON failed: {}", e)))
        }

        /// Send raw gRPC frame over HTTP/2 using reqwest.
        async fn raw_grpc_call(
            &self,
            url: &str,
            request_bytes: &[u8],
        ) -> Result<Vec<u8>, ProbeError> {
            // gRPC frame: 1 byte compression + 4 bytes length + payload
            let mut frame = Vec::with_capacity(5 + request_bytes.len());
            frame.push(0u8); // no compression
            frame.extend_from_slice(&(request_bytes.len() as u32).to_be_bytes());
            frame.extend_from_slice(request_bytes);

            let resp = self
                .client
                .post(url)
                .header("content-type", "application/grpc")
                .header("te", "trailers")
                .body(frame)
                .send()
                .await
                .map_err(|e| ProbeError::Connection(format!("gRPC request failed: {}", e)))?;

            let status = resp.status();
            let body = resp
                .bytes()
                .await
                .map_err(|e| ProbeError::Protocol(format!("read response failed: {}", e)))?;

            if !status.is_success() {
                return Err(ProbeError::Protocol(format!(
                    "gRPC HTTP status {}: {}",
                    status,
                    String::from_utf8_lossy(&body)
                )));
            }

            if body.len() < 5 {
                return Err(ProbeError::Protocol(format!(
                    "response too short for gRPC frame ({} bytes)",
                    body.len()
                )));
            }

            // Strip 5-byte gRPC frame header
            Ok(body[5..].to_vec())
        }
    }

    fn grpc_observation() -> Observation {
        let mut obs = Observation::tcp();
        obs.kind = ProbeKind::Grpc;
        obs
    }

    /// Parse a simple varint enum from health check response.
    fn parse_health_status(bytes: &[u8]) -> i32 {
        // HealthCheckResponse has one field: enum status = 1
        // Protobuf: tag 0x08 (field 1, varint), then the enum value
        if bytes.len() >= 2 && bytes[0] == 0x08 {
            bytes[1] as i32
        } else {
            0 // UNKNOWN
        }
    }

    fn extract_grpc_code(err: &str) -> String {
        for code in [
            "UNAVAILABLE",
            "NOT_FOUND",
            "INVALID_ARGUMENT",
            "UNAUTHENTICATED",
            "PERMISSION_DENIED",
            "INTERNAL",
            "UNIMPLEMENTED",
            "DEADLINE_EXCEEDED",
            "RESOURCE_EXHAUSTED",
        ] {
            if err.to_uppercase().contains(code) {
                return code.to_string();
            }
        }
        "UNKNOWN".into()
    }
}

#[cfg(feature = "grpc")]
pub use inner::GrpcProbe;

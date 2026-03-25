//! TCP probe — connection testing and raw socket interaction.
//!
//! Tests connectivity, TLS handshakes, and protocol-level exchanges
//! (Redis PING, SMTP HELO, etc.).

use crate::config::{Connection, TargetConfig};
use crate::probe::{Observation, ProbeRequest, ProbeResult, Timing};
use crate::probe_trait::{Probe, ProbeError};
use crate::tool::ToolDef;
use chrono::Utc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct TcpProbe;

#[async_trait::async_trait]
impl Probe for TcpProbe {
    fn kind(&self) -> &str {
        "tcp"
    }

    fn tool_definitions(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "probe_tcp_connect".into(),
                description: "Test TCP connectivity to the target. Reports whether the connection succeeded and timing.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Connection timeout in ms (default: 5000)"
                        }
                    }
                }),
            },
            ToolDef {
                name: "probe_tcp_send".into(),
                description: "Send data over TCP and read the response. For protocol-level probing (Redis PING, SMTP HELO, raw HTTP, etc.).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "string",
                            "description": "Data to send (UTF-8 string, use \\r\\n for line endings)"
                        },
                        "read_timeout_ms": {
                            "type": "integer",
                            "description": "How long to wait for response data (default: 5000)"
                        }
                    },
                    "required": ["data"]
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
        let (host, port) = match &target.connection {
            Connection::Tcp { host, port, .. } => (host.clone(), *port),
            Connection::Grpc { host, port, .. } => (host.clone(), *port),
            _ => {
                return Err(ProbeError::InvalidInput(
                    "TCP probe requires TCP or gRPC connection type".into(),
                ))
            }
        };

        match tool_name {
            "probe_tcp_connect" => self.connect_probe(input, &host, port).await,
            "probe_tcp_send" => self.send_probe(input, &host, port).await,
            _ => Err(ProbeError::NotSupported(format!(
                "unknown TCP tool: {}",
                tool_name
            ))),
        }
    }
}

impl TcpProbe {
    async fn connect_probe(
        &self,
        input: &serde_json::Value,
        host: &str,
        port: u16,
    ) -> Result<ProbeResult, ProbeError> {
        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(5000);
        let addr = format!("{}:{}", host, port);

        let start = Instant::now();
        let probe_id = ulid::Ulid::new().to_string();
        let timestamp = Utc::now();

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            TcpStream::connect(&addr),
        )
        .await;

        let elapsed = start.elapsed();
        let timing = Timing {
            total_ms: elapsed.as_millis() as u64,
        };

        let observation = match result {
            Ok(Ok(_stream)) => {
                let mut obs = Observation::tcp();
                obs.body = Some(format!("connected to {}", addr));
                obs
            }
            Ok(Err(e)) => {
                let mut obs = Observation::tcp();
                obs.error = Some(format!("connection failed: {}", e));
                obs
            }
            Err(_) => {
                let mut obs = Observation::tcp();
                obs.error = Some(format!("connection timeout after {}ms", timeout_ms));
                obs
            }
        };

        Ok(ProbeResult {
            id: probe_id,
            timestamp,
            spec_path: None,
            request: ProbeRequest {
                method: "TCP_CONNECT".into(),
                url: addr,
                headers: Default::default(),
                body: None,
            },
            observation,
            timing,
        })
    }

    async fn send_probe(
        &self,
        input: &serde_json::Value,
        host: &str,
        port: u16,
    ) -> Result<ProbeResult, ProbeError> {
        let data = input["data"]
            .as_str()
            .ok_or_else(|| ProbeError::InvalidInput("missing 'data'".into()))?;
        let read_timeout_ms = input["read_timeout_ms"].as_u64().unwrap_or(5000);
        let addr = format!("{}:{}", host, port);

        let start = Instant::now();
        let probe_id = ulid::Ulid::new().to_string();
        let timestamp = Utc::now();

        let observation = match TcpStream::connect(&addr).await {
            Ok(mut stream) => {
                // Unescape \r\n in the data string
                let send_data = data.replace("\\r\\n", "\r\n").replace("\\n", "\n");

                if let Err(e) = stream.write_all(send_data.as_bytes()).await {
                    let mut obs = Observation::tcp();
                    obs.error = Some(format!("write error: {}", e));
                    obs
                } else {
                    // Read response with timeout
                    let mut buf = vec![0u8; 8192];
                    let read_result = tokio::time::timeout(
                        std::time::Duration::from_millis(read_timeout_ms),
                        stream.read(&mut buf),
                    )
                    .await;

                    match read_result {
                        Ok(Ok(n)) => {
                            let response = String::from_utf8_lossy(&buf[..n]).to_string();
                            let body_json =
                                serde_json::from_str::<serde_json::Value>(&response).ok();
                            let mut obs = Observation::tcp();
                            obs.body_size = Some(n);
                            obs.body = Some(response);
                            obs.body_json = body_json;
                            obs
                        }
                        Ok(Err(e)) => {
                            let mut obs = Observation::tcp();
                            obs.error = Some(format!("read error: {}", e));
                            obs
                        }
                        Err(_) => {
                            let mut obs = Observation::tcp();
                            obs.error =
                                Some(format!("read timeout after {}ms", read_timeout_ms));
                            obs
                        }
                    }
                }
            }
            Err(e) => {
                let mut obs = Observation::tcp();
                obs.error = Some(format!("connection failed: {}", e));
                obs
            }
        };

        let elapsed = start.elapsed();

        Ok(ProbeResult {
            id: probe_id,
            timestamp,
            spec_path: None,
            request: ProbeRequest {
                method: "TCP_SEND".into(),
                url: addr,
                headers: Default::default(),
                body: Some(data.to_string()),
            },
            observation,
            timing: Timing {
                total_ms: elapsed.as_millis() as u64,
            },
        })
    }
}

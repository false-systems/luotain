//! CLI probe — run commands and observe output.
//!
//! Executes commands via the shell, captures stdout, stderr, and exit code.
//! For testing CLI tools, scripts, database migrations, container commands, etc.

use crate::config::{Connection, TargetConfig};
use crate::probe::{Observation, ProbeRequest, ProbeResult, Timing};
use crate::probe_trait::{Probe, ProbeError};
use crate::tool::ToolDef;
use chrono::Utc;
use std::time::Instant;
use tokio::process::Command;

pub struct CliProbe;

#[async_trait::async_trait]
impl Probe for CliProbe {
    fn kind(&self) -> &str {
        "cli"
    }

    fn tool_definitions(&self) -> Vec<ToolDef> {
        vec![ToolDef {
            name: "probe_cli".into(),
            description: "Run a CLI command and observe the output. Returns: exit code, stdout, stderr, timing (ms). The command is appended to the target's base command if configured.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to run (e.g., 'ls -la', 'curl http://...', 'psql -c \"SELECT 1\"')"
                    },
                    "stdin": {
                        "type": "string",
                        "description": "Input to pipe to stdin"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Timeout in ms (default: 30000)"
                    }
                },
                "required": ["command"]
            }),
        }]
    }

    async fn execute(
        &self,
        _tool_name: &str,
        input: &serde_json::Value,
        target: &TargetConfig,
    ) -> Result<ProbeResult, ProbeError> {
        let user_command = input["command"]
            .as_str()
            .ok_or_else(|| ProbeError::InvalidInput("missing 'command'".into()))?;

        let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(30_000);

        // Build full command: prepend target's base command if CLI type
        let full_command = match &target.connection {
            Connection::Cli { command, .. } => {
                if command.is_empty() {
                    user_command.to_string()
                } else {
                    format!("{} {}", command, user_command)
                }
            }
            _ => user_command.to_string(),
        };

        // Determine shell
        let shell = match &target.connection {
            Connection::Cli {
                shell: Some(s), ..
            } => s.clone(),
            _ => "sh".to_string(),
        };

        let start = Instant::now();
        let probe_id = ulid::Ulid::new().to_string();
        let timestamp = Utc::now();

        let mut cmd = Command::new(&shell);
        cmd.arg("-c").arg(&full_command);

        // Pipe stdin if provided
        let stdin_data = input["stdin"].as_str().map(|s| s.to_string());
        if stdin_data.is_some() {
            cmd.stdin(std::process::Stdio::piped());
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            async {
                let mut child = cmd.spawn()?;
                if let Some(ref data) = stdin_data {
                    if let Some(mut stdin) = child.stdin.take() {
                        use tokio::io::AsyncWriteExt;
                        let _ = stdin.write_all(data.as_bytes()).await;
                        drop(stdin);
                    }
                }
                child.wait_with_output().await
            },
        )
        .await;

        let elapsed = start.elapsed();
        let timing = Timing {
            total_ms: elapsed.as_millis() as u64,
        };

        let observation = match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code();

                // Try to parse stdout as JSON
                let body_json = serde_json::from_str::<serde_json::Value>(&stdout).ok();

                let mut obs = Observation::cli();
                obs.exit_code = exit_code;
                obs.body = if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                };
                obs.body_json = body_json;
                obs.stderr = if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                };
                obs.body_size = obs.body.as_ref().map(|b| b.len());
                obs
            }
            Ok(Err(e)) => {
                let mut obs = Observation::cli();
                obs.error = Some(format!("execution error: {}", e));
                obs
            }
            Err(_) => {
                let mut obs = Observation::cli();
                obs.error = Some(format!("timeout after {}ms", timeout_ms));
                obs
            }
        };

        Ok(ProbeResult {
            id: probe_id,
            timestamp,
            spec_path: None,
            request: ProbeRequest {
                method: "CLI".into(),
                url: full_command,
                headers: Default::default(),
                body: None,
            },
            observation,
            timing,
        })
    }
}

//! The Probe trait — domain contract for all probe types.
//!
//! Each probe type (HTTP, CLI, TCP, gRPC) implements this trait.
//! The agent dispatches tool calls through the ProbeRegistry,
//! which routes to the right probe by tool name.

use crate::config::TargetConfig;
use crate::probe::ProbeResult;
use crate::tool::ToolDef;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("connection error: {0}")]
    Connection(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("timeout after {0}ms")]
    Timeout(u64),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not supported: {0}")]
    NotSupported(String),
}

/// A probe that can observe a target system.
#[async_trait::async_trait]
pub trait Probe: Send + Sync {
    /// Unique identifier for this probe type.
    fn kind(&self) -> &str;

    /// Tool definitions this probe exposes to the LLM agent.
    fn tool_definitions(&self) -> Vec<ToolDef>;

    /// Execute a tool call and return a probe result.
    async fn execute(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        target: &TargetConfig,
    ) -> Result<ProbeResult, ProbeError>;
}

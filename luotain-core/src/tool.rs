//! Tool definitions that probes expose to the agent.
//!
//! This lives in luotain-core (not luotain-judge) to preserve dependency
//! direction: Core does not know about the Judge. The Judge maps ToolDef
//! to its own Tool type at the boundary.

/// A tool definition a probe contributes to the agent's toolkit.
#[derive(Debug, Clone)]
pub struct ToolDef {
    /// Tool name the agent calls (e.g., "probe_http", "probe_cli")
    pub name: String,
    /// Description for the LLM
    pub description: String,
    /// JSON Schema for parameters
    pub parameters: serde_json::Value,
}

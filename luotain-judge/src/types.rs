//! Domain types for the Judgment Context.
//!
//! These types form the ubiquitous language of the agent loop:
//! Turn, ToolCall, ToolResult, and the response from the LLM.
//! They abstract over wire formats (Anthropic vs OpenAI) so the
//! agent loop speaks one language regardless of provider.

use serde::{Deserialize, Serialize};

/// A single turn in the agent conversation.
///
/// The conversation alternates: User → Assistant → ToolResults → Assistant → ...
#[derive(Debug, Clone)]
pub enum Turn {
    /// User message (spec content + target info)
    User(String),
    /// LLM response (may contain text and/or tool calls)
    Assistant(AssistantMessage),
    /// Results of executing the LLM's tool calls
    ToolResults(Vec<ToolResult>),
}

/// The LLM's response in a single turn.
#[derive(Debug, Clone)]
pub struct AssistantMessage {
    /// Text the LLM produced (reasoning, explanations)
    pub text: Option<String>,
    /// Tool calls the LLM wants to make
    pub tool_calls: Vec<ToolCall>,
    /// Why the LLM stopped generating
    pub stop_reason: StopReason,
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned call ID (for matching results)
    pub id: String,
    /// Tool name (e.g., "probe_http", "record_verdict")
    pub name: String,
    /// Arguments as JSON
    pub input: serde_json::Value,
}

/// The result of executing a tool call.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Matches the ToolCall.id
    pub tool_call_id: String,
    /// Result content (JSON string)
    pub content: String,
    /// Whether the tool execution itself failed
    pub is_error: bool,
}

/// Tool definition exposed to the LLM.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's parameters
    pub parameters: serde_json::Value,
}

/// Why the LLM stopped generating.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Natural end of response
    EndTurn,
    /// Wants to use tools
    ToolUse,
    /// Hit token limit
    MaxTokens,
}

impl AssistantMessage {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

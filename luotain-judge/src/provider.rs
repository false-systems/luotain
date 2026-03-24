//! The JudgeProvider trait — anti-corruption layer between our domain
//! and external LLM APIs. Each provider converts our Turn/Tool types
//! to/from its wire format. The agent loop never touches wire formats.

use crate::types::{AssistantMessage, Tool, Turn};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JudgeError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("parse error: {0}")]
    Parse(String),
    #[error("max turns exceeded ({0})")]
    MaxTurns(usize),
    #[error("missing API key — set ANTHROPIC_API_KEY or OPENAI_API_KEY, or pass --api-key")]
    MissingApiKey,
    #[error("probe error: {0}")]
    Probe(String),
}

/// Provider for LLM chat with tool use.
///
/// This is a port — in DDD terms, it defines what the Judgment Context
/// needs from the outside world. Each implementation (Anthropic, OpenAI)
/// is an adapter.
#[async_trait::async_trait]
pub trait JudgeProvider: Send + Sync {
    /// Send a conversation with tools to the LLM and get a response.
    async fn chat(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[Tool],
    ) -> Result<AssistantMessage, JudgeError>;

    /// Provider name for logging/reporting.
    fn name(&self) -> &str;

    /// Model name for logging/reporting.
    fn model(&self) -> &str;
}

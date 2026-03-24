//! Anthropic Claude API adapter.
//!
//! Converts our Turn/Tool types to/from the Anthropic Messages API format.
//! Handles: content blocks (text + tool_use), tool_result as user content,
//! and the Anthropic-specific response structure.

use crate::provider::{JudgeError, JudgeProvider};
use crate::types::{AssistantMessage, StopReason, Tool, ToolCall, Turn};

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }

    fn convert_messages(&self, turns: &[Turn]) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();

        for turn in turns {
            match turn {
                Turn::User(text) => {
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": text,
                    }));
                }
                Turn::Assistant(msg) => {
                    let mut content = Vec::new();
                    if let Some(text) = &msg.text {
                        if !text.is_empty() {
                            content.push(serde_json::json!({
                                "type": "text",
                                "text": text,
                            }));
                        }
                    }
                    for tc in &msg.tool_calls {
                        content.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.input,
                        }));
                    }
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": content,
                    }));
                }
                Turn::ToolResults(results) => {
                    let content: Vec<_> = results
                        .iter()
                        .map(|r| {
                            let mut block = serde_json::json!({
                                "type": "tool_result",
                                "tool_use_id": r.tool_call_id,
                                "content": r.content,
                            });
                            if r.is_error {
                                block["is_error"] = serde_json::json!(true);
                            }
                            block
                        })
                        .collect();
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": content,
                    }));
                }
            }
        }

        messages
    }

    fn convert_tools(&self, tools: &[Tool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect()
    }

    fn parse_response(
        &self,
        json: serde_json::Value,
    ) -> Result<AssistantMessage, JudgeError> {
        let content = json["content"]
            .as_array()
            .ok_or_else(|| JudgeError::Parse("missing content array".into()))?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in content {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(t) = block["text"].as_str() {
                        text_parts.push(t.to_string());
                    }
                }
                Some("tool_use") => {
                    tool_calls.push(ToolCall {
                        id: block["id"].as_str().unwrap_or("").to_string(),
                        name: block["name"].as_str().unwrap_or("").to_string(),
                        input: block["input"].clone(),
                    });
                }
                _ => {}
            }
        }

        let stop_reason = match json["stop_reason"].as_str() {
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };

        Ok(AssistantMessage {
            text: if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            },
            tool_calls,
            stop_reason,
        })
    }
}

#[async_trait::async_trait]
impl JudgeProvider for AnthropicProvider {
    async fn chat(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[Tool],
    ) -> Result<AssistantMessage, JudgeError> {
        let messages = self.convert_messages(turns);
        let tool_defs = self.convert_tools(tools);

        let mut body = serde_json::json!({
            "model": &self.model,
            "max_tokens": 4096,
            "system": system,
            "messages": messages,
        });

        if !tool_defs.is_empty() {
            body["tools"] = serde_json::json!(tool_defs);
        }

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(JudgeError::Api {
                status,
                body: body_text,
            });
        }

        let resp_json: serde_json::Value = resp.json().await?;
        self.parse_response(resp_json)
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }
}

//! OpenAI-compatible API adapter.
//!
//! Works with: OpenAI, Ollama, Groq, Together, OpenRouter, vLLM, LM Studio —
//! anything that speaks the OpenAI chat completions format.

use crate::provider::{JudgeError, JudgeProvider};
use crate::types::{AssistantMessage, StopReason, Tool, ToolCall, Turn};

pub struct OpenAiProvider {
    api_key: Option<String>,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        Self {
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn convert_messages(
        &self,
        system: &str,
        turns: &[Turn],
    ) -> Vec<serde_json::Value> {
        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": system,
        })];

        for turn in turns {
            match turn {
                Turn::User(text) => {
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": text,
                    }));
                }
                Turn::Assistant(msg) => {
                    let mut m = serde_json::json!({ "role": "assistant" });
                    if let Some(text) = &msg.text {
                        m["content"] = serde_json::json!(text);
                    }
                    if !msg.tool_calls.is_empty() {
                        let tcs: Vec<_> = msg
                            .tool_calls
                            .iter()
                            .map(|tc| {
                                serde_json::json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.input.to_string(),
                                    }
                                })
                            })
                            .collect();
                        m["tool_calls"] = serde_json::json!(tcs);
                    }
                    messages.push(m);
                }
                Turn::ToolResults(results) => {
                    // OpenAI: each tool result is a separate message
                    for r in results {
                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": r.tool_call_id,
                            "content": r.content,
                        }));
                    }
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
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect()
    }

    fn parse_response(
        &self,
        json: serde_json::Value,
    ) -> Result<AssistantMessage, JudgeError> {
        let choice = json["choices"]
            .as_array()
            .and_then(|c| c.first())
            .ok_or_else(|| JudgeError::Parse("no choices in response".into()))?;

        let message = &choice["message"];
        let text = message["content"].as_str().map(|s| s.to_string());

        let mut tool_calls = Vec::new();
        if let Some(tcs) = message["tool_calls"].as_array() {
            for tc in tcs {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let input: serde_json::Value =
                    serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                tool_calls.push(ToolCall { id, name, input });
            }
        }

        let stop_reason = match choice["finish_reason"].as_str() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ => {
                if tool_calls.is_empty() {
                    StopReason::EndTurn
                } else {
                    StopReason::ToolUse
                }
            }
        };

        Ok(AssistantMessage {
            text,
            tool_calls,
            stop_reason,
        })
    }
}

#[async_trait::async_trait]
impl JudgeProvider for OpenAiProvider {
    async fn chat(
        &self,
        system: &str,
        turns: &[Turn],
        tools: &[Tool],
    ) -> Result<AssistantMessage, JudgeError> {
        let messages = self.convert_messages(system, turns);
        let tool_defs = self.convert_tools(tools);

        let mut body = serde_json::json!({
            "model": &self.model,
            "messages": messages,
        });

        if !tool_defs.is_empty() {
            body["tools"] = serde_json::json!(tool_defs);
        }

        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body);

        if let Some(key) = &self.api_key {
            req = req.header("authorization", format!("Bearer {}", key));
        }

        let resp = req.send().await?;
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
        "openai-compat"
    }

    fn model(&self) -> &str {
        &self.model
    }
}

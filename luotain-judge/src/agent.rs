//! The Agent — drives the spec → probe → verdict loop.
//!
//! This is the core entity of the Judgment Context. It holds state
//! for a single spec evaluation: the conversation history, collected
//! probes, and the eventual verdict.
//!
//! In DDD terms, each `test_spec` call is a complete lifecycle:
//! the Agent is created, drives probes, accumulates evidence,
//! and produces a verdict — then the lifecycle ends.

use crate::prompt;
use crate::provider::{JudgeError, JudgeProvider};
use crate::types::{Tool, ToolCall, ToolResult, Turn};
use luotain_core::http::HttpProbe;
use luotain_core::probe::{ProbeRequest, ProbeResult};
use luotain_core::session::VerdictOutcome;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Configuration for the agent loop.
pub struct AgentConfig {
    /// Maximum LLM turns per spec (prevents runaway loops)
    pub max_turns: usize,
    /// Maximum probes per spec (cost control)
    pub max_probes: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 25,
            max_probes: 50,
        }
    }
}

/// Result of testing a single spec — the aggregate output of one agent lifecycle.
#[derive(Debug, Clone, Serialize)]
pub struct SpecResult {
    pub spec_path: String,
    pub verdict: VerdictOutcome,
    pub evidence: Vec<String>,
    pub notes: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<FailureDetail>,
    pub probes: Vec<ProbeResult>,
    pub turns: usize,
    pub duration_ms: u64,
}

/// Detail about a specific behavior that failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDetail {
    pub behavior: String,
    pub expected: String,
    pub observed: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_id: Option<String>,
}

/// The blackbox testing agent.
pub struct Agent {
    provider: Box<dyn JudgeProvider>,
    http_probe: HttpProbe,
    config: AgentConfig,
}

impl Agent {
    pub fn new(provider: Box<dyn JudgeProvider>, config: AgentConfig) -> Self {
        Self {
            provider,
            http_probe: HttpProbe::new(),
            config,
        }
    }

    /// Test a single spec against a live target system.
    ///
    /// This drives a complete agent loop:
    /// 1. Present the spec to the LLM
    /// 2. LLM calls probe_http to test behaviors
    /// 3. We execute probes and return observations
    /// 4. LLM calls record_verdict when satisfied
    pub async fn test_spec(
        &self,
        spec_path: &str,
        spec_content: &str,
        target: &str,
    ) -> Result<SpecResult, JudgeError> {
        let start = Instant::now();
        let system = prompt::system_prompt();

        let mut turns = vec![Turn::User(format!(
            "## Spec: {}\n\n{}\n\n## Target\n\n{}",
            spec_path, spec_content, target
        ))];

        let tools = self.tool_definitions();
        let mut probes: Vec<ProbeResult> = Vec::new();
        let mut verdict: Option<VerdictData> = None;
        let mut probe_count: usize = 0;
        let mut turn_count: usize = 0;

        for _ in 0..self.config.max_turns {
            turn_count += 1;
            let response = self.provider.chat(&system, &turns, &tools).await?;

            if !response.has_tool_calls() {
                // Agent ended without calling record_verdict
                if verdict.is_none() {
                    verdict = Some(VerdictData {
                        outcome: VerdictOutcome::Inconclusive,
                        evidence: probes.iter().map(|p| p.id.clone()).collect(),
                        notes: response
                            .text
                            .unwrap_or_else(|| "Agent ended without verdict".into()),
                        failures: vec![],
                    });
                }
                break;
            }

            // Capture tool calls before moving response into turns
            let tool_calls: Vec<ToolCall> = response.tool_calls.clone();
            turns.push(Turn::Assistant(response));

            // Execute each tool call
            let mut results = Vec::new();
            for tc in &tool_calls {
                let (content, is_error) = self
                    .execute_tool(tc, target, &mut probes, &mut verdict, &mut probe_count)
                    .await;
                results.push(ToolResult {
                    tool_call_id: tc.id.clone(),
                    content,
                    is_error,
                });
            }

            turns.push(Turn::ToolResults(results));

            // If verdict was recorded, we're done
            if verdict.is_some() {
                break;
            }

            // Check probe limit
            if probe_count >= self.config.max_probes {
                tracing::warn!(
                    spec = %spec_path,
                    probes = probe_count,
                    limit = self.config.max_probes,
                    "probe limit reached"
                );
                verdict = Some(VerdictData {
                    outcome: VerdictOutcome::Inconclusive,
                    evidence: probes.iter().map(|p| p.id.clone()).collect(),
                    notes: format!(
                        "Probe limit reached ({}/{})",
                        probe_count, self.config.max_probes
                    ),
                    failures: vec![],
                });
                break;
            }
        }

        let verdict_data = verdict.unwrap_or(VerdictData {
            outcome: VerdictOutcome::Inconclusive,
            evidence: vec![],
            notes: format!("Max turns reached ({}) without verdict", self.config.max_turns),
            failures: vec![],
        });

        Ok(SpecResult {
            spec_path: spec_path.to_string(),
            verdict: verdict_data.outcome,
            evidence: verdict_data.evidence,
            notes: verdict_data.notes,
            failures: verdict_data.failures,
            probes,
            turns: turn_count,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn execute_tool(
        &self,
        call: &ToolCall,
        target: &str,
        probes: &mut Vec<ProbeResult>,
        verdict: &mut Option<VerdictData>,
        probe_count: &mut usize,
    ) -> (String, bool) {
        match call.name.as_str() {
            "probe_http" => {
                *probe_count += 1;
                self.execute_probe_http(call, target, probes).await
            }
            "record_verdict" => self.execute_record_verdict(call, probes, verdict),
            other => (format!("Unknown tool: {}", other), true),
        }
    }

    async fn execute_probe_http(
        &self,
        call: &ToolCall,
        target: &str,
        probes: &mut Vec<ProbeResult>,
    ) -> (String, bool) {
        let input = &call.input;

        let method = input["method"].as_str().unwrap_or("GET");
        let path = input["path"].as_str().unwrap_or("/");

        let full_url = format!(
            "{}/{}",
            target.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let mut headers = HashMap::new();
        if let Some(h) = input["headers"].as_object() {
            for (k, v) in h {
                if let Some(vs) = v.as_str() {
                    headers.insert(k.clone(), vs.to_string());
                }
            }
        }

        let body = input["body"].as_str().map(String::from);

        let request = ProbeRequest {
            method: method.to_string(),
            url: full_url,
            headers,
            body,
        };

        match self.http_probe.probe(&request).await {
            Ok(result) => {
                let json = serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| "failed to serialize".into());
                probes.push(result);
                (json, false)
            }
            Err(e) => (format!("Probe error: {}", e), true),
        }
    }

    fn execute_record_verdict(
        &self,
        call: &ToolCall,
        probes: &[ProbeResult],
        verdict: &mut Option<VerdictData>,
    ) -> (String, bool) {
        let input = &call.input;

        let outcome_str = input["outcome"].as_str().unwrap_or("inconclusive");
        let outcome = match outcome_str {
            "pass" => VerdictOutcome::Pass,
            "fail" => VerdictOutcome::Fail,
            "skip" => VerdictOutcome::Skip,
            _ => VerdictOutcome::Inconclusive,
        };

        let evidence: Vec<String> = input["evidence"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| probes.iter().map(|p| p.id.clone()).collect());

        let notes = input["notes"].as_str().unwrap_or("").to_string();

        let failures: Vec<FailureDetail> = input["failures"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        Some(FailureDetail {
                            behavior: f["behavior"].as_str()?.to_string(),
                            expected: f["expected"].as_str()?.to_string(),
                            observed: f["observed"].as_str()?.to_string(),
                            probe_id: f["probe_id"].as_str().map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        *verdict = Some(VerdictData {
            outcome,
            evidence,
            notes,
            failures,
        });

        (
            serde_json::json!({
                "recorded": true,
                "outcome": outcome_str,
            })
            .to_string(),
            false,
        )
    }

    fn tool_definitions(&self) -> Vec<Tool> {
        vec![
            Tool {
                name: "probe_http".into(),
                description: "Send an HTTP request to the target and observe the response. Returns: status code, headers, body (raw + parsed JSON), content type, body size, timing (ms).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "method": {
                            "type": "string",
                            "description": "HTTP method (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS)"
                        },
                        "path": {
                            "type": "string",
                            "description": "URL path relative to target (e.g., '/api/login', '/health')"
                        },
                        "headers": {
                            "type": "object",
                            "description": "Request headers",
                            "additionalProperties": { "type": "string" }
                        },
                        "body": {
                            "type": "string",
                            "description": "Request body (typically JSON string)"
                        }
                    },
                    "required": ["method", "path"]
                }),
            },
            Tool {
                name: "record_verdict".into(),
                description: "Record your final verdict after testing all behaviors in the spec.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "outcome": {
                            "type": "string",
                            "enum": ["pass", "fail", "skip", "inconclusive"],
                            "description": "pass=all match, fail=mismatch found, skip=can't test, inconclusive=ambiguous"
                        },
                        "evidence": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Probe IDs supporting this verdict"
                        },
                        "notes": {
                            "type": "string",
                            "description": "What you tested and found (be specific — mention status codes, values)"
                        },
                        "failures": {
                            "type": "array",
                            "description": "Failed behaviors (only when outcome=fail)",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "behavior": { "type": "string" },
                                    "expected": { "type": "string" },
                                    "observed": { "type": "string" },
                                    "probe_id": { "type": "string" }
                                },
                                "required": ["behavior", "expected", "observed"]
                            }
                        }
                    },
                    "required": ["outcome", "notes"]
                }),
            },
        ]
    }
}

/// Internal verdict data accumulated during the agent loop.
struct VerdictData {
    outcome: VerdictOutcome,
    evidence: Vec<String>,
    notes: String,
    failures: Vec<FailureDetail>,
}

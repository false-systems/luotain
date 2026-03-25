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
use luotain_core::config::TargetConfig;
use luotain_core::probe::ProbeResult;
use luotain_core::registry::ProbeRegistry;
use luotain_core::session::VerdictOutcome;
use serde::{Deserialize, Serialize};
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
    registry: ProbeRegistry,
    config: AgentConfig,
}

impl Agent {
    pub fn new(
        provider: Box<dyn JudgeProvider>,
        registry: ProbeRegistry,
        config: AgentConfig,
    ) -> Self {
        Self {
            provider,
            registry,
            config,
        }
    }

    /// Test a single spec against a live target system.
    pub async fn test_spec(
        &self,
        spec_path: &str,
        spec_content: &str,
        target: &TargetConfig,
    ) -> Result<SpecResult, JudgeError> {
        let start = Instant::now();

        // Build dynamic system prompt listing available probe tools
        let probe_names: Vec<&str> = self
            .registry
            .tool_definitions()
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        let system = prompt::system_prompt(&probe_names);

        let target_desc = target_description(target);

        let mut turns = vec![Turn::User(format!(
            "## Spec: {}\n\n{}\n\n## Target\n\n{}",
            spec_path, spec_content, target_desc
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

            let tool_calls: Vec<ToolCall> = response.tool_calls.clone();
            turns.push(Turn::Assistant(response));

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

            if verdict.is_some() {
                break;
            }

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
            notes: format!(
                "Max turns reached ({}) without verdict",
                self.config.max_turns
            ),
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
        target: &TargetConfig,
        probes: &mut Vec<ProbeResult>,
        verdict: &mut Option<VerdictData>,
        probe_count: &mut usize,
    ) -> (String, bool) {
        if call.name == "record_verdict" {
            return self.execute_record_verdict(call, probes, verdict);
        }

        // Dispatch to probe registry
        if self.registry.has_tool(&call.name) {
            *probe_count += 1;
            match self.registry.execute(&call.name, &call.input, target).await {
                Ok(result) => {
                    let json = serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| "failed to serialize".into());
                    probes.push(result);
                    (json, false)
                }
                Err(e) => (format!("Probe error: {}", e), true),
            }
        } else {
            (format!("Unknown tool: {}", call.name), true)
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

    /// Build tool definitions from registry + the always-present record_verdict.
    fn tool_definitions(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self
            .registry
            .tool_definitions()
            .iter()
            .map(|d| Tool {
                name: d.name.clone(),
                description: d.description.clone(),
                parameters: d.parameters.clone(),
            })
            .collect();

        tools.push(Tool {
            name: "record_verdict".into(),
            description: "Record your final verdict after testing all behaviors in the spec."
                .into(),
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
                        "description": "What you tested and found (be specific)"
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
        });

        tools
    }
}

/// Human-readable target description for the LLM.
fn target_description(target: &TargetConfig) -> String {
    use luotain_core::config::Connection;
    match &target.connection {
        Connection::Http { base_url } => base_url.clone(),
        Connection::Grpc { host, port, .. } => format!("gRPC at {}:{}", host, port),
        Connection::Tcp { host, port, .. } => format!("TCP at {}:{}", host, port),
        Connection::Cli { command, .. } => format!("CLI: {}", command),
    }
}

struct VerdictData {
    outcome: VerdictOutcome,
    evidence: Vec<String>,
    notes: String,
    failures: Vec<FailureDetail>,
}

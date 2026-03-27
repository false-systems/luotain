//! Luotain Loop — continuous blackbox probe runner.
//!
//! Reads a product directory, spins up an LLM agent session on each interval,
//! probes the system, and writes results. Uses OpenAI-compatible APIs so it
//! works with free/local models (Ollama, Mistral, etc.).

use chrono::Utc;
use clap::Parser;
use luotain_core::http::HttpProbe;
use luotain_core::product::ProductTree;
use luotain_core::result::{FeatureResult, SpecResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "luotain-loop", about = "Continuous blackbox probe runner")]
struct Args {
    /// Path to the product directory
    #[arg(long)]
    product: PathBuf,

    /// Probe interval in seconds (default: 300)
    #[arg(long, default_value = "300")]
    interval: u64,

    /// OpenAI-compatible API base URL (default: http://localhost:11434/v1 for Ollama)
    #[arg(long, default_value = "http://localhost:11434/v1")]
    agent_url: String,

    /// Model name (default: llama3)
    #[arg(long, default_value = "llama3")]
    model: String,

    /// API key (optional, for hosted providers)
    #[arg(long)]
    api_key: Option<String>,

    /// Target URL override (otherwise uses _config.md)
    #[arg(long)]
    target: Option<String>,

    /// Adversarial mode — try to break features
    #[arg(long)]
    adversarial: bool,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ToolCallResponse {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: FunctionCall,
}

#[derive(Serialize, Deserialize, Clone)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallResponse>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    info!(
        product = %args.product.display(),
        interval = args.interval,
        model = %args.model,
        adversarial = args.adversarial,
        "Starting luotain-loop"
    );

    let product = ProductTree::open(&args.product)?;
    let target = resolve_target(&args, &product)?;

    loop {
        info!("Starting probe cycle");
        match run_cycle(&args, &product, &target).await {
            Ok(summary) => {
                info!(
                    passed = summary.passed,
                    failed = summary.failed,
                    skipped = summary.skipped,
                    "Cycle complete"
                );
            }
            Err(e) => {
                error!(error = %e, "Cycle failed");
            }
        }

        info!(interval = args.interval, "Sleeping until next cycle");
        tokio::time::sleep(Duration::from_secs(args.interval)).await;
    }
}

fn resolve_target(args: &Args, product: &ProductTree) -> anyhow::Result<String> {
    if let Some(ref t) = args.target {
        return Ok(t.clone());
    }
    // Try _config.md via luotain-core's config system
    let specs_dir = product.root().join("specs");
    if specs_dir.is_dir() {
        if let Ok(Some(config)) = luotain_core::config::load_config(&specs_dir, None) {
            if let luotain_core::config::Connection::Http { base_url } = config.connection {
                return Ok(base_url);
            }
        }
    }
    // Also try _config.md at product root
    let product_root = product.root();
    if let Ok(Some(config)) = luotain_core::config::load_config(product_root, None) {
        if let luotain_core::config::Connection::Http { base_url } = config.connection {
            return Ok(base_url);
        }
    }
    anyhow::bail!("No target URL. Use --target or set base_url in _config.md")
}

struct CycleSummary {
    passed: usize,
    failed: usize,
    skipped: usize,
}

async fn run_cycle(
    args: &Args,
    product: &ProductTree,
    target: &str,
) -> anyhow::Result<CycleSummary> {
    let product_desc = product.read_product()?;
    let specs = product.specs()?;
    let spec_list = specs.list_specs()?;

    if spec_list.is_empty() {
        warn!("No specs found");
        return Ok(CycleSummary { passed: 0, failed: 0, skipped: 0 });
    }

    let http_probe = HttpProbe::new();
    let client = reqwest::Client::new();
    let today = Utc::now().format("%Y-%m-%d").to_string();

    let mut summary = CycleSummary { passed: 0, failed: 0, skipped: 0 };

    for spec_path in &spec_list {
        let spec_content = specs.read_spec(spec_path)?;
        info!(spec = %spec_path, "Evaluating spec");

        match evaluate_spec(
            &client,
            &http_probe,
            args,
            target,
            &product_desc,
            spec_path,
            &spec_content,
        )
        .await
        {
            Ok(result) => {
                match result.verdict.as_str() {
                    "pass" => summary.passed += 1,
                    "fail" => summary.failed += 1,
                    _ => summary.skipped += 1,
                }
                info!(spec = %spec_path, verdict = %result.verdict, "Spec evaluated");
                if let Err(e) = product.write_result(&today, &result) {
                    error!(error = %e, "Failed to write result");
                }
            }
            Err(e) => {
                error!(spec = %spec_path, error = %e, "Failed to evaluate spec");
                summary.skipped += 1;
            }
        }
    }

    Ok(summary)
}

async fn evaluate_spec(
    client: &reqwest::Client,
    http_probe: &HttpProbe,
    args: &Args,
    target: &str,
    product_desc: &str,
    spec_path: &str,
    spec_content: &str,
) -> anyhow::Result<SpecResult> {
    let system_prompt = build_system_prompt(product_desc, target, args.adversarial);

    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: Some(system_prompt),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".into(),
            content: Some(format!(
                "Evaluate this spec: {}\n\n{}",
                spec_path, spec_content
            )),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let tools = build_tools();
    let mut probe_ids = Vec::new();

    // Agent loop: max 15 turns
    for _ in 0..15 {
        let request = ChatRequest {
            model: args.model.clone(),
            messages: messages.clone(),
            tools: Some(tools.clone()),
        };

        let mut req = client
            .post(format!("{}/chat/completions", args.agent_url))
            .json(&request);

        if let Some(ref key) = args.api_key {
            req = req.bearer_auth(key);
        }

        let resp_raw = req.send().await?;
        let status = resp_raw.status();
        if !status.is_success() {
            let body = resp_raw.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("agent API error: HTTP {} - {}", status, body));
        }
        let resp: ChatResponse = resp_raw.json().await?;
        let choice = resp.choices.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("no choices in response"))?;

        // If model returned tool calls, execute them
        if let Some(ref tool_calls) = choice.message.tool_calls {
            // Add assistant message with tool calls
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: choice.message.content.clone(),
                tool_calls: Some(tool_calls.clone()),
                tool_call_id: None,
            });

            for tc in tool_calls {
                let args_json: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
                    Ok(v) => v,
                    Err(e) => {
                        let tool_result = serde_json::json!({
                            "error": format!("invalid JSON arguments for tool '{}': {}", tc.function.name, e)
                        }).to_string();
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: Some(tool_result),
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                        });
                        continue;
                    }
                };

                let tool_result = match tc.function.name.as_str() {
                    "probe_http" => {
                        execute_http_probe(http_probe, target, &args_json, &mut probe_ids).await
                    }
                    "record_result" => {
                        // Agent is recording its verdict — parse and return
                        let result = parse_spec_result(spec_path, &args_json, &probe_ids, args.adversarial);
                        return Ok(result);
                    }
                    other => {
                        serde_json::json!({"error": format!("unknown tool: {}", other)}).to_string()
                    }
                };

                messages.push(ChatMessage {
                    role: "tool".into(),
                    content: Some(tool_result),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                });
            }
        } else {
            // No tool calls — model finished with text. Try to extract verdict.
            let text = choice.message.content.unwrap_or_default();
            let lowercase = text.to_lowercase();
            let has_fail = lowercase.split_whitespace()
                .any(|w| w.trim_matches(|c: char| !c.is_ascii_alphabetic()) == "fail");
            let has_pass = lowercase.split_whitespace()
                .any(|w| w.trim_matches(|c: char| !c.is_ascii_alphabetic()) == "pass");
            let verdict = if has_fail {
                "fail"
            } else if has_pass {
                "pass"
            } else {
                "inconclusive"
            };

            return Ok(SpecResult {
                spec: spec_path.to_string(),
                timestamp: Utc::now(),
                verdict: verdict.to_string(),
                features: vec![],
                probes: probe_ids,
                mode: if args.adversarial { Some("adversarial".into()) } else { None },
            });
        }

        // Check if model signaled stop
        if choice.finish_reason.as_deref() == Some("stop") && choice.message.tool_calls.is_none() {
            break;
        }
    }

    // Exhausted turns
    Ok(SpecResult {
        spec: spec_path.to_string(),
        timestamp: Utc::now(),
        verdict: "inconclusive".into(),
        features: vec![],
        probes: probe_ids,
        mode: if args.adversarial { Some("adversarial".into()) } else { None },
    })
}

fn build_system_prompt(product_desc: &str, target: &str, adversarial: bool) -> String {
    let mode_instructions = if adversarial {
        "You are in ADVERSARIAL MODE. Your goal is to try to BREAK each feature described in the spec. \
         Send wrong inputs, missing fields, boundary values, malformed payloads, unexpected HTTP methods. \
         A 'pass' means the system handled your attack gracefully (correct error codes, no 500s, no hangs). \
         A 'fail' means you found a vulnerability (500 errors, hangs, data corruption, wrong error codes)."
    } else {
        "Your goal is to verify that each feature described in the spec works as described. \
         A 'pass' means behavior matches the spec. A 'fail' means observed behavior differs."
    };

    format!(
        "You are a blackbox testing agent. You test software by probing it from the outside.\n\n\
         ## Product\n{}\n\n\
         ## Target\n{}\n\n\
         ## Instructions\n{}\n\n\
         ## Available Tools\n\
         - probe_http: Send HTTP requests and observe responses\n\
         - record_result: Record your verdict with per-feature breakdown\n\n\
         ## Process\n\
         1. Read the spec carefully — identify each testable feature\n\
         2. Send probes to verify each feature\n\
         3. Call record_result with your verdict and per-feature breakdown\n\n\
         Always call record_result when done. Include a 'why' for any failed feature.",
        product_desc, target, mode_instructions
    )
}

fn build_tools() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "probe_http",
                "description": "Send an HTTP request to the target system",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "method": { "type": "string", "description": "HTTP method" },
                        "path": { "type": "string", "description": "URL path (appended to target base)" },
                        "headers": { "type": "object", "description": "HTTP headers" },
                        "body": { "type": "string", "description": "Request body" }
                    },
                    "required": ["method", "path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "record_result",
                "description": "Record your final verdict for this spec",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "verdict": {
                            "type": "string",
                            "enum": ["pass", "fail", "skip", "inconclusive"]
                        },
                        "features": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "description": { "type": "string" },
                                    "verdict": { "type": "string", "enum": ["pass", "fail", "skip"] },
                                    "why": { "type": "string" }
                                },
                                "required": ["description", "verdict"]
                            }
                        }
                    },
                    "required": ["verdict", "features"]
                }
            }
        }),
    ]
}

async fn execute_http_probe(
    http_probe: &HttpProbe,
    target: &str,
    args: &serde_json::Value,
    probe_ids: &mut Vec<String>,
) -> String {
    let method = args["method"].as_str().unwrap_or("GET");
    let path = args["path"].as_str().unwrap_or("/");

    let full_url = format!(
        "{}/{}",
        target.trim_end_matches('/'),
        path.trim_start_matches('/')
    );

    let mut headers = std::collections::HashMap::new();
    if let Some(h) = args.get("headers").and_then(|h| h.as_object()) {
        for (k, v) in h {
            if let Some(v_str) = v.as_str() {
                headers.insert(k.clone(), v_str.to_string());
            }
        }
    }

    let body = args.get("body").and_then(|b| b.as_str()).map(|s| s.to_string());

    let request = luotain_core::probe::ProbeRequest {
        method: method.to_string(),
        url: full_url,
        headers,
        body,
    };

    match http_probe.probe(&request).await {
        Ok(result) => {
            probe_ids.push(result.id.clone());
            serde_json::to_string(&result).unwrap_or_else(|_| "{}".into())
        }
        Err(e) => {
            serde_json::json!({"error": e.to_string()}).to_string()
        }
    }
}

fn parse_spec_result(
    spec_path: &str,
    args: &serde_json::Value,
    probe_ids: &[String],
    adversarial: bool,
) -> SpecResult {
    let verdict = args["verdict"].as_str().unwrap_or("inconclusive").to_string();

    let features: Vec<FeatureResult> = args
        .get("features")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .map(|f| FeatureResult {
                    description: f["description"].as_str().unwrap_or("").to_string(),
                    verdict: f["verdict"].as_str().unwrap_or("skip").to_string(),
                    why: f.get("why").and_then(|w| w.as_str()).map(String::from),
                })
                .collect()
        })
        .unwrap_or_default();

    SpecResult {
        spec: spec_path.to_string(),
        timestamp: Utc::now(),
        verdict,
        features,
        probes: probe_ids.to_vec(),
        mode: if adversarial { Some("adversarial".into()) } else { None },
    }
}

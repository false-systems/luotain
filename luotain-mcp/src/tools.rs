use luotain_core::http::HttpProbe;
use luotain_core::probe::ProbeRequest;
use luotain_core::session::{SessionHandle, Verdict, VerdictOutcome};
use luotain_core::spec::SpecTree;
use std::collections::HashMap;
use std::path::PathBuf;

pub enum McpResponse {
    Result(serde_json::Value),
    Error { code: i32, message: String },
}

pub struct LuotainState {
    spec_root: PathBuf,
    target: String,
    http_probe: HttpProbe,
    session: SessionHandle,
}

impl LuotainState {
    pub fn new(spec_root: PathBuf, target: String) -> Self {
        let session = SessionHandle::new(
            spec_root.to_string_lossy().to_string(),
            target.clone(),
        );
        Self {
            spec_root,
            target,
            http_probe: HttpProbe::new(),
            session,
        }
    }

    pub async fn handle(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> McpResponse {
        match method {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(params).await,
            _ => McpResponse::Error {
                code: -32601,
                message: format!("Method not found: {}", method),
            },
        }
    }

    fn handle_initialize(&self) -> McpResponse {
        McpResponse::Result(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "luotain",
                "version": "0.1.0"
            }
        }))
    }

    fn handle_tools_list(&self) -> McpResponse {
        McpResponse::Result(serde_json::json!({
            "tools": [
                {
                    "name": "luotain_list_specs",
                    "description": "List the spec tree structure. Returns directories and markdown spec files that describe expected behavior of the target system.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "luotain_read_spec",
                    "description": "Read a spec file's markdown content. Specs describe expected behavior for a specific area of the target system.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative path to the spec file (e.g., 'auth/login.md')"
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "luotain_probe_http",
                    "description": "Send an HTTP request to the target system and observe the response. Returns status, headers, body (parsed as JSON if applicable), timing, and errors.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "method": {
                                "type": "string",
                                "description": "HTTP method (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS)"
                            },
                            "url": {
                                "type": "string",
                                "description": "Full URL or path (joined with target base URL if relative)"
                            },
                            "headers": {
                                "type": "object",
                                "description": "HTTP headers as key-value pairs",
                                "additionalProperties": { "type": "string" }
                            },
                            "body": {
                                "type": "string",
                                "description": "Request body (typically JSON string)"
                            },
                            "spec_path": {
                                "type": "string",
                                "description": "Which spec this probe relates to (for session tracking)"
                            }
                        },
                        "required": ["method", "url"]
                    }
                },
                {
                    "name": "luotain_record_verdict",
                    "description": "Record a pass/fail verdict for a spec. The agent's judgment on whether the target system satisfies the spec's described behavior.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "spec_path": {
                                "type": "string",
                                "description": "Relative path to the spec file"
                            },
                            "outcome": {
                                "type": "string",
                                "enum": ["pass", "fail", "skip", "inconclusive"],
                                "description": "The verdict outcome"
                            },
                            "evidence": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Probe IDs that support this verdict"
                            },
                            "notes": {
                                "type": "string",
                                "description": "Agent's reasoning for the verdict"
                            }
                        },
                        "required": ["spec_path", "outcome"]
                    }
                },
                {
                    "name": "luotain_report",
                    "description": "Get the current session report: total probes, verdicts per spec, pass/fail/skip counts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }
            ]
        }))
    }

    async fn handle_tools_call(&self, params: Option<serde_json::Value>) -> McpResponse {
        let params = params.unwrap_or(serde_json::json!({}));
        let tool_name = params
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        let result = match tool_name {
            "luotain_list_specs" => self.tool_list_specs(),
            "luotain_read_spec" => self.tool_read_spec(&arguments),
            "luotain_probe_http" => self.tool_probe_http(&arguments).await,
            "luotain_record_verdict" => self.tool_record_verdict(&arguments),
            "luotain_report" => self.tool_report(),
            _ => Err(format!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(content) => McpResponse::Result(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&content).unwrap_or_default()
                }]
            })),
            Err(e) => McpResponse::Result(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", e)
                }],
                "isError": true
            })),
        }
    }

    fn tool_list_specs(&self) -> Result<serde_json::Value, String> {
        let tree = SpecTree::open(&self.spec_root).map_err(|e| e.to_string())?;
        let root = tree.walk().map_err(|e| e.to_string())?;
        serde_json::to_value(root).map_err(|e| e.to_string())
    }

    fn tool_read_spec(&self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or("missing 'path' parameter")?;
        let tree = SpecTree::open(&self.spec_root).map_err(|e| e.to_string())?;
        let content = tree.read_spec(path).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "path": path,
            "content": content
        }))
    }

    async fn tool_probe_http(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let method = args
            .get("method")
            .and_then(|m| m.as_str())
            .ok_or("missing 'method'")?;
        let url_str = args
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or("missing 'url'")?;

        // Join relative URLs with target base
        let full_url = if url_str.starts_with("http://") || url_str.starts_with("https://") {
            url_str.to_string()
        } else {
            let base = self.target.trim_end_matches('/');
            let path = url_str.trim_start_matches('/');
            format!("{}/{}", base, path)
        };

        let mut headers = HashMap::new();
        if let Some(h) = args.get("headers").and_then(|h| h.as_object()) {
            for (k, v) in h {
                if let Some(v_str) = v.as_str() {
                    headers.insert(k.clone(), v_str.to_string());
                }
            }
        }

        let body = args
            .get("body")
            .and_then(|b| b.as_str())
            .map(|s| s.to_string());
        let spec_path = args
            .get("spec_path")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());

        let request = ProbeRequest {
            method: method.to_string(),
            url: full_url,
            headers,
            body,
        };

        let mut result = self
            .http_probe
            .probe(&request)
            .await
            .map_err(|e| e.to_string())?;
        result.spec_path = spec_path;

        self.session.record_probe(result.clone());

        serde_json::to_value(result).map_err(|e| e.to_string())
    }

    fn tool_record_verdict(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let spec_path = args
            .get("spec_path")
            .and_then(|s| s.as_str())
            .ok_or("missing 'spec_path'")?;
        let outcome_str = args
            .get("outcome")
            .and_then(|o| o.as_str())
            .ok_or("missing 'outcome'")?;

        let outcome = match outcome_str {
            "pass" => VerdictOutcome::Pass,
            "fail" => VerdictOutcome::Fail,
            "skip" => VerdictOutcome::Skip,
            "inconclusive" => VerdictOutcome::Inconclusive,
            other => return Err(format!("invalid outcome: {}", other)),
        };

        let evidence: Vec<String> = args
            .get("evidence")
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let notes = args
            .get("notes")
            .and_then(|n| n.as_str())
            .map(String::from);

        let verdict = Verdict {
            spec_path: spec_path.to_string(),
            outcome,
            evidence,
            notes,
            timestamp: chrono::Utc::now(),
        };

        self.session.record_verdict(verdict);

        Ok(serde_json::json!({
            "recorded": true,
            "spec_path": spec_path,
            "outcome": outcome_str
        }))
    }

    fn tool_report(&self) -> Result<serde_json::Value, String> {
        let report = self
            .session
            .report()
            .ok_or("failed to get session report")?;
        serde_json::to_value(report).map_err(|e| e.to_string())
    }
}

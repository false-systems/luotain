use luotain_core::http::HttpProbe;
use luotain_core::probe::ProbeRequest;
use luotain_core::product::ProductTree;
use luotain_core::result::SpecResult;
use luotain_core::session::{SessionHandle, Verdict, VerdictOutcome};
use luotain_core::spec::SpecTree;
use std::collections::HashMap;
use std::path::PathBuf;

pub enum McpResponse {
    Result(serde_json::Value),
    Error { code: i32, message: String },
}

/// Operating mode: standalone spec tree or product directory.
enum Mode {
    SpecTree {
        spec_root: PathBuf,
    },
    Product {
        product: ProductTree,
    },
}

pub struct LuotainState {
    mode: Mode,
    target: String,
    http_probe: HttpProbe,
    session: SessionHandle,
    adversarial: bool,
}

impl LuotainState {
    pub fn new(spec_root: PathBuf, target: String) -> Self {
        let session = SessionHandle::new(
            spec_root.to_string_lossy().to_string(),
            target.clone(),
        );
        Self {
            mode: Mode::SpecTree { spec_root },
            target,
            http_probe: HttpProbe::new(),
            session,
            adversarial: false,
        }
    }

    pub fn new_product(product_root: PathBuf, target: String, adversarial: bool) -> Result<Self, String> {
        let product = ProductTree::open(&product_root).map_err(|e| e.to_string())?;
        let session = SessionHandle::new(
            product_root.to_string_lossy().to_string(),
            target.clone(),
        );
        Ok(Self {
            mode: Mode::Product { product },
            target,
            http_probe: HttpProbe::new(),
            session,
            adversarial,
        })
    }

    fn spec_tree(&self) -> Result<SpecTree, String> {
        match &self.mode {
            Mode::SpecTree { spec_root } => {
                SpecTree::open(spec_root).map_err(|e| e.to_string())
            }
            Mode::Product { product } => {
                product.specs().map_err(|e| e.to_string())
            }
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
                "version": "0.2.0"
            }
        }))
    }

    fn handle_tools_list(&self) -> McpResponse {
        let mut tools = Vec::new();

        // Product-mode tools
        if matches!(self.mode, Mode::Product { .. }) {
            tools.push(serde_json::json!({
                "name": "luotain_read_product",
                "description": "Read product.md — the product description. Call this first before any probes. This is the agent's only knowledge of the system under test.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }));
        }

        // Common tools
        tools.push(serde_json::json!({
            "name": "luotain_list_specs",
            "description": "List the spec tree structure. Returns directories and markdown spec files that describe expected behavior of the target system.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }));

        tools.push(serde_json::json!({
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
        }));

        tools.push(serde_json::json!({
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
        }));

        tools.push(serde_json::json!({
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
        }));

        tools.push(serde_json::json!({
            "name": "luotain_report",
            "description": "Get the current session report: total probes, verdicts per spec, pass/fail/skip counts.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }));

        // Product-mode result tools
        if matches!(self.mode, Mode::Product { .. }) {
            tools.push(serde_json::json!({
                "name": "luotain_write_result",
                "description": "Write a SpecResult JSON to results/YYYY-MM-DD/<spec_path>.json. The result includes per-feature verdicts.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "result": {
                            "type": "object",
                            "description": "The SpecResult JSON object with spec, timestamp, verdict, features, probes fields"
                        }
                    },
                    "required": ["result"]
                }
            }));

            tools.push(serde_json::json!({
                "name": "luotain_read_results",
                "description": "Read all results for a given date. Returns summary of all spec results.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "date": {
                            "type": "string",
                            "description": "Date in YYYY-MM-DD format. Defaults to today."
                        }
                    }
                }
            }));
        }

        McpResponse::Result(serde_json::json!({ "tools": tools }))
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
            "luotain_read_product" => self.tool_read_product(),
            "luotain_list_specs" => self.tool_list_specs(),
            "luotain_read_spec" => self.tool_read_spec(&arguments),
            "luotain_probe_http" => self.tool_probe_http(&arguments).await,
            "luotain_record_verdict" => self.tool_record_verdict(&arguments),
            "luotain_report" => self.tool_report(),
            "luotain_write_result" => self.tool_write_result(&arguments),
            "luotain_read_results" => self.tool_read_results(&arguments),
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

    fn tool_read_product(&self) -> Result<serde_json::Value, String> {
        match &self.mode {
            Mode::Product { product } => {
                let content = product.read_product().map_err(|e| e.to_string())?;
                let mut resp = serde_json::json!({
                    "content": content,
                });
                if self.adversarial {
                    resp["mode"] = serde_json::json!("adversarial");
                    resp["instructions"] = serde_json::json!(
                        "ADVERSARIAL MODE: Try to break each feature. Send wrong inputs, missing fields, boundary values, malformed payloads. A 'pass' means the system handled it gracefully (correct error codes, no 500s, no hangs)."
                    );
                }
                Ok(resp)
            }
            Mode::SpecTree { .. } => Err("read_product is only available in product mode".into()),
        }
    }

    fn tool_list_specs(&self) -> Result<serde_json::Value, String> {
        let tree = self.spec_tree()?;
        let root = tree.walk().map_err(|e| e.to_string())?;
        serde_json::to_value(root).map_err(|e| e.to_string())
    }

    fn tool_read_spec(&self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or("missing 'path' parameter")?;
        let tree = self.spec_tree()?;
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

    fn tool_write_result(&self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        match &self.mode {
            Mode::Product { product } => {
                let result_json = args
                    .get("result")
                    .ok_or("missing 'result' parameter")?;
                let mut result: SpecResult =
                    serde_json::from_value(result_json.clone()).map_err(|e| e.to_string())?;

                // Tag adversarial mode
                if self.adversarial {
                    result.mode = Some("adversarial".into());
                }

                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                let path = product.write_result(&today, &result).map_err(|e| e.to_string())?;

                Ok(serde_json::json!({
                    "written": true,
                    "path": path.to_string_lossy(),
                    "spec": result.spec,
                    "verdict": result.verdict
                }))
            }
            Mode::SpecTree { .. } => Err("write_result is only available in product mode".into()),
        }
    }

    fn tool_read_results(&self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        match &self.mode {
            Mode::Product { product } => {
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                let date = args
                    .get("date")
                    .and_then(|d| d.as_str())
                    .unwrap_or(&today);

                if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                    return Err("invalid 'date' parameter; expected format YYYY-MM-DD".into());
                }

                let results = product.read_results(date).map_err(|e| e.to_string())?;

                let passed = results.iter().filter(|r| r.verdict == "pass").count();
                let failed = results.iter().filter(|r| r.verdict == "fail").count();

                Ok(serde_json::json!({
                    "date": date,
                    "total": results.len(),
                    "passed": passed,
                    "failed": failed,
                    "results": results
                }))
            }
            Mode::SpecTree { .. } => Err("read_results is only available in product mode".into()),
        }
    }
}

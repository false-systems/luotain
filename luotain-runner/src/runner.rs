//! Test run orchestration — the Orchestration Context.
//!
//! The Runner coordinates a complete test pass: walks the spec tree,
//! invokes the judge agent for each spec, and assembles the report.
//! It does NOT interpret specs or produce verdicts — that's the judge's job.

use crate::report::TestReport;
use luotain_core::config::{self, ConfigError, Connection, TargetConfig};
use luotain_core::session::VerdictOutcome;
use luotain_core::spec::SpecTree;
use luotain_judge::agent::Agent;
use luotain_judge::provider::JudgeError;
use std::path::Path;
use std::time::Instant;

/// Configuration for a test run.
pub struct RunConfig {
    pub spec_root: String,
    /// CLI --target override (takes precedence over _config.md)
    pub target_override: Option<String>,
    /// Environment name for config overrides (--env staging)
    pub env: Option<String>,
}

/// Orchestrates a complete test pass over a spec tree.
pub struct Runner {
    agent: Agent,
}

impl Runner {
    pub fn new(agent: Agent) -> Self {
        Self { agent }
    }

    /// Run all specs and produce a report.
    pub async fn run(&self, config: &RunConfig) -> Result<TestReport, RunError> {
        let start = Instant::now();

        // Resolve target config: CLI override > _config.md > error
        let target_config = self.resolve_target(config)?;
        let target_url = match &target_config.connection {
            Connection::Http { base_url } => base_url.clone(),
            Connection::Grpc { host, port, .. } => format!("{}:{}", host, port),
            Connection::Tcp { host, port, .. } => format!("{}:{}", host, port),
            Connection::Cli { command, .. } => command.clone(),
        };

        let tree =
            SpecTree::open(&config.spec_root).map_err(|e| RunError::Spec(e.to_string()))?;
        let spec_paths = tree
            .list_specs()
            .map_err(|e| RunError::Spec(e.to_string()))?;

        if spec_paths.is_empty() {
            tracing::warn!(root = %config.spec_root, "no specs found");
        }

        let mut results = Vec::new();

        for spec_path in &spec_paths {
            tracing::info!(spec = %spec_path, "testing");

            let content = tree
                .read_spec(spec_path)
                .map_err(|e| RunError::Spec(e.to_string()))?;

            match self
                .agent
                .test_spec(spec_path, &content, &target_config)
                .await
            {
                Ok(result) => {
                    let symbol = match result.verdict {
                        VerdictOutcome::Pass => "PASS",
                        VerdictOutcome::Fail => "FAIL",
                        VerdictOutcome::Skip => "SKIP",
                        VerdictOutcome::Inconclusive => "INCONCLUSIVE",
                    };
                    tracing::info!(
                        spec = %spec_path,
                        verdict = symbol,
                        probes = result.probes.len(),
                        turns = result.turns,
                        ms = result.duration_ms,
                        "done"
                    );
                    results.push(result);
                }
                Err(e) => {
                    tracing::error!(spec = %spec_path, error = %e, "judge error");
                    return Err(RunError::Judge(e));
                }
            }
        }

        Ok(TestReport::from_results(
            config.spec_root.clone(),
            target_url,
            results,
            start.elapsed().as_millis() as u64,
        ))
    }

    fn resolve_target(&self, config: &RunConfig) -> Result<TargetConfig, RunError> {
        // CLI --target always wins
        if let Some(url) = &config.target_override {
            let mut tc = config::config_from_url(url.clone());
            // Still load _config.md for auth even with URL override
            if let Ok(Some(file_config)) =
                config::load_config(Path::new(&config.spec_root), config.env.as_deref())
            {
                tc.auth = file_config.auth;
            }
            return Ok(tc);
        }

        // Try _config.md
        match config::load_config(Path::new(&config.spec_root), config.env.as_deref())? {
            Some(tc) => Ok(tc),
            None => Err(RunError::NoTarget),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("spec error: {0}")]
    Spec(String),
    #[error("judge error: {0}")]
    Judge(#[from] JudgeError),
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("no target — provide --target URL or add _config.md to the spec root")]
    NoTarget,
}

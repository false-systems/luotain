//! Test run orchestration — the Orchestration Context.
//!
//! The Runner coordinates a complete test pass: walks the spec tree,
//! invokes the judge agent for each spec, and assembles the report.
//! It does NOT interpret specs or produce verdicts — that's the judge's job.

use crate::report::TestReport;
use luotain_core::session::VerdictOutcome;
use luotain_core::spec::SpecTree;
use luotain_judge::agent::Agent;
use luotain_judge::provider::JudgeError;
use std::time::Instant;

/// Configuration for a test run.
pub struct RunConfig {
    pub spec_root: String,
    pub target: String,
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
                .test_spec(spec_path, &content, &config.target)
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
            config.target.clone(),
            results,
            start.elapsed().as_millis() as u64,
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("spec error: {0}")]
    Spec(String),
    #[error("judge error: {0}")]
    Judge(#[from] JudgeError),
}

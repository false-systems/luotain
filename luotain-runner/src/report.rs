//! Test report — the aggregate root of a completed test run.

use luotain_core::session::VerdictOutcome;
use luotain_judge::agent::SpecResult;
use serde::Serialize;

/// Full test report from a run.
#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub session_id: String,
    pub spec_root: String,
    pub target: String,
    pub duration_ms: u64,
    pub summary: Summary,
    pub specs: Vec<SpecResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub inconclusive: usize,
}

impl TestReport {
    pub fn from_results(
        spec_root: String,
        target: String,
        specs: Vec<SpecResult>,
        duration_ms: u64,
    ) -> Self {
        let total = specs.len();
        let passed = specs
            .iter()
            .filter(|s| s.verdict == VerdictOutcome::Pass)
            .count();
        let failed = specs
            .iter()
            .filter(|s| s.verdict == VerdictOutcome::Fail)
            .count();
        let skipped = specs
            .iter()
            .filter(|s| s.verdict == VerdictOutcome::Skip)
            .count();
        let inconclusive = specs
            .iter()
            .filter(|s| s.verdict == VerdictOutcome::Inconclusive)
            .count();

        Self {
            session_id: ulid::Ulid::new().to_string(),
            spec_root,
            target,
            duration_ms,
            summary: Summary {
                total,
                passed,
                failed,
                skipped,
                inconclusive,
            },
            specs,
        }
    }

    /// Exit code for CI: 0 = all pass, 1 = failures
    pub fn exit_code(&self) -> i32 {
        if self.summary.failed > 0 {
            1
        } else {
            0
        }
    }

    /// Print human-readable summary to stderr.
    pub fn print_summary(&self) {
        eprintln!();
        eprintln!("── Luotain Report ──────────────────────────────");
        eprintln!("Target:  {}", self.target);
        eprintln!("Specs:   {}", self.spec_root);
        eprintln!("Time:    {}ms", self.duration_ms);
        eprintln!();

        for spec in &self.specs {
            let symbol = match spec.verdict {
                VerdictOutcome::Pass => "  ✓",
                VerdictOutcome::Fail => "  ✗",
                VerdictOutcome::Skip => "  ⊘",
                VerdictOutcome::Inconclusive => "  ?",
            };
            eprintln!("{} {}", symbol, spec.spec_path);
            if !spec.notes.is_empty() {
                // Indent notes, truncate if very long
                let notes = if spec.notes.len() > 200 {
                    format!("{}...", &spec.notes[..200])
                } else {
                    spec.notes.clone()
                };
                eprintln!("    {}", notes);
            }
            for f in &spec.failures {
                eprintln!(
                    "    FAIL: {} — expected: {}, got: {}",
                    f.behavior, f.expected, f.observed
                );
            }
        }

        eprintln!();
        eprintln!(
            "  {}/{} passed, {} failed, {} skipped, {} inconclusive",
            self.summary.passed,
            self.summary.total,
            self.summary.failed,
            self.summary.skipped,
            self.summary.inconclusive,
        );
        eprintln!("────────────────────────────────────────────────");
        eprintln!();
    }
}

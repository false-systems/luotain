use crate::probe::ProbeResult;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A test session — collects probes and verdicts for a spec tree run.
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub spec_root: String,
    pub target: String,
    pub started_at: DateTime<Utc>,
    pub probes: Vec<ProbeResult>,
    pub verdicts: HashMap<String, Verdict>,
}

/// The agent's verdict on whether a spec is satisfied.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub spec_path: String,
    pub outcome: VerdictOutcome,
    /// Probe IDs that support this verdict
    pub evidence: Vec<String>,
    /// Agent's reasoning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VerdictOutcome {
    Pass,
    Fail,
    Skip,
    Inconclusive,
}

/// Thread-safe session handle for use across async tasks.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    inner: Arc<Mutex<Session>>,
}

impl SessionHandle {
    pub fn new(spec_root: String, target: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Session {
                id: ulid::Ulid::new().to_string(),
                spec_root,
                target,
                started_at: Utc::now(),
                probes: Vec::new(),
                verdicts: HashMap::new(),
            })),
        }
    }

    pub fn record_probe(&self, result: ProbeResult) {
        if let Ok(mut session) = self.inner.lock() {
            session.probes.push(result);
        }
    }

    pub fn record_verdict(&self, verdict: Verdict) {
        if let Ok(mut session) = self.inner.lock() {
            session.verdicts.insert(verdict.spec_path.clone(), verdict);
        }
    }

    pub fn report(&self) -> Option<SessionReport> {
        let session = self.inner.lock().ok()?;

        let passed = session
            .verdicts
            .values()
            .filter(|v| v.outcome == VerdictOutcome::Pass)
            .count();
        let failed = session
            .verdicts
            .values()
            .filter(|v| v.outcome == VerdictOutcome::Fail)
            .count();
        let skipped = session
            .verdicts
            .values()
            .filter(|v| v.outcome == VerdictOutcome::Skip)
            .count();
        let inconclusive = session
            .verdicts
            .values()
            .filter(|v| v.outcome == VerdictOutcome::Inconclusive)
            .count();

        Some(SessionReport {
            session_id: session.id.clone(),
            spec_root: session.spec_root.clone(),
            target: session.target.clone(),
            total_probes: session.probes.len(),
            total_specs: session.verdicts.len(),
            passed,
            failed,
            skipped,
            inconclusive,
            verdicts: session.verdicts.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionReport {
    pub session_id: String,
    pub spec_root: String,
    pub target: String,
    pub total_probes: usize,
    pub total_specs: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub inconclusive: usize,
    pub verdicts: HashMap<String, Verdict>,
}

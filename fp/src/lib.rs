//! FALSE Protocol integration for Luotain.
//!
//! Maps Luotain probe results to FALSE Protocol occurrences so every probe
//! becomes a correlatable event in the AHTI knowledge graph.
//!
//! ## Type Namespace: `probe.*`
//!
//! | Type | When |
//! |------|------|
//! | `probe.http.response` | After each HTTP probe |
//! | `probe.tcp.connect` | After each TCP probe |
//! | `probe.cli.execute` | After each CLI probe |
//! | `probe.grpc.call` | After each gRPC probe |
//! | `probe.session.started` | When session begins |
//! | `probe.session.completed` | When session ends |
//! | `probe.verdict.recorded` | When agent records verdict |

use luotain_core::probe::{ProbeKind, ProbeResult};
use luotain_core::session::VerdictOutcome;
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::debug;

/// A FALSE Protocol occurrence emitted by Luotain.
#[derive(Debug, Clone, Serialize)]
pub struct Occurrence {
    /// Type string: probe.http.response, probe.cli.execute, etc.
    #[serde(rename = "type")]
    pub event_type: String,
    /// What failed (empty if success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub what_failed: Option<String>,
    /// Why it matters — spec path or context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_it_matters: Option<String>,
    /// Possible causes if something went wrong
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub possible_causes: Vec<String>,
    /// Severity: info, warning, error
    pub severity: String,
    /// Outcome: success, failure, unknown
    pub outcome: String,
    /// Probe ID for correlation
    pub probe_id: String,
    /// Timestamp
    pub timestamp: String,
}

/// Non-blocking event emitter. Sends occurrences to a channel.
/// Dropping events if the channel is full — probes must never block on emission.
#[derive(Clone)]
pub struct Emitter {
    tx: mpsc::Sender<Occurrence>,
}

impl Emitter {
    /// Create an emitter and its receiver. Buffer size controls backpressure.
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<Occurrence>) {
        let (tx, rx) = mpsc::channel(buffer);
        (Self { tx }, rx)
    }

    /// Emit a FALSE Protocol event from a probe result. Non-blocking.
    pub fn emit_probe(&self, result: &ProbeResult) {
        let event_type = match result.observation.kind {
            ProbeKind::Http => "probe.http.response",
            ProbeKind::Tcp => "probe.tcp.connect",
            ProbeKind::Cli => "probe.cli.execute",
            ProbeKind::Grpc => "probe.grpc.call",
            ProbeKind::Sql => "probe.sql.query",
        };

        let what_failed = result.observation.error.clone();
        let severity = severity_from_status(result.observation.status);
        let outcome = outcome_from_probe(result);

        let mut possible_causes = Vec::new();
        if let Some(ref err) = result.observation.error {
            if err.contains("timeout") {
                possible_causes.push("target may be overloaded or unreachable".into());
            }
            if err.contains("connection") {
                possible_causes.push("target may be down or network issue".into());
            }
        }
        if result.observation.status == Some(500) {
            possible_causes.push("internal server error — check target logs".into());
        }

        let occurrence = Occurrence {
            event_type: event_type.to_string(),
            what_failed,
            why_it_matters: result.spec_path.clone(),
            possible_causes,
            severity: severity.to_string(),
            outcome: outcome.to_string(),
            probe_id: result.id.clone(),
            timestamp: result.timestamp.to_rfc3339(),
        };

        // Non-blocking send — drop if channel full
        if self.tx.try_send(occurrence).is_err() {
            debug!("FP event dropped (channel full)");
        }
    }

    /// Emit a verdict event.
    pub fn emit_verdict(&self, spec_path: &str, outcome: &VerdictOutcome) {
        let occurrence = Occurrence {
            event_type: "probe.verdict.recorded".to_string(),
            what_failed: if matches!(outcome, VerdictOutcome::Fail) {
                Some(format!("spec {} failed", spec_path))
            } else {
                None
            },
            why_it_matters: Some(spec_path.to_string()),
            possible_causes: vec![],
            severity: match outcome {
                VerdictOutcome::Pass => "info".to_string(),
                VerdictOutcome::Fail => "error".to_string(),
                VerdictOutcome::Skip | VerdictOutcome::Inconclusive => "warning".to_string(),
            },
            outcome: outcome_from_verdict(outcome).to_string(),
            probe_id: String::new(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if self.tx.try_send(occurrence).is_err() {
            debug!("FP verdict event dropped (channel full)");
        }
    }
}

/// Map HTTP status to FALSE Protocol severity level.
pub fn severity_from_status(status: Option<u16>) -> &'static str {
    match status {
        Some(s) if s < 400 => "info",
        Some(s) if s < 500 => "warning",
        Some(_) => "error",
        None => "error",
    }
}

/// Map probe result to FALSE Protocol outcome.
pub fn outcome_from_probe(result: &ProbeResult) -> &'static str {
    if result.observation.error.is_some() {
        "failure"
    } else {
        "success"
    }
}

/// Map verdict to FALSE Protocol outcome.
pub fn outcome_from_verdict(outcome: &VerdictOutcome) -> &'static str {
    match outcome {
        VerdictOutcome::Pass => "success",
        VerdictOutcome::Fail => "failure",
        VerdictOutcome::Skip => "unknown",
        VerdictOutcome::Inconclusive => "unknown",
    }
}

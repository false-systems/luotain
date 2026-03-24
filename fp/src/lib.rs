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
//! | `probe.session.started` | When session begins |
//! | `probe.session.completed` | When session ends |
//! | `probe.verdict.recorded` | When agent records verdict |

use luotain_core::probe::ProbeResult;
use luotain_core::session::VerdictOutcome;

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

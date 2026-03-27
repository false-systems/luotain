//! Result types for spec evaluation.
//!
//! A `SpecResult` is written after evaluating a single spec. It contains
//! per-feature verdicts so failures are traceable to individual behaviors.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Result of evaluating a single spec against a live system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecResult {
    /// Relative spec path (e.g. "auth/login.md")
    pub spec: String,
    /// When the evaluation completed
    pub timestamp: DateTime<Utc>,
    /// Overall verdict: pass, fail, skip, inconclusive
    pub verdict: String,
    /// Per-feature breakdown
    pub features: Vec<FeatureResult>,
    /// Probe IDs used during evaluation
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<String>,
    /// Test mode: "standard" or "adversarial"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Result for a single feature/behavior within a spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureResult {
    /// What was being tested (from the spec bullet point)
    pub description: String,
    /// pass, fail, skip
    pub verdict: String,
    /// Explanation when verdict is not pass
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl SpecResult {
    /// Compute overall verdict from features: fail if any fail, pass if all pass.
    pub fn verdict_from_features(features: &[FeatureResult]) -> &'static str {
        if features.iter().any(|f| f.verdict == "fail") {
            "fail"
        } else if features.iter().all(|f| f.verdict == "pass") {
            "pass"
        } else if features.iter().all(|f| f.verdict == "skip") {
            "skip"
        } else {
            "inconclusive"
        }
    }
}

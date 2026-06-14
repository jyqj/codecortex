//! BuildExplain — build-side explainability envelope, the counterpart of
//! [`crate::graph_explain::GraphExplain`] (which covers graph *read* tools).
//!
//! The build pipeline already surfaces a few structured signals on
//! [`crate::IndexReport`]-equivalent reports — dirty-propagation status and
//! per-phase timing — but the *decisions* the postprocess/analysis passes make
//! (which signature gate ran vs skipped and why, when community detection or
//! co-change analysis degraded) were only ever emitted to `tracing`, invisible
//! to the agent or caller asking "why did synthesis skip this build?" or "why
//! are all symbols in community 0?". This module is the additive envelope that
//! collects those decisions as they happen and surfaces them alongside the
//! existing report fields.
//!
//! All fields skip serialization when empty, so a build with nothing to report
//! serializes the envelope as `{}`; callers attach it only when
//! [`BuildExplain::is_empty`] is false (see [`BuildExplainCollector::finish_non_empty`]).
//!
//! Scope is deliberately narrow: it carries gate decisions and degrade notes.
//! It does NOT duplicate `dirty_propagation` or `phase_timing` (already on the
//! report), and it does not carry per-pass synthesis delta counts (those stay in
//! `tracing` until a future round widens the envelope).

use serde::{Deserialize, Serialize};

fn is_empty_vec(value: &[GateDecisionRecord]) -> bool {
    value.is_empty()
}

fn is_empty_str_vec(value: &[String]) -> bool {
    value.is_empty()
}

/// One signature-gate decision recorded during the postprocess/analysis COMPUTE
/// stage (`run: bool` + the stable reason token). Mirrors the `GateDecision`
/// the gates already compute, but serializable and collectable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateDecisionRecord {
    /// Pass / gate identifier (e.g. `"synthesis_round"`, `"community"`,
    /// `"git_cochange"`, `"infra"`).
    pub pass: String,
    /// Whether the pass ran this build.
    pub run: bool,
    /// Stable reason token (e.g. `"signature unchanged"`, `"full rebuild"`).
    pub reason: String,
}

/// Build-side explainability envelope. Additive only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildExplain {
    /// Signature-gate decisions for the postprocess/analysis passes, in the
    /// order evaluated (synthesis_round, community, git_cochange, infra).
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub gate_decisions: Vec<GateDecisionRecord>,
    /// Degrade notes — stable tokens for passes that produced a degraded
    /// result rather than skipping (e.g. `"community_edge_cap_exceeded"`,
    /// `"cochange_unavailable"`).
    #[serde(default, skip_serializing_if = "is_empty_str_vec")]
    pub degraded: Vec<String>,
}

impl BuildExplain {
    /// True when there is nothing worth reporting.
    pub fn is_empty(&self) -> bool {
        self.gate_decisions.is_empty() && self.degraded.is_empty()
    }
}

/// Incremental builder for [`BuildExplain`], threaded `&mut` through the
/// postprocess/analysis COMPUTE stages so each decision/degrade is recorded
/// where it happens.
#[derive(Debug, Default)]
pub struct BuildExplainCollector {
    explain: BuildExplain,
}

impl BuildExplainCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a signature-gate decision (alongside the existing `tracing`
    /// emission; this does not replace it).
    pub fn record_gate(&mut self, pass: &str, run: bool, reason: &str) {
        self.explain.gate_decisions.push(GateDecisionRecord {
            pass: pass.to_string(),
            run,
            reason: reason.to_string(),
        });
    }

    /// Record a degrade token — a pass that produced a degraded result rather
    /// than skipping (community edge-cap, co-change unavailable, ...).
    pub fn record_degraded(&mut self, token: &str) {
        self.explain.degraded.push(token.to_string());
    }

    pub fn is_empty(&self) -> bool {
        self.explain.is_empty()
    }

    pub fn finish(self) -> BuildExplain {
        self.explain
    }

    /// Finish, returning `None` when there is nothing to report so callers can
    /// skip attaching an empty envelope.
    pub fn finish_non_empty(self) -> Option<BuildExplain> {
        if self.explain.is_empty() {
            None
        } else {
            Some(self.explain)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_envelope_serializes_to_empty_object() {
        let explain = BuildExplain::default();
        assert!(explain.is_empty());
        assert_eq!(
            serde_json::to_value(&explain).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn finish_non_empty_skips_empty_collector() {
        assert!(BuildExplainCollector::new().finish_non_empty().is_none());

        let mut collector = BuildExplainCollector::new();
        collector.record_gate("community", false, "signature unchanged");
        let explain = collector.finish_non_empty().expect("non-empty");
        assert_eq!(explain.gate_decisions.len(), 1);
        assert_eq!(explain.gate_decisions[0].pass, "community");
        assert!(!explain.gate_decisions[0].run);
    }

    #[test]
    fn gate_decisions_and_degraded_roundtrip() {
        let mut collector = BuildExplainCollector::new();
        collector.record_gate("synthesis_round", true, "signature changed");
        collector.record_gate("git_cochange", false, "cache key unchanged");
        collector.record_degraded("community_edge_cap_exceeded");
        let explain = collector.finish();

        let value = serde_json::to_value(&explain).unwrap();
        assert_eq!(value["gate_decisions"][0]["run"], true);
        assert_eq!(value["gate_decisions"][1]["reason"], "cache key unchanged");
        assert_eq!(
            value["degraded"],
            serde_json::json!(["community_edge_cap_exceeded"])
        );

        let roundtrip: BuildExplain = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, explain);
    }
}

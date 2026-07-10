//! GraphExplain — unified explainability envelope for graph read tools.
//!
//! Every graph tool (impact / trace_path / graph_query / search graph
//! enrichment) used to invent its own truncation metadata, and DB read
//! failures were silently swallowed into empty results. This module provides
//! one shared, serialization-friendly envelope ([`GraphExplain`]) plus an
//! incremental collector ([`GraphExplainCollector`]) so read paths can record
//! degradation (truncation, swallowed read errors, synthetic-edge usage) as
//! it happens and surface it additively in tool responses.
//!
//! All fields skip serialization when empty/zero/false, so an envelope with
//! nothing to report serializes as `{}` — callers should attach it only when
//! [`GraphExplain::is_empty`] is false (see `finish_non_empty`). The one
//! exception is `declared_edge_kinds` (a tool's static graph-subset contract
//! from `graph_catalog::tool_graph_subsets`): it rides along whenever the
//! envelope is attached but never makes the envelope "worth attaching" on its
//! own — except for tools without a dynamic collector, which attach a
//! [`GraphExplain::declared_only`] envelope additively.

use std::collections::BTreeMap;

use crate::graph_catalog::EdgeKindSet;
use serde::{Deserialize, Serialize};

/// Maximum number of entries kept in [`GraphExplain::read_errors`]; further
/// errors only bump `read_errors_dropped` so a failing query in a loop cannot
/// bloat the response.
pub const MAX_READ_ERRORS: usize = 8;

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Unified explainability envelope shared by all graph read tools.
///
/// Additive only: tools attach this alongside their existing fields (it may
/// duplicate legacy `truncated`/`truncated_reason` fields for compatibility).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphExplain {
    /// Edge kinds this tool surface is DECLARED to consult, as catalog kind
    /// names (e.g. "CALLS") from `graph_catalog::tool_graph_subsets`. Static
    /// contract metadata — `edge_kinds_used` below keeps recording what was
    /// actually traversed (dispatch tokens, e.g. "call"/"http_bridge"). Does
    /// NOT count toward [`GraphExplain::is_empty`]: the declaration alone is
    /// not "something to report", so clean runs keep omitting the envelope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_edge_kinds: Vec<String>,
    /// Edge kinds actually traversed/projected (e.g. "call", "http_bridge"),
    /// deduplicated in first-seen order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_kinds_used: Vec<String>,
    /// Edges that were synthesized (e.g. HTTP/async bridges) rather than
    /// parsed from source.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub synthetic_edge_count: usize,
    /// Edges carrying runtime evidence (observed at runtime).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub runtime_evidence_edge_count: usize,
    /// True when the result was clipped by any budget or degraded by a read
    /// error mid-walk.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    /// Stable token for the FIRST cause that clipped the result:
    /// "output_budget" | "default_limit" | "max_depth" | "max_paths" |
    /// "max_expansions" | "max_nodes" | "max_per_layer" | "result_limit" |
    /// "bridge_cap" | "db_error:<op>" | ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_reason: Option<String>,
    /// Capped `"<op>: <error>"` messages for DB reads that failed but were
    /// degraded to a partial/empty result instead of failing the tool call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_errors: Vec<String>,
    /// Number of read errors beyond [`MAX_READ_ERRORS`] (count only).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub read_errors_dropped: usize,
    /// Categorized synthesis notes — stable `"category: count"` entries for
    /// synthesized-edge sources that skipped or degraded (e.g. HTTP bridge
    /// edges dropped to `no_caller_uid` / `no_normalized_path` /
    /// `no_route_handler`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synthesis_notes: Vec<String>,
}

impl GraphExplain {
    /// Envelope carrying only a tool's declared edge-kind contract. Used by
    /// tool surfaces without a dynamic collector (relations/cycles/
    /// type_hierarchy) to surface their graph subset additively.
    pub fn declared_only(set: EdgeKindSet) -> Self {
        debug_assert!(
            set.unknown_kinds().is_empty(),
            "declared edge kinds missing from catalog: {:?}",
            set.unknown_kinds()
        );
        Self {
            declared_edge_kinds: set.iter().map(str::to_string).collect(),
            ..Self::default()
        }
    }

    /// True when there is nothing worth reporting beyond the static
    /// declaration (`declared_edge_kinds` is contract metadata, not a
    /// degradation signal, so it is deliberately ignored here).
    pub fn is_empty(&self) -> bool {
        self.edge_kinds_used.is_empty()
            && self.synthetic_edge_count == 0
            && self.runtime_evidence_edge_count == 0
            && !self.truncated
            && self.truncated_reason.is_none()
            && self.read_errors.is_empty()
            && self.read_errors_dropped == 0
            && self.synthesis_notes.is_empty()
    }
}

/// Incremental builder for [`GraphExplain`], passed `&mut` through graph read
/// paths so each degraded-but-usable fallback is recorded where it happens.
#[derive(Debug, Default)]
pub struct GraphExplainCollector {
    explain: GraphExplain,
    synthesis_counts: BTreeMap<String, usize>,
}

impl GraphExplainCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a DB read failure that was degraded to a partial/empty result.
    /// Entries beyond [`MAX_READ_ERRORS`] only bump `read_errors_dropped`.
    /// Also emits a `tracing::warn!` so the failure is visible even when the
    /// envelope itself is not surfaced by the caller.
    pub fn record_read_error(&mut self, op: &str, err: &dyn std::fmt::Display) {
        tracing::warn!(op, error = %err, "graph read degraded to partial result");
        if self.explain.read_errors.len() < MAX_READ_ERRORS {
            self.explain.read_errors.push(format!("{op}: {err}"));
        } else {
            self.explain.read_errors_dropped += 1;
        }
    }

    /// Record the tool's declared edge-kind subset (contract metadata from
    /// `graph_catalog::tool_graph_subsets`). Replaces any previous
    /// declaration; does not affect [`GraphExplain::is_empty`] or the dynamic
    /// `edge_kinds_used` counters.
    pub fn declare_edge_kinds(&mut self, set: EdgeKindSet) {
        debug_assert!(
            set.unknown_kinds().is_empty(),
            "declared edge kinds missing from catalog: {:?}",
            set.unknown_kinds()
        );
        self.explain.declared_edge_kinds = set.iter().map(str::to_string).collect();
    }

    /// Note an edge kind actually traversed/projected (deduplicated,
    /// first-seen order).
    pub fn note_edge_kind(&mut self, kind: &str) {
        if !self
            .explain
            .edge_kinds_used
            .iter()
            .any(|existing| existing == kind)
        {
            self.explain.edge_kinds_used.push(kind.to_string());
        }
    }

    pub fn add_synthetic_edges(&mut self, count: usize) {
        self.explain.synthetic_edge_count += count;
    }

    pub fn add_runtime_evidence_edges(&mut self, count: usize) {
        self.explain.runtime_evidence_edge_count += count;
    }

    /// Record a synthesis-side note: `count` edges in `category` were skipped
    /// or degraded (e.g. an HTTP call that synthesized no bridge edge). The
    /// first report per category wins (idempotent across the per-node calls of
    /// one walk); the counts surface as `"category: count"` in
    /// [`GraphExplain::synthesis_notes`].
    pub fn note_synthesis(&mut self, category: &str, count: usize) {
        self.synthesis_counts
            .entry(category.to_string())
            .or_insert(count);
    }

    /// Mark the result truncated. The first recorded reason wins (it is the
    /// primary cause); later calls still keep `truncated` set.
    pub fn mark_truncated(&mut self, reason: &str) {
        self.explain.truncated = true;
        if self.explain.truncated_reason.is_none() {
            self.explain.truncated_reason = Some(reason.to_string());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.explain.is_empty() && self.synthesis_counts.is_empty()
    }

    fn into_explain(mut self) -> GraphExplain {
        self.explain.synthesis_notes = self
            .synthesis_counts
            .into_iter()
            .map(|(category, count)| format!("{category}: {count}"))
            .collect();
        self.explain
    }

    pub fn finish(self) -> GraphExplain {
        self.into_explain()
    }

    /// Finish, returning `None` when there is nothing to report so callers
    /// can skip attaching an empty envelope.
    pub fn finish_non_empty(self) -> Option<GraphExplain> {
        let explain = self.into_explain();
        if explain.is_empty() {
            None
        } else {
            Some(explain)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_envelope_serializes_to_empty_object() {
        let explain = GraphExplain::default();
        assert!(explain.is_empty());
        assert_eq!(
            serde_json::to_value(&explain).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn finish_non_empty_skips_empty_collector() {
        assert!(GraphExplainCollector::new().finish_non_empty().is_none());

        let mut collector = GraphExplainCollector::new();
        collector.mark_truncated("max_depth");
        let explain = collector.finish_non_empty().expect("non-empty envelope");
        assert!(explain.truncated);
        assert_eq!(explain.truncated_reason.as_deref(), Some("max_depth"));
    }

    #[test]
    fn read_errors_capped_with_dropped_count() {
        let mut collector = GraphExplainCollector::new();
        for idx in 0..(MAX_READ_ERRORS + 3) {
            collector.record_read_error("reverse_callers", &format!("boom {idx}"));
        }
        let explain = collector.finish();
        assert_eq!(explain.read_errors.len(), MAX_READ_ERRORS);
        assert_eq!(explain.read_errors_dropped, 3);
        assert_eq!(explain.read_errors[0], "reverse_callers: boom 0");
    }

    #[test]
    fn first_truncated_reason_wins() {
        let mut collector = GraphExplainCollector::new();
        collector.mark_truncated("max_paths");
        collector.mark_truncated("max_depth");
        let explain = collector.finish();
        assert!(explain.truncated);
        assert_eq!(explain.truncated_reason.as_deref(), Some("max_paths"));
    }

    #[test]
    fn declared_edge_kinds_do_not_make_envelope_non_empty() {
        use crate::graph_catalog::tool_graph_subsets;

        // Declaration alone: still "nothing to report" (clean runs keep
        // omitting the envelope), but the field serializes when present.
        let mut collector = GraphExplainCollector::new();
        collector.declare_edge_kinds(tool_graph_subsets::IMPACT);
        assert!(collector.is_empty());
        assert!(collector.finish_non_empty().is_none());

        // Combined with a dynamic signal, the declaration rides along.
        let mut collector = GraphExplainCollector::new();
        collector.declare_edge_kinds(tool_graph_subsets::IMPACT);
        collector.note_edge_kind("call");
        let explain = collector.finish_non_empty().expect("non-empty");
        assert_eq!(
            explain.declared_edge_kinds,
            tool_graph_subsets::IMPACT.kinds().to_vec()
        );
        assert_eq!(explain.edge_kinds_used, vec!["call"]);

        let value = serde_json::to_value(&explain).unwrap();
        assert_eq!(
            value["declared_edge_kinds"],
            serde_json::json!(tool_graph_subsets::IMPACT.kinds())
        );
        let roundtrip: GraphExplain = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, explain);
    }

    #[test]
    fn declared_only_carries_just_the_contract() {
        use crate::graph_catalog::tool_graph_subsets;

        let explain = GraphExplain::declared_only(tool_graph_subsets::CYCLES);
        assert!(explain.is_empty(), "contract metadata is not a report");
        assert_eq!(
            explain.declared_edge_kinds,
            tool_graph_subsets::CYCLES.kinds().to_vec()
        );
        let value = serde_json::to_value(&explain).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "declared_edge_kinds": ["IMPORTS", "CALLS"] })
        );
    }

    #[test]
    fn edge_kinds_deduplicated_in_first_seen_order() {
        let mut collector = GraphExplainCollector::new();
        collector.note_edge_kind("call");
        collector.note_edge_kind("http_bridge");
        collector.note_edge_kind("call");
        let explain = collector.finish();
        assert_eq!(explain.edge_kinds_used, vec!["call", "http_bridge"]);
    }

    #[test]
    fn non_empty_fields_serialize_and_roundtrip() {
        let mut collector = GraphExplainCollector::new();
        collector.note_edge_kind("call");
        collector.add_synthetic_edges(2);
        collector.add_runtime_evidence_edges(1);
        collector.mark_truncated("max_nodes");
        collector.record_read_error("suggested_test_files", &"disk error");
        let explain = collector.finish();

        let value = serde_json::to_value(&explain).unwrap();
        assert_eq!(value["edge_kinds_used"], serde_json::json!(["call"]));
        assert_eq!(value["synthetic_edge_count"], 2);
        assert_eq!(value["runtime_evidence_edge_count"], 1);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["truncated_reason"], "max_nodes");
        // Zero counters stay omitted.
        assert!(value.get("read_errors_dropped").is_none());

        let roundtrip: GraphExplain = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, explain);
    }

    #[test]
    fn synthesis_notes_first_count_wins_and_finishes_as_strings() {
        let mut collector = GraphExplainCollector::new();
        collector.note_synthesis("no_route_handler", 5);
        // Idempotent across per-node calls in one walk: no overwrite, no accumulate.
        collector.note_synthesis("no_route_handler", 5);
        collector.note_synthesis("no_caller_uid", 2);
        assert!(!collector.is_empty());
        let explain = collector.finish();
        // BTreeMap ordering: alphabetical by category.
        assert_eq!(
            explain.synthesis_notes,
            vec![
                "no_caller_uid: 2".to_string(),
                "no_route_handler: 5".to_string()
            ]
        );
        // An empty collector still finishes to None.
        assert!(GraphExplainCollector::new().finish_non_empty().is_none());
    }
}

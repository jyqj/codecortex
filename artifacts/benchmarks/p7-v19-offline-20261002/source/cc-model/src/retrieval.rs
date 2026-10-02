//! Retrieval boundaries. Caller constraints and ranking hints have different semantics.
use crate::{
    identity::DocumentRef, search::SearchRequest, source::ByteSpan, CcError, CcResult, Language,
};
use serde::{Deserialize, Serialize};

/// Explicit model input projection. It is not source evidence, a vector, or a
/// durable document identity. `source_range` locates untouched original bytes
/// inside `text`; `input_hash` hashes exactly the text sent to a future provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingInput {
    pub format_version: u32,
    pub text: String,
    pub input_hash: String,
    pub render_key: String,
    pub source_range: crate::source::ByteSpan,
    pub source_snapshot_id: String,
    pub source_slice_digest: String,
    pub metadata_truncated: bool,
    pub token_estimate: u32,
    pub token_estimator: String,
}

/// Conjunctive scope within the owning project's index. `None` adds no restriction;
/// `Some([])` is an explicitly empty set and can never become unrestricted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HardScope {
    pub path_prefix: Option<String>,
    pub languages: Option<Vec<Language>>,
    pub file_paths: Option<Vec<String>>,
}
impl From<&SearchRequest> for HardScope {
    fn from(request: &SearchRequest) -> Self {
        Self {
            path_prefix: request.path_prefix.clone(),
            languages: request.languages.clone(),
            file_paths: request.file_paths.clone(),
        }
    }
}
impl HardScope {
    pub fn is_empty(&self) -> bool {
        self.languages.as_ref().is_some_and(Vec::is_empty)
            || self.file_paths.as_ref().is_some_and(Vec::is_empty)
    }
    pub fn passes(&self, file_path: &str, language: Language) -> bool {
        crate::repo_path::is_canonical_file(file_path)
            && self
                .path_prefix
                .as_ref()
                .is_none_or(|p| crate::repo_path::is_within(file_path, p))
            && self
                .languages
                .as_ref()
                .is_none_or(|ls| ls.contains(&language))
            && self
                .file_paths
                .as_ref()
                .is_none_or(|fs| fs.iter().any(|p| p == file_path))
    }
    /// Component-aware prefix intersection; disjoint scopes deny all results.
    /// Filesystem canonicalization is handled separately at the source-read boundary.
    pub fn intersect(&self, other: &Self) -> Self {
        let (path_prefix, disjoint) = match (&self.path_prefix, &other.path_prefix) {
            (Some(a), Some(b)) if crate::repo_path::is_within(a.trim_end_matches('/'), b) => {
                (Some(a.clone()), false)
            }
            (Some(a), Some(b)) if crate::repo_path::is_within(b.trim_end_matches('/'), a) => {
                (Some(b.clone()), false)
            }
            (Some(a), Some(_)) => (Some(a.clone()), true),
            (Some(a), None) => (Some(a.clone()), false),
            (None, b) => (b.clone(), false),
        };
        let languages = intersect_sets(&self.languages, &other.languages);
        let mut file_paths = intersect_sets(&self.file_paths, &other.file_paths);
        if disjoint {
            file_paths = Some(Vec::new());
        }
        if let (Some(files), Some(prefix)) = (&mut file_paths, &path_prefix) {
            files.retain(|p| crate::repo_path::is_within(p, prefix));
        }
        Self {
            path_prefix,
            languages,
            file_paths,
        }
    }
}
fn intersect_sets<T: Clone + PartialEq>(a: &Option<Vec<T>>, b: &Option<Vec<T>>) -> Option<Vec<T>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.iter().filter(|v| b.contains(v)).cloned().collect()),
        (Some(a), None) => Some(a.clone()),
        (None, b) => b.clone(),
    }
}
/// Ordered hints bias scoring/scan priority but never define search permission.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SoftHints {
    pub working_files: Vec<String>,
    pub recent_files: Vec<String>,
    pub pinned_files: Vec<String>,
    pub overlay_files: Vec<String>,
    pub preselected_files: Vec<String>,
}
impl From<&SearchRequest> for SoftHints {
    fn from(request: &SearchRequest) -> Self {
        Self {
            working_files: request.boost_file_paths.clone().unwrap_or_default(),
            recent_files: request.recent_file_paths.clone().unwrap_or_default(),
            pinned_files: request.pinned_file_paths.clone().unwrap_or_default(),
            overlay_files: request.overlay_file_paths.clone().unwrap_or_default(),
            preselected_files: Vec::new(),
        }
    }
}
/// Per-query grep coverage, retained even when the hit list is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrepDiagnostics {
    pub status: String,
    pub scan_cap: usize,
    pub scanned: usize,
    pub soft_scanned: usize,
    pub prefilter_scanned: usize,
    pub fallback_scanned: usize,
    pub skipped_duplicates: usize,
    pub reason: Option<String>,
    pub prefilter_failed: bool,
    /// Successful stage receipts; a failed prefilter makes these incomplete.
    #[serde(default)]
    pub stages: Vec<crate::retrieval_cost::GrepStageWork>,
}
impl GrepDiagnostics {
    pub fn is_partial(&self) -> bool {
        self.status == "partial"
    }
}

/// Versioned retrieval candidate identity. `legacy_chunk_id` remains a wire/
/// hydration locator; fusion identity is `(doc_key, doc_version)`.
pub const CANDIDATE_REF_SCHEMA_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRef {
    pub schema_version: u32,
    pub document: DocumentRef,
    pub source_span: ByteSpan,
    pub legacy_chunk_id: String,
    pub lane_id: String,
    /// One-based rank inside the producing lane.
    pub lane_rank: usize,
    /// Diagnostic score in the lane's native scale. Fusion never mixes it.
    pub raw_score: f64,
    pub scoring_spec: String,
    /// Exact symbol/path identity tier. Ambiguous exact matches may all carry
    /// this flag; it is not a uniqueness or semantic-resolution claim.
    pub exact_identity: bool,
}
impl CandidateRef {
    pub fn validate(&self) -> CcResult<()> {
        let hash =
            |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
        if self.schema_version != CANDIDATE_REF_SCHEMA_VERSION
            || !hash(&self.document.doc_key)
            || !hash(&self.document.doc_version)
            || self.source_span.start >= self.source_span.end
            || self
                .document
                .entity_key
                .as_ref()
                .is_some_and(|key| !hash(key))
            || self
                .document
                .encoding_key
                .as_ref()
                .is_some_and(|key| !hash(key))
            || self.legacy_chunk_id.is_empty()
            || self.legacy_chunk_id.len() > 8192
            || self.lane_id.is_empty()
            || self.lane_id.len() > 120
            || self.lane_rank == 0
            || !self.raw_score.is_finite()
            || self.scoring_spec.is_empty()
            || self.scoring_spec.len() > 512
        {
            return Err(CcError::InvalidParams(
                "invalid retrieval candidate identity/rank/score".into(),
            ));
        }
        Ok(())
    }
    pub fn fusion_identity(&self) -> (&str, &str) {
        (&self.document.doc_key, &self.document.doc_version)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneStatus {
    Disabled,
    NotConfigured,
    Complete,
    Partial,
    Timeout,
    Unavailable,
    Error,
    Cancelled,
}
impl LaneStatus {
    pub fn is_fusable(self) -> bool {
        matches!(self, Self::Complete | Self::Partial)
    }
}

/// Coverage is scoped to the lane's declared hard-scope query, not the whole
/// repository. `total_lower_bound` is safe when a candidate cap truncates work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneCoverage {
    pub scope: String,
    pub complete: bool,
    pub examined: Option<usize>,
    pub total_lower_bound: usize,
}
impl LaneCoverage {
    pub fn not_run() -> Self {
        Self {
            scope: "hard_scope".into(),
            complete: false,
            examined: None,
            total_lower_bound: 0,
        }
    }
    pub fn complete(examined: Option<usize>, candidates: usize) -> Self {
        Self {
            scope: "hard_scope".into(),
            complete: true,
            examined,
            total_lower_bound: candidates,
        }
    }
    pub fn partial(examined: Option<usize>, lower_bound: usize) -> Self {
        Self {
            scope: "hard_scope".into(),
            complete: false,
            examined,
            total_lower_bound: lower_bound,
        }
    }
}

pub const LANE_OUTCOME_SCHEMA_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneOutcome {
    pub schema_version: u32,
    pub lane_id: String,
    /// RRF weight. Native scores stay diagnostic and are never added directly.
    pub weight: f64,
    pub status: LaneStatus,
    pub elapsed_us: u64,
    pub candidate_count: usize,
    pub coverage: LaneCoverage,
    pub truncation_reason: Option<String>,
    pub candidates: Vec<CandidateRef>,
}
impl LaneOutcome {
    /// Deterministic bounded retrieval can be cached, but transient failures
    /// must be retried even if no index epoch changed in the meantime.
    pub fn is_cacheable(&self) -> bool {
        matches!(
            self.status,
            LaneStatus::Disabled | LaneStatus::NotConfigured | LaneStatus::Complete
        ) || (self.status == LaneStatus::Partial
            && matches!(
                self.truncation_reason.as_deref(),
                Some(
                    "candidate_limit"
                        | "scan_cap"
                        | "path_token_limit"
                        | "graph_expansion_limit"
                        | "graph_source_unmapped"
                )
            ))
    }

    pub fn disabled(lane_id: impl Into<String>, weight: f64) -> Self {
        Self {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: lane_id.into(),
            weight,
            status: LaneStatus::Disabled,
            elapsed_us: 0,
            candidate_count: 0,
            coverage: LaneCoverage::not_run(),
            truncation_reason: None,
            candidates: Vec::new(),
        }
    }
    pub fn validate(&self) -> CcResult<()> {
        use std::collections::HashSet;
        if self.schema_version != LANE_OUTCOME_SCHEMA_VERSION
            || self.lane_id.is_empty()
            || self.lane_id.len() > 120
            || !self.weight.is_finite()
            || self.weight < 0.0
            || self.candidate_count != self.candidates.len()
            || self.coverage.total_lower_bound < self.candidate_count
            || self.coverage.scope != "hard_scope"
            || self
                .truncation_reason
                .as_ref()
                .is_some_and(|reason| reason.is_empty() || reason.len() > 1024)
            || (matches!(
                self.status,
                LaneStatus::Partial
                    | LaneStatus::Timeout
                    | LaneStatus::Unavailable
                    | LaneStatus::Error
                    | LaneStatus::Cancelled
            ) && self.truncation_reason.is_none())
            || (self.status == LaneStatus::Complete && self.truncation_reason.is_some())
            || (self.status == LaneStatus::Complete && !self.coverage.complete)
            || (self.status != LaneStatus::Complete && self.coverage.complete)
            || (matches!(
                self.status,
                LaneStatus::Disabled
                    | LaneStatus::NotConfigured
                    | LaneStatus::Timeout
                    | LaneStatus::Unavailable
                    | LaneStatus::Error
                    | LaneStatus::Cancelled
            ) && !self.candidates.is_empty())
        {
            return Err(CcError::InvalidParams("invalid lane outcome".into()));
        }
        let mut identities = HashSet::with_capacity(self.candidates.len());
        for (index, candidate) in self.candidates.iter().enumerate() {
            candidate.validate()?;
            if candidate.lane_id != self.lane_id
                || candidate.lane_rank != index + 1
                || !identities.insert((
                    candidate.document.doc_key.as_str(),
                    candidate.document.doc_version.as_str(),
                ))
            {
                return Err(CcError::InvalidParams(
                    "lane candidate rank/identity is invalid or duplicated".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_and_explicit_empty_roundtrip_differ() {
        let absent = HardScope::default();
        let empty = HardScope {
            file_paths: Some(vec![]),
            ..Default::default()
        };
        assert_ne!(
            serde_json::to_value(&absent).unwrap(),
            serde_json::to_value(&empty).unwrap()
        );
        let recovered: HardScope =
            serde_json::from_value(serde_json::to_value(&empty).unwrap()).unwrap();
        assert!(recovered.is_empty());
        assert!(!recovered.passes("a.rs", Language::Rust));
        assert!(absent.passes("a.rs", Language::Rust));
    }
    #[test]
    fn intersections_never_admit_a_file_rejected_by_either_side() {
        let scopes = [
            HardScope::default(),
            HardScope {
                path_prefix: Some("src/".into()),
                ..Default::default()
            },
            HardScope {
                path_prefix: Some("src/api/".into()),
                ..Default::default()
            },
            HardScope {
                path_prefix: Some("tests/".into()),
                ..Default::default()
            },
            HardScope {
                file_paths: Some(vec![]),
                ..Default::default()
            },
            HardScope {
                languages: Some(vec![Language::Rust]),
                ..Default::default()
            },
            HardScope {
                file_paths: Some(vec!["src/a.rs".into(), "tests/b.py".into()]),
                ..Default::default()
            },
        ];
        for a in &scopes {
            for b in &scopes {
                for (p, l) in [
                    ("src/a.rs", Language::Rust),
                    ("src/api/a.py", Language::Python),
                    ("tests/b.py", Language::Python),
                ] {
                    assert_eq!(
                        a.intersect(b).passes(p, l),
                        a.passes(p, l) && b.passes(p, l)
                    );
                    assert_eq!(a.intersect(b).passes(p, l), b.intersect(a).passes(p, l));
                }
            }
        }
    }
    #[test]
    fn soft_hints_never_create_a_hard_scope() {
        let req = SearchRequest {
            boost_file_paths: Some(vec!["b.rs".into(), "a.rs".into()]),
            ..Default::default()
        };
        assert_eq!(HardScope::from(&req), HardScope::default());
        assert_eq!(SoftHints::from(&req).working_files, vec!["b.rs", "a.rs"]);
    }

    fn candidate(key: char, rank: usize, raw_score: f64) -> CandidateRef {
        CandidateRef {
            schema_version: CANDIDATE_REF_SCHEMA_VERSION,
            document: DocumentRef {
                doc_key: key.to_string().repeat(64),
                doc_version: ((key as u8 + 1) as char).to_string().repeat(64),
                entity_key: None,
                encoding_key: None,
            },
            source_span: ByteSpan { start: 0, end: 1 },
            legacy_chunk_id: format!("chunk:{key}"),
            lane_id: "exact_symbol".into(),
            lane_rank: rank,
            raw_score,
            scoring_spec: "exact-symbol-v1".into(),
            exact_identity: true,
        }
    }

    #[test]
    fn lane_outcome_distinguishes_disabled_from_complete_empty() {
        let disabled = LaneOutcome::disabled("path", 1.0);
        disabled.validate().unwrap();
        let complete = LaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: "path".into(),
            weight: 1.0,
            status: LaneStatus::Complete,
            elapsed_us: 3,
            candidate_count: 0,
            coverage: LaneCoverage::complete(Some(0), 0),
            truncation_reason: None,
            candidates: Vec::new(),
        };
        complete.validate().unwrap();
        assert_ne!(disabled.status, complete.status);
        assert!(!disabled.coverage.complete);
        assert!(complete.coverage.complete);
    }

    #[test]
    fn lane_status_requires_consistent_reason_and_coverage() {
        for status in [
            LaneStatus::Partial,
            LaneStatus::Timeout,
            LaneStatus::Unavailable,
            LaneStatus::Error,
            LaneStatus::Cancelled,
        ] {
            let mut lane = LaneOutcome::disabled("path", 1.0);
            lane.status = status;
            assert!(lane.validate().is_err(), "missing reason for {status:?}");
            lane.truncation_reason = Some("authored_failure_or_limit".into());
            lane.validate().unwrap();
            let roundtrip: LaneOutcome =
                serde_json::from_str(&serde_json::to_string(&lane).unwrap()).unwrap();
            assert_eq!(lane, roundtrip);
        }
        let mut complete = LaneOutcome::disabled("path", 1.0);
        complete.status = LaneStatus::Complete;
        complete.coverage = LaneCoverage::complete(Some(0), 0);
        complete.truncation_reason = Some("candidate_limit".into());
        assert!(
            complete.validate().is_err(),
            "complete must not claim truncated work"
        );
    }

    #[test]
    fn malformed_span_optional_identity_and_coverage_fail_closed() {
        let mut c = candidate('a', 1, 0.0);
        c.source_span = ByteSpan { start: 2, end: 1 };
        assert!(c.validate().is_err());
        c.source_span = ByteSpan { start: 0, end: 1 };
        c.document.encoding_key = Some("malformed".into());
        assert!(c.validate().is_err());
        let mut lane = LaneOutcome::disabled("path", 1.0);
        lane.status = LaneStatus::Error;
        assert!(!lane.is_cacheable());
        lane.status = LaneStatus::Partial;
        lane.truncation_reason = Some("prefilter_error".into());
        assert!(!lane.is_cacheable());
        lane.truncation_reason = Some("candidate_limit".into());
        assert!(lane.is_cacheable());
        lane.candidates = vec![candidate('a', 1, 0.0)];
        lane.candidates[0].lane_id = "path".into();
        lane.candidate_count = 1;
        assert!(
            lane.validate().is_err(),
            "coverage cannot undercount returned candidates"
        );
    }

    #[test]
    fn lane_outcome_rejects_non_finite_scores_and_duplicate_versions() {
        let bad_score = LaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: "exact_symbol".into(),
            weight: 1.0,
            status: LaneStatus::Complete,
            elapsed_us: 1,
            candidate_count: 1,
            coverage: LaneCoverage::complete(None, 1),
            truncation_reason: None,
            candidates: vec![candidate('a', 1, f64::NAN)],
        };
        assert!(bad_score.validate().is_err());

        let first = candidate('b', 1, 1.0);
        let mut duplicate = first.clone();
        duplicate.lane_rank = 2;
        duplicate.legacy_chunk_id = "chunk:other".into();
        let duplicated = LaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: "exact_symbol".into(),
            weight: 1.0,
            status: LaneStatus::Complete,
            elapsed_us: 1,
            candidate_count: 2,
            coverage: LaneCoverage::complete(None, 2),
            truncation_reason: None,
            candidates: vec![first, duplicate],
        };
        assert!(duplicated.validate().is_err());
    }
}

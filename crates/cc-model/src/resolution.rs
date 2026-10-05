//! Durable, bounded resolver evidence. Internal catalog indices never cross this
//! boundary. Ambiguity is not a chosen graph target, and absent evidence is not
//! a successful empty lookup. Dependencies belong to the same file transaction.
use crate::{CcError, CcResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
// v3 preserves legal symbolic/Unicode type names while excluding ellipsis.
// Public intermediate v2 manifests can contain missing type evidence; schema
// v24 rebuilds those persisted rows, and this boundary rejects legacy payloads.
pub const RESOLUTION_VERSION: u32 = 3;
/// Syntax proves a binding exists but its target is not statically supported.
/// Do not replace it with a coincidental global name or type-catalog fallback.
pub const PARSER_UNSUPPORTED_BINDING: &str = "parser_unsupported_binding";
/// B1 declaration ownership does not establish call-site binding. Keep this
/// C++ negative evidence terminal through generic resolution and dirty reload.
pub const CPP_QUALIFIED_OWNER_UNPROVEN_BINDING: &str = "cpp_qualified_owner_unproven";
/// A C++ namespace-function target lacks declaration-owner proof. Name-only
/// resolution and type backfill must not turn this negative evidence into a UID.
pub const CPP_NAMESPACE_UNPROVEN_BINDING: &str = "cpp_namespace_owner_unproven";
pub const MAX_RESOLUTION_RECORDS: usize = 4096;
pub const MAX_RESOLUTION_DEPENDENCIES: usize = 4096;
pub const MAX_RESOLUTION_CANDIDATES: usize = 32;
pub const MAX_RESOLUTION_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionTarget {
    pub file_path: String,
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub qname: Option<String>,
    pub kind: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolutionOutcome {
    Resolved {
        target: ResolutionTarget,
        strategy: String,
        confidence: f64,
    },
    Ambiguous {
        candidates: Vec<ResolutionTarget>,
        reason: String,
        candidate_count_lower_bound: usize,
        truncated: bool,
    },
    Unresolved {
        reason: String,
    },
    Unsupported {
        capability: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionRecord {
    pub site_kind: String,
    pub site_id: String,
    pub query: String,
    pub outcome: ResolutionOutcome,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    TargetSurface,
    NameBucket,
    MissingPath,
    ModuleConfig,
    PackageFiles,
    SymbolInventory,
    FileInventory,
}
impl DependencyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TargetSurface => "target_surface",
            Self::NameBucket => "name_bucket",
            Self::MissingPath => "missing_path",
            Self::ModuleConfig => "module_config",
            Self::PackageFiles => "package_files",
            Self::SymbolInventory => "symbol_inventory",
            Self::FileInventory => "file_inventory",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionDependency {
    pub kind: DependencyKind,
    pub key: String,
}
impl ResolutionDependency {
    pub fn new(kind: DependencyKind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionManifest {
    pub version: u32,
    pub records: Vec<ResolutionRecord>,
    /// Optional module evidence is published with the same owning file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<crate::project_model::ModuleResolution>,
    pub dependencies: BTreeSet<ResolutionDependency>,
    pub complete: bool,
    pub omitted_records: u64,
    pub reasons: BTreeSet<String>,
    #[serde(skip)]
    used_bytes: usize,
}
impl Default for ResolutionManifest {
    fn default() -> Self {
        let mut m = Self::new();
        m.complete = false;
        m.reasons.insert("resolver_not_run".into());
        m.fallback_dependencies();
        m
    }
}
impl ResolutionManifest {
    pub fn new() -> Self {
        Self {
            version: RESOLUTION_VERSION,
            records: vec![],
            modules: vec![],
            dependencies: BTreeSet::new(),
            complete: true,
            omitted_records: 0,
            reasons: BTreeSet::new(),
            used_bytes: 0,
        }
    }
    fn ensure_accounted(&mut self) {
        if self.used_bytes == 0
            && (!self.records.is_empty()
                || !self.dependencies.is_empty()
                || !self.modules.is_empty())
        {
            self.used_bytes =
                serde_json::to_vec(&(&self.records, &self.dependencies, &self.modules))
                    .map_or(MAX_RESOLUTION_BYTES, |b| b.len());
        }
    }
    pub fn mark_incomplete(&mut self, reason: impl Into<String>) {
        self.complete = false;
        self.reasons.insert(reason.into());
        self.fallback_dependencies();
    }
    fn fallback_dependencies(&mut self) {
        for kind in [
            DependencyKind::SymbolInventory,
            DependencyKind::FileInventory,
            DependencyKind::ModuleConfig,
        ] {
            self.dependencies
                .insert(ResolutionDependency::new(kind, "*"));
        }
    }
    pub fn dependency(&mut self, kind: DependencyKind, key: impl Into<String>) {
        self.ensure_accounted();
        let d = ResolutionDependency::new(kind, key);
        let size = serde_json::to_vec(&d).map_or(MAX_RESOLUTION_BYTES, |b| b.len());
        if self.dependencies.contains(&d) {
            return;
        }
        if d.key.len() > 4096
            || self.dependencies.len() >= MAX_RESOLUTION_DEPENDENCIES
            || self.used_bytes.saturating_add(size) > MAX_RESOLUTION_BYTES
        {
            self.complete = false;
            self.reasons.insert("dependency_budget_exceeded".into());
            self.fallback_dependencies();
        } else {
            self.used_bytes += size;
            self.dependencies.insert(d);
        }
    }
    pub fn record(&mut self, record: ResolutionRecord) {
        self.ensure_accounted();
        let size = serde_json::to_vec(&record)
            .expect("finite resolver record")
            .len();
        if self.records.len() + self.modules.len() >= MAX_RESOLUTION_RECORDS
            || self.used_bytes.saturating_add(size) > MAX_RESOLUTION_BYTES
        {
            self.omitted_records += 1;
            self.complete = false;
            self.reasons.insert("outcome_budget_exceeded".into());
            self.fallback_dependencies();
            return;
        }
        self.used_bytes += size;
        self.records.push(record);
    }
    pub fn record_module(&mut self, record: crate::project_model::ModuleResolution) {
        self.ensure_accounted();
        let size = serde_json::to_vec(&record).map_or(MAX_RESOLUTION_BYTES, |b| b.len());
        if self.records.len() + self.modules.len() >= MAX_RESOLUTION_RECORDS
            || self.used_bytes.saturating_add(size) > MAX_RESOLUTION_BYTES
        {
            self.omitted_records += 1;
            self.mark_incomplete("module_evidence_budget_exceeded");
        } else {
            self.used_bytes += size;
            self.modules.push(record);
        }
    }
    pub fn normalize(&mut self) {
        self.modules.sort_by(|a, b| {
            (&a.import_string, &a.request_key).cmp(&(&b.import_string, &b.request_key))
        });
        self.modules.dedup();
        self.records
            .sort_by(|a, b| (&a.site_kind, &a.site_id).cmp(&(&b.site_kind, &b.site_id)));
        self.records.dedup_by(|a, b| a == b);
        // Runtime-only byte accounting never participates in persistence/equality.
        self.used_bytes = 0;
    }
    pub fn validate(&self) -> CcResult<()> {
        let invalid =
            |reason: &str| CcError::InvalidParams(format!("invalid resolution manifest: {reason}"));
        let target_valid = |t: &ResolutionTarget| {
            crate::repo_path::is_canonical_file(&t.file_path)
                && !t.symbol_id.is_empty()
                && !t.kind.is_empty()
                && t.symbol_uid.as_ref().is_none_or(|u| !u.is_empty())
        };
        if self.version != RESOLUTION_VERSION {
            return Err(invalid("unsupported version"));
        }
        if self.records.len() + self.modules.len() > MAX_RESOLUTION_RECORDS
            || self.dependencies.len() > MAX_RESOLUTION_DEPENDENCIES + 3
        {
            return Err(invalid("record/dependency limit"));
        }
        if self.complete && (self.omitted_records != 0 || !self.reasons.is_empty()) {
            return Err(invalid("complete with missing evidence"));
        }
        if self
            .dependencies
            .iter()
            .any(|d| d.key.is_empty() || d.key.len() > 4096)
        {
            return Err(invalid("invalid dependency key"));
        }
        let mut modules = BTreeSet::new();
        for r in &self.modules {
            if r.import_string.is_empty()
                || r.import_string.len() > 4096
                || !modules.insert((&r.import_string, &r.request_key))
                || r.request_key.len() > 16384
                || r.strategy.is_empty()
                || r.module_resolution.is_empty()
                || !r.valid_target_shape()
                || r.resolved_path
                    .as_ref()
                    .is_some_and(|p| !crate::repo_path::is_canonical_file(p))
                || r.probes
                    .iter()
                    .chain(&r.config_dependencies)
                    .any(|p| !crate::repo_path::is_canonical_file(p))
            {
                return Err(invalid("invalid module resolution evidence"));
            }
        }
        let mut sites = BTreeSet::new();
        for r in &self.records {
            if r.site_id.is_empty() || r.site_kind.is_empty() {
                return Err(invalid("empty site identity"));
            }
            if !sites.insert((&r.site_kind, &r.site_id)) {
                return Err(invalid(&format!(
                    "conflicting duplicate {} site {}",
                    r.site_kind, r.site_id
                )));
            }
            match &r.outcome {
                ResolutionOutcome::Resolved {
                    target,
                    strategy,
                    confidence,
                } => {
                    if !target_valid(target) {
                        return Err(invalid("invalid resolved target identity"));
                    }
                    if strategy.is_empty()
                        || !confidence.is_finite()
                        || !(0.0..=1.0).contains(confidence)
                    {
                        return Err(invalid("invalid resolved provenance"));
                    }
                }
                ResolutionOutcome::Ambiguous {
                    candidates,
                    reason,
                    candidate_count_lower_bound,
                    truncated,
                } => {
                    if reason.is_empty()
                        || candidates.len() > MAX_RESOLUTION_CANDIDATES
                        || *candidate_count_lower_bound < candidates.len()
                        || (!*truncated
                            && (candidates.len() < 2
                                || *candidate_count_lower_bound != candidates.len()))
                    {
                        return Err(invalid("invalid ambiguity cardinality"));
                    }
                    if !candidates.iter().all(target_valid) {
                        return Err(invalid("invalid ambiguous target identity"));
                    }
                    if candidates
                        .iter()
                        .map(|t| (&t.file_path, &t.symbol_id))
                        .collect::<BTreeSet<_>>()
                        .len()
                        != candidates.len()
                    {
                        return Err(invalid("duplicate candidate identity"));
                    }
                }
                ResolutionOutcome::Unresolved { reason } => {
                    if reason.is_empty() {
                        return Err(invalid("missing unresolved reason"));
                    }
                }
                ResolutionOutcome::Unsupported { capability } => {
                    if capability.is_empty() {
                        return Err(invalid("missing capability reason"));
                    }
                }
            }
        }
        Ok(())
    }
}
/// Shared projection for detecting candidate-bucket changes without hashing
/// function bodies or duplicating different DB/in-memory fingerprint formulas.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResolutionSymbol {
    #[serde(
        default,
        skip_serializing_if = "crate::cpp_owner::CppQualifiedOwnerState::is_non_b1"
    )]
    pub cpp_qualified_owner: crate::cpp_owner::CppQualifiedOwnerState,
    pub name: String,
    pub qname: Option<String>,
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub kind: String,
    pub signature: Option<String>,
    pub receiver_type: Option<String>,
    pub param_types: Option<String>,
    pub return_type: Option<String>,
    pub param_count: Option<u32>,
    pub base_types: Option<String>,
    pub implements: Option<String>,
}
impl From<&crate::SymbolRecord> for ResolutionSymbol {
    fn from(s: &crate::SymbolRecord) -> Self {
        Self {
            cpp_qualified_owner: s.cpp_qualified_owner,
            name: s.name.clone(),
            qname: s.qname.clone(),
            symbol_id: s.symbol_id.clone(),
            symbol_uid: s.symbol_uid.clone(),
            kind: s.kind.as_str().into(),
            signature: s.signature.clone(),
            receiver_type: s.receiver_type.clone(),
            param_types: s.param_types.clone(),
            return_type: s.return_type.clone(),
            param_count: s.param_count,
            base_types: s.base_types.clone(),
            implements: s.implements.clone(),
        }
    }
}
pub fn resolution_name_keys(raw: &str) -> BTreeSet<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return BTreeSet::new();
    }
    let dotted = raw.replace("::", ".");
    let leaf = dotted.rsplit('.').next().unwrap_or(raw).trim();
    [raw.to_lowercase(), leaf.to_lowercase()]
        .into_iter()
        // Keep the exact spelling even for an unknown/malformed query, but
        // a trailing separator does not identify an empty-name dependency.
        .filter(|key| !key.is_empty())
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolutionCoverage {
    #[serde(default)]
    pub modules_resolved: u64,
    #[serde(default)]
    pub modules_unresolved: u64,
    #[serde(default)]
    pub modules_unsupported: u64,
    #[serde(default)]
    pub modules_external: u64,
    #[serde(default)]
    pub modules_ambiguous: u64,
    #[serde(default)]
    pub modules_unknown: u64,
    #[serde(default)]
    pub packages_resolved: u64,
    pub processed_files: u64,
    pub resolved: u64,
    pub ambiguous: u64,
    pub unresolved: u64,
    pub unsupported: u64,
    pub incomplete_files: u64,
    pub omitted_records: u64,
}
impl ResolutionCoverage {
    pub fn observe(&mut self, m: &ResolutionManifest) {
        for module in &m.modules {
            match module.status {
                crate::project_model::ModuleStatus::Resolved => {
                    self.modules_resolved += 1;
                    self.packages_resolved += u64::from(module.resolved_package.is_some());
                }
                crate::project_model::ModuleStatus::External => self.modules_external += 1,
                crate::project_model::ModuleStatus::Ambiguous => self.modules_ambiguous += 1,
                crate::project_model::ModuleStatus::Unknown => self.modules_unknown += 1,
                crate::project_model::ModuleStatus::Unresolved => self.modules_unresolved += 1,
                crate::project_model::ModuleStatus::Unsupported => self.modules_unsupported += 1,
            }
        }
        self.processed_files += 1;
        self.incomplete_files += u64::from(!m.complete);
        self.omitted_records += m.omitted_records;
        for r in &m.records {
            match r.outcome {
                ResolutionOutcome::Resolved { .. } => self.resolved += 1,
                ResolutionOutcome::Ambiguous { .. } => self.ambiguous += 1,
                ResolutionOutcome::Unresolved { .. } => self.unresolved += 1,
                ResolutionOutcome::Unsupported { .. } => self.unsupported += 1,
            }
        }
    }
}

#[cfg(test)]
mod name_key_regression_tests {
    use super::*;

    #[test]
    fn legacy_manifest_versions_are_rejected() {
        for version in [1, 2] {
            let legacy = format!(
                r#"{{"version":{version},"records":[],"dependencies":[],"complete":true,"omitted_records":0,"reasons":[]}}"#
            );
            let manifest: ResolutionManifest = serde_json::from_str(&legacy).unwrap();
            assert!(
                matches!(manifest.validate(), Err(CcError::InvalidParams(reason)) if reason.contains("unsupported version"))
            );
        }
        assert!(ResolutionManifest::new().validate().is_ok());
    }

    #[test]
    fn qualified_and_unicode_names_keep_exact_and_leaf_invalidation_keys() {
        for (raw, expected) in [
            ("pkg.Type", vec!["pkg.type", "type"]),
            ("pkg::Type", vec!["pkg::type", "type"]),
            (" 数据.类型 ", vec!["数据.类型", "类型"]),
            ("École.Élève", vec!["école.élève", "élève"]),
            ("_Hidden", vec!["_hidden"]),
        ] {
            assert_eq!(
                resolution_name_keys(raw),
                expected.into_iter().map(String::from).collect()
            );
        }
    }

    #[test]
    fn trailing_separator_and_punctuation_never_create_empty_leaf_keys() {
        for raw in ["...", ".", "::", "pkg.", "pkg::", "pkg.   ", "?"] {
            let keys = resolution_name_keys(raw);
            assert!(keys.contains(&raw.trim().to_lowercase()), "{raw:?}");
            assert!(!keys.contains(""), "{raw:?}");
            let mut manifest = ResolutionManifest::new();
            for key in keys {
                manifest.dependency(DependencyKind::NameBucket, key);
            }
            manifest.validate().unwrap();
        }
        assert!(resolution_name_keys("").is_empty());
        assert!(resolution_name_keys(" \t\n").is_empty());
    }

    #[test]
    fn explicit_empty_dependency_still_fails_strict_validation() {
        let mut manifest = ResolutionManifest::new();
        manifest.dependency(DependencyKind::NameBucket, "");
        assert!(
            matches!(manifest.validate(), Err(CcError::InvalidParams(reason)) if reason.contains("invalid dependency key"))
        );
    }
}

//! Projection of actual resolver results plus bounded dependencies. Re-running
//! this after framework enrichment refreshes evidence, not name resolution.
use super::{catalog::SymbolCatalog, types::NameResolution};
use cc_model::{package_surface::PackageKey, parse::ParseOutcome, resolution::*};
use std::collections::{BTreeMap, BTreeSet};
impl SymbolCatalog {
    fn evidence_target(&self, idx: usize) -> ResolutionTarget {
        let e = &self.entries[idx];
        ResolutionTarget {
            file_path: e.file_path.clone(),
            symbol_id: e.symbol_id.clone(),
            symbol_uid: e.symbol_uid.clone(),
            qname: e.qname.clone(),
            kind: e.kind.as_str().into(),
        }
    }
    pub(in crate::resolver) fn decision_record(
        &self,
        kind: &str,
        id: &str,
        query: &str,
        result: &NameResolution,
    ) -> ResolutionRecord {
        let outcome = match result {
            NameResolution::Resolved(r) => ResolutionOutcome::Resolved {
                target: self.evidence_target(r.catalog_index),
                strategy: r.strategy_name().into(),
                confidence: r.confidence,
            },
            NameResolution::Ambiguous {
                candidates,
                reason,
                count_lower_bound,
                truncated,
            } => ResolutionOutcome::Ambiguous {
                candidates: candidates
                    .iter()
                    .map(|&i| self.evidence_target(i))
                    .collect(),
                reason: (*reason).into(),
                candidate_count_lower_bound: *count_lower_bound,
                truncated: *truncated,
            },
            NameResolution::Unresolved(reason) => ResolutionOutcome::Unresolved {
                reason: (*reason).into(),
            },
        };
        ResolutionRecord {
            site_kind: kind.into(),
            site_id: id.into(),
            query: query.into(),
            outcome,
        }
    }
    fn recorded_target(
        &self,
        file: Option<&str>,
        id: Option<&str>,
        uid: Option<&str>,
        strategy: &str,
        confidence: f64,
    ) -> Option<ResolutionOutcome> {
        let index = uid.and_then(|u| self.find_by_uid(u))?;
        let e = &self.entries[index];
        if file.is_some_and(|f| f != e.file_path) || id.is_some_and(|i| i != e.symbol_id) {
            return None;
        }
        Some(ResolutionOutcome::Resolved {
            target: self.evidence_target(index),
            strategy: if strategy.is_empty() {
                "existing_binding".into()
            } else {
                strategy.into()
            },
            confidence: if confidence.is_finite() {
                confidence.clamp(0.0, 1.0)
            } else {
                0.0
            },
        })
    }
    pub(crate) fn seal_resolution_manifest(&self, _file: &str, outcome: &mut ParseOutcome) {
        let old = std::mem::replace(&mut outcome.resolution, ResolutionManifest::new());
        let previous: BTreeMap<_, _> = old
            .records
            .into_iter()
            .filter(|r| !matches!(r.outcome, ResolutionOutcome::Resolved { .. }))
            .map(|r| ((r.site_kind.clone(), r.site_id.clone()), r))
            .collect();
        let mut m = ResolutionManifest::new();
        // A file's imports/configuration can alter candidates even when the
        // final selected target is unchanged. P3 will refine config granularity.
        if !outcome.imports.is_empty()
            || !outcome.public_surface.conditions.is_empty()
            || PackageKey::from_surface(&outcome.public_surface).is_some()
        {
            m.dependency(DependencyKind::ModuleConfig, "*");
        }
        if let Some(key) = PackageKey::from_surface(&outcome.public_surface) {
            for k in key.contribution_keys() {
                m.dependency(DependencyKind::PackageFiles, k.storage_key());
            }
        }
        for import in &outcome.imports {
            // The captured module layer owns all path probes. Keep inventory
            // invalidation conservative here; do not rerun a language-agnostic
            // JS/Python path guesser while sealing symbol evidence.
            m.dependency(DependencyKind::FileInventory, "*");
            if let Some(path) = &import.resolved_path {
                m.dependency(DependencyKind::TargetSurface, path.clone());
            }
            for name in [&import.imported_name, &import.alias].into_iter().flatten() {
                record_names(&mut m, name);
            }
        }
        for r in &outcome.symbol_refs {
            let query = r.ref_name.as_deref().unwrap_or(&r.symbol_name);
            record_names(&mut m, query);
            let result = self
                .recorded_target(
                    r.target_file_path.as_deref(),
                    r.target_symbol_id.as_deref(),
                    r.target_symbol_uid.as_deref(),
                    &r.resolution_strategy,
                    r.resolution_confidence,
                )
                .or_else(|| {
                    previous
                        .get(&("symbol_ref".into(), r.ref_id.clone()))
                        .map(|r| r.outcome.clone())
                })
                .unwrap_or_else(|| ResolutionOutcome::Unresolved {
                    reason: "no_current_target".into(),
                });
            record_result(&mut m, "symbol_ref", &r.ref_id, query, result);
        }
        for r in &outcome.call_edges {
            record_names(&mut m, &r.callee_symbol);
            if let Some(receiver) = r
                .receiver_expr
                .as_deref()
                .or_else(|| r.callee_symbol.rsplit_once('.').map(|(head, _)| head))
            {
                record_names(&mut m, receiver);
                m.dependency(DependencyKind::SymbolInventory, "*");
            }
            let result = self
                .recorded_target(
                    r.target_file_path.as_deref(),
                    r.target_symbol_id.as_deref(),
                    r.callee_symbol_uid.as_deref(),
                    &r.resolution_strategy,
                    r.resolution_confidence,
                )
                .or_else(|| {
                    previous
                        .get(&("call".into(), r.edge_id.clone()))
                        .map(|r| r.outcome.clone())
                })
                .unwrap_or_else(|| ResolutionOutcome::Unresolved {
                    reason: "no_current_target".into(),
                });
            record_result(&mut m, "call", &r.edge_id, &r.callee_symbol, result);
        }
        for r in &outcome.semantic_edges {
            // Hierarchy/containment is regenerated outside name resolution;
            // it may be present on dirty reload but not in fresh parse output.
            // Never mistake those derived structural edges for lookup evidence.
            if matches!(
                r.relation_kind,
                cc_model::edge::SemanticRelation::Defines
                    | cc_model::edge::SemanticRelation::DefinesMethod
                    | cc_model::edge::SemanticRelation::ContainsFile
                    | cc_model::edge::SemanticRelation::ContainsModule
            ) {
                continue;
            }
            record_names(&mut m, &r.target_symbol);
            let result = self
                .recorded_target(
                    None,
                    None,
                    r.target_symbol_uid.as_deref(),
                    "semantic_target",
                    r.confidence,
                )
                .unwrap_or_else(|| ResolutionOutcome::Unresolved {
                    reason: "no_semantic_target".into(),
                });
            record_result(&mut m, "semantic", &r.edge_id, &r.target_symbol, result);
        }
        for r in &outcome.route_edges {
            let query = r.handler_name.as_deref().unwrap_or("");
            record_names(&mut m, query);
            let result = self
                .recorded_target(
                    None,
                    r.handler_symbol_id.as_deref(),
                    r.handler_symbol_uid.as_deref(),
                    r.resolution_strategy.as_deref().unwrap_or("route_handler"),
                    r.resolution_confidence.unwrap_or(0.0),
                )
                .unwrap_or_else(|| ResolutionOutcome::Unresolved {
                    reason: "no_route_target".into(),
                });
            record_result(&mut m, "route", &r.edge_id, query, result);
        }
        if outcome
            .public_surface
            .extractor_version
            .starts_with("conservative-")
        {
            m.record(ResolutionRecord {
                site_kind: "capability".into(),
                site_id: "public_surface".into(),
                query: String::new(),
                outcome: ResolutionOutcome::Unsupported {
                    capability: outcome.public_surface.reasons.join(","),
                },
            });
        }
        if !old.complete {
            for reason in old.reasons {
                m.mark_incomplete(reason);
            }
            m.omitted_records = m.omitted_records.max(old.omitted_records);
        }
        m.normalize();
        outcome.resolution = m;
    }
}
fn record_result(
    m: &mut ResolutionManifest,
    kind: &str,
    id: &str,
    query: &str,
    result: ResolutionOutcome,
) {
    match &result {
        ResolutionOutcome::Resolved { target, .. } => {
            m.dependency(DependencyKind::TargetSurface, target.file_path.clone())
        }
        ResolutionOutcome::Ambiguous { candidates, .. } => {
            for target in candidates {
                m.dependency(DependencyKind::TargetSurface, target.file_path.clone());
            }
        }
        _ => {}
    }
    m.record(ResolutionRecord {
        site_kind: kind.into(),
        site_id: id.into(),
        query: query.into(),
        outcome: result,
    });
}
fn record_names(m: &mut ResolutionManifest, raw: &str) {
    for key in name_keys(raw) {
        m.dependency(DependencyKind::NameBucket, key);
    }
}
/// Shared query/event spelling: exact qualified names and leaf/type segments.
pub(crate) fn name_keys(raw: &str) -> BTreeSet<String> {
    resolution_name_keys(raw)
}

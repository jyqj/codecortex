//! Outcome resolution and semantic edges.

use std::collections::{BTreeSet, HashMap};

use cc_model::edge::{CallEdgeRecord, ResolutionKind};
use cc_model::parse::ParseOutcome;
use cc_model::resolution::{
    ResolutionManifest, ResolutionOutcome, ResolutionRecord, CPP_NAMESPACE_UNPROVEN_BINDING,
};
use cc_model::symbol::SymbolKind;

use crate::type_catalog::TypeCatalog;

use super::catalog::SymbolCatalog;
use super::helpers::*;
use super::types::*;

/// With schema v26, the C++ parser produces Function + container only for its
/// AST-proven namespace scope A. Use Scanner's language detector, not name or
/// qname spelling guesses. Generic catalog lookup carries no C++ owner proof.
fn needs_cpp_namespace_proof(entry: &CatalogEntry) -> bool {
    entry.kind == SymbolKind::Function
        && entry.container.is_some()
        && cc_parsers::detect_language(&entry.file_path) == cc_model::Language::Cpp
}

fn namespace_negative(site_kind: &str, site_id: &str, query: &str) -> ResolutionRecord {
    ResolutionRecord {
        site_kind: site_kind.into(),
        site_id: site_id.into(),
        query: query.into(),
        outcome: ResolutionOutcome::Unresolved {
            reason: CPP_NAMESPACE_UNPROVEN_BINDING.into(),
        },
    }
}

fn block_namespace_ref(
    sref: &mut cc_model::symbol::SymbolRefRecord,
    manifest: &mut ResolutionManifest,
) {
    sref.target_symbol_id = None;
    sref.target_symbol_uid = None;
    sref.target_file_path = None;
    sref.resolution_kind = ResolutionKind::Unresolved;
    sref.resolution_confidence = 0.0;
    sref.resolution_strategy = CPP_NAMESPACE_UNPROVEN_BINDING.into();
    manifest.record(namespace_negative(
        "symbol_ref",
        &sref.ref_id,
        &sref.symbol_name,
    ));
}

fn block_namespace_call(edge: &mut CallEdgeRecord, manifest: &mut ResolutionManifest) {
    edge.target_symbol_id = None;
    edge.callee_symbol_uid = None;
    edge.target_file_path = None;
    edge.resolution_kind = ResolutionKind::Unresolved;
    edge.resolution_confidence = 0.0;
    edge.resolution_strategy = CPP_NAMESPACE_UNPROVEN_BINDING.into();
    manifest.record(namespace_negative(
        "call",
        &edge.edge_id,
        &edge.callee_symbol,
    ));
}

/// Whether the type-catalog pass may touch an already-processed call edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeUpgradeGate {
    /// Scope/import/parser-proven result — never touched.
    Skip,
    /// Unresolved or generic-heuristic result — overwritten unconditionally
    /// (the pre-existing `dominated` fallback path).
    Backfill,
    /// Name-evidence-only result (global-unique / suffix / fuzzy*) — may be
    /// replaced by a *different* target proposed at strictly higher
    /// confidence, with the upgrade provenance recorded in the strategy.
    Upgrade,
}

fn type_upgrade_gate(edge: &CallEdgeRecord) -> TypeUpgradeGate {
    match edge.resolution_kind {
        ResolutionKind::Unresolved
            if matches!(
                edge.resolution_strategy.as_str(),
                "ambiguous"
                    | "authoritative_unresolved"
                    | cc_model::resolution::PARSER_UNSUPPORTED_BINDING
                    | CPP_NAMESPACE_UNPROVEN_BINDING
                    | cc_model::project_model::MODULE_BLOCKED_BINDING
            ) =>
        {
            TypeUpgradeGate::Skip
        }
        ResolutionKind::Unresolved => TypeUpgradeGate::Backfill,
        ResolutionKind::Heuristic => match edge.resolution_strategy.as_str() {
            "global_unique" | "suffix" | "fuzzy_single" | "fuzzy_signal" | "fuzzy_arg_count"
            | "fuzzy_receiver" | "fuzzy_multi" => TypeUpgradeGate::Upgrade,
            _ => TypeUpgradeGate::Backfill,
        },
        _ => TypeUpgradeGate::Skip,
    }
}

/// A type-catalog resolution proposal for a call edge.
struct TypeCatalogCandidate {
    catalog_index: usize,
    uid: String,
    kind: ResolutionKind,
    confidence: f64,
    strategy: &'static str,
}

impl TypeCatalogCandidate {
    fn apply(self, edge: &mut CallEdgeRecord, entries: &[CatalogEntry]) {
        let e = &entries[self.catalog_index];
        edge.target_symbol_id = Some(e.symbol_id.clone());
        edge.target_file_path = Some(e.file_path.clone());
        edge.callee_symbol_uid = Some(self.uid);
        edge.resolution_kind = self.kind;
        edge.resolution_confidence = self.confidence;
        edge.resolution_strategy = self.strategy.to_string();
        edge.parser_confidence = self.confidence;
    }
}

impl SymbolCatalog {
    // -----------------------------------------------------------------------
    // Resolve entire ParseOutcome
    // -----------------------------------------------------------------------

    /// Resolve unresolved call-edges and symbol-refs in an outcome.
    ///
    /// Enhanced pipeline: uses the full recursive resolver when scope/import
    /// data is available, falling back to the simple find_best strategy.
    pub fn resolve_outcome(&self, file_path: &str, outcome: &mut ParseOutcome) {
        let context = Self::build_resolution_context(outcome, file_path);
        self.resolve_outcome_with_context(file_path, outcome, &context);
    }

    /// Resolve unresolved call-edges and symbol-refs using a pre-built context.
    pub fn resolve_outcome_with_context(
        &self,
        file_path: &str,
        outcome: &mut ParseOutcome,
        context: &ResolutionContext,
    ) {
        let scopes = &context.scopes;
        let imports = &context.imports;
        outcome.resolution = cc_model::resolution::ResolutionManifest::new();

        // Resolve symbol refs
        for sref in &mut outcome.symbol_refs {
            if sref.resolution_strategy == CPP_NAMESPACE_UNPROVEN_BINDING {
                block_namespace_ref(sref, &mut outcome.resolution);
                continue;
            }
            if matches!(
                sref.resolution_strategy.as_str(),
                cc_model::resolution::PARSER_UNSUPPORTED_BINDING
                    | cc_model::project_model::MODULE_BLOCKED_BINDING
            ) {
                outcome
                    .resolution
                    .record(cc_model::resolution::ResolutionRecord {
                        site_kind: "symbol_ref".into(),
                        site_id: sref.ref_id.clone(),
                        query: sref.symbol_name.clone(),
                        outcome: cc_model::resolution::ResolutionOutcome::Unsupported {
                            capability: if sref.resolution_strategy
                                == cc_model::project_model::MODULE_BLOCKED_BINDING
                            {
                                "module_context_unavailable"
                            } else {
                                "python_dynamic_lexical_binding"
                            }
                            .into(),
                        },
                    });
                continue;
            }
            if sref.resolution_strategy.is_empty() {
                sref.resolution_strategy =
                    default_resolution_strategy(sref.resolution_kind).to_string();
            }
            if sref.resolution_confidence <= 0.0 {
                sref.resolution_confidence = default_resolution_confidence(sref.resolution_kind);
            }
            if sref.target_symbol_id.is_some() {
                continue;
            }
            let raw = sref.ref_name.as_deref().unwrap_or(&sref.symbol_name);
            {
                let decision = self.resolve_decision(NameRequest {
                    name: raw,
                    file: file_path,
                    line: sref.line,
                    scopes,
                    imports,
                    container: sref.container.as_deref(),
                    signals: CallSiteSignals::default(),
                });
                if matches!(decision, NameResolution::Ambiguous { .. }) {
                    sref.resolution_kind = ResolutionKind::Unresolved;
                    sref.resolution_confidence = 0.0;
                    sref.resolution_strategy = "ambiguous".into();
                    outcome.resolution.record(self.decision_record(
                        "symbol_ref",
                        &sref.ref_id,
                        raw,
                        &decision,
                    ));
                    continue;
                }
                if let NameResolution::Resolved(result) = decision {
                    let e = &self.entries[result.catalog_index];
                    if needs_cpp_namespace_proof(e) {
                        block_namespace_ref(sref, &mut outcome.resolution);
                        continue;
                    }
                    sref.target_symbol_id = Some(e.symbol_id.clone());
                    sref.target_file_path = Some(e.file_path.clone());
                    sref.target_symbol_uid = e.symbol_uid.clone();
                    sref.resolution_kind = result.resolution_kind.to_resolution_kind();
                    sref.resolution_confidence = result.confidence;
                    sref.resolution_strategy = result.strategy_name().to_string();
                    continue;
                }
            }
            // A completed ladder miss is not retried through a weaker fallback.
        }

        // Resolve call edges
        for edge in &mut outcome.call_edges {
            if edge.resolution_strategy == CPP_NAMESPACE_UNPROVEN_BINDING {
                block_namespace_call(edge, &mut outcome.resolution);
                continue;
            }
            if edge.resolution_strategy == cc_model::project_model::MODULE_BLOCKED_BINDING {
                // A lost import must not retain a prior resolver's call classification.
                edge.call_kind = "unresolved_import".into();
                edge.dispatch_kind = cc_model::edge::DispatchKind::Dynamic;
            }
            if matches!(
                edge.resolution_strategy.as_str(),
                cc_model::resolution::PARSER_UNSUPPORTED_BINDING
                    | cc_model::project_model::MODULE_BLOCKED_BINDING
            ) {
                outcome
                    .resolution
                    .record(cc_model::resolution::ResolutionRecord {
                        site_kind: "call".into(),
                        site_id: edge.edge_id.clone(),
                        query: edge.callee_symbol.clone(),
                        outcome: cc_model::resolution::ResolutionOutcome::Unsupported {
                            capability: if edge.resolution_strategy
                                == cc_model::project_model::MODULE_BLOCKED_BINDING
                            {
                                "module_context_unavailable"
                            } else {
                                "python_dynamic_lexical_binding"
                            }
                            .into(),
                        },
                    });
                continue;
            }
            if edge.resolution_strategy.is_empty() {
                edge.resolution_strategy =
                    default_resolution_strategy(edge.resolution_kind).to_string();
            }
            if edge.resolution_confidence <= 0.0 {
                edge.resolution_confidence = default_resolution_confidence(edge.resolution_kind);
            }
            if edge.target_symbol_id.is_some() {
                continue;
            }
            // Re-resolution can revoke a former target. Recompute syntax-only
            // classification even on a miss, rather than retaining old state.
            edge.call_kind = Self::classify_call_kind(&edge.callee_symbol, imports).into();
            edge.dispatch_kind = cc_model::edge::DispatchKind::Direct;
            {
                // Call-site signals: arg count from the parser, receiver from
                // the explicit receiver expression or the dotted callee head
                // ("obj.method" → "obj").
                let receiver = edge.receiver_expr.as_deref().or_else(|| {
                    edge.callee_symbol
                        .rsplit_once('.')
                        .map(|(receiver, _)| receiver)
                });
                let signals = CallSiteSignals {
                    arg_count: edge.arg_count,
                    receiver,
                };
                let qualified_go = if file_path.ends_with(".go") {
                    receiver
                        .filter(|head| imports.iter().any(|i| i.local_name == *head))
                        .map(|head| format!("{head}.{}", edge.callee_symbol))
                } else {
                    None
                };
                let decision = self.resolve_decision(NameRequest {
                    name: qualified_go.as_deref().unwrap_or(&edge.callee_symbol),
                    file: file_path,
                    line: edge.line,
                    scopes,
                    imports,
                    container: edge.caller_symbol.as_deref(),
                    signals,
                });
                if matches!(decision, NameResolution::Ambiguous { .. }) {
                    edge.resolution_kind = ResolutionKind::Unresolved;
                    edge.resolution_confidence = 0.0;
                    edge.resolution_strategy = "ambiguous".into();
                    outcome.resolution.record(self.decision_record(
                        "call",
                        &edge.edge_id,
                        &edge.callee_symbol,
                        &decision,
                    ));
                    continue;
                }
                if matches!(
                    decision,
                    NameResolution::Unresolved("authoritative_lookup_miss")
                ) {
                    edge.resolution_strategy = "authoritative_unresolved".into();
                    outcome.resolution.record(self.decision_record(
                        "call",
                        &edge.edge_id,
                        &edge.callee_symbol,
                        &decision,
                    ));
                    continue;
                }
                if let NameResolution::Resolved(result) = decision {
                    let e = &self.entries[result.catalog_index];
                    if needs_cpp_namespace_proof(e) {
                        block_namespace_call(edge, &mut outcome.resolution);
                        continue;
                    }
                    edge.target_symbol_id = Some(e.symbol_id.clone());
                    edge.target_file_path = Some(e.file_path.clone());
                    edge.callee_symbol_uid = e.symbol_uid.clone();
                    edge.resolution_kind = result.resolution_kind.to_resolution_kind();
                    edge.resolution_confidence = result.confidence;
                    edge.resolution_strategy = result.strategy_name().to_string();
                    edge.dispatch_kind = cc_model::edge::DispatchKind::Direct;
                    edge.call_kind =
                        if qualified_go.is_some() && e.kind == cc_model::SymbolKind::Function {
                            "imported".into()
                        } else {
                            Self::classify_call_kind(&edge.callee_symbol, imports).to_string()
                        };
                    continue;
                }
            }
            // Preserve explicit miss/ambiguity rather than selecting an unrelated fallback.
        }

        // Type-catalog assisted resolution for call edges. Two regimes,
        // decided by `type_upgrade_gate`:
        // - Backfill: unresolved / generic-heuristic edges are overwritten
        //   unconditionally (the pre-existing `dominated` path).
        // - Upgrade: edges resolved by name-evidence-only ladder steps may be
        //   replaced when the catalog proposes a *different* target at
        //   strictly higher confidence; scope/import-proven results are
        //   never touched.
        if let Some(ref tc) = self.type_catalog {
            for edge in &mut outcome.call_edges {
                let gate = type_upgrade_gate(edge);
                if gate == TypeUpgradeGate::Skip {
                    continue;
                }
                let candidate = match self.type_catalog_candidate(tc, edge) {
                    Some(candidate) => candidate,
                    None => continue,
                };
                if needs_cpp_namespace_proof(&self.entries[candidate.catalog_index]) {
                    block_namespace_call(edge, &mut outcome.resolution);
                    continue;
                }
                match gate {
                    TypeUpgradeGate::Backfill => candidate.apply(edge, &self.entries),
                    TypeUpgradeGate::Upgrade => {
                        let differs = edge
                            .callee_symbol_uid
                            .as_deref()
                            .map(|current| current != candidate.uid)
                            .unwrap_or(true);
                        if differs && candidate.confidence > edge.resolution_confidence {
                            let upgraded_from = std::mem::take(&mut edge.resolution_strategy);
                            let strategy = candidate.strategy;
                            candidate.apply(edge, &self.entries);
                            edge.resolution_strategy =
                                format!("{}:upgraded_from={}", strategy, upgraded_from);
                        }
                    }
                    TypeUpgradeGate::Skip => {}
                }
            }
        }

        // Resolve caller_symbol_uid on call edges
        for edge in &mut outcome.call_edges {
            if edge.caller_symbol_uid.is_some() {
                continue;
            }
            if let Some(ref caller) = edge.caller_symbol {
                if let Some(idx) = self.find_best(caller, file_path) {
                    let e = &self.entries[idx];
                    edge.caller_symbol_uid = e.symbol_uid.clone();
                    edge.caller_symbol_id = Some(e.symbol_id.clone());
                }
            }
        }

        // Resolve caller_symbol_uid on http_call_edges by enclosing symbol range
        for hce in &mut outcome.http_call_edges {
            if hce.caller_symbol_uid.is_some() {
                continue;
            }
            if let Some(indices) = self.by_file.get(file_path) {
                let mut best: Option<(usize, u32)> = None; // (index, span)
                for &idx in indices {
                    let e = &self.entries[idx];
                    if e.start_line <= hce.line && hce.line <= e.end_line {
                        let span = e.end_line - e.start_line;
                        if best.is_none() || span < best.unwrap().1 {
                            best = Some((idx, span));
                        }
                    }
                }
                if let Some((idx, _)) = best {
                    let e = &self.entries[idx];
                    hce.caller_symbol_uid = e.symbol_uid.clone();
                }
            }
        }

        // Resolve route edges through the single three-tier entry point
        // (see `resolve_route_handler` for the tier order and semantics).
        for route in &mut outcome.route_edges {
            if route.handler_symbol_id.is_some() {
                continue;
            }
            let handler = match route.handler_name {
                Some(ref h) => h.clone(),
                None => continue,
            };
            if let Some(resolution) =
                self.resolve_route_handler(&handler, file_path, route.line, scopes, imports)
            {
                let e = &self.entries[resolution.catalog_index];
                route.handler_symbol_id = Some(e.symbol_id.clone());
                route.handler_symbol_uid = e.symbol_uid.clone();
                route.resolution_strategy = Some(resolution.strategy);
                route.resolution_confidence = Some(resolution.confidence);
            }
        }
        self.seal_resolution_manifest(file_path, outcome);
    }

    /// Build a type-catalog resolution proposal for a call edge, trying the
    /// signal sources in fixed precedence: type-assign-inferred receiver,
    /// raw receiver expression, then arg-count disambiguation.
    fn type_catalog_candidate(
        &self,
        tc: &TypeCatalog,
        edge: &CallEdgeRecord,
    ) -> Option<TypeCatalogCandidate> {
        // Extract the leaf method name from dotted callee ("obj.parse" → "parse")
        let callee = &edge.callee_symbol;
        let leaf = callee.rsplit('.').next().unwrap_or(callee);

        // If receiver_expr is a variable name like "x", resolve x → Foo via
        // type_assigns, then look up Foo.method_name.
        if let Some(ref recv) = edge.receiver_expr {
            if let Some(resolved_type) = tc.resolve_var_type(&edge.file_path, recv) {
                if let Some(uid) = tc.resolve_method_by_receiver(leaf, resolved_type) {
                    if let Some(idx) = self.find_by_uid(uid) {
                        return Some(TypeCatalogCandidate {
                            catalog_index: idx,
                            uid: uid.to_string(),
                            kind: ResolutionKind::ScopeResolved,
                            confidence: 0.90,
                            strategy: "type_assign_receiver",
                        });
                    }
                }
            }
        }

        // Receiver-based resolution using the raw receiver expression.
        if let Some(ref recv) = edge.receiver_expr {
            if let Some(uid) = tc.resolve_method_by_receiver(leaf, recv) {
                if let Some(idx) = self.find_by_uid(uid) {
                    return Some(TypeCatalogCandidate {
                        catalog_index: idx,
                        uid: uid.to_string(),
                        kind: ResolutionKind::Qualified,
                        confidence: 0.95,
                        strategy: "receiver_type",
                    });
                }
            }
        }

        // Arg-count disambiguation.
        if let Some(arg_count) = edge.arg_count {
            if let Some(uid) = tc.resolve_method_by_arg_count(leaf, arg_count) {
                if let Some(idx) = self.find_by_uid(uid) {
                    return Some(TypeCatalogCandidate {
                        catalog_index: idx,
                        uid: uid.to_string(),
                        kind: ResolutionKind::ScopeResolved,
                        confidence: 0.9,
                        strategy: "arg_count",
                    });
                }
            }
        }

        None
    }

    /// Resolve semantic edge UIDs and backfill using a pre-built context.
    pub fn resolve_semantic_edges_and_backfill_with_context(
        &self,
        _file_path: &str,
        outcome: &mut ParseOutcome,
        context: &ResolutionContext,
    ) {
        let imports = &context.imports;

        // --- W6: resolve source/target UIDs on semantic_edges ---
        for edge in &mut outcome.semantic_edges {
            // Resolve source_symbol_uid (usually a class/struct in the same file)
            if edge.source_symbol_uid.is_none() {
                if let Some(idx) =
                    self.find_by_name_in_file(&edge.source_symbol, &edge.file_path, true)
                {
                    edge.source_symbol_uid = self.entries[idx].symbol_uid.clone();
                    tracing::debug!(
                        source = %edge.source_symbol,
                        file = %edge.file_path,
                        "semantic_edge: resolved source UID"
                    );
                } else {
                    tracing::debug!(
                        source = %edge.source_symbol,
                        file = %edge.file_path,
                        "semantic_edge: source UID unresolved"
                    );
                }
            }

            // Resolve target_symbol_uid (parent class/interface/trait — may be cross-file)
            if edge.target_symbol_uid.is_none() {
                let resolved =
                    self.resolve_semantic_target(&edge.target_symbol, &edge.file_path, imports);
                if let Some(idx) = resolved {
                    edge.target_symbol_uid = self.entries[idx].symbol_uid.clone();
                    tracing::debug!(
                        target = %edge.target_symbol,
                        file = %edge.file_path,
                        "semantic_edge: resolved target UID"
                    );
                } else {
                    tracing::debug!(
                        target = %edge.target_symbol,
                        file = %edge.file_path,
                        "semantic_edge: target UID unresolved"
                    );
                }
            }
        }

        // --- W4: backfill symbols base_types / implements from semantic_edges ---
        // Three-layer key maps for precise matching:
        //   1. source UID (most precise)
        //   2. source QName (qualified fallback)
        //   3. (file_path, short_name) (last resort)

        // Inherits maps
        let mut inherits_by_uid: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut inherits_by_qname: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut inherits_by_file_name: HashMap<(String, String), BTreeSet<String>> = HashMap::new();

        // Implements maps
        let mut implements_by_uid: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut implements_by_qname: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut implements_by_file_name: HashMap<(String, String), BTreeSet<String>> =
            HashMap::new();

        for edge in &outcome.semantic_edges {
            // Determine target's canonical name (prefer qname from catalog)
            let target_canonical = if let Some(ref target_uid) = edge.target_symbol_uid {
                self.canonical_name_for_uid(target_uid)
                    .unwrap_or_else(|| edge.target_symbol.clone())
            } else {
                edge.target_symbol.clone()
            };

            match edge.relation_kind {
                cc_model::edge::SemanticRelation::Inherits => {
                    // Layer 1: by source UID
                    if let Some(ref uid) = edge.source_symbol_uid {
                        inherits_by_uid
                            .entry(uid.clone())
                            .or_default()
                            .insert(target_canonical.clone());
                    }
                    // Layer 2: by source qname (look up from catalog)
                    if let Some(ref uid) = edge.source_symbol_uid {
                        if let Some(qn) = self.canonical_name_for_uid(uid) {
                            inherits_by_qname
                                .entry(qn.to_lowercase())
                                .or_default()
                                .insert(target_canonical.clone());
                        }
                    }
                    // Layer 3: by (file_path, short_name)
                    inherits_by_file_name
                        .entry((edge.file_path.clone(), edge.source_symbol.clone()))
                        .or_default()
                        .insert(target_canonical);
                }
                cc_model::edge::SemanticRelation::Implements => {
                    if let Some(ref uid) = edge.source_symbol_uid {
                        implements_by_uid
                            .entry(uid.clone())
                            .or_default()
                            .insert(target_canonical.clone());
                    }
                    if let Some(ref uid) = edge.source_symbol_uid {
                        if let Some(qn) = self.canonical_name_for_uid(uid) {
                            implements_by_qname
                                .entry(qn.to_lowercase())
                                .or_default()
                                .insert(target_canonical.clone());
                        }
                    }
                    implements_by_file_name
                        .entry((edge.file_path.clone(), edge.source_symbol.clone()))
                        .or_default()
                        .insert(target_canonical);
                }
                _ => {}
            }
        }

        // Apply backfill with three-layer fallback
        for symbol in &mut outcome.symbols {
            if symbol.base_types.is_none() {
                let bases = symbol
                    .symbol_uid
                    .as_ref()
                    .and_then(|uid| inherits_by_uid.get(uid))
                    .or_else(|| {
                        symbol
                            .qname
                            .as_ref()
                            .and_then(|qn| inherits_by_qname.get(&qn.to_lowercase()))
                    })
                    .or_else(|| {
                        inherits_by_file_name.get(&(symbol.file_path.clone(), symbol.name.clone()))
                    });

                if let Some(v) = bases {
                    symbol.base_types = Some(v.iter().cloned().collect::<Vec<_>>().join(", "));
                    tracing::debug!(
                        symbol = %symbol.name,
                        base_types = %symbol.base_types.as_deref().unwrap_or(""),
                        "backfilled base_types"
                    );
                }
            }
            if symbol.implements.is_none() {
                let impls = symbol
                    .symbol_uid
                    .as_ref()
                    .and_then(|uid| implements_by_uid.get(uid))
                    .or_else(|| {
                        symbol
                            .qname
                            .as_ref()
                            .and_then(|qn| implements_by_qname.get(&qn.to_lowercase()))
                    })
                    .or_else(|| {
                        implements_by_file_name
                            .get(&(symbol.file_path.clone(), symbol.name.clone()))
                    });

                if let Some(v) = impls {
                    symbol.implements = Some(v.iter().cloned().collect::<Vec<_>>().join(", "));
                    tracing::debug!(
                        symbol = %symbol.name,
                        implements = %symbol.implements.as_deref().unwrap_or(""),
                        "backfilled implements"
                    );
                }
            }
        }
    }

    /// Get the canonical (qname-first) name for a catalog entry by UID.
    ///
    /// Returns the qname if available, otherwise the short name. Used to produce
    /// stable, unambiguous type references for backfilled base_types / implements.
    pub(in crate::resolver) fn canonical_name_for_uid(&self, uid: &str) -> Option<String> {
        self.entries
            .get(*self.by_uid.get(uid)?)
            .map(|e| e.qname.clone().unwrap_or_else(|| e.name.clone()))
    }

    /// Find a symbol by name within a specific file.
    ///
    /// When `prefer_type` is true, prefers Class/Interface/Enum entries — the
    /// typical source of a semantic edge.
    pub(in crate::resolver) fn find_by_name_in_file(
        &self,
        name: &str,
        file_path: &str,
        prefer_type: bool,
    ) -> Option<usize> {
        let candidates = self.same_file_named(file_path, name);
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() == 1 {
            return Some(candidates[0]);
        }
        if prefer_type {
            let types: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|&i| {
                    matches!(
                        self.entries[i].kind,
                        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
                    )
                })
                .collect();
            if !types.is_empty() {
                return pick_unique(&self.entries, &types);
            }
        }
        pick_unique(&self.entries, &candidates)
    }

    /// Resolve a semantic edge target name to a catalog index.
    ///
    /// Strategy:
    /// 1. Same-file by name (prefer type-like kinds)
    /// 2. Import resolution
    /// 3. Global unique name (prefer type-like kinds)
    pub(in crate::resolver) fn resolve_semantic_target(
        &self,
        target_name: &str,
        file_path: &str,
        imports: &[ImportBinding],
    ) -> Option<usize> {
        // 1. Same-file
        if let Some(idx) = self.find_by_name_in_file(target_name, file_path, true) {
            return Some(idx);
        }

        // 2. Import resolution
        if let Some(idx) = self.resolve_via_imports(imports, target_name) {
            return Some(idx);
        }

        // 3. Global unique name with type-kind preference
        let lower = target_name.to_lowercase();
        if let Some(indices) = self.by_name.get(&lower) {
            // Filter to type-like entries first
            let type_candidates: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| {
                    matches!(
                        self.entries[i].kind,
                        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
                    )
                })
                .collect();
            if type_candidates.len() == 1 {
                return Some(type_candidates[0]);
            }
            // Fallback: any unique candidate
            if indices.len() == 1 {
                return Some(indices[0]);
            }
            // Multiple candidates — try import-distance tie-breaking
            let pool = if !type_candidates.is_empty() {
                &type_candidates
            } else {
                indices
            };
            return best_by_import_distance(&self.entries, pool, file_path);
        }

        None
    }
}

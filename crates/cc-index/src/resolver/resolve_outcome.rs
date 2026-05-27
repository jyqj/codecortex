//! Outcome resolution and semantic edges.

use std::collections::{BTreeSet, HashMap};

use cc_model::edge::ResolutionKind;
use cc_model::parse::ParseOutcome;
use cc_model::symbol::SymbolKind;

use super::catalog::SymbolCatalog;
use super::helpers::*;
use super::types::*;

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
        let has_rich_context = !scopes.is_empty() || !imports.is_empty();

        // Resolve symbol refs
        for sref in &mut outcome.symbol_refs {
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
            if has_rich_context {
                if let Some(result) = self.resolve_name(
                    raw,
                    file_path,
                    sref.line,
                    &scopes,
                    &imports,
                    sref.container.as_deref(),
                ) {
                    let e = &self.entries[result.catalog_index];
                    sref.target_symbol_id = Some(e.symbol_id.clone());
                    sref.target_file_path = Some(e.file_path.clone());
                    sref.target_symbol_uid = e.symbol_uid.clone();
                    sref.resolution_kind = result.resolution_kind.to_resolution_kind();
                    sref.resolution_confidence = result.confidence;
                    sref.resolution_strategy = result.resolution_kind.strategy_name().to_string();
                    continue;
                }
            }
            // Fallback: simple find_best
            if let Some(idx) = self.find_best(raw, file_path) {
                let e = &self.entries[idx];
                sref.target_symbol_id = Some(e.symbol_id.clone());
                sref.target_file_path = Some(e.file_path.clone());
                sref.target_symbol_uid = e.symbol_uid.clone();
                sref.resolution_kind = if e.file_path == file_path {
                    ResolutionKind::ScopeResolved
                } else {
                    ResolutionKind::Heuristic
                };
                sref.resolution_confidence = default_resolution_confidence(sref.resolution_kind);
                sref.resolution_strategy = if e.file_path == file_path {
                    "same_file_fallback".to_string()
                } else {
                    "global_fallback".to_string()
                };
            }
        }

        // Resolve call edges
        for edge in &mut outcome.call_edges {
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
            if has_rich_context {
                if let Some(result) = self.resolve_name(
                    &edge.callee_symbol,
                    file_path,
                    edge.line,
                    &scopes,
                    &imports,
                    edge.caller_symbol.as_deref(),
                ) {
                    let e = &self.entries[result.catalog_index];
                    edge.target_symbol_id = Some(e.symbol_id.clone());
                    edge.target_file_path = Some(e.file_path.clone());
                    edge.callee_symbol_uid = e.symbol_uid.clone();
                    edge.resolution_kind = result.resolution_kind.to_resolution_kind();
                    edge.resolution_confidence = result.confidence;
                    edge.resolution_strategy = result.resolution_kind.strategy_name().to_string();
                    edge.dispatch_kind = cc_model::edge::DispatchKind::Direct;
                    edge.call_kind =
                        Self::classify_call_kind(&edge.callee_symbol, &imports).to_string();
                    continue;
                }
            }
            // Fallback
            if let Some(idx) = self.find_best(&edge.callee_symbol, file_path) {
                let e = &self.entries[idx];
                edge.target_symbol_id = Some(e.symbol_id.clone());
                edge.target_file_path = Some(e.file_path.clone());
                edge.callee_symbol_uid = e.symbol_uid.clone();
                edge.resolution_kind = if e.file_path == file_path {
                    ResolutionKind::ScopeResolved
                } else {
                    ResolutionKind::Heuristic
                };
                edge.resolution_confidence = default_resolution_confidence(edge.resolution_kind);
                edge.resolution_strategy = if e.file_path == file_path {
                    "same_file_fallback".to_string()
                } else {
                    "global_fallback".to_string()
                };
            }
        }

        // Type-catalog assisted resolution for call edges
        if let Some(ref tc) = self.type_catalog {
            for edge in &mut outcome.call_edges {
                // Only attempt type-based resolution for edges that are still unresolved
                // or resolved at heuristic level (low confidence)
                let dominated = edge.resolution_kind == ResolutionKind::Unresolved
                    || edge.resolution_kind == ResolutionKind::Heuristic;
                if !dominated {
                    continue;
                }

                // Extract the leaf method name from dotted callee (e.g., "obj.parse" → "parse")
                let callee = &edge.callee_symbol;
                let leaf = callee.rsplit('.').next().unwrap_or(callee);

                // Try type_assign-enhanced receiver resolution first:
                // If receiver_expr is a variable name like "x", resolve x → Foo
                // via type_assigns, then look up Foo.method_name.
                if let Some(ref recv) = edge.receiver_expr {
                    if let Some(resolved_type) = tc.resolve_var_type(&edge.file_path, recv) {
                        if let Some(uid) = tc.resolve_method_by_receiver(leaf, resolved_type) {
                            if let Some(idx) = self.find_by_uid(uid) {
                                let e = &self.entries[idx];
                                edge.target_symbol_id = Some(e.symbol_id.clone());
                                edge.target_file_path = Some(e.file_path.clone());
                                edge.callee_symbol_uid = Some(uid.to_string());
                                edge.resolution_kind = ResolutionKind::ScopeResolved;
                                edge.resolution_confidence = 0.90;
                                edge.resolution_strategy = "type_assign_receiver".to_string();
                                edge.parser_confidence = 0.90;
                                continue;
                            }
                        }
                    }
                }

                // Try receiver-based resolution (using raw receiver expression)
                if let Some(ref recv) = edge.receiver_expr {
                    if let Some(uid) = tc.resolve_method_by_receiver(leaf, recv) {
                        if let Some(idx) = self.find_by_uid(uid) {
                            let e = &self.entries[idx];
                            edge.target_symbol_id = Some(e.symbol_id.clone());
                            edge.target_file_path = Some(e.file_path.clone());
                            edge.callee_symbol_uid = Some(uid.to_string());
                            edge.resolution_kind = ResolutionKind::Qualified;
                            edge.resolution_confidence = 0.95;
                            edge.resolution_strategy = "receiver_type".to_string();
                            edge.parser_confidence = 0.95;
                            continue;
                        }
                    }
                }

                // Try arg-count disambiguation
                if let Some(ac) = edge.arg_count {
                    if let Some(uid) = tc.resolve_method_by_arg_count(leaf, ac) {
                        if let Some(idx) = self.find_by_uid(uid) {
                            let e = &self.entries[idx];
                            edge.target_symbol_id = Some(e.symbol_id.clone());
                            edge.target_file_path = Some(e.file_path.clone());
                            edge.callee_symbol_uid = Some(uid.to_string());
                            edge.resolution_kind = ResolutionKind::ScopeResolved;
                            edge.resolution_confidence = 0.9;
                            edge.resolution_strategy = "arg_count".to_string();
                            edge.parser_confidence = 0.9;
                        }
                    }
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

        // Resolve route edges — three-tier strategy:
        // Tier 1: Import-traced dotted handler resolution (cross-module)
        // Tier 2: Rich context resolution via resolve_name
        // Tier 3: Global handler resolution without same-file preference
        for route in &mut outcome.route_edges {
            if route.handler_symbol_id.is_some() {
                continue;
            }
            let handler = match route.handler_name {
                Some(ref h) => h.clone(),
                None => continue,
            };

            // Tier 1: Dotted handler resolution (e.g. "userCtrl.getUsers", "controllers.users.list")
            if handler.contains('.') {
                if let Some(idx) =
                    self.resolve_dotted_handler(&handler, file_path, &scopes, &imports)
                {
                    let e = &self.entries[idx];
                    route.handler_symbol_id = Some(e.symbol_id.clone());
                    route.handler_symbol_uid = e.symbol_uid.clone();
                    continue;
                }
            }

            // Tier 2: Rich context resolution (scopes + imports)
            if has_rich_context {
                if let Some(result) =
                    self.resolve_name(&handler, file_path, route.line, &scopes, &imports, None)
                {
                    let e = &self.entries[result.catalog_index];
                    route.handler_symbol_id = Some(e.symbol_id.clone());
                    route.handler_symbol_uid = e.symbol_uid.clone();
                    continue;
                }
            }

            // Tier 3: Global handler resolution (no same-file preference)
            if let Some(idx) = self.resolve_handler_global(&handler, file_path, &imports) {
                let e = &self.entries[idx];
                route.handler_symbol_id = Some(e.symbol_id.clone());
                route.handler_symbol_uid = e.symbol_uid.clone();
            }
        }
    }

    /// Resolve semantic edge UIDs and backfill symbol base_types/implements.
    ///
    /// This should be called **before** building the TypeCatalog so that
    /// `base_types` / `implements` are populated when `TypeCatalog::build_from_symbols`
    /// reads them.
    pub fn resolve_semantic_edges_and_backfill(&self, file_path: &str, outcome: &mut ParseOutcome) {
        let context = Self::build_resolution_context(outcome, file_path);
        self.resolve_semantic_edges_and_backfill_with_context(file_path, outcome, &context);
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
                    self.resolve_semantic_target(&edge.target_symbol, &edge.file_path, &imports);
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
            // Prefer Class / Interface / Enum
            for &idx in &candidates {
                let kind = self.entries[idx].kind;
                if matches!(
                    kind,
                    SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
                ) {
                    return Some(idx);
                }
            }
        }
        // Fallback: first candidate
        candidates.into_iter().next()
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
            return best_by_import_distance(&self.entries, pool, file_path)
                .or_else(|| pool.first().copied());
        }

        None
    }
}

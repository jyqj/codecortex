//! Route handler resolution.

use std::collections::HashMap;

use cc_model::symbol::SymbolKind;

use super::catalog::SymbolCatalog;
use super::helpers::*;
use super::types::*;

/// Tier-1 dotted resolution traces a unique handler through imports, scope
/// bindings, or qname lookup — evidence comparable to the ladder's import
/// step (`InternalResKind::ImportResolved`, 0.85).
const ROUTE_DOTTED_CONFIDENCE: f64 = 0.85;

/// Tier-3 global resolution is leaf-name scoring with no scope/import proof —
/// same weight as the resolver's generic `global_fallback`
/// (`default_resolution_confidence(Heuristic)`, 0.5).
const ROUTE_GLOBAL_CONFIDENCE: f64 = 0.5;

/// Outcome of [`SymbolCatalog::resolve_route_handler`]: the winning catalog
/// entry plus provenance (which tier won and at what confidence).
#[derive(Clone, Debug)]
pub(crate) struct RouteHandlerResolution {
    pub(crate) catalog_index: usize,
    /// "route_dotted", "route_ladder:<ladder strategy>", or "route_global".
    pub(crate) strategy: String,
    pub(crate) confidence: f64,
}

impl SymbolCatalog {
    // -----------------------------------------------------------------------
    // Cross-file route handler resolution
    // -----------------------------------------------------------------------

    /// Single entry point for route-handler resolution.
    ///
    /// Tier order (first hit wins):
    /// 1. [`Self::resolve_dotted_handler`] — dotted names only
    ///    ("ctrl.method", "controllers.users.list"): import/scope-traced
    ///    member-chain resolution, then qname / qname-suffix lookup.
    /// 2. [`Self::resolve_name`] — the full resolution ladder with the
    ///    file's rich context; only attempted when scopes or imports exist.
    ///    Its strategy name and confidence are carried into the returned
    ///    provenance ("route_ladder:<strategy>").
    /// 3. [`Self::resolve_handler_global`] — global leaf-name scoring with
    ///    no same-file preference (routes typically call cross-file
    ///    handlers).
    pub(crate) fn resolve_route_handler(
        &self,
        handler: &str,
        file_path: &str,
        line: u32,
        scopes: &HashMap<String, CatalogScope>,
        imports: &[ImportBinding],
    ) -> Option<RouteHandlerResolution> {
        // Tier 1: dotted handler resolution
        if handler.contains('.') {
            if let Some(idx) = self.resolve_dotted_handler(handler, file_path, scopes, imports) {
                return Some(RouteHandlerResolution {
                    catalog_index: idx,
                    strategy: "route_dotted".to_string(),
                    confidence: ROUTE_DOTTED_CONFIDENCE,
                });
            }
        }

        // Tier 2: rich context resolution (scopes + imports)
        if !scopes.is_empty() || !imports.is_empty() {
            if let Some(result) = self.resolve_name(handler, file_path, line, scopes, imports, None)
            {
                return Some(RouteHandlerResolution {
                    catalog_index: result.catalog_index,
                    strategy: format!("route_ladder:{}", result.strategy_name()),
                    confidence: result.confidence,
                });
            }
        }

        // Tier 3: global handler resolution (no same-file preference)
        self.resolve_handler_global(handler, file_path, imports)
            .map(|idx| RouteHandlerResolution {
                catalog_index: idx,
                strategy: "route_global".to_string(),
                confidence: ROUTE_GLOBAL_CONFIDENCE,
            })
    }

    /// Tier 1 of [`Self::resolve_route_handler`] — prefer that entry point.
    ///
    /// Resolve a dotted handler name (e.g. "userCtrl.getUsers", "controllers.users.list")
    /// by tracing through imports to find the target symbol in another module.
    pub(in crate::resolver) fn resolve_dotted_handler(
        &self,
        handler_name: &str,
        file_path: &str,
        scopes: &HashMap<String, CatalogScope>,
        imports: &[ImportBinding],
    ) -> Option<usize> {
        let parts: Vec<&str> = handler_name.split('.').collect();
        if parts.len() < 2 {
            return None;
        }

        // Strategy A: Resolve via imports (handles "userCtrl.getUsers" where userCtrl is imported)
        if let Some(idx) = self.resolve_via_imports(imports, handler_name) {
            let e = &self.entries[idx];
            if is_handler_like(e.kind) {
                return Some(idx);
            }
        }

        // Strategy B: Resolve the head via scope bindings, then chain the rest
        if let Some(head_idx) = self.resolve_via_scope_bindings(scopes, file_path, parts[0], 0) {
            if let Some(idx) = self.resolve_member_chain_from(
                head_idx,
                &parts[1..],
                &self.entries[head_idx].file_path,
            ) {
                return Some(idx);
            }
        }

        // Strategy C: Resolve head as a same-file symbol, then chain members across files
        let head_candidates = self.same_file_named(file_path, parts[0]);
        if let Some(head_idx) = pick_unique(&self.entries, &head_candidates) {
            let target_file = &self.entries[head_idx].file_path.clone();
            if let Some(idx) = self.resolve_member_chain_from(head_idx, &parts[1..], target_file) {
                return Some(idx);
            }
        }

        // Strategy D: Try qualified-name lookup for the full dotted name
        let full_lower = handler_name.to_lowercase();
        if let Some(indices) = self.by_qname.get(&full_lower) {
            let func_matches: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| is_handler_like(self.entries[i].kind))
                .collect();
            if func_matches.len() == 1 {
                return Some(func_matches[0]);
            }
        }

        // Strategy E: Suffix match on qname — handles "controllers.users.list" matching
        // a qname like "src.controllers.users.list"
        let suffix = format!(".{}", full_lower);
        let mut suffix_matches: Vec<usize> = Vec::new();
        for (qname, indices) in &self.by_qname {
            if qname.ends_with(&suffix) || *qname == full_lower {
                for &idx in indices {
                    if is_handler_like(self.entries[idx].kind) {
                        suffix_matches.push(idx);
                    }
                }
            }
        }
        let unique = dedup_by_id(&self.entries, &suffix_matches);
        if unique.len() == 1 {
            return Some(unique[0]);
        }

        None
    }

    /// Tier 3 of [`Self::resolve_route_handler`] — prefer that entry point.
    ///
    /// Global route handler resolution without same-file preference.
    ///
    /// Unlike `find_best`, this does NOT prefer symbols in the same file as the
    /// route registration. Instead it prefers:
    /// 1. Symbols in files imported by the route file
    /// 2. Symbols with handler/controller-like kind
    /// 3. Shortest qualified name (most specific match)
    pub(in crate::resolver) fn resolve_handler_global(
        &self,
        handler_name: &str,
        route_file: &str,
        imports: &[ImportBinding],
    ) -> Option<usize> {
        let leaf = handler_name.rsplit('.').next().unwrap_or(handler_name);
        let leaf_lower = leaf.to_lowercase();

        // Collect candidates from by_name matching the leaf
        let candidates = self.by_name.get(&leaf_lower)?;
        let unique = dedup_by_id(&self.entries, candidates);

        // Filter to function/method-like symbols only
        let func_candidates: Vec<usize> = unique
            .into_iter()
            .filter(|&i| is_handler_like(self.entries[i].kind))
            .collect();

        if func_candidates.is_empty() {
            return None;
        }
        if func_candidates.len() == 1 {
            return Some(func_candidates[0]);
        }

        // Score candidates: prefer import-reachable and handler-kind symbols
        let mut scored: Vec<(usize, i32)> = func_candidates
            .iter()
            .map(|&idx| {
                let e = &self.entries[idx];
                let mut score: i32 = 0;

                // Prefer symbols in imported files
                if is_import_reachable(&e.file_path, imports) {
                    score += 10;
                }

                // Prefer handler/controller kinds
                if matches!(
                    e.kind,
                    SymbolKind::RouteHandler | SymbolKind::Controller | SymbolKind::Middleware
                ) {
                    score += 5;
                }

                // Prefer shorter qname (more specific)
                let qlen = e.qname.as_ref().map(|q| q.len()).unwrap_or(e.name.len());
                score -= (qlen as i32) / 10;

                // Penalize same-file matches (routes typically call cross-file handlers)
                if e.file_path == route_file {
                    score -= 2;
                }

                (idx, score)
            })
            .collect();

        scored.sort_by_key(|entry| std::cmp::Reverse(entry.1));

        // Only return if the top score is distinctly better, or there's just one winner
        if scored.len() >= 2 && scored[0].1 == scored[1].1 {
            // Tie — fall back to import distance
            let tied: Vec<usize> = scored
                .iter()
                .take_while(|&&(_, s)| s == scored[0].1)
                .map(|&(idx, _)| idx)
                .collect();
            return best_by_import_distance(&self.entries, &tied, route_file);
        }

        Some(scored[0].0)
    }

    /// Find-best fallback: prefer qname → same-file → import-distance.
    pub(in crate::resolver) fn find_best(&self, name: &str, current_file: &str) -> Option<usize> {
        let lower = name.to_lowercase();

        // Try qualified name first
        if let Some(indices) = self.by_qname.get(&lower) {
            if indices.len() == 1 {
                return Some(indices[0]);
            }
            // Prefer same-file via the nested index (O(1)) instead of scanning
            // the global qname Vec. The Vec is O(symbols-sharing-this-qname),
            // so a linear same-file `find` here makes per-edge caller/callee
            // resolution O(N^2) on corpora where one name (e.g. a `process`
            // method) is defined across many files. `by_file_qname` is built in
            // the same insertion order as `by_qname`, so its first entry equals
            // the first same-file hit the linear scan would have returned.
            if let Some(idx) = self.same_file_first(&self.by_file_qname, current_file, &lower) {
                return Some(idx);
            }
            // A bucket larger than `max_fuzzy_pool` is too ambiguous to resolve
            // by path heuristics (same cap as the fuzzy ladder); scanning it
            // per reference is O(N²) on shared names. Leave it unresolved.
            if indices.len() > self.max_fuzzy_pool {
                return None;
            }
            // Fall back to import-distance tie-breaking
            return best_by_import_distance(&self.entries, indices, current_file)
                .or_else(|| indices.first().copied());
        }

        // Try by name
        if let Some(indices) = self.by_name.get(&lower) {
            // Prefer same-file via the nested index (O(1)); see the by_qname note.
            if let Some(idx) = self.same_file_first(&self.by_file_name, current_file, &lower) {
                return Some(idx);
            }
            if indices.len() > self.max_fuzzy_pool {
                return None;
            }
            // Fall back to import-distance tie-breaking
            return best_by_import_distance(&self.entries, indices, current_file)
                .or_else(|| indices.first().copied());
        }

        None
    }

    /// First catalog index in `nested[file][key_lower]`, or `None`. Shared by
    /// the qname/name tiers of [`Self::find_best`] to take the same-file
    /// preference through the prebuilt nested index in O(1) rather than a
    /// linear scan of the global by-name/by-qname Vec.
    fn same_file_first(
        &self,
        nested: &std::collections::HashMap<String, std::collections::HashMap<String, Vec<usize>>>,
        file: &str,
        key_lower: &str,
    ) -> Option<usize> {
        nested
            .get(file)
            .and_then(|m| m.get(key_lower))
            .and_then(|v| v.first())
            .copied()
    }

    // -----------------------------------------------------------------------
    // Public lookup helpers (for framework resolvers)
    // -----------------------------------------------------------------------

    /// Look up a symbol by name and return its (symbol_uid, file_path) if found.
    ///
    /// Uses the same heuristics as `find_best` (qname → name, prefer same-file,
    /// import-distance tie-breaking).
    pub fn lookup_symbol(&self, name: &str, hint_file: &str) -> Option<(String, String)> {
        let idx = self.find_best(name, hint_file)?;
        let entry = &self.entries[idx];
        Some((
            entry.symbol_uid.clone().unwrap_or_default(),
            entry.file_path.clone(),
        ))
    }

    /// Look up all symbols with a given name (case-insensitive).
    /// Returns vec of (symbol_uid, file_path, kind).
    pub fn lookup_all_by_name(&self, name: &str) -> Vec<(String, String, cc_model::SymbolKind)> {
        let lower = name.to_lowercase();
        let indices = match self.by_name.get(&lower) {
            Some(v) => v,
            None => return Vec::new(),
        };
        indices
            .iter()
            .map(|&i| {
                let e = &self.entries[i];
                (
                    e.symbol_uid.clone().unwrap_or_default(),
                    e.file_path.clone(),
                    e.kind,
                )
            })
            .collect()
    }
}

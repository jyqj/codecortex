//! Route handler resolution.

use std::collections::HashMap;

use cc_model::symbol::SymbolKind;

use super::catalog::SymbolCatalog;
use super::helpers::*;
use super::types::*;

impl SymbolCatalog {
    // -----------------------------------------------------------------------
    // Cross-file route handler resolution
    // -----------------------------------------------------------------------

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

        scored.sort_by(|a, b| b.1.cmp(&a.1));

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
            // Prefer same-file
            if let Some(&idx) = indices
                .iter()
                .find(|&&i| self.entries[i].file_path == current_file)
            {
                return Some(idx);
            }
            // Fall back to import-distance tie-breaking
            return best_by_import_distance(&self.entries, indices, current_file)
                .or_else(|| indices.first().copied());
        }

        // Try by name
        if let Some(indices) = self.by_name.get(&lower) {
            // Prefer same-file
            if let Some(&idx) = indices
                .iter()
                .find(|&&i| self.entries[i].file_path == current_file)
            {
                return Some(idx);
            }
            // Fall back to import-distance tie-breaking
            return best_by_import_distance(&self.entries, indices, current_file)
                .or_else(|| indices.first().copied());
        }

        None
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

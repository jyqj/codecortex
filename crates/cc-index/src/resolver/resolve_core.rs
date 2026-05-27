//! Core recursive name resolution.

use std::collections::{HashMap, HashSet};

use super::catalog::SymbolCatalog;
use super::helpers::*;
use super::types::*;

impl SymbolCatalog {
    // -----------------------------------------------------------------------
    // Scope chain resolution
    // -----------------------------------------------------------------------

    /// Walk parent_id chain upward, returning scope IDs from innermost to outermost.
    pub fn scope_chain<'a>(
        &self,
        scopes: &'a HashMap<String, CatalogScope>,
        scope_id: &str,
    ) -> Vec<&'a CatalogScope> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut current = Some(scope_id.to_string());
        while let Some(ref sid) = current {
            if !seen.insert(sid.clone()) {
                break;
            }
            if let Some(scope) = scopes.get(sid) {
                chain.push(scope);
                current = scope.parent_id.clone();
            } else {
                break;
            }
        }
        chain
    }

    /// Count hops from `current` to `target` in scope chain. None if unreachable.
    pub fn scope_distance(
        &self,
        scopes: &HashMap<String, CatalogScope>,
        current: &str,
        target: &str,
    ) -> Option<u32> {
        let chain = self.scope_chain(scopes, current);
        chain
            .iter()
            .position(|s| s.scope_id == target)
            .map(|p| p as u32)
    }

    /// Find innermost scope containing the given line in a file.
    pub fn scope_for_line<'a>(
        &self,
        scopes: &'a HashMap<String, CatalogScope>,
        file: &str,
        line: u32,
    ) -> Option<&'a CatalogScope> {
        let mut candidates: Vec<&CatalogScope> = scopes
            .values()
            .filter(|s| s.file_path == file && s.start_line <= line && line <= s.end_line)
            .collect();
        // Innermost = smallest span
        candidates.sort_by_key(|s| (s.end_line - s.start_line, s.start_line));
        candidates.into_iter().next()
    }

    // -----------------------------------------------------------------------
    // Local binding resolution
    // -----------------------------------------------------------------------

    /// Resolve `name` via scope chain bindings starting from the scope at `line`.
    pub fn resolve_via_scope_bindings(
        &self,
        scopes: &HashMap<String, CatalogScope>,
        file: &str,
        name: &str,
        line: u32,
    ) -> Option<usize> {
        let scope = self.scope_for_line(scopes, file, line)?;
        let chain = self.scope_chain(scopes, &scope.scope_id);

        let parts: Vec<&str> = name.split('.').collect();
        let head = parts[0];
        let tail = &parts[1..];

        for scope in &chain {
            for binding in &scope.bindings {
                if binding.name != head {
                    continue;
                }
                // Found binding — try to resolve to catalog entry
                let entry_idx = binding
                    .symbol_uid
                    .as_ref()
                    .and_then(|uid| self.find_by_uid(uid));
                if let Some(idx) = entry_idx {
                    if tail.is_empty() {
                        return Some(idx);
                    }
                    // Follow member chain
                    return self.resolve_member_chain_from(idx, tail, file);
                }
                // Binding found but no catalog entry → shadowed local
                if tail.is_empty() {
                    return None;
                }
                return None;
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Member chain resolution
    // -----------------------------------------------------------------------

    /// Resolve a dotted name like "foo.bar.baz" in the context of `file`.
    pub(in crate::resolver) fn resolve_member_chain(
        &self,
        parts: &[&str],
        file: &str,
    ) -> Option<usize> {
        if parts.is_empty() {
            return None;
        }
        // Resolve head
        let head_candidates = self.same_file_named(file, parts[0]);
        let head_idx = pick_unique(&self.entries, &head_candidates)?;
        if parts.len() == 1 {
            return Some(head_idx);
        }
        self.resolve_member_chain_from(head_idx, &parts[1..], file)
    }

    /// Resolve remaining member parts starting from a known base entry.
    pub(in crate::resolver) fn resolve_member_chain_from(
        &self,
        base_idx: usize,
        tail: &[&str],
        file: &str,
    ) -> Option<usize> {
        let mut current = base_idx;
        for &part in tail {
            current = self.resolve_member_step(current, part, file)?;
        }
        Some(current)
    }

    /// Resolve a single member access step: current.part.
    ///
    /// 5-layer strategy:
    /// 1. Qualified name (container.member)
    /// 2. Container-based same scope
    /// 3. Constructor pattern (class → method)
    /// 4. Cross-file global qualified name
    /// 5. Cross-file container
    pub(in crate::resolver) fn resolve_member_step(
        &self,
        container_idx: usize,
        member: &str,
        _file: &str,
    ) -> Option<usize> {
        let container = &self.entries[container_idx];
        let container_qname = container.qname.as_deref().unwrap_or(&container.name);

        // 1. Direct qname: "ClassName.method"
        let member_qname = format!("{}.{}", container_qname, member);
        let direct = self.same_file_qname(&container.file_path, &member_qname);
        if let Some(idx) = pick_unique(&self.entries, &direct) {
            return Some(idx);
        }

        // 2. Container-based: members where container == current.qname
        let file_entries = self.by_file.get(&container.file_path);
        if let Some(indices) = file_entries {
            let members: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| {
                    let e = &self.entries[i];
                    e.container.as_deref() == Some(container_qname)
                        && e.name.eq_ignore_ascii_case(member)
                })
                .collect();
            if let Some(idx) = pick_unique(&self.entries, &members) {
                return Some(idx);
            }
        }

        // 3. Constructor pattern: if container is variable/function/param, search classes
        if matches!(
            container.kind,
            cc_model::symbol::SymbolKind::Function
                | cc_model::symbol::SymbolKind::Variable
                | cc_model::symbol::SymbolKind::Property
        ) {
            if let Some(indices) = self.by_file.get(&container.file_path) {
                for &i in indices {
                    let e = &self.entries[i];
                    if e.kind != cc_model::symbol::SymbolKind::Class {
                        continue;
                    }
                    let cls_member_qname =
                        format!("{}.{}", e.qname.as_deref().unwrap_or(&e.name), member);
                    let cls_matches = self.same_file_qname(&e.file_path, &cls_member_qname);
                    if let Some(idx) = pick_unique(&self.entries, &cls_matches) {
                        return Some(idx);
                    }
                }
            }
        }

        // 4. Cross-file global: search by_qname for "container.member"
        let cross = self.by_qname.get(&member_qname.to_lowercase());
        if let Some(indices) = cross {
            if indices.len() == 1 {
                return Some(indices[0]);
            }
        }

        // 5. Cross-file container: find members with matching container across all files
        let cross_by_name = self.by_name.get(&member.to_lowercase());
        if let Some(indices) = cross_by_name {
            let matches: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| {
                    self.entries[i]
                        .container
                        .as_ref()
                        .map(|c| c.eq_ignore_ascii_case(container_qname))
                        .unwrap_or(false)
                })
                .collect();
            if let Some(idx) = pick_unique(&self.entries, &matches) {
                return Some(idx);
            }
        }

        None
    }

    // -----------------------------------------------------------------------
    // Import resolution
    // -----------------------------------------------------------------------

    /// Resolve a name via import bindings.
    pub fn resolve_via_imports(&self, imports: &[ImportBinding], name: &str) -> Option<usize> {
        let parts: Vec<&str> = name.split('.').collect();
        let head = parts[0];
        let tail = &parts[1..];

        // Find matching import
        let binding = imports.iter().find(|b| b.local_name == head)?;

        if binding.is_namespace {
            // Namespace import: the tail is the exported name
            if tail.is_empty() {
                return None;
            }
            let export_name = tail[0];
            let target = self.resolve_export(&binding.source_module, export_name)?;
            if tail.len() == 1 {
                return Some(target);
            }
            return self.resolve_member_chain_from(
                target,
                &tail[1..],
                &self.entries[target].file_path,
            );
        }

        // Named / default import
        let imported_name = binding.imported_name.as_deref().unwrap_or(head);

        let target = self.resolve_export(&binding.source_module, imported_name)?;
        if tail.is_empty() {
            return Some(target);
        }
        self.resolve_member_chain_from(target, tail, &self.entries[target].file_path)
    }

    /// Resolve an exported name from a module path.
    pub(in crate::resolver) fn resolve_export(
        &self,
        module_path: &str,
        export_name: &str,
    ) -> Option<usize> {
        let exact = self.exported(module_path, export_name);
        if let Some(idx) = pick_unique(&self.entries, &exact) {
            return Some(idx);
        }
        // Fallback: default export
        if export_name == "default" {
            let defaults: Vec<usize> = self
                .by_file
                .get(module_path)
                .map(|indices| {
                    indices
                        .iter()
                        .copied()
                        .filter(|&i| self.entries[i].is_default_export)
                        .collect()
                })
                .unwrap_or_default();
            return pick_unique(&self.entries, &defaults);
        }
        None
    }

    // -----------------------------------------------------------------------
    // Complete recursive resolver
    // -----------------------------------------------------------------------

    /// Full resolution pipeline for a name at a given location.
    ///
    /// Resolution order:
    /// 1. If name starts with "this." or "self." → resolve member on owner class
    /// 2. Check scope bindings
    /// 3. Check same-file candidates (prefer closest scope)
    /// 4. Check imports
    /// 5. Check qualified-name suffix matches
    /// 6. Check globally unique leaf names
    /// 7. Fall back to fuzzy candidate selection
    pub fn resolve_name(
        &self,
        name: &str,
        file: &str,
        line: u32,
        scopes: &HashMap<String, CatalogScope>,
        imports: &[ImportBinding],
        container: Option<&str>,
    ) -> Option<ResolveResult> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }

        let key = ResolveKey {
            name: trimmed.to_string(),
            file_path: file.to_string(),
            line,
            container: container.map(|s| s.to_string()),
        };

        // Check cache
        if let Ok(mut cache) = self.resolve_cache.lock() {
            if let Some(cached) = cache.get(&key) {
                return cached.clone();
            }
        }

        let result = self.resolve_name_inner(trimmed, file, line, scopes, imports, container);

        if let Ok(mut cache) = self.resolve_cache.lock() {
            cache.put(key, result.clone());
        }
        result
    }

    /// Core resolution logic (uncached). Called by `resolve_name`.
    pub(in crate::resolver) fn resolve_name_inner(
        &self,
        name: &str,
        file: &str,
        line: u32,
        scopes: &HashMap<String, CatalogScope>,
        imports: &[ImportBinding],
        container: Option<&str>,
    ) -> Option<ResolveResult> {
        let parts: Vec<&str> = name.split('.').collect();
        let head = parts[0];
        let tail = &parts[1..];

        // 1. this/self resolution
        if (head == "this" || head == "self") && !tail.is_empty() {
            let owner = self.owner_class_qname(file, container);
            if let Some(ref owner_qname) = owner {
                let member_qname = format!("{}.{}", owner_qname, tail[0]);
                let candidates = self.same_file_qname(file, &member_qname);
                if let Some(idx) = pick_unique(&self.entries, &candidates) {
                    if tail.len() == 1 {
                        return Some(ResolveResult {
                            catalog_index: idx,
                            resolution_kind: InternalResKind::Exact,
                            confidence: InternalResKind::Exact.base_confidence(),
                        });
                    }
                    if let Some(final_idx) = self.resolve_member_chain_from(idx, &tail[1..], file) {
                        return Some(ResolveResult {
                            catalog_index: final_idx,
                            resolution_kind: InternalResKind::Qualified,
                            confidence: InternalResKind::Qualified.base_confidence(),
                        });
                    }
                    return Some(ResolveResult {
                        catalog_index: idx,
                        resolution_kind: InternalResKind::Exact,
                        confidence: InternalResKind::Exact.base_confidence(),
                    });
                }
            }
            return None;
        }

        // 2. Scope bindings
        if let Some(idx) = self.resolve_via_scope_bindings(scopes, file, name, line) {
            return Some(ResolveResult {
                catalog_index: idx,
                resolution_kind: InternalResKind::ScopeResolved,
                confidence: InternalResKind::ScopeResolved.base_confidence(),
            });
        }

        // 3. Same-file candidates
        if let Some(idx) = self.best_same_file_candidate(name, file, scopes, line, container) {
            let kind = if parts.len() > 1 {
                InternalResKind::Qualified
            } else {
                InternalResKind::ScopeResolved
            };
            return Some(ResolveResult {
                catalog_index: idx,
                resolution_kind: kind,
                confidence: kind.base_confidence(),
            });
        }

        // 4. Import resolution
        if let Some(idx) = self.resolve_via_imports(imports, name) {
            return Some(ResolveResult {
                catalog_index: idx,
                resolution_kind: InternalResKind::ImportResolved,
                confidence: InternalResKind::ImportResolved.base_confidence(),
            });
        }

        // 5. Qualified-name suffix resolution
        if let Some(result) = self.try_suffix_match(name, file) {
            return Some(result);
        }

        // 6. Global unique leaf-name resolution
        let leaf = parts.last().unwrap_or(&head);
        if let Some(result) = self.try_global_unique(leaf, imports) {
            return Some(result);
        }

        // 7. Fuzzy fallback with tiered candidate selection
        if let Some(candidates) = self.by_name.get(&leaf.to_lowercase()) {
            let unique = dedup_by_id(&self.entries, candidates);
            let count = unique.len();

            if count == 1 {
                let idx = unique[0];
                let mut conf = InternalResKind::FuzzySingle.base_confidence();
                if !imports.is_empty()
                    && !is_import_reachable(&self.entries[idx].file_path, imports)
                {
                    conf *= 0.5;
                }
                return Some(ResolveResult {
                    catalog_index: idx,
                    resolution_kind: InternalResKind::FuzzySingle,
                    confidence: conf,
                });
            }

            if count > 1 {
                let base = InternalResKind::FuzzyMulti.base_confidence();
                let penalized = candidate_count_penalty(base, count);

                let reachable: Vec<usize> = if !imports.is_empty() {
                    unique
                        .iter()
                        .copied()
                        .filter(|&i| is_import_reachable(&self.entries[i].file_path, imports))
                        .collect()
                } else {
                    Vec::new()
                };

                let (chosen, conf) = if reachable.len() == 1 {
                    (
                        Some(reachable[0]),
                        candidate_count_penalty(
                            InternalResKind::FuzzySingle.base_confidence(),
                            count,
                        ),
                    )
                } else if !reachable.is_empty() {
                    (
                        best_by_import_distance(&self.entries, &reachable, file),
                        penalized,
                    )
                } else {
                    (
                        best_by_import_distance(&self.entries, &unique, file),
                        penalized * 0.5,
                    )
                };

                if let Some(idx) = chosen {
                    return Some(ResolveResult {
                        catalog_index: idx,
                        resolution_kind: InternalResKind::FuzzyMulti,
                        confidence: conf,
                    });
                }
            }
        }

        None
    }

    pub(in crate::resolver) fn try_global_unique(
        &self,
        leaf_name: &str,
        imports: &[ImportBinding],
    ) -> Option<ResolveResult> {
        let candidates = self.by_name.get(&leaf_name.to_lowercase())?;
        let unique = dedup_by_id(&self.entries, candidates);
        if unique.len() != 1 {
            return None;
        }
        let idx = unique[0];
        let mut confidence = InternalResKind::GlobalUnique.base_confidence();
        if !imports.is_empty() && !is_import_reachable(&self.entries[idx].file_path, imports) {
            confidence *= 0.6;
        }
        Some(ResolveResult {
            catalog_index: idx,
            resolution_kind: InternalResKind::GlobalUnique,
            confidence,
        })
    }

    pub(in crate::resolver) fn try_suffix_match(
        &self,
        name: &str,
        file: &str,
    ) -> Option<ResolveResult> {
        if !name.contains('.') {
            return None;
        }
        let needle = name.to_lowercase();
        let suffix = format!(".{}", needle);
        let mut matches: Vec<usize> = Vec::new();
        for (qname, indices) in &self.by_qname {
            let q = qname.to_lowercase();
            if q == needle || q.ends_with(&suffix) {
                matches.extend(indices.iter().copied());
            }
        }
        let unique = dedup_by_id(&self.entries, &matches);
        if unique.is_empty() {
            return None;
        }
        let count = unique.len();
        let idx = if count == 1 {
            unique[0]
        } else {
            best_by_import_distance(&self.entries, &unique, file)?
        };
        Some(ResolveResult {
            catalog_index: idx,
            resolution_kind: InternalResKind::SuffixMatch,
            confidence: candidate_count_penalty(
                InternalResKind::SuffixMatch.base_confidence(),
                count,
            ),
        })
    }

    /// Find the best same-file candidate, preferring scope proximity.
    pub(in crate::resolver) fn best_same_file_candidate(
        &self,
        name: &str,
        file: &str,
        scopes: &HashMap<String, CatalogScope>,
        line: u32,
        container: Option<&str>,
    ) -> Option<usize> {
        let parts: Vec<&str> = name.split('.').collect();

        // If multi-part, try exact qname first
        if parts.len() > 1 {
            let exact = self.same_file_qname(file, name);
            if let Some(idx) = pick_unique(&self.entries, &exact) {
                return Some(idx);
            }
            // Try member chain resolution
            return self.resolve_member_chain(&parts, file);
        }

        // Single name: owner class member
        let short = parts[0];
        if let Some(owner_qname) = self.owner_class_qname(file, container) {
            let owner_member = format!("{}.{}", owner_qname, short);
            let candidates = self.same_file_qname(file, &owner_member);
            if let Some(idx) = pick_unique(&self.entries, &candidates) {
                return Some(idx);
            }
        }

        // All same-file matches
        let same_file = self.same_file_named(file, short);
        if same_file.is_empty() {
            return None;
        }

        // Deduplicate by symbol_id
        let unique = dedup_by_id(&self.entries, &same_file);
        if unique.len() == 1 {
            return Some(unique[0]);
        }

        // Rank by scope distance
        let current_scope = self.scope_for_line(scopes, file, line);
        let current_scope_id = current_scope.map(|s| s.scope_id.as_str());

        let mut ranked: Vec<(usize, (u32, usize))> = unique
            .iter()
            .map(|&idx| {
                let e = &self.entries[idx];
                let distance = current_scope_id
                    .and_then(|csid| {
                        e.scope_id
                            .as_deref()
                            .and_then(|esid| self.scope_distance(scopes, csid, esid))
                    })
                    .unwrap_or(10_000);
                let qlen = e.qname.as_ref().map(|q| q.len()).unwrap_or(e.name.len());
                (idx, (distance, qlen))
            })
            .collect();
        ranked.sort_by_key(|&(_, key)| key);
        ranked.first().map(|&(idx, _)| idx)
    }
}

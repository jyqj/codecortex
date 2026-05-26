//! Symbol catalog and recursive cross-file resolution.
//!
//! SymbolCatalog builds a lookup directory from parsed symbols, then resolves
//! call-edges and symbol-refs to their target symbol IDs / UIDs using a
//! multi-layer strategy: scope bindings → same-file candidates → imports →
//! global unique name fallback.

use std::collections::{BTreeSet, HashMap, HashSet};

use cc_model::edge::{ResolutionKind, SemanticEdgeRecord, SemanticRelation};
use cc_model::parse::ParseOutcome;
use cc_model::scope::ScopeBinding;
use cc_model::symbol::{SymbolKind, SymbolRecord};

use crate::type_catalog::TypeCatalog;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A scope extracted from parse output, used for scope-chain resolution.
#[derive(Clone, Debug)]
pub struct CatalogScope {
    pub scope_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub bindings: Vec<ScopeBinding>,
}

/// An import binding used during resolution.
#[derive(Clone, Debug)]
pub struct ImportBinding {
    pub local_name: String,
    pub source_module: String,
    pub imported_name: Option<String>, // None for namespace import
    pub file_path: String,
    pub is_namespace: bool,
    pub is_default: bool,
}

/// Per-file derived resolver context computed once from a [`ParseOutcome`].
#[derive(Clone, Debug, Default)]
pub struct ResolutionContext {
    pub scopes: HashMap<String, CatalogScope>,
    pub imports: Vec<ImportBinding>,
}

/// Internal resolution kind — maps to `ResolutionKind` for output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternalResKind {
    Exact,
    Qualified,
    ScopeResolved,
    ImportResolved,
    /// Globally unique leaf-name match with no stronger scope/import proof.
    GlobalUnique,
    /// Qualified-name suffix match (e.g. pkg.mod.Func ↔ mod.Func).
    SuffixMatch,
    Heuristic,
    /// Single candidate resolved by name (no scope/import proof).
    FuzzySingle,
    /// Multiple candidates, resolved by import-distance tie-breaking.
    FuzzyMulti,
    Unresolved,
}

impl InternalResKind {
    fn to_resolution_kind(self) -> ResolutionKind {
        match self {
            Self::Exact => ResolutionKind::Exact,
            Self::Qualified => ResolutionKind::Qualified,
            Self::ScopeResolved => ResolutionKind::ScopeResolved,
            Self::ImportResolved => ResolutionKind::ScopeResolved, // best available mapping
            Self::GlobalUnique
            | Self::SuffixMatch
            | Self::Heuristic
            | Self::FuzzySingle
            | Self::FuzzyMulti => ResolutionKind::Heuristic,
            Self::Unresolved => ResolutionKind::Unresolved,
        }
    }

    fn base_confidence(self) -> f64 {
        match self {
            Self::Exact => 1.0,
            Self::Qualified => 0.95,
            Self::ScopeResolved => 0.9,
            Self::ImportResolved => 0.85,
            Self::GlobalUnique => 0.75,
            Self::SuffixMatch => 0.65,
            Self::Heuristic => 0.5,
            Self::FuzzySingle => 0.40,
            Self::FuzzyMulti => 0.30,
            Self::Unresolved => 0.0,
        }
    }

    fn strategy_name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Qualified => "qualified",
            Self::ScopeResolved => "scope",
            Self::ImportResolved => "import_map",
            Self::GlobalUnique => "global_unique",
            Self::SuffixMatch => "suffix",
            Self::Heuristic => "heuristic",
            Self::FuzzySingle => "fuzzy_single",
            Self::FuzzyMulti => "fuzzy_multi",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Result of recursive name resolution.
#[derive(Clone, Debug)]
pub struct ResolveResult {
    pub catalog_index: usize,
    pub resolution_kind: InternalResKind,
    pub confidence: f64,
}

fn default_resolution_confidence(kind: ResolutionKind) -> f64 {
    match kind {
        ResolutionKind::Exact => 1.0,
        ResolutionKind::Qualified => 0.95,
        ResolutionKind::ScopeResolved => 0.9,
        ResolutionKind::Heuristic => 0.5,
        ResolutionKind::Unresolved => 0.0,
    }
}

fn default_resolution_strategy(kind: ResolutionKind) -> &'static str {
    match kind {
        ResolutionKind::Exact => "parser_exact",
        ResolutionKind::Qualified => "parser_qualified",
        ResolutionKind::ScopeResolved => "parser_scope",
        ResolutionKind::Heuristic => "heuristic",
        ResolutionKind::Unresolved => "unresolved",
    }
}

// ---------------------------------------------------------------------------
// CatalogEntry
// ---------------------------------------------------------------------------

/// A single entry in the symbol catalog (extended from original).
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct CatalogEntry {
    symbol_id: String,
    symbol_uid: Option<String>,
    name: String,
    file_path: String,
    kind: SymbolKind,
    container: Option<String>,
    qname: Option<String>,
    export_name: Option<String>,
    is_default_export: bool,
    start_line: u32,
    end_line: u32,
    scope_id: Option<String>,
}

// ---------------------------------------------------------------------------
// SymbolCatalog
// ---------------------------------------------------------------------------

/// Cross-file symbol directory used during indexing to resolve references.
pub struct SymbolCatalog {
    entries: Vec<CatalogEntry>,
    by_name: HashMap<String, Vec<usize>>,
    by_uid: HashMap<String, usize>,
    by_qname: HashMap<String, Vec<usize>>,
    by_id: HashMap<String, Vec<usize>>,
    by_file: HashMap<String, Vec<usize>>,
    by_export: HashMap<(String, String), Vec<usize>>,
    /// Lightweight type catalog for method dispatch resolution.
    type_catalog: Option<TypeCatalog>,
}

impl SymbolCatalog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_name: HashMap::new(),
            by_uid: HashMap::new(),
            by_qname: HashMap::new(),
            by_id: HashMap::new(),
            by_file: HashMap::new(),
            by_export: HashMap::new(),
            type_catalog: None,
        }
    }

    /// Build (or rebuild) the TypeCatalog from the given symbols.
    ///
    /// Call this after all symbols have been registered via `add_symbols`,
    /// typically right before resolving call-edges.
    pub fn build_type_catalog(&mut self, all_symbols: &[SymbolRecord]) {
        let tc = TypeCatalog::build_from_symbols(all_symbols);
        if tc.has_methods() {
            tracing::debug!("TypeCatalog built with method entries");
        }
        self.type_catalog = Some(tc);
    }

    /// Register all symbols from a parsed file.
    pub fn add_symbols(&mut self, symbols: &[SymbolRecord]) {
        for sym in symbols {
            let idx = self.entries.len();
            let entry = CatalogEntry {
                symbol_id: sym.symbol_id.clone(),
                symbol_uid: sym.symbol_uid.clone(),
                name: sym.name.clone(),
                file_path: sym.file_path.clone(),
                kind: sym.kind,
                container: sym.container.clone(),
                qname: sym.qname.clone(),
                export_name: sym.export_name.clone(),
                is_default_export: sym.is_default_export,
                start_line: sym.start_line,
                end_line: sym.end_line,
                scope_id: sym.scope_id.clone(),
            };
            self.entries.push(entry);

            // by_name (lowercase)
            self.by_name
                .entry(sym.name.to_lowercase())
                .or_default()
                .push(idx);

            // by_uid
            if let Some(ref uid) = sym.symbol_uid {
                self.by_uid.insert(uid.clone(), idx);
            }

            // by_qname (lowercase)
            if let Some(ref qname) = sym.qname {
                self.by_qname
                    .entry(qname.to_lowercase())
                    .or_default()
                    .push(idx);
            }

            // by_id
            self.by_id
                .entry(sym.symbol_id.clone())
                .or_default()
                .push(idx);

            // by_file
            self.by_file
                .entry(sym.file_path.clone())
                .or_default()
                .push(idx);

            // by_export: export_name, name, and "default" for default exports
            let mut export_names: HashSet<String> = HashSet::new();
            export_names.insert(sym.name.to_lowercase());
            if let Some(ref en) = sym.export_name {
                export_names.insert(en.to_lowercase());
            }
            if sym.is_default_export {
                export_names.insert("default".to_string());
            }
            for en in export_names {
                self.by_export
                    .entry((sym.file_path.clone(), en))
                    .or_default()
                    .push(idx);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Lookup helpers
    // -----------------------------------------------------------------------

    #[allow(dead_code)]
    fn entry(&self, idx: usize) -> &CatalogEntry {
        &self.entries[idx]
    }

    /// Find entries in the same file matching a name (case-insensitive).
    fn same_file_named(&self, file_path: &str, name: &str) -> Vec<usize> {
        let needle = name.to_lowercase();
        self.by_file
            .get(file_path)
            .map(|indices| {
                indices
                    .iter()
                    .copied()
                    .filter(|&i| {
                        let e = &self.entries[i];
                        e.name.to_lowercase() == needle
                            || e.qname
                                .as_ref()
                                .map(|q| q.to_lowercase() == needle)
                                .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find entries in the same file matching a qname (case-insensitive).
    fn same_file_qname(&self, file_path: &str, qname: &str) -> Vec<usize> {
        let needle = qname.to_lowercase();
        self.by_file
            .get(file_path)
            .map(|indices| {
                indices
                    .iter()
                    .copied()
                    .filter(|&i| {
                        self.entries[i]
                            .qname
                            .as_ref()
                            .map(|q| q.to_lowercase() == needle)
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find exported symbols by file + export name.
    fn exported(&self, file_path: &str, export_name: &str) -> Vec<usize> {
        self.by_export
            .get(&(file_path.to_string(), export_name.to_lowercase()))
            .cloned()
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    fn find_by_id(&self, symbol_id: &str) -> Option<usize> {
        self.by_id.get(symbol_id).and_then(|v| v.first().copied())
    }

    fn find_by_uid(&self, uid: &str) -> Option<usize> {
        self.by_uid.get(uid).copied()
    }

    /// Walk upward from `container` qname to find the owning class.
    fn owner_class_qname(&self, file_path: &str, container: Option<&str>) -> Option<String> {
        let mut qname = container?.to_string();
        loop {
            let direct = self.same_file_qname(file_path, &qname);
            if let Some(&idx) = direct.first() {
                if self.entries[idx].kind == SymbolKind::Class {
                    return self.entries[idx].qname.clone();
                }
            }
            if let Some(dot_pos) = qname.rfind('.') {
                qname.truncate(dot_pos);
            } else {
                break;
            }
        }
        None
    }

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
    pub fn resolve_member_chain(&self, parts: &[&str], file: &str) -> Option<usize> {
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
    fn resolve_member_chain_from(
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
    fn resolve_member_step(
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
            SymbolKind::Function | SymbolKind::Variable | SymbolKind::Property
        ) {
            if let Some(indices) = self.by_file.get(&container.file_path) {
                for &i in indices {
                    let e = &self.entries[i];
                    if e.kind != SymbolKind::Class {
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
    fn resolve_export(&self, module_path: &str, export_name: &str) -> Option<usize> {
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
        let name = name.trim();
        if name.is_empty() {
            return None;
        }

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

    fn try_global_unique(
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

    fn try_suffix_match(&self, name: &str, file: &str) -> Option<ResolveResult> {
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
    fn best_same_file_candidate(
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

    // -----------------------------------------------------------------------
    // Alias map and call classification
    // -----------------------------------------------------------------------

    /// Build alias map: local_name → qualified imported name.
    pub fn build_alias_map(imports: &[ImportBinding]) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for imp in imports {
            let qualified = if let Some(ref imported) = imp.imported_name {
                format!("{}:{}", imp.source_module, imported)
            } else {
                format!("{}:{}", imp.source_module, imp.local_name)
            };
            map.insert(imp.local_name.clone(), qualified);
        }
        map
    }

    /// Classify a call by its callee name and import context.
    pub fn classify_call_kind(callee_name: &str, imports: &[ImportBinding]) -> &'static str {
        if callee_name.contains('.') {
            let last = callee_name.rsplit('.').next().unwrap_or(callee_name);
            if last.starts_with(|c: char| c.is_uppercase()) || last == "__init__" {
                return "constructor";
            }
            return "method";
        }
        // Check if head is an import
        let head = callee_name.split('.').next().unwrap_or(callee_name);
        if imports.iter().any(|b| b.local_name == head) {
            return "imported";
        }
        if callee_name.starts_with(|c: char| c.is_uppercase()) {
            return "constructor";
        }
        "local"
    }

    // -----------------------------------------------------------------------
    // Build helpers from ParseOutcome
    // -----------------------------------------------------------------------

    /// Build CatalogScope map from ParseOutcome scopes.
    pub fn build_scopes(outcome: &ParseOutcome) -> HashMap<String, CatalogScope> {
        outcome
            .scopes
            .iter()
            .map(|s| {
                (
                    s.scope_id.clone(),
                    CatalogScope {
                        scope_id: s.scope_id.clone(),
                        parent_id: s.parent_scope_id.clone(),
                        name: s.name.clone().unwrap_or_default(),
                        file_path: s.file_path.clone(),
                        start_line: s.start_line,
                        end_line: s.end_line,
                        bindings: s.bindings.clone(),
                    },
                )
            })
            .collect()
    }

    /// Build ImportBinding list from ParseOutcome imports.
    pub fn build_import_bindings(outcome: &ParseOutcome, file_path: &str) -> Vec<ImportBinding> {
        outcome
            .imports
            .iter()
            .filter_map(|imp| {
                let resolved = imp.resolved_path.as_ref()?;
                let alias = imp
                    .alias
                    .as_ref()
                    .or(imp.imported_name.as_ref())
                    .cloned()
                    .unwrap_or_else(|| {
                        imp.import_string
                            .rsplit('/')
                            .next()
                            .unwrap_or(&imp.import_string)
                            .rsplit('.')
                            .next()
                            .unwrap_or(&imp.import_string)
                            .to_string()
                    });
                if alias.trim().is_empty() {
                    return None;
                }
                Some(ImportBinding {
                    local_name: alias,
                    source_module: resolved.clone(),
                    imported_name: imp.imported_name.clone(),
                    file_path: file_path.to_string(),
                    is_namespace: imp.is_namespace,
                    is_default: imp.is_default,
                })
            })
            .collect()
    }

    /// Build the reusable derived context for a single file.
    pub fn build_resolution_context(outcome: &ParseOutcome, file_path: &str) -> ResolutionContext {
        ResolutionContext {
            scopes: Self::build_scopes(outcome),
            imports: Self::build_import_bindings(outcome, file_path),
        }
    }

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

                // Try receiver-based resolution first
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
    fn canonical_name_for_uid(&self, uid: &str) -> Option<String> {
        self.entries
            .get(*self.by_uid.get(uid)?)
            .map(|e| e.qname.clone().unwrap_or_else(|| e.name.clone()))
    }

    /// Find a symbol by name within a specific file.
    ///
    /// When `prefer_type` is true, prefers Class/Interface/Enum entries — the
    /// typical source of a semantic edge.
    fn find_by_name_in_file(
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
    fn resolve_semantic_target(
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

    // -----------------------------------------------------------------------
    // Cross-file route handler resolution
    // -----------------------------------------------------------------------

    /// Resolve a dotted handler name (e.g. "userCtrl.getUsers", "controllers.users.list")
    /// by tracing through imports to find the target symbol in another module.
    fn resolve_dotted_handler(
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
    fn resolve_handler_global(
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
    fn find_best(&self, name: &str, current_file: &str) -> Option<usize> {
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
    // USES_TYPE derivation
    // -----------------------------------------------------------------------

    /// Try to resolve a type atom name to a symbol UID via the catalog.
    fn resolve_type_atom(&self, atom: &str, file_path: &str) -> Option<String> {
        // 1. Try same-file match (type-like kinds preferred)
        if let Some(idx) = self.find_by_name_in_file(atom, file_path, true) {
            return self.entries[idx].symbol_uid.clone();
        }

        // 2. Try global unique match for type-like symbols
        let lower = atom.to_lowercase();
        if let Some(indices) = self.by_name.get(&lower) {
            let type_matches: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|&i| is_type_like(self.entries[i].kind))
                .collect();

            if type_matches.len() == 1 {
                return self.entries[type_matches[0]].symbol_uid.clone();
            }
        }

        None
    }

    /// Derive USES_TYPE edges from symbol type fields (receiver_type, param_types, return_type).
    ///
    /// For each symbol with type annotations, creates semantic edges to the referenced types.
    /// This avoids needing each parser to explicitly extract type usage relationships.
    pub fn derive_uses_type_edges(&self, file_path: &str, outcome: &mut ParseOutcome) {
        let mut seen: HashSet<(String, String)> = HashSet::new(); // (source_uid, target_name)
        let mut new_edges: Vec<SemanticEdgeRecord> = Vec::new();

        for symbol in &outcome.symbols {
            let source_uid = match symbol.symbol_uid.as_ref() {
                Some(uid) => uid.clone(),
                None => continue,
            };

            // Collect type strings to process
            let type_strings: Vec<&str> = [
                symbol.receiver_type.as_deref(),
                symbol.param_types.as_deref(),
                symbol.return_type.as_deref(),
            ]
            .iter()
            .filter_map(|o| *o)
            .collect();

            for type_str in type_strings {
                for atom in type_atoms(type_str) {
                    let key = (source_uid.clone(), atom.clone());
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.insert(key);

                    // Try to resolve the type atom to a catalog entry
                    let target_uid = self.resolve_type_atom(&atom, file_path);

                    new_edges.push(SemanticEdgeRecord {
                        edge_id: format!(
                            "se-{}:{}:uses_type:{}",
                            file_path, symbol.start_line, atom
                        ),
                        file_path: file_path.to_string(),
                        source_symbol: symbol.name.clone(),
                        source_symbol_uid: Some(source_uid.clone()),
                        target_symbol: atom,
                        target_symbol_uid: target_uid,
                        relation_kind: SemanticRelation::UsesType,
                        line: symbol.start_line,
                        confidence: 0.8, // derived, not declared
                        parser_tier: symbol.parser_tier,
                    });
                }
            }
        }

        if !new_edges.is_empty() {
            tracing::debug!(
                file = file_path,
                count = new_edges.len(),
                "derived USES_TYPE edges from type annotations"
            );
            outcome.semantic_edges.extend(new_edges);
        }
    }
}

impl Default for SymbolCatalog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Resolution heuristics (inspired by codebase-memory-mcp registry.c)
// ---------------------------------------------------------------------------

/// Penalize confidence when multiple candidates exist.
///
/// For 1–3 candidates: no penalty. For 4+: linear decay capped at 3/count.
/// This prevents over-confident resolution when many symbols share the same name.
fn candidate_count_penalty(base: f64, count: usize) -> f64 {
    if count <= 3 {
        base
    } else {
        base * (3.0 / count as f64).min(1.0)
    }
}

/// Check whether a candidate symbol's file is reachable through the current
/// file's import chain.
///
/// Returns true if any import's source module is a dot-prefix of the
/// candidate's module path (or vice versa), indicating the candidate lives
/// in an imported module tree. Unreachable candidates get a 0.5× confidence
/// penalty.
///
/// Uses prefix matching (not substring) to avoid false positives like
/// `"a.b.cd"` matching `"a.b.c"`.
fn is_import_reachable(candidate_file: &str, imports: &[ImportBinding]) -> bool {
    if imports.is_empty() {
        return false;
    }
    let cand_mod = strip_ext_to_dotted(candidate_file);

    for imp in imports {
        let src = strip_ext_to_dotted(&imp.source_module);
        if dotted_prefix_match(&cand_mod, &src) {
            return true;
        }
    }
    false
}

/// Strip file extension and convert path separators to dots.
fn strip_ext_to_dotted(path: &str) -> String {
    let stripped = path
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx")
        .trim_end_matches(".rs")
        .trim_end_matches(".go")
        .trim_end_matches(".java");
    stripped.replace('/', ".")
}

/// Check if `a` is a dot-prefix of `b` (or vice versa).
///
/// "src.utils" is a prefix of "src.utils.helpers" but NOT of "src.utilsXtra".
fn dotted_prefix_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // a is prefix of b: b starts with a followed by '.' or end
    if b.len() > a.len() && b.starts_with(a) && b.as_bytes()[a.len()] == b'.' {
        return true;
    }
    // b is prefix of a
    if a.len() > b.len() && a.starts_with(b) && a.as_bytes()[b.len()] == b'.' {
        return true;
    }
    false
}

/// Count the number of common `/`-separated directory prefix segments between
/// two file paths. Extensions are stripped first so that `a.py` vs `a.ts`
/// in the same directory still count as co-located.
fn common_path_prefix_len(a: &str, b: &str) -> usize {
    let seg_a: Vec<&str> = strip_ext(a).split('/').collect();
    let seg_b: Vec<&str> = strip_ext(b).split('/').collect();
    seg_a
        .iter()
        .zip(seg_b.iter())
        .take_while(|(x, y)| x == y)
        .count()
}

/// Strip common source-file extensions for path comparison.
fn strip_ext(path: &str) -> &str {
    for ext in &[".py", ".ts", ".tsx", ".js", ".jsx", ".rs", ".go", ".java"] {
        if let Some(stripped) = path.strip_suffix(ext) {
            return stripped;
        }
    }
    path
}

/// Among multiple candidate indices, pick the one whose file shares the
/// longest common path prefix with `current_file`.
fn best_by_import_distance(
    entries: &[CatalogEntry],
    candidates: &[usize],
    current_file: &str,
) -> Option<usize> {
    candidates
        .iter()
        .copied()
        .max_by_key(|&idx| common_path_prefix_len(&entries[idx].file_path, current_file))
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Check whether a symbol kind represents a function/method/handler — the kinds
/// that can serve as route handlers.
fn is_handler_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::RouteHandler
            | SymbolKind::Controller
            | SymbolKind::Middleware
            | SymbolKind::Hook
            | SymbolKind::Component
    )
}

/// Pick unique entry from candidates (deduplicated by symbol_id).
fn pick_unique(entries: &[CatalogEntry], candidates: &[usize]) -> Option<usize> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for &idx in candidates {
        if seen.insert(&entries[idx].symbol_id) {
            unique.push(idx);
        }
    }
    if unique.len() == 1 {
        Some(unique[0])
    } else {
        None
    }
}

/// Deduplicate indices by symbol_id.
fn dedup_by_id(entries: &[CatalogEntry], indices: &[usize]) -> Vec<usize> {
    let mut seen = HashSet::new();
    indices
        .iter()
        .copied()
        .filter(|&i| seen.insert(entries[i].symbol_id.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// USES_TYPE helpers
// ---------------------------------------------------------------------------

/// Check whether a symbol kind represents a type definition.
fn is_type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::TypeAlias
    )
}

/// Extract individual type names from a type expression string.
///
/// Handles generics, unions, and container types:
/// - `Promise<User>` -> `["Promise", "User"]`
/// - `Vec<Result<Foo, Err>>` -> `["Vec", "Result", "Foo"]`
/// - `A | B` -> `["A", "B"]`
/// - `Option<T>` -> `["Option"]`
/// - `*http.Client` -> `["http.Client"]`
/// - `&str` -> filtered out (primitive)
/// - `Dict[str, Any]` -> `["Dict"]` (str/Any are primitives)
///
/// Filters out:
/// - Built-in primitives (int, str, bool, float, void, any, None, etc.)
/// - Empty strings
/// - Single-char type params (T, K, V, etc.)
fn type_atoms(raw: &str) -> Vec<String> {
    // Remove pointer/reference prefixes
    let s = raw.trim_start_matches(|c: char| c == '*' || c == '&');

    // Split on delimiters: < > [ ] , | ( ) and whitespace
    let mut atoms = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            '<' | '>' | '[' | ']' | ',' | '|' | '(' | ')' | ' ' => {
                let token = current.trim().to_string();
                if !token.is_empty() {
                    atoms.push(token);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let token = current.trim().to_string();
    if !token.is_empty() {
        atoms.push(token);
    }

    // Filter primitives and single-char type params
    static PRIMITIVES: &[&str] = &[
        "int",
        "str",
        "string",
        "bool",
        "boolean",
        "float",
        "f32",
        "f64",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "isize",
        "char",
        "void",
        "any",
        "none",
        "null",
        "undefined",
        "object",
        "number",
        "never",
        "unknown",
        "byte",
        "short",
        "long",
        "double",
        "self",
        "error",
    ];

    atoms
        .into_iter()
        .filter(|a| {
            let lower = a.to_lowercase();
            // Skip primitives
            if PRIMITIVES.contains(&lower.as_str()) {
                return false;
            }
            // Skip single-char type params (T, K, V, E, etc.)
            if a.len() == 1 && a.chars().next().unwrap().is_ascii_uppercase() {
                return false;
            }
            !a.is_empty()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cc_model::symbol::SymbolKind;
    use cc_model::ParserTier;

    #[allow(clippy::too_many_arguments)]
    fn make_symbol(
        name: &str,
        file: &str,
        uid: Option<&str>,
        kind: SymbolKind,
        container: Option<&str>,
        qname: Option<&str>,
        start_line: u32,
        end_line: u32,
    ) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("sym_{}", name),
            file_path: file.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.8,
            qname: qname.map(String::from),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: uid.map(String::from),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    fn make_simple_symbol(name: &str, file: &str, uid: Option<&str>) -> SymbolRecord {
        make_symbol(
            name,
            file,
            uid,
            SymbolKind::Function,
            None,
            Some(&format!(
                "{}.{}",
                file.replace('/', ".").trim_end_matches(".py"),
                name
            )),
            1,
            10,
        )
    }

    fn make_scope(
        id: &str,
        parent: Option<&str>,
        name: &str,
        file: &str,
        start: u32,
        end: u32,
        bindings: Vec<ScopeBinding>,
    ) -> CatalogScope {
        CatalogScope {
            scope_id: id.to_string(),
            parent_id: parent.map(String::from),
            name: name.to_string(),
            file_path: file.to_string(),
            start_line: start,
            end_line: end,
            bindings,
        }
    }

    // ------------------------------------------------------------------
    // Test 1: scope_chain walks parents correctly
    // ------------------------------------------------------------------
    #[test]
    fn test_scope_chain_walks_parents() {
        let catalog = SymbolCatalog::new();
        let mut scopes = HashMap::new();
        scopes.insert(
            "s1".to_string(),
            make_scope("s1", None, "module", "a.py", 1, 100, vec![]),
        );
        scopes.insert(
            "s2".to_string(),
            make_scope("s2", Some("s1"), "func_a", "a.py", 5, 50, vec![]),
        );
        scopes.insert(
            "s3".to_string(),
            make_scope("s3", Some("s2"), "if_block", "a.py", 10, 30, vec![]),
        );

        let chain = catalog.scope_chain(&scopes, "s3");
        let ids: Vec<&str> = chain.iter().map(|s| s.scope_id.as_str()).collect();
        assert_eq!(ids, vec!["s3", "s2", "s1"]);
    }

    // ------------------------------------------------------------------
    // Test 2: resolve_member_chain resolves "a.b.c"
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_member_chain() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_symbol(
                "MyClass",
                "app.py",
                Some("uid:class"),
                SymbolKind::Class,
                None,
                Some("MyClass"),
                1,
                50,
            ),
            make_symbol(
                "service",
                "app.py",
                Some("uid:svc"),
                SymbolKind::Variable,
                None,
                Some("service"),
                1,
                50,
            ),
            make_symbol(
                "handler",
                "app.py",
                Some("uid:handler"),
                SymbolKind::Method,
                Some("MyClass"),
                Some("MyClass.handler"),
                10,
                20,
            ),
            make_symbol(
                "inner",
                "app.py",
                Some("uid:inner"),
                SymbolKind::Method,
                Some("MyClass.handler"),
                Some("MyClass.handler.inner"),
                12,
                18,
            ),
        ]);

        // Resolve "MyClass.handler.inner"
        let parts = vec!["MyClass", "handler", "inner"];
        let result = catalog.resolve_member_chain(&parts, "app.py");
        assert!(result.is_some());
        assert_eq!(catalog.entry(result.unwrap()).symbol_id, "sym_inner");
    }

    // ------------------------------------------------------------------
    // Test 3: resolve_via_imports follows import binding
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_via_imports() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_symbol(
            "helper",
            "utils.py",
            Some("uid:helper"),
            SymbolKind::Function,
            None,
            Some("helper"),
            1,
            10,
        )]);

        let imports = vec![ImportBinding {
            local_name: "helper".to_string(),
            source_module: "utils.py".to_string(),
            imported_name: Some("helper".to_string()),
            file_path: "app.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_via_imports(&imports, "helper");
        assert!(result.is_some());
        assert_eq!(catalog.entry(result.unwrap()).symbol_id, "sym_helper");
    }

    // ------------------------------------------------------------------
    // Test 4: resolve_name full pipeline
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_name_full_pipeline() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_symbol(
                "local_func",
                "main.py",
                Some("uid:local"),
                SymbolKind::Function,
                None,
                Some("local_func"),
                5,
                15,
            ),
            make_symbol(
                "imported_func",
                "lib.py",
                Some("uid:imported"),
                SymbolKind::Function,
                None,
                Some("imported_func"),
                1,
                10,
            ),
            make_symbol(
                "unique_global",
                "other.py",
                Some("uid:global"),
                SymbolKind::Function,
                None,
                Some("unique_global"),
                1,
                10,
            ),
        ]);

        let mut scopes = HashMap::new();
        scopes.insert(
            "mod".to_string(),
            make_scope(
                "mod",
                None,
                "module",
                "main.py",
                1,
                100,
                vec![ScopeBinding {
                    name: "local_func".to_string(),
                    kind: "function".to_string(),
                    symbol_uid: Some("uid:local".to_string()),
                }],
            ),
        );

        let imports = vec![ImportBinding {
            local_name: "imported_func".to_string(),
            source_module: "lib.py".to_string(),
            imported_name: Some("imported_func".to_string()),
            file_path: "main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        // 1. Scope binding resolution
        let result = catalog.resolve_name("local_func", "main.py", 10, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(catalog.entry(r.catalog_index).symbol_id, "sym_local_func");
        assert_eq!(r.resolution_kind, InternalResKind::ScopeResolved);

        // 2. Import resolution
        let result = catalog.resolve_name("imported_func", "main.py", 10, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(
            catalog.entry(r.catalog_index).symbol_id,
            "sym_imported_func"
        );
        // imported_func is resolved either via same-file or import depending on catalog state
        // In this case it should resolve via imports since it's in lib.py
        assert!(
            r.resolution_kind == InternalResKind::ImportResolved
                || r.resolution_kind == InternalResKind::Heuristic
        );

        // 3. Global unique fallback
        let result = catalog.resolve_name("unique_global", "main.py", 10, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(
            catalog.entry(r.catalog_index).symbol_id,
            "sym_unique_global"
        );
        assert_eq!(r.resolution_kind, InternalResKind::GlobalUnique);
    }

    // ------------------------------------------------------------------
    // Test 5: original test — resolve_prefers_same_file
    // ------------------------------------------------------------------
    #[test]
    fn resolve_prefers_same_file() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_simple_symbol("greet", "a.py", Some("uid:a")),
            make_simple_symbol("greet", "b.py", Some("uid:b")),
        ]);

        let mut outcome = ParseOutcome::default();
        outcome.call_edges.push(cc_model::edge::CallEdgeRecord {
            edge_id: "e1".into(),
            file_path: "a.py".into(),
            callee_symbol: "greet".into(),
            line: 5,
            ..Default::default()
        });

        catalog.resolve_outcome("a.py", &mut outcome);
        assert_eq!(
            outcome.call_edges[0].callee_symbol_uid.as_deref(),
            Some("uid:a")
        );
    }

    // ------------------------------------------------------------------
    // Test 6: scope_distance returns correct hop count
    // ------------------------------------------------------------------
    #[test]
    fn test_scope_distance() {
        let catalog = SymbolCatalog::new();
        let mut scopes = HashMap::new();
        scopes.insert(
            "s1".to_string(),
            make_scope("s1", None, "module", "a.py", 1, 100, vec![]),
        );
        scopes.insert(
            "s2".to_string(),
            make_scope("s2", Some("s1"), "func", "a.py", 5, 50, vec![]),
        );
        scopes.insert(
            "s3".to_string(),
            make_scope("s3", Some("s2"), "block", "a.py", 10, 30, vec![]),
        );

        assert_eq!(catalog.scope_distance(&scopes, "s3", "s3"), Some(0));
        assert_eq!(catalog.scope_distance(&scopes, "s3", "s2"), Some(1));
        assert_eq!(catalog.scope_distance(&scopes, "s3", "s1"), Some(2));
        assert_eq!(catalog.scope_distance(&scopes, "s1", "s3"), None);
    }

    // ------------------------------------------------------------------
    // Test 7: build_alias_map
    // ------------------------------------------------------------------
    #[test]
    fn test_build_alias_map() {
        let imports = vec![
            ImportBinding {
                local_name: "helper".to_string(),
                source_module: "utils.py".to_string(),
                imported_name: Some("do_help".to_string()),
                file_path: "app.py".to_string(),
                is_namespace: false,
                is_default: false,
            },
            ImportBinding {
                local_name: "ns".to_string(),
                source_module: "lib.py".to_string(),
                imported_name: None,
                file_path: "app.py".to_string(),
                is_namespace: true,
                is_default: false,
            },
        ];

        let map = SymbolCatalog::build_alias_map(&imports);
        assert_eq!(map.get("helper").unwrap(), "utils.py:do_help");
        assert_eq!(map.get("ns").unwrap(), "lib.py:ns");
    }

    // ------------------------------------------------------------------
    // Test 8: classify_call_kind
    // ------------------------------------------------------------------
    #[test]
    fn test_classify_call_kind() {
        let imports = vec![ImportBinding {
            local_name: "ext".to_string(),
            source_module: "lib.py".to_string(),
            imported_name: Some("ext".to_string()),
            file_path: "app.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        assert_eq!(
            SymbolCatalog::classify_call_kind("obj.method", &imports),
            "method"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("obj.ClassName", &imports),
            "constructor"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("ext", &imports),
            "imported"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("local_fn", &imports),
            "local"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("MyClass", &imports),
            "constructor"
        );
    }

    // ------------------------------------------------------------------
    // Test 9: candidate_count_penalty
    // ------------------------------------------------------------------
    #[test]
    fn test_candidate_count_penalty() {
        // 1-3 candidates: no penalty
        assert_eq!(candidate_count_penalty(0.5, 1), 0.5);
        assert_eq!(candidate_count_penalty(0.5, 3), 0.5);
        // 4+: linear decay
        let p4 = candidate_count_penalty(0.5, 4);
        assert!((p4 - 0.375).abs() < 1e-6); // 0.5 * 3/4
        let p6 = candidate_count_penalty(0.5, 6);
        assert!((p6 - 0.25).abs() < 1e-6); // 0.5 * 3/6
        let p10 = candidate_count_penalty(0.5, 10);
        assert!((p10 - 0.15).abs() < 1e-6); // 0.5 * 3/10
    }

    // ------------------------------------------------------------------
    // Test 10: import reachability
    // ------------------------------------------------------------------
    #[test]
    fn test_import_reachable() {
        let imports = vec![ImportBinding {
            local_name: "helper".to_string(),
            source_module: "src/utils/helpers.py".to_string(),
            imported_name: Some("helper".to_string()),
            file_path: "app.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        // Reachable: candidate is in the imported module tree
        assert!(is_import_reachable("src/utils/helpers.py", &imports));
        // Reachable: candidate is a parent/sibling path
        assert!(is_import_reachable("src/utils.py", &imports));
        // Unreachable: completely different module
        assert!(!is_import_reachable("tests/conftest.py", &imports));
        // Empty imports → always unreachable
        assert!(!is_import_reachable("anything.py", &[]));
    }

    // ------------------------------------------------------------------
    // Test 11: common_path_prefix_len
    // ------------------------------------------------------------------
    #[test]
    fn test_common_path_prefix_len() {
        // "src/pkg/module" vs "src/pkg/other" → 2 common segments ("src", "pkg")
        assert_eq!(
            common_path_prefix_len("src/pkg/module.py", "src/pkg/other.py"),
            2
        );
        assert_eq!(common_path_prefix_len("src/a.py", "tests/b.py"), 0);
        // Same file → all segments match ("src", "pkg", "mod")
        assert_eq!(
            common_path_prefix_len("src/pkg/mod.py", "src/pkg/mod.py"),
            3
        );
        // Cross-language same dir still matches directories
        assert_eq!(
            common_path_prefix_len("src/pkg/mod.py", "src/pkg/mod.ts"),
            3
        );
    }

    // ------------------------------------------------------------------
    // Test 12: best_by_import_distance picks closest path
    // ------------------------------------------------------------------
    #[test]
    fn test_best_by_import_distance() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_simple_symbol("helper", "src/utils/helpers.py", Some("uid:near")),
            make_simple_symbol("helper", "vendor/ext/helpers.py", Some("uid:far")),
        ]);

        let candidates = vec![0, 1];
        let result = best_by_import_distance(&catalog.entries, &candidates, "src/utils/main.py");
        assert_eq!(result, Some(0)); // src/utils is closer
    }

    // ------------------------------------------------------------------
    // Test 13: fuzzy single resolution with import-unreachable penalty
    // ------------------------------------------------------------------
    #[test]
    fn test_fuzzy_single_unreachable_penalty() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_symbol(
            "rare_func",
            "vendor/deep/module.py",
            Some("uid:rare"),
            SymbolKind::Function,
            None,
            Some("vendor.deep.module.rare_func"),
            1,
            10,
        )]);

        let scopes = HashMap::new();
        let imports = vec![ImportBinding {
            local_name: "utils".to_string(),
            source_module: "src/utils.py".to_string(),
            imported_name: Some("utils".to_string()),
            file_path: "main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_name("rare_func", "main.py", 5, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.resolution_kind, InternalResKind::GlobalUnique);
        // 0.75 * 0.6 = 0.45 (import-unreachable penalty)
        assert!((r.confidence - 0.45).abs() < 1e-6);
    }

    // ------------------------------------------------------------------
    // Test 14: fuzzy multi resolution with candidate penalty
    // ------------------------------------------------------------------
    #[test]
    fn test_fuzzy_multi_candidate_penalty() {
        let mut catalog = SymbolCatalog::new();
        // 4 candidates with the same name in different files — use distinct symbol_ids
        let mut syms: Vec<SymbolRecord> = vec![
            make_symbol(
                "process",
                "src/a.py",
                Some("uid:a"),
                SymbolKind::Function,
                None,
                Some("src.a.process"),
                1,
                10,
            ),
            make_symbol(
                "process",
                "src/b.py",
                Some("uid:b"),
                SymbolKind::Function,
                None,
                Some("src.b.process"),
                1,
                10,
            ),
            make_symbol(
                "process",
                "lib/c.py",
                Some("uid:c"),
                SymbolKind::Function,
                None,
                Some("lib.c.process"),
                1,
                10,
            ),
            make_symbol(
                "process",
                "vendor/d.py",
                Some("uid:d"),
                SymbolKind::Function,
                None,
                Some("vendor.d.process"),
                1,
                10,
            ),
        ];
        // Override symbol_ids to be unique (make_symbol uses name only)
        for (i, s) in syms.iter_mut().enumerate() {
            s.symbol_id = format!("sym_process_{}", i);
        }
        catalog.add_symbols(&syms);

        let scopes = HashMap::new();
        let imports = vec![ImportBinding {
            local_name: "a_mod".to_string(),
            source_module: "src/a.py".to_string(),
            imported_name: None,
            file_path: "main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_name("process", "src/main.py", 5, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.resolution_kind, InternalResKind::FuzzyMulti);
        // Should pick src/a.py (the only reachable candidate)
        assert_eq!(catalog.entries[r.catalog_index].file_path, "src/a.py");
        // Confidence: FuzzySingle base(0.40) * penalty(3/4) = 0.30
        assert!((r.confidence - 0.30).abs() < 1e-6);
    }

    // ------------------------------------------------------------------
    // type_atoms unit tests
    // ------------------------------------------------------------------

    #[test]
    fn test_type_atoms_simple() {
        assert_eq!(type_atoms("User"), vec!["User"]);
        assert_eq!(type_atoms("MyStruct"), vec!["MyStruct"]);
    }

    #[test]
    fn test_type_atoms_generics() {
        assert_eq!(type_atoms("Promise<User>"), vec!["Promise", "User"]);
        assert_eq!(
            type_atoms("Vec<Result<Foo, Bar>>"),
            vec!["Vec", "Result", "Foo", "Bar"]
        );
        // Single-char type param T is filtered
        assert_eq!(type_atoms("Option<T>"), vec!["Option"]);
    }

    #[test]
    fn test_type_atoms_union() {
        // Single-char uppercase letters are filtered as type params
        assert!(type_atoms("A | B").is_empty());
        // Real type names survive
        assert_eq!(type_atoms("User | Admin"), vec!["User", "Admin"]);
    }

    #[test]
    fn test_type_atoms_primitives_filtered() {
        assert!(type_atoms("int").is_empty());
        assert!(type_atoms("str").is_empty());
        assert!(type_atoms("bool").is_empty());
        assert!(type_atoms("void").is_empty());
        assert!(type_atoms("f64").is_empty());
        assert!(type_atoms("usize").is_empty());
    }

    #[test]
    fn test_type_atoms_single_char_type_params() {
        assert!(type_atoms("T").is_empty());
        assert!(type_atoms("K").is_empty());
        assert!(type_atoms("V").is_empty());
        assert!(type_atoms("E").is_empty());
    }

    #[test]
    fn test_type_atoms_pointer_reference() {
        assert_eq!(type_atoms("*http.Client"), vec!["http.Client"]);
        assert!(type_atoms("&str").is_empty()); // str is primitive
        assert_eq!(type_atoms("&MyType"), vec!["MyType"]);
    }

    #[test]
    fn test_type_atoms_python_brackets() {
        assert_eq!(type_atoms("Dict[str, Any]"), vec!["Dict"]);
        assert_eq!(
            type_atoms("List[Tuple[Foo, Bar]]"),
            vec!["List", "Tuple", "Foo", "Bar"]
        );
    }

    #[test]
    fn test_type_atoms_mixed_delimiters() {
        assert_eq!(
            type_atoms("Map<String, List<User>>"),
            vec!["Map", "List", "User"] // String is primitive
        );
    }

    // ------------------------------------------------------------------
    // is_type_like tests
    // ------------------------------------------------------------------

    #[test]
    fn test_is_type_like() {
        assert!(is_type_like(SymbolKind::Class));
        assert!(is_type_like(SymbolKind::Interface));
        assert!(is_type_like(SymbolKind::Enum));
        assert!(is_type_like(SymbolKind::TypeAlias));
        assert!(!is_type_like(SymbolKind::Function));
        assert!(!is_type_like(SymbolKind::Method));
        assert!(!is_type_like(SymbolKind::Variable));
    }

    // ------------------------------------------------------------------
    // derive_uses_type_edges tests
    // ------------------------------------------------------------------

    fn make_typed_symbol(
        name: &str,
        file: &str,
        uid: &str,
        kind: SymbolKind,
        receiver_type: Option<&str>,
        param_types: Option<&str>,
        return_type: Option<&str>,
    ) -> SymbolRecord {
        let mut sym = make_symbol(name, file, Some(uid), kind, None, None, 10, 20);
        sym.receiver_type = receiver_type.map(String::from);
        sym.param_types = param_types.map(String::from);
        sym.return_type = return_type.map(String::from);
        sym
    }

    #[test]
    fn test_derive_uses_type_receiver() {
        let mut catalog = SymbolCatalog::new();

        let class_sym = make_symbol(
            "MyStruct",
            "src/main.rs",
            Some("uid:mystruct"),
            SymbolKind::Class,
            None,
            None,
            1,
            5,
        );
        let method_sym = make_typed_symbol(
            "do_work",
            "src/main.rs",
            "uid:do_work",
            SymbolKind::Method,
            Some("MyStruct"),
            None,
            None,
        );

        catalog.add_symbols(&[class_sym, method_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(method_sym);

        catalog.derive_uses_type_edges("src/main.rs", &mut outcome);

        assert_eq!(outcome.semantic_edges.len(), 1);
        let edge = &outcome.semantic_edges[0];
        assert_eq!(edge.relation_kind, SemanticRelation::UsesType);
        assert_eq!(edge.source_symbol, "do_work");
        assert_eq!(edge.target_symbol, "MyStruct");
        assert_eq!(edge.target_symbol_uid, Some("uid:mystruct".to_string()));
        assert!((edge.confidence - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_derive_uses_type_params() {
        let mut catalog = SymbolCatalog::new();

        let user_sym = make_symbol(
            "User",
            "src/models.rs",
            Some("uid:user"),
            SymbolKind::Class,
            None,
            None,
            1,
            10,
        );
        let config_sym = make_symbol(
            "Config",
            "src/config.rs",
            Some("uid:config"),
            SymbolKind::Class,
            None,
            None,
            1,
            10,
        );
        let func_sym = make_typed_symbol(
            "process",
            "src/handler.rs",
            "uid:process",
            SymbolKind::Function,
            None,
            Some("User, int, Config"),
            None,
        );

        catalog.add_symbols(&[user_sym, config_sym, func_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/handler.rs", &mut outcome);

        // Should have edges for User and Config, but not int (primitive)
        assert_eq!(outcome.semantic_edges.len(), 2);
        let targets: Vec<&str> = outcome
            .semantic_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"User"));
        assert!(targets.contains(&"Config"));
    }

    #[test]
    fn test_derive_uses_type_return() {
        let mut catalog = SymbolCatalog::new();

        let result_sym = make_symbol(
            "Result",
            "src/types.rs",
            Some("uid:result"),
            SymbolKind::Enum,
            None,
            None,
            1,
            10,
        );
        let func_sym = make_typed_symbol(
            "fetch",
            "src/api.rs",
            "uid:fetch",
            SymbolKind::Function,
            None,
            None,
            Some("Result<User>"),
        );

        catalog.add_symbols(&[result_sym, func_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/api.rs", &mut outcome);

        // Should have edges for Result and User
        assert_eq!(outcome.semantic_edges.len(), 2);
        let targets: Vec<&str> = outcome
            .semantic_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"Result"));
        assert!(targets.contains(&"User"));
    }

    #[test]
    fn test_derive_uses_type_generic_split() {
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_typed_symbol(
            "transform",
            "src/lib.rs",
            "uid:transform",
            SymbolKind::Function,
            None,
            Some("Vec<Foo>"),
            Some("HashMap<Bar, Baz>"),
        );
        catalog.add_symbols(&[func_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/lib.rs", &mut outcome);

        let targets: Vec<&str> = outcome
            .semantic_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"Vec"));
        assert!(targets.contains(&"Foo"));
        assert!(targets.contains(&"HashMap"));
        assert!(targets.contains(&"Bar"));
        assert!(targets.contains(&"Baz"));
        assert_eq!(outcome.semantic_edges.len(), 5);
    }

    #[test]
    fn test_derive_uses_type_dedup() {
        // Same type appears in both param_types and return_type — should only get one edge
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_typed_symbol(
            "roundtrip",
            "src/lib.rs",
            "uid:roundtrip",
            SymbolKind::Function,
            None,
            Some("User"),
            Some("User"),
        );
        catalog.add_symbols(&[func_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/lib.rs", &mut outcome);

        assert_eq!(outcome.semantic_edges.len(), 1);
        assert_eq!(outcome.semantic_edges[0].target_symbol, "User");
    }

    #[test]
    fn test_derive_uses_type_no_uid_symbol_skipped() {
        // Symbols without symbol_uid should be skipped
        let mut catalog = SymbolCatalog::new();

        let mut func_sym = make_typed_symbol(
            "nouid",
            "src/lib.rs",
            "uid:nouid",
            SymbolKind::Function,
            None,
            Some("Foo"),
            None,
        );
        func_sym.symbol_uid = None; // remove UID
        catalog.add_symbols(&[func_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/lib.rs", &mut outcome);

        assert!(outcome.semantic_edges.is_empty());
    }

    #[test]
    fn test_derive_uses_type_primitives_only() {
        // All types are primitives — no edges should be generated
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_typed_symbol(
            "add",
            "src/math.rs",
            "uid:add",
            SymbolKind::Function,
            None,
            Some("int, float, bool"),
            Some("f64"),
        );
        catalog.add_symbols(&[func_sym.clone()]);

        let mut outcome = ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/math.rs", &mut outcome);

        assert!(outcome.semantic_edges.is_empty());
    }

    // ------------------------------------------------------------------
    // Route handler resolution tests
    // ------------------------------------------------------------------

    fn make_route_edge(
        file: &str,
        route: &str,
        handler: Option<&str>,
        line: u32,
    ) -> cc_model::edge::RouteEdgeRecord {
        cc_model::edge::RouteEdgeRecord {
            edge_id: format!("route_{}", route),
            file_path: file.to_string(),
            route_path: route.to_string(),
            handler_name: handler.map(String::from),
            method: Some("GET".to_string()),
            line,
            start_col: 0,
            end_line: None,
            end_col: 0,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: None,
            router_symbol_uid: None,
            framework: None,
            route_kind: None,
            confidence: 0.8,
            parser_tier: ParserTier::TreeSitter,
        }
    }

    #[test]
    fn test_route_dotted_handler_via_import() {
        // Scenario: import { userCtrl } from './controllers/user'
        //           app.get('/users', userCtrl.getUsers)
        let mut catalog = SymbolCatalog::new();

        // Controller file has a class with a method
        let class_sym = make_symbol(
            "UserController",
            "src/controllers/user.ts",
            Some("uid:UserController"),
            SymbolKind::Class,
            None,
            Some("UserController"),
            1,
            50,
        );
        let method_sym = make_symbol(
            "getUsers",
            "src/controllers/user.ts",
            Some("uid:getUsers"),
            SymbolKind::Method,
            Some("UserController"),
            Some("UserController.getUsers"),
            10,
            20,
        );
        catalog.add_symbols(&[class_sym, method_sym]);

        let imports = vec![ImportBinding {
            local_name: "userCtrl".to_string(),
            source_module: "src/controllers/user.ts".to_string(),
            imported_name: Some("UserController".to_string()),
            file_path: "src/routes.ts".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let scopes = HashMap::new();
        let result =
            catalog.resolve_dotted_handler("userCtrl.getUsers", "src/routes.ts", &scopes, &imports);
        assert!(result.is_some(), "Should resolve dotted handler via import");
        let idx = result.unwrap();
        assert_eq!(catalog.entries[idx].name, "getUsers");
        assert_eq!(
            catalog.entries[idx].symbol_uid.as_deref(),
            Some("uid:getUsers")
        );
    }

    #[test]
    fn test_route_dotted_handler_via_qname() {
        // Scenario: controllers.users.list — resolved via qualified name
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_symbol(
            "list",
            "src/controllers/users.ts",
            Some("uid:list"),
            SymbolKind::Function,
            Some("users"),
            Some("controllers.users.list"),
            5,
            15,
        );
        catalog.add_symbols(&[func_sym]);

        let scopes = HashMap::new();
        let imports = vec![];
        let result = catalog.resolve_dotted_handler(
            "controllers.users.list",
            "src/app.ts",
            &scopes,
            &imports,
        );
        assert!(result.is_some(), "Should resolve dotted handler via qname");
        assert_eq!(catalog.entries[result.unwrap()].name, "list");
    }

    #[test]
    fn test_route_handler_global_prefers_imported() {
        // Scenario: handler "getUsers" exists in two files; one is imported
        let mut catalog = SymbolCatalog::new();

        let sym_a = make_symbol(
            "getUsers",
            "src/controllers/user.ts",
            Some("uid:getUsers_ctrl"),
            SymbolKind::Function,
            None,
            Some("user.getUsers"),
            1,
            10,
        );
        let sym_b = make_symbol(
            "getUsers",
            "src/test/mock.ts",
            Some("uid:getUsers_mock"),
            SymbolKind::Function,
            None,
            Some("mock.getUsers"),
            1,
            10,
        );
        catalog.add_symbols(&[sym_a, sym_b]);

        let imports = vec![ImportBinding {
            local_name: "userCtrl".to_string(),
            source_module: "src/controllers/user.ts".to_string(),
            imported_name: Some("UserController".to_string()),
            file_path: "src/routes.ts".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_handler_global("getUsers", "src/routes.ts", &imports);
        assert!(result.is_some(), "Should resolve via global handler lookup");
        assert_eq!(
            catalog.entries[result.unwrap()].symbol_uid.as_deref(),
            Some("uid:getUsers_ctrl"),
            "Should prefer the handler in the imported file"
        );
    }

    #[test]
    fn test_route_handler_global_unique_function() {
        // Scenario: handler name is globally unique — resolves directly
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_symbol(
            "handleLogin",
            "src/auth/handler.ts",
            Some("uid:handleLogin"),
            SymbolKind::Function,
            None,
            Some("handler.handleLogin"),
            1,
            10,
        );
        catalog.add_symbols(&[func_sym]);

        let imports = vec![];
        let result = catalog.resolve_handler_global("handleLogin", "src/routes.ts", &imports);
        assert!(result.is_some());
        assert_eq!(
            catalog.entries[result.unwrap()].symbol_uid.as_deref(),
            Some("uid:handleLogin")
        );
    }

    #[test]
    fn test_route_handler_global_skips_non_function() {
        // Scenario: "User" exists as a class — should not resolve as handler
        let mut catalog = SymbolCatalog::new();

        let class_sym = make_symbol(
            "User",
            "src/models/user.ts",
            Some("uid:User"),
            SymbolKind::Class,
            None,
            Some("User"),
            1,
            50,
        );
        catalog.add_symbols(&[class_sym]);

        let imports = vec![];
        let result = catalog.resolve_handler_global("User", "src/routes.ts", &imports);
        assert!(result.is_none(), "Should not resolve class as handler");
    }

    #[test]
    fn test_route_resolve_outcome_three_tier() {
        // Integration test: verify the full three-tier pipeline in resolve_outcome
        let mut catalog = SymbolCatalog::new();

        // Cross-file handler (dotted)
        let ctrl_class = make_symbol(
            "AuthController",
            "src/controllers/auth.ts",
            Some("uid:AuthController"),
            SymbolKind::Class,
            None,
            Some("AuthController"),
            1,
            100,
        );
        let ctrl_method = make_symbol(
            "login",
            "src/controllers/auth.ts",
            Some("uid:login"),
            SymbolKind::Method,
            Some("AuthController"),
            Some("AuthController.login"),
            10,
            30,
        );
        // Simple name handler (tier 2/3)
        let handler_func = make_symbol(
            "healthCheck",
            "src/handlers/health.ts",
            Some("uid:healthCheck"),
            SymbolKind::Function,
            None,
            Some("health.healthCheck"),
            1,
            10,
        );

        catalog.add_symbols(&[ctrl_class, ctrl_method, handler_func]);

        let mut outcome = ParseOutcome::default();
        // Import in the route file
        outcome.imports.push(cc_model::edge::ImportRecord {
            import_string: "./controllers/auth".to_string(),
            imported_name: Some("AuthController".to_string()),
            alias: Some("authCtrl".to_string()),
            resolved_path: Some("src/controllers/auth.ts".to_string()),
            file_path: "src/routes.ts".to_string(),
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        });

        // Route 1: dotted handler (tier 1)
        outcome.route_edges.push(make_route_edge(
            "src/routes.ts",
            "/login",
            Some("authCtrl.login"),
            5,
        ));

        // Route 2: simple cross-file handler (tier 3)
        outcome.route_edges.push(make_route_edge(
            "src/routes.ts",
            "/health",
            Some("healthCheck"),
            10,
        ));

        catalog.resolve_outcome("src/routes.ts", &mut outcome);

        // Verify route 1 resolved to the method
        assert_eq!(
            outcome.route_edges[0].handler_symbol_uid.as_deref(),
            Some("uid:login"),
            "Dotted handler should resolve via tier 1"
        );

        // Verify route 2 resolved to the function
        assert_eq!(
            outcome.route_edges[1].handler_symbol_uid.as_deref(),
            Some("uid:healthCheck"),
            "Simple handler should resolve via tier 2 or 3"
        );
    }

    #[test]
    fn test_route_already_resolved_skipped() {
        // Routes with handler_symbol_id already set should be skipped
        let mut catalog = SymbolCatalog::new();
        let func = make_simple_symbol("handler", "src/app.ts", Some("uid:handler"));
        catalog.add_symbols(&[func]);

        let mut outcome = ParseOutcome::default();
        let mut route = make_route_edge("src/app.ts", "/test", Some("handler"), 5);
        route.handler_symbol_id = Some("existing_id".to_string());
        route.handler_symbol_uid = Some("existing_uid".to_string());
        outcome.route_edges.push(route);

        catalog.resolve_outcome("src/app.ts", &mut outcome);

        // Should remain unchanged
        assert_eq!(
            outcome.route_edges[0].handler_symbol_uid.as_deref(),
            Some("existing_uid")
        );
    }

    #[test]
    fn test_route_namespace_import_dotted_handler() {
        // Scenario: import * as controllers from './controllers'
        //           app.get('/users', controllers.getUsers)
        let mut catalog = SymbolCatalog::new();

        // Create the symbol with an export_name so the namespace import can find it
        let mut func_sym = make_symbol(
            "getUsers",
            "src/controllers/index.ts",
            Some("uid:getUsers"),
            SymbolKind::Function,
            None,
            Some("controllers.getUsers"),
            5,
            15,
        );
        func_sym.export_name = Some("getUsers".to_string());
        catalog.add_symbols(&[func_sym]);

        let imports = vec![ImportBinding {
            local_name: "controllers".to_string(),
            source_module: "src/controllers/index.ts".to_string(),
            imported_name: None,
            file_path: "src/routes.ts".to_string(),
            is_namespace: true,
            is_default: false,
        }];

        let scopes = HashMap::new();
        let result = catalog.resolve_dotted_handler(
            "controllers.getUsers",
            "src/routes.ts",
            &scopes,
            &imports,
        );
        assert!(
            result.is_some(),
            "Should resolve namespace import dotted handler"
        );
        assert_eq!(
            catalog.entries[result.unwrap()].symbol_uid.as_deref(),
            Some("uid:getUsers")
        );
    }
}

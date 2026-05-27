//! SymbolCatalog struct definition and construction/lookup methods.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Mutex;

use lru::LruCache;

use cc_model::parse::ParseOutcome;
use cc_model::symbol::{SymbolKind, SymbolRecord};

use crate::type_catalog::TypeCatalog;

use super::types::*;

/// Cross-file symbol directory used during indexing to resolve references.
pub struct SymbolCatalog {
    pub(in crate::resolver) entries: Vec<CatalogEntry>,
    pub(in crate::resolver) by_name: HashMap<String, Vec<usize>>,
    pub(in crate::resolver) by_uid: HashMap<String, usize>,
    pub(in crate::resolver) by_qname: HashMap<String, Vec<usize>>,
    pub(in crate::resolver) by_id: HashMap<String, Vec<usize>>,
    pub(in crate::resolver) by_file: HashMap<String, Vec<usize>>,
    pub(in crate::resolver) by_export: HashMap<(String, String), Vec<usize>>,
    /// Lightweight type catalog for method dispatch resolution.
    pub(in crate::resolver) type_catalog: Option<TypeCatalog>,
    /// LRU cache for `resolve_name` results to avoid redundant resolution.
    pub(in crate::resolver) resolve_cache: Mutex<LruCache<ResolveKey, Option<ResolveResult>>>,
}

impl SymbolCatalog {
    pub fn new() -> Self {
        let cache_size = std::env::var("CODECORTEX_RESOLVER_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(8192);
        Self {
            entries: Vec::new(),
            by_name: HashMap::new(),
            by_uid: HashMap::new(),
            by_qname: HashMap::new(),
            by_id: HashMap::new(),
            by_file: HashMap::new(),
            by_export: HashMap::new(),
            type_catalog: None,
            resolve_cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::new(8192).unwrap()),
            )),
        }
    }

    /// Clear the resolve_name LRU cache.
    pub fn clear_resolve_cache(&self) {
        if let Ok(mut cache) = self.resolve_cache.lock() {
            cache.clear();
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

    /// Feed type assignment records into the TypeCatalog for variable type inference.
    ///
    /// Call this after `build_type_catalog`, passing type_assigns from each
    /// parsed file's `ParseOutcome`.
    pub fn add_type_assigns(&mut self, assigns: &[cc_model::type_assign::TypeAssignRecord]) {
        if let Some(ref mut tc) = self.type_catalog {
            tc.add_type_assigns(assigns);
        }
    }

    /// Convenience: feed type_assigns from all write units into the TypeCatalog.
    pub fn add_type_assigns_from_outcomes(
        &mut self,
        write_units: &[cc_db::index_db::FileWriteUnit],
    ) {
        if self.type_catalog.is_none() {
            return;
        }
        for unit in write_units {
            if !unit.outcome.type_assigns.is_empty() {
                self.add_type_assigns(&unit.outcome.type_assigns);
            }
        }
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
    pub(in crate::resolver) fn entry(&self, idx: usize) -> &CatalogEntry {
        &self.entries[idx]
    }

    /// Find entries in the same file matching a name (case-insensitive).
    pub(in crate::resolver) fn same_file_named(&self, file_path: &str, name: &str) -> Vec<usize> {
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
    pub(in crate::resolver) fn same_file_qname(&self, file_path: &str, qname: &str) -> Vec<usize> {
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
    pub(in crate::resolver) fn exported(&self, file_path: &str, export_name: &str) -> Vec<usize> {
        self.by_export
            .get(&(file_path.to_string(), export_name.to_lowercase()))
            .cloned()
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub(in crate::resolver) fn find_by_id(&self, symbol_id: &str) -> Option<usize> {
        self.by_id.get(symbol_id).and_then(|v| v.first().copied())
    }

    pub(in crate::resolver) fn find_by_uid(&self, uid: &str) -> Option<usize> {
        self.by_uid.get(uid).copied()
    }

    /// Walk upward from `container` qname to find the owning class.
    pub(in crate::resolver) fn owner_class_qname(&self, file_path: &str, container: Option<&str>) -> Option<String> {
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
    // Alias map and call classification (static methods)
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
    // Build helpers from ParseOutcome (static methods)
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
}

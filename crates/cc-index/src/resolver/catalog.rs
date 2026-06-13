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
    /// leaf segment (last `.`-token, lowercase) of each qname -> indices.
    /// Suffix resolution (`try_suffix_match`) only ever matches qnames whose
    /// final segment equals the query's final segment, so this index replaces
    /// the per-call O(distinct qnames) scan of `by_qname` with an O(1) probe of
    /// the leaf bucket — the difference between O(N²) and O(N·k̄) on corpora
    /// where many files share a method/leaf name.
    pub(in crate::resolver) by_qname_leaf: HashMap<String, Vec<usize>>,
    pub(in crate::resolver) by_file: HashMap<String, Vec<usize>>,
    pub(in crate::resolver) by_export: HashMap<String, HashMap<String, Vec<usize>>>,
    /// file_path -> name_lowercase -> Vec<usize>: nested index for same-file name lookup.
    pub(in crate::resolver) by_file_name: HashMap<String, HashMap<String, Vec<usize>>>,
    /// file_path -> qname_lowercase -> Vec<usize>: nested index for same-file qname lookup.
    pub(in crate::resolver) by_file_qname: HashMap<String, HashMap<String, Vec<usize>>>,
    /// Lightweight type catalog for method dispatch resolution.
    pub(in crate::resolver) type_catalog: Option<TypeCatalog>,
    /// LRU cache for `resolve_name` results to avoid redundant resolution.
    pub(in crate::resolver) resolve_cache: Mutex<LruCache<u64, Option<ResolveResult>>>,
    /// Above this many same-named candidates, name-only resolution
    /// (global-unique / fuzzy import-distance, and the `find_best` fallback)
    /// is treated as non-resolvable: a name shared by hundreds of symbols —
    /// typically a function-local variable the parsers also surface globally
    /// (`left`, `value`, …) — cannot be disambiguated by path heuristics, and
    /// scanning the whole bucket per reference is the dominant cold-build
    /// O(N²) cost. Bucket lookups at or below this stay exact.
    pub(in crate::resolver) max_fuzzy_pool: usize,
}

impl SymbolCatalog {
    pub fn new() -> Self {
        let cache_size = std::env::var("CODECORTEX_RESOLVER_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(8192);
        let max_fuzzy_pool = std::env::var("CODECORTEX_RESOLVER_MAX_POOL")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(256);
        Self {
            entries: Vec::new(),
            by_name: HashMap::new(),
            by_uid: HashMap::new(),
            by_qname: HashMap::new(),
            by_qname_leaf: HashMap::new(),
            by_file: HashMap::new(),
            by_export: HashMap::new(),
            by_file_name: HashMap::new(),
            by_file_qname: HashMap::new(),
            type_catalog: None,
            resolve_cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::new(8192).unwrap()),
            )),
            max_fuzzy_pool,
        }
    }

    /// Clear the resolve_name LRU cache.
    pub fn clear_resolve_cache(&self) {
        if let Ok(mut cache) = self.resolve_cache.lock() {
            cache.clear();
        }
    }

    /// Build (or rebuild) the TypeCatalog from the given symbols (any
    /// iterable of references — callers can chain persisted and batch
    /// symbols without materializing an owned concatenation).
    ///
    /// Call this after all symbols have been registered via `add_symbols`,
    /// typically right before resolving call-edges.
    pub fn build_type_catalog<'a, I>(&mut self, all_symbols: I)
    where
        I: IntoIterator<Item = &'a SymbolRecord>,
    {
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
        // Pre-size the symbol-cardinality structures. The single-file
        // incremental path rebuilds the catalog from every persisted symbol, so
        // the first `add_symbols` call seeds hundreds of thousands of rows; the
        // `entries` Vec holds a wide `CatalogEntry` each, and growing it from
        // zero reallocates ~log2(n) times copying every prior element. The
        // file-keyed maps stay unreserved (their cardinality is ~file count, so
        // reserving `n` would over-allocate several-fold).
        let n = symbols.len();
        self.entries.reserve(n);
        self.by_uid.reserve(n);
        self.by_name.reserve(n);
        self.by_qname.reserve(n);
        self.by_qname_leaf.reserve(n);
        for sym in symbols {
            let idx = self.entries.len();
            let name_lower = sym.name.to_lowercase();
            let qname_lower = sym.qname.as_ref().map(|q| q.to_lowercase());
            let entry = CatalogEntry {
                symbol_id: sym.symbol_id.clone(),
                symbol_uid: sym.symbol_uid.clone(),
                name: sym.name.clone(),
                file_path: sym.file_path.clone(),
                kind: sym.kind,
                container: sym.container.clone(),
                qname: sym.qname.clone(),
                is_default_export: sym.is_default_export,
                start_line: sym.start_line,
                end_line: sym.end_line,
                scope_id: sym.scope_id.clone(),
            };
            self.entries.push(entry);

            // by_name (lowercase)
            self.by_name
                .entry(name_lower.clone())
                .or_default()
                .push(idx);

            // by_uid
            if let Some(ref uid) = sym.symbol_uid {
                self.by_uid.insert(uid.clone(), idx);
            }

            // by_qname (lowercase) + leaf-segment index for suffix lookup
            if let Some(ref ql) = qname_lower {
                self.by_qname.entry(ql.clone()).or_default().push(idx);
                let leaf = ql.rsplit('.').next().unwrap_or(ql.as_str());
                self.by_qname_leaf
                    .entry(leaf.to_string())
                    .or_default()
                    .push(idx);
            }

            // by_file
            self.by_file
                .entry(sym.file_path.clone())
                .or_default()
                .push(idx);

            // by_file_name (nested: file_path -> name_lowercase -> indices)
            self.by_file_name
                .entry(sym.file_path.clone())
                .or_default()
                .entry(name_lower.clone())
                .or_default()
                .push(idx);

            // by_file_qname (nested: file_path -> qname_lowercase -> indices)
            if let Some(ref ql) = qname_lower {
                self.by_file_qname
                    .entry(sym.file_path.clone())
                    .or_default()
                    .entry(ql.clone())
                    .or_default()
                    .push(idx);
            }

            // by_export: export_name, name, and "default" for default exports
            let mut export_names: HashSet<String> = HashSet::new();
            export_names.insert(name_lower);
            if let Some(ref en) = sym.export_name {
                export_names.insert(en.to_lowercase());
            }
            if sym.is_default_export {
                export_names.insert("default".to_string());
            }
            for en in export_names {
                self.by_export
                    .entry(sym.file_path.clone())
                    .or_default()
                    .entry(en)
                    .or_default()
                    .push(idx);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Lookup helpers
    // -----------------------------------------------------------------------

    #[cfg(test)]
    pub(in crate::resolver) fn entry(&self, idx: usize) -> &CatalogEntry {
        &self.entries[idx]
    }

    /// Find entries in the same file matching a name (case-insensitive).
    ///
    /// Matches against both `name` and `qname` (same semantics as before),
    /// but uses composite HashMap indices instead of O(n) linear scan.
    pub(in crate::resolver) fn same_file_named(&self, file_path: &str, name: &str) -> Vec<usize> {
        let needle = name.to_lowercase();

        // Two-level lookup without allocating a tuple key
        let name_hits = self
            .by_file_name
            .get(file_path)
            .and_then(|m| m.get(&needle));
        let qname_hits = self
            .by_file_qname
            .get(file_path)
            .and_then(|m| m.get(&needle));

        match (name_hits, qname_hits) {
            (None, None) => Vec::new(),
            (Some(v), None) => v.clone(),
            (None, Some(v)) => v.clone(),
            (Some(a), Some(b)) => {
                // Merge and deduplicate; both vecs are typically small.
                let mut merged = a.clone();
                for &idx in b {
                    if !merged.contains(&idx) {
                        merged.push(idx);
                    }
                }
                merged
            }
        }
    }

    /// Find entries in the same file matching a qname (case-insensitive).
    ///
    /// Uses composite HashMap index instead of O(n) linear scan.
    pub(in crate::resolver) fn same_file_qname(&self, file_path: &str, qname: &str) -> Vec<usize> {
        self.by_file_qname
            .get(file_path)
            .and_then(|m| m.get(&qname.to_lowercase()))
            .cloned()
            .unwrap_or_default()
    }

    /// Find exported symbols by file + export name.
    pub(in crate::resolver) fn exported(&self, file_path: &str, export_name: &str) -> Vec<usize> {
        self.by_export
            .get(file_path)
            .and_then(|m| m.get(&export_name.to_lowercase()))
            .cloned()
            .unwrap_or_default()
    }

    pub(in crate::resolver) fn find_by_uid(&self, uid: &str) -> Option<usize> {
        self.by_uid.get(uid).copied()
    }

    /// Walk upward from `container` qname to find the owning class.
    pub(in crate::resolver) fn owner_class_qname(
        &self,
        file_path: &str,
        container: Option<&str>,
    ) -> Option<String> {
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
            // No parser currently emits lexical scopes, so the scope map is empty;
            // scope-chain resolution stays dormant until a producer is wired up.
            scopes: HashMap::new(),
            imports: Self::build_import_bindings(outcome, file_path),
        }
    }
}

/// Focused micro-benchmark of the incremental `resolve` catalog floor: the
/// single-file path rebuilds the whole catalog from every persisted symbol
/// (`build_resolution_catalog`), so its cost scales with the project symbol
/// total, not the changed file. This isolates the in-memory rebuild
/// (`add_symbols` + `build_type_catalog`) from the same-file lookup hot path so
/// a structural change can be A/B'd on both axes (build vs lookup) before
/// touching the resolver.
///
/// `#[ignore]`d — run explicitly (release for meaningful numbers):
///
/// ```sh
/// cargo test -p cc-index --release catalog_build_bench -- --ignored --nocapture
/// ```
#[cfg(test)]
mod catalog_build_bench {
    use super::*;
    use cc_model::symbol::{SymbolKind, SymbolRecord};
    use cc_model::ParserTier;
    use std::time::Instant;

    fn gen_symbols(n: usize, per_file: usize) -> Vec<SymbolRecord> {
        (0..n)
            .map(|i| {
                let file_idx = i / per_file;
                let sidx = i % per_file;
                let rel = format!("src/module_{:03}/file_{:05}.rs", file_idx % 200, file_idx);
                let name = format!("fn_{:05}_{}", file_idx, sidx);
                SymbolRecord {
                    symbol_id: format!("sym:{}:{}", rel, i),
                    file_path: rel,
                    name: name.clone(),
                    kind: SymbolKind::Function,
                    container: None,
                    start_line: (sidx * 10 + 1) as u32,
                    end_line: (sidx * 10 + 8) as u32,
                    start_col: 0,
                    end_col: 1,
                    signature: None,
                    doc: None,
                    parser_tier: ParserTier::TreeSitter,
                    parser_confidence: 1.0,
                    qname: Some(name.clone()),
                    parent_symbol_id: None,
                    scope_id: None,
                    export_name: Some(name),
                    is_default_export: false,
                    symbol_uid: Some(format!("uid:{}:{}", file_idx, sidx)),
                    framework_role: None,
                    receiver_type: None,
                    param_types: None,
                    return_type: None,
                    param_count: None,
                    base_types: None,
                    implements: None,
                }
            })
            .collect()
    }

    #[test]
    #[ignore = "catalog build/lookup micro-benchmark; run with --release --nocapture"]
    fn bench_catalog_build_scaling() {
        const LOOKUPS: usize = 100_000;
        // Fixed project size (~50k-file scale); sweep symbols-per-file so the
        // nested-map vs linear-filter lookup crossover is visible — real files
        // are not uniformly 5 symbols each, and the linear scan grows with it.
        const N: usize = 278_000;
        for &per_file in &[5usize, 10, 25, 50, 100] {
            let n = N;
            let syms = gen_symbols(n, per_file);
            let files = n / per_file;

            // Build: 8-map registration over every persisted symbol.
            let t = Instant::now();
            let mut cat = SymbolCatalog::new();
            cat.add_symbols(&syms);
            let add = t.elapsed();

            // Second full pass: dispatch/type catalog.
            let t = Instant::now();
            cat.build_type_catalog(syms.iter());
            let tc = t.elapsed();

            // Same-file lookups (the resolve hot path). Keys are pre-generated
            // so the timed loop measures only `same_file_named`, not formatting.
            let keys: Vec<(String, String)> = (0..LOOKUPS)
                .map(|k| {
                    let fi = k.wrapping_mul(2_654_435_761) % files;
                    (
                        format!("src/module_{:03}/file_{:05}.rs", fi % 200, fi),
                        format!("fn_{:05}_{}", fi, k % per_file),
                    )
                })
                .collect();
            let t = Instant::now();
            let mut hits = 0usize;
            for (file, name) in &keys {
                hits += cat.same_file_named(file, name).len();
            }
            let lookup = t.elapsed();

            // A/B: the lookup cost if the nested `by_file_name`/`by_file_qname`
            // indices were dropped and same-file lookup fell back to a linear
            // filter over `by_file` (+ an `entries[idx]` probe per candidate).
            // This is what "eliminate the nested maps to halve build" would
            // cost on the resolve hot path — measured, not assumed.
            let t = Instant::now();
            let mut hits_lin = 0usize;
            for (file, name) in &keys {
                if let Some(idxs) = cat.by_file.get(file) {
                    for &idx in idxs {
                        let e = &cat.entries[idx];
                        if e.name.eq_ignore_ascii_case(name)
                            || e.qname
                                .as_deref()
                                .is_some_and(|q| q.eq_ignore_ascii_case(name))
                        {
                            hits_lin += 1;
                        }
                    }
                }
            }
            let lookup_lin = t.elapsed();

            eprintln!(
                "N={} per_file={:>3} files={:>6}: add_symbols={:>9.1?} build_tc={:>8.1?} | \
                 nested={:>6.0?}/call linear={:>6.0?}/call [hits {}/{}]",
                n,
                per_file,
                files,
                add,
                tc,
                lookup / LOOKUPS as u32,
                lookup_lin / LOOKUPS as u32,
                hits,
                hits_lin,
            );
        }
    }
}

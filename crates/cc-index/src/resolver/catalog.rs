//! SymbolCatalog struct definition and construction/lookup methods.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Mutex;

use lru::LruCache;

use cc_model::parse::ParseOutcome;
use cc_model::symbol::{SymbolKind, SymbolRecord};

use crate::type_catalog::TypeCatalog;

use super::types::*;

/// Number of independently locked shards in [`ShardedResolveCache`]. Must be
/// a power of two (the shard is selected by masking the key's low bits).
const RESOLVE_CACHE_SHARDS: usize = 16;

/// Sharded LRU cache for `resolve_name` results.
///
/// The resolve phase runs one rayon worker per write unit
/// (`indexer_phases::resolve`), and every worker funnels its lookups through
/// this cache. A single `Mutex<LruCache>` serialized those workers on one
/// global lock; splitting the key space over [`RESOLVE_CACHE_SHARDS`]
/// independently locked shards keeps the same total capacity and hit
/// semantics (keys are pre-hashed u64s, so the low bits pick a shard
/// uniformly) while letting parallel resolvers proceed contention-free.
pub(in crate::resolver) struct ShardedResolveCache {
    shards: Vec<Mutex<LruCache<u64, Option<ResolveResult>>>>,
}

impl ShardedResolveCache {
    pub(in crate::resolver) fn new(total_capacity: usize) -> Self {
        let per_shard = (total_capacity / RESOLVE_CACHE_SHARDS).max(1);
        let per_shard = NonZeroUsize::new(per_shard).expect("per-shard capacity >= 1");
        Self {
            shards: (0..RESOLVE_CACHE_SHARDS)
                .map(|_| Mutex::new(LruCache::new(per_shard)))
                .collect(),
        }
    }

    fn shard(&self, key: u64) -> &Mutex<LruCache<u64, Option<ResolveResult>>> {
        &self.shards[(key as usize) & (RESOLVE_CACHE_SHARDS - 1)]
    }

    /// Outer `Option`: cache hit or miss. Inner `Option`: the cached
    /// resolution outcome (misses are cached too).
    pub(in crate::resolver) fn get(&self, key: u64) -> Option<Option<ResolveResult>> {
        match self.shard(key).lock() {
            Ok(mut cache) => cache.get(&key).cloned(),
            Err(_) => None,
        }
    }

    pub(in crate::resolver) fn put(&self, key: u64, value: Option<ResolveResult>) {
        if let Ok(mut cache) = self.shard(key).lock() {
            cache.put(key, value);
        }
    }

    pub(in crate::resolver) fn clear(&self) {
        for shard in &self.shards {
            if let Ok(mut cache) = shard.lock() {
                cache.clear();
            }
        }
    }
}

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
    pub(in crate::resolver) reexports: HashMap<String, Vec<cc_model::edge::ImportRecord>>,
    pub(in crate::resolver) by_export: HashMap<String, HashMap<String, Vec<usize>>>,
    /// file_path -> name_lowercase -> Vec<usize>: nested index for same-file name lookup.
    pub(in crate::resolver) by_file_name: HashMap<String, HashMap<String, Vec<usize>>>,
    /// file_path -> qname_lowercase -> Vec<usize>: nested index for same-file qname lookup.
    pub(in crate::resolver) by_file_qname: HashMap<String, HashMap<String, Vec<usize>>>,
    /// Lightweight type catalog for method dispatch resolution.
    pub(in crate::resolver) type_catalog: Option<TypeCatalog>,
    /// Sharded LRU cache for `resolve_name` results to avoid redundant
    /// resolution (see [`ShardedResolveCache`] for the sharding rationale).
    pub(in crate::resolver) resolve_cache: ShardedResolveCache,
    /// Above this many same-named candidates, name-only resolution
    /// (global-unique / fuzzy import-distance, and the `find_best` fallback)
    /// is treated as non-resolvable: a name shared by hundreds of symbols —
    /// typically a function-local variable the parsers also surface globally
    /// (`left`, `value`, …) — cannot be disambiguated by path heuristics, and
    /// scanning the whole bucket per reference is the dominant cold-build
    /// O(N²) cost. Bucket lookups at or below this stay exact.
    pub(in crate::resolver) max_fuzzy_pool: usize,
    /// Tombstoned `entries` slots left behind by [`Self::remove_files`].
    /// Slots are never reused (surviving indices stay valid); the cross-build
    /// cache uses the dead/live ratio as its compaction policy.
    pub(in crate::resolver) dead: usize,
}

impl SymbolCatalog {
    pub fn set_reexports(&mut self, imports: Vec<cc_model::edge::ImportRecord>) {
        self.reexports.clear();
        self.resolve_cache.clear();
        for import in imports {
            if import.is_reexport {
                self.reexports
                    .entry(import.file_path.clone())
                    .or_default()
                    .push(import);
            }
        }
    }
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
            reexports: HashMap::new(),
            by_file_name: HashMap::new(),
            by_file_qname: HashMap::new(),
            type_catalog: None,
            resolve_cache: ShardedResolveCache::new(cache_size.max(1)),
            max_fuzzy_pool,
            dead: 0,
        }
    }

    /// Number of live (non-tombstoned) entries.
    pub(crate) fn live_len(&self) -> usize {
        self.entries.len() - self.dead
    }

    /// Whether tombstoned slots outweigh live entries — the cross-build
    /// cache's signal to drop this catalog and rebuild fresh instead of
    /// carrying ever-growing dead weight (compaction by reconstruction).
    ///
    /// The absolute floor keeps the policy a *memory* bound rather than a
    /// ratio fetish: on small repositories a single batch can touch most
    /// files (dead briefly exceeds live), but a few thousand tombstones are
    /// ~1 MB — not worth forfeiting reuse over. At scale the ratio term
    /// dominates and bounds the catalog at ~2× its live size.
    pub(crate) fn should_compact(&self) -> bool {
        const TOMBSTONE_FLOOR: usize = 4096;
        self.dead > self.live_len().max(TOMBSTONE_FLOOR)
    }

    /// Clear the resolve_name LRU cache.
    pub fn clear_resolve_cache(&self) {
        self.resolve_cache.clear();
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

    /// Remove every entry belonging to `files` from all lookup maps, driving
    /// the same removal through the embedded [`TypeCatalog`] (when built).
    ///
    /// `entries` slots are tombstoned, never reused, so surviving indices —
    /// including those captured in earlier [`ResolveResult`]s — stay valid;
    /// the resolve LRU is cleared because its cached results may point at
    /// removed entries. Global-bucket cleanup is batched per distinct key:
    /// each bucket is scanned once regardless of how many removed entries
    /// share its key (a hot name like `value` costs one retain, not one per
    /// removed local).
    pub(crate) fn remove_files(&mut self, files: &HashSet<String>) {
        self.clear_resolve_cache();

        let mut removed_indices: Vec<usize> = Vec::new();
        for file in files {
            if let Some(indices) = self.by_file.remove(file) {
                removed_indices.extend(indices);
            }
            self.by_file_name.remove(file);
            self.by_file_qname.remove(file);
            self.by_export.remove(file);
        }
        if removed_indices.is_empty() {
            return;
        }
        let removed_set: HashSet<usize> = removed_indices.iter().copied().collect();

        // Distinct global-bucket keys contributed by the removed entries.
        let mut name_keys: HashSet<String> = HashSet::new();
        let mut qname_keys: HashSet<String> = HashSet::new();
        let mut leaf_keys: HashSet<String> = HashSet::new();
        for &idx in &removed_indices {
            let entry = &self.entries[idx];
            name_keys.insert(entry.name.to_lowercase());
            if let Some(ref q) = entry.qname {
                let ql = q.to_lowercase();
                leaf_keys.insert(ql.rsplit('.').next().unwrap_or(&ql).to_string());
                qname_keys.insert(ql);
            }
            if let Some(ref uid) = entry.symbol_uid {
                // uid is file-scoped, so the current mapping (last-wins)
                // necessarily points at an entry of the same removed file.
                if self
                    .by_uid
                    .get(uid)
                    .is_some_and(|i| removed_set.contains(i))
                {
                    self.by_uid.remove(uid);
                }
            }
        }
        for key in &name_keys {
            if let Some(bucket) = self.by_name.get_mut(key) {
                bucket.retain(|i| !removed_set.contains(i));
                if bucket.is_empty() {
                    self.by_name.remove(key);
                }
            }
        }
        for key in &qname_keys {
            if let Some(bucket) = self.by_qname.get_mut(key) {
                bucket.retain(|i| !removed_set.contains(i));
                if bucket.is_empty() {
                    self.by_qname.remove(key);
                }
            }
        }
        for key in &leaf_keys {
            if let Some(bucket) = self.by_qname_leaf.get_mut(key) {
                bucket.retain(|i| !removed_set.contains(i));
                if bucket.is_empty() {
                    self.by_qname_leaf.remove(key);
                }
            }
        }

        // Mirror the removal into the type catalog before tombstoning (the
        // key facets are read from the still-live entries).
        if let Some(mut tc) = self.type_catalog.take() {
            let metas: Vec<crate::type_catalog::SymbolKeyMeta<'_>> = removed_indices
                .iter()
                .map(|&idx| {
                    let e = &self.entries[idx];
                    crate::type_catalog::SymbolKeyMeta {
                        name: &e.name,
                        qname: e.qname.as_deref(),
                        kind: e.kind,
                        symbol_uid: e.symbol_uid.as_deref(),
                    }
                })
                .collect();
            tc.remove_files(&metas, files);
            drop(metas);
            self.type_catalog = Some(tc);
        }

        for idx in removed_indices {
            self.entries[idx] = CatalogEntry {
                symbol_id: String::new(),
                symbol_uid: None,
                name: String::new(),
                file_path: String::new(),
                kind: SymbolKind::Variable,
                container: None,
                qname: None,
                start_line: 0,
                end_line: 0,
                scope_id: None,
            };
            self.dead += 1;
        }
    }

    /// Feed the current batch's symbols into the already-built
    /// [`TypeCatalog`] — the delta counterpart of `build_type_catalog` used
    /// when the catalog (type catalog included) was reused from the
    /// cross-build cache and only the batch's contributions are missing.
    pub(crate) fn type_catalog_add_symbols<'a, I>(&mut self, symbols: I)
    where
        I: IntoIterator<Item = &'a SymbolRecord>,
    {
        if let Some(ref mut tc) = self.type_catalog {
            for sym in symbols {
                tc.add_symbol(sym);
            }
        }
    }

    /// Reset build-local variable type assignments on a reused type catalog
    /// (fresh builds start empty; a cached catalog must match).
    pub(crate) fn reset_type_assigns(&mut self) {
        if let Some(ref mut tc) = self.type_catalog {
            tc.reset_type_assigns();
        }
    }

    /// Register all symbols from a parsed file.
    pub fn add_symbols(&mut self, symbols: &[SymbolRecord]) {
        self.add_symbols_iter(symbols.len(), symbols.iter());
    }

    /// [`Self::add_symbols`] over any reference iterator, so callers holding
    /// a shared seed snapshot ([`cc_db::SeedRows`]) can register the rows
    /// without materializing an owned concatenation. `n` pre-sizes the
    /// symbol-cardinality structures (pass the exact count).
    pub fn add_symbols_iter<'a>(
        &mut self,
        n: usize,
        symbols: impl Iterator<Item = &'a SymbolRecord>,
    ) {
        // Pre-size the symbol-cardinality structures. The single-file
        // incremental path rebuilds the catalog from every persisted symbol, so
        // the first `add_symbols` call seeds hundreds of thousands of rows; the
        // `entries` Vec holds a wide `CatalogEntry` each, and growing it from
        // zero reallocates ~log2(n) times copying every prior element. The
        // file-keyed maps stay unreserved (their cardinality is ~file count, so
        // reserving `n` would over-allocate several-fold).
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

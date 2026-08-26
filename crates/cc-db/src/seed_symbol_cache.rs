//! Cross-build cache of the resolver seed-symbol snapshot.
//!
//! `resolver_seed_symbols_excluding` historically re-loaded every persisted
//! symbol from SQLite on each incremental build to seed the resolver's
//! `SymbolCatalog`/`TypeCatalog` — an O(repo) cost that dominated the
//! resolve phase on large repositories even for single-file batches. This
//! module keeps the loaded snapshot alive on the [`IndexDb`] handle (the
//! only host that survives across builds — `Indexer` is rebuilt per build)
//! and serves subsequent loads from memory.
//!
//! # Validity (no manual invalidation hooks)
//!
//! The cache is keyed on the write-time-maintained `symbols_seed` aggregate
//! (see [`crate::signature_agg`]): a persisted `(count, sum-of-row-hashes)`
//! multiset homomorphism over exactly the 15 seed columns. Every production
//! write path that mutates `symbols` rows already keeps the aggregates in
//! sync inside its own transaction (path-scoped deltas for incremental
//! writers, baseline recompute for full rebuilds), so:
//!
//! - equal token ⇒ equal seed-row multiset (up to 64-bit collision odds,
//!   the same guarantee class as the postprocess signature gates);
//! - any seed mutation — in-process or cross-process — moves the persisted
//!   token, and the next read misses and reloads;
//! - writes that do not touch seed columns (community assignment, ref
//!   rewrites, metadata, postprocess edges) leave the token unchanged, so
//!   the cache survives a full build's post-batch write traffic.
//!
//! Databases without a stored aggregate baseline (raw-SQL fixtures,
//! pre-aggregate files) never engage the cache and keep the historical
//! direct-load behavior. Raw-SQL mutations on a database that *does* carry a
//! baseline bypass the aggregate maintenance and could leave the token stale
//! — the same documented caveat the signature gates already carry.
//!
//! # Maintenance
//!
//! The hot path ([`IndexDb::write_incremental_batch`]) updates the snapshot
//! in place after its transaction commits: remove the batch/removed files'
//! entries, insert the freshly written units' seed projections — the exact
//! file-scoped delta the transaction applied to the `symbols` table. All
//! other writers simply move the token; the next read repopulates.
//!
//! # Memory
//!
//! One seed projection per persisted symbol (roughly 0.3–0.6 KB each, so
//! ~20–35 MB for a 10k-file / 55k-symbol repository). Repositories above
//! `CODECORTEX_SEED_CACHE_MAX_SYMBOLS` (default 500_000, `0` disables) skip
//! the cache and keep the per-build direct load.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::ParserTier;

use crate::index_db::{FileWriteUnit, IndexDb};
use crate::signature_agg::RowAgg;

/// Seed-symbol rows handed to the resolver, in the seed query's
/// `ORDER BY file_path, start_line` order.
///
/// The `Shared` variant is the cache-hit shape: one `Arc` per file cloned out
/// of the snapshot, zero row clones — consumers only ever iterate the rows by
/// reference, so materializing an owned `Vec<SymbolRecord>` per build (an
/// O(all-symbols) deep clone) bought nothing. `Owned` is the direct-SQL shape
/// (cache miss / no aggregate baseline).
pub enum SeedRows {
    Owned(Vec<SymbolRecord>),
    Shared(Vec<Arc<Vec<SymbolRecord>>>),
}

impl SeedRows {
    pub fn empty() -> Self {
        SeedRows::Owned(Vec::new())
    }

    pub fn iter(&self) -> impl Iterator<Item = &SymbolRecord> + Clone {
        match self {
            SeedRows::Owned(rows) => itertools_either::Either::Left(rows.iter()),
            SeedRows::Shared(files) => {
                itertools_either::Either::Right(files.iter().flat_map(|rows| rows.iter()))
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            SeedRows::Owned(rows) => rows.len(),
            SeedRows::Shared(files) => files.iter().map(|rows| rows.len()).sum(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Flatten into an owned `Vec` (test helpers and the legacy
    /// `resolver_seed_symbols_excluding` contract).
    pub fn into_vec(self) -> Vec<SymbolRecord> {
        match self {
            SeedRows::Owned(rows) => rows,
            SeedRows::Shared(files) => {
                let mut out = Vec::with_capacity(files.iter().map(|rows| rows.len()).sum());
                for rows in files {
                    out.extend(rows.iter().cloned());
                }
                out
            }
        }
    }
}

/// Minimal local stand-in for `itertools::Either` so the two `iter()` arms
/// can unify without an extra dependency.
mod itertools_either {
    pub enum Either<L, R> {
        Left(L),
        Right(R),
    }

    impl<L, R, T> Iterator for Either<L, R>
    where
        L: Iterator<Item = T>,
        R: Iterator<Item = T>,
    {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            match self {
                Either::Left(iter) => iter.next(),
                Either::Right(iter) => iter.next(),
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            match self {
                Either::Left(iter) => iter.size_hint(),
                Either::Right(iter) => iter.size_hint(),
            }
        }
    }

    impl<L: Clone, R: Clone> Clone for Either<L, R> {
        fn clone(&self) -> Self {
            match self {
                Either::Left(iter) => Either::Left(iter.clone()),
                Either::Right(iter) => Either::Right(iter.clone()),
            }
        }
    }
}

/// Cached seed snapshot: rows grouped per file (map order = `file_path`
/// byte order = SQLite `ORDER BY file_path` order), each file's rows in
/// `start_line` order — materialization reproduces the seed query's
/// `ORDER BY file_path, start_line`. Per-file vectors are `Arc`-shared with
/// cache hits handed out earlier; batch maintenance copies-on-write only the
/// touched files (`Arc::make_mut`).
pub(crate) struct SeedSymbolCache {
    /// The `symbols_seed` aggregate this snapshot corresponds to.
    token: RowAgg,
    by_file: BTreeMap<String, Arc<Vec<SymbolRecord>>>,
    total: usize,
}

/// Cache capacity in symbols; `0` disables the cache entirely. Shared knob:
/// the resolver's cross-build catalog cache (hosted on the same handle,
/// validated by the same `symbols_seed` token) applies the identical cap so
/// one env var governs both layers of seed-derived memory.
pub fn seed_cache_max_symbols() -> usize {
    std::env::var("CODECORTEX_SEED_CACHE_MAX_SYMBOLS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(500_000)
}

/// Project a freshly parsed symbol onto the seed-column subset, mirroring
/// the `SEED_COLUMNS` SELECT in `index_db_query` field by field (including
/// its defaults for the columns the SELECT does not read and the lenient
/// kind round-trip). A cached entry must be indistinguishable from the same
/// row read back from the database — a consumer that adds a column to the
/// SELECT must extend this projection and the `symbols_seed` aggregate
/// columns in `signature_agg` in the same change.
fn project_seed(sym: &SymbolRecord) -> SymbolRecord {
    SymbolRecord {
        symbol_id: sym.symbol_id.clone(),
        file_path: sym.file_path.clone(),
        name: sym.name.clone(),
        kind: SymbolKind::from_str_lenient(sym.kind.as_str()).unwrap_or(SymbolKind::Variable),
        container: sym.container.clone(),
        start_line: sym.start_line,
        end_line: sym.end_line,
        start_col: 0,
        end_col: 0,
        signature: None,
        doc: None,
        parser_tier: ParserTier::Generic,
        parser_confidence: 0.0,
        qname: sym.qname.clone(),
        parent_symbol_id: None,
        scope_id: None,
        export_name: sym.export_name.clone(),
        is_default_export: sym.is_default_export,
        symbol_uid: sym.symbol_uid.clone(),
        framework_role: None,
        receiver_type: sym.receiver_type.clone(),
        param_types: None,
        return_type: None,
        param_count: sym.param_count,
        base_types: sym.base_types.clone(),
        implements: sym.implements.clone(),
    }
}

impl SeedSymbolCache {
    /// Build a snapshot from rows already in seed shape and seed order (the
    /// direct query result). `None` when over `cap`.
    fn build(token: RowAgg, rows: &[SymbolRecord], cap: usize) -> Option<Self> {
        if rows.len() > cap {
            return None;
        }
        let mut grouped: BTreeMap<String, Vec<SymbolRecord>> = BTreeMap::new();
        for row in rows {
            grouped
                .entry(row.file_path.clone())
                .or_default()
                .push(row.clone());
        }
        Some(Self {
            token,
            by_file: grouped
                .into_iter()
                .map(|(path, rows)| (path, Arc::new(rows)))
                .collect(),
            total: rows.len(),
        })
    }

    /// Materialize the snapshot minus `excluded_files`, in the seed query's
    /// `ORDER BY file_path, start_line` order. Shares the per-file vectors —
    /// one `Arc` clone per file, zero row clones.
    fn materialize(&self, excluded_files: &[String]) -> SeedRows {
        let excluded: HashSet<&str> = excluded_files.iter().map(String::as_str).collect();
        let mut files = Vec::with_capacity(self.by_file.len());
        for (file_path, rows) in &self.by_file {
            if excluded.contains(file_path.as_str()) {
                continue;
            }
            files.push(Arc::clone(rows));
        }
        SeedRows::Shared(files)
    }

    /// Apply the file-scoped delta of one committed incremental batch:
    /// remove every batch/removed path's entries, insert the written units'
    /// seed projections (grouped by each symbol's own `file_path`, exactly
    /// like the SQL insert). Returns `false` when the result exceeds `cap`.
    fn apply_batch<'u>(
        &mut self,
        post_token: RowAgg,
        to_remove: &[String],
        units: impl Iterator<Item = &'u FileWriteUnit> + Clone,
        cap: usize,
    ) -> bool {
        for path in to_remove {
            self.by_file.remove(path);
        }
        for unit in units.clone() {
            self.by_file.remove(&unit.rel_path);
        }
        // Fold within-batch conflicts exactly like the SQL `INSERT OR
        // REPLACE` does: a later row sharing a `symbol_id` (PK) or
        // `symbol_uid` (UNIQUE, NULLs never conflict) with an earlier one
        // replaces it (reachable: two same-file trait impls of one method
        // name share a uid). Iteration order matches the SQL execution
        // order — normal units then dirty units, each unit's symbols in
        // outcome order — so last-wins picks the same surviving row.
        let mut kept: Vec<Option<SymbolRecord>> = Vec::new();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        let mut by_uid: HashMap<String, usize> = HashMap::new();
        for unit in units {
            for sym in &unit.outcome.symbols {
                let row = project_seed(sym);
                if let Some(replaced) = by_id.insert(row.symbol_id.clone(), kept.len()) {
                    kept[replaced] = None;
                }
                if let Some(uid) = row.symbol_uid.clone() {
                    if let Some(replaced) = by_uid.insert(uid, kept.len()) {
                        kept[replaced] = None;
                    }
                }
                kept.push(Some(row));
            }
        }
        let mut touched: HashSet<String> = HashSet::new();
        for row in kept.into_iter().flatten() {
            touched.insert(row.file_path.clone());
            // Copy-on-write: only the touched file's vector is cloned when a
            // previous cache hit still shares it.
            Arc::make_mut(
                self.by_file
                    .entry(row.file_path.clone())
                    .or_insert_with(|| Arc::new(Vec::new())),
            )
            .push(row);
        }
        // Restore per-file start_line order (stable: ties keep insert order,
        // matching the rowid order a fresh load would observe — a folded
        // row's rowid stems from its last, surviving insert).
        for path in &touched {
            if let Some(rows) = self.by_file.get_mut(path) {
                Arc::make_mut(rows).sort_by_key(|s| s.start_line);
            }
        }
        self.total = self.by_file.values().map(|rows| rows.len()).sum();
        self.token = post_token;
        self.total <= cap
    }
}

impl IndexDb {
    /// Serve a seed load from the cache when the persisted token matches.
    /// The hit shares the snapshot's per-file vectors (no row clones).
    pub(crate) fn seed_cache_materialize(
        &self,
        token: RowAgg,
        excluded_files: &[String],
    ) -> Option<SeedRows> {
        let guard = self.seed_cache.lock().ok()?;
        let cache = guard.as_ref()?;
        if cache.token != token {
            return None;
        }
        Some(cache.materialize(excluded_files))
    }

    /// Populate the cache from a full direct load whose token was verified
    /// stable across the load (caller re-reads the stored aggregate).
    pub(crate) fn seed_cache_store(&self, token: RowAgg, rows: &[SymbolRecord]) {
        let Ok(mut guard) = self.seed_cache.lock() else {
            return;
        };
        *guard = SeedSymbolCache::build(token, rows, seed_cache_max_symbols());
    }

    /// Carry the cache across one committed incremental batch. `pre`/`post`
    /// are the `symbols_seed` aggregates read inside the batch transaction
    /// before and after its mutations; `None` (no stored baseline) drops the
    /// cache.
    pub(crate) fn seed_cache_apply_batch(
        &self,
        pre: Option<RowAgg>,
        post: Option<RowAgg>,
        to_remove: &[String],
        normal_units: &[FileWriteUnit],
        dirty_units: &[FileWriteUnit],
    ) {
        let Ok(mut guard) = self.seed_cache.lock() else {
            return;
        };
        let (Some(pre), Some(post)) = (pre, post) else {
            *guard = None;
            return;
        };
        let units = || normal_units.iter().chain(dirty_units.iter());
        let mut cache = match guard.take() {
            // A concurrent reader already refilled against the committed
            // state; keep it.
            Some(cache) if cache.token == post => {
                *guard = Some(cache);
                return;
            }
            Some(cache) if cache.token == pre => cache,
            // Cold start: `count == 0` proves the symbols table was empty
            // before this batch (the count half of the aggregate is exact),
            // so the batch delta alone reconstructs the snapshot.
            None if pre.count == 0 => SeedSymbolCache {
                token: pre,
                by_file: BTreeMap::new(),
                total: 0,
            },
            // Unknown basis (stale snapshot or cold cache on a non-empty
            // table): stay cold, the next read repopulates.
            _ => return,
        };
        if cache.apply_batch(post, to_remove, units(), seed_cache_max_symbols()) {
            *guard = Some(cache);
        }
    }

    /// Cached symbol count, for tests asserting the cache is engaged.
    #[doc(hidden)]
    pub fn seed_cache_len(&self) -> Option<usize> {
        self.seed_cache.lock().ok()?.as_ref().map(|c| c.total)
    }

    /// Take the cross-build resolver catalog parked on this handle, leaving
    /// the slot empty. cc-index owns the concrete type (the slot is
    /// type-erased so cc-db stays independent of it); the taker must
    /// validate the payload against the persisted `symbols_seed` aggregate
    /// before trusting it — exactly the seed cache's own contract.
    pub fn take_resolver_catalog(&self) -> Option<Box<dyn std::any::Any + Send>> {
        self.resolver_catalog_slot.lock().ok()?.take()
    }

    /// Park a cross-build resolver catalog on this handle (replacing any
    /// previous occupant).
    pub fn store_resolver_catalog(&self, catalog: Box<dyn std::any::Any + Send>) {
        if let Ok(mut guard) = self.resolver_catalog_slot.lock() {
            *guard = Some(catalog);
        }
    }

    /// Drop any parked resolver catalog (full rebuilds replace the whole
    /// symbol table, so a stale catalog would only waste memory waiting to
    /// fail its token check).
    pub fn clear_resolver_catalog(&self) {
        if let Ok(mut guard) = self.resolver_catalog_slot.lock() {
            *guard = None;
        }
    }

    /// Whether a resolver catalog is currently parked (tests / diagnostics).
    #[doc(hidden)]
    pub fn resolver_catalog_parked(&self) -> bool {
        self.resolver_catalog_slot
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_row(file: &str, name: &str, start_line: u32) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("{file}:{name}"),
            file_path: file.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            container: None,
            start_line,
            end_line: start_line + 4,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Generic,
            parser_confidence: 0.0,
            qname: Some(format!("{file}.{name}")),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(format!("uid:{file}:{name}")),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    #[test]
    fn build_respects_capacity_cap() {
        let rows = vec![seed_row("a.rs", "one", 1), seed_row("a.rs", "two", 10)];
        assert!(SeedSymbolCache::build(RowAgg::default(), &rows, 1).is_none());
        let cache = SeedSymbolCache::build(RowAgg::default(), &rows, 2).expect("within cap");
        assert_eq!(cache.total, 2);
    }

    #[test]
    fn materialize_orders_and_excludes() {
        let rows = vec![
            seed_row("a.rs", "one", 5),
            seed_row("a.rs", "two", 9),
            seed_row("b.rs", "three", 1),
        ];
        let cache = SeedSymbolCache::build(RowAgg::default(), &rows, 100).unwrap();
        let all = cache.materialize(&[]);
        assert_eq!(
            all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["one", "two", "three"],
            "file_path then start_line order"
        );
        let filtered = cache.materialize(&["a.rs".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered.iter().next().unwrap().file_path, "b.rs");
    }

    fn open_db() -> (tempfile::TempDir, IndexDb) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("seed_cache.db")).unwrap().0;
        (tmp, db)
    }

    fn unit(file: &str, symbols: Vec<SymbolRecord>) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: file.to_string(),
            language: cc_model::Language::Rust,
            content_hash: format!("hash-{file}-{}", symbols.len()),
            mtime: 1.0,
            size: 1,
            outcome: cc_model::parse::ParseOutcome {
                symbols,
                ..Default::default()
            },
        }
    }

    fn write_batch(
        db: &IndexDb,
        to_remove: &[String],
        normal: &[FileWriteUnit],
        dirty: &[FileWriteUnit],
    ) {
        db.write_incremental_batch(
            to_remove,
            normal,
            dirty,
            &[],
            &[],
            &crate::index_db::PrecompressedChunks::new(),
        )
        .unwrap();
    }

    /// Sorted serde fingerprints, so multisets compare without `PartialEq`
    /// on `SymbolRecord`.
    fn fingerprints(rows: &[SymbolRecord]) -> Vec<String> {
        let mut keys: Vec<String> = rows
            .iter()
            .map(|s| serde_json::to_string(s).unwrap())
            .collect();
        keys.sort();
        keys
    }

    /// Cached read must equal a direct SQL load, for both exclusion shapes.
    fn assert_cache_equals_direct(db: &IndexDb, label: &str) {
        let conn = db.read_conn().unwrap();
        for excluded in [Vec::new(), vec!["a.rs".to_string()]] {
            let cached = db.resolver_seed_symbols_excluding(&excluded).unwrap();
            let direct = IndexDb::load_seed_rows_on(&conn, &excluded).unwrap();
            assert_eq!(
                fingerprints(&cached),
                fingerprints(&direct),
                "{label}: cached seed rows diverged from a direct load (excluded: {excluded:?})"
            );
        }
    }

    /// The committed-batch delta must keep the cached snapshot identical to
    /// a direct load across add / modify / dirty-rewrite / remove batches,
    /// without ever reloading (the cache stays warm through every step).
    #[test]
    fn batch_delta_matches_direct_load() {
        let (_tmp, db) = open_db();
        // No-op batch initializes the aggregate baseline on the fresh
        // database, so the first real batch cold-starts the cache from its
        // own delta (pre-batch symbol count is provably zero).
        write_batch(&db, &[], &[], &[]);
        write_batch(
            &db,
            &[],
            &[
                unit(
                    "a.rs",
                    vec![seed_row("a.rs", "one", 1), seed_row("a.rs", "two", 9)],
                ),
                unit("b.rs", vec![seed_row("b.rs", "three", 1)]),
            ],
            &[],
        );
        assert_eq!(
            db.seed_cache_len(),
            Some(3),
            "cold-start must warm the cache"
        );
        assert_cache_equals_direct(&db, "after add batch");

        // Modify: replace a.rs with different rows (one carries seed-column
        // backfill the catalog depends on).
        let mut enriched = seed_row("a.rs", "one", 1);
        enriched.base_types = Some("Base".to_string());
        enriched.implements = Some("Iface".to_string());
        write_batch(&db, &[], &[unit("a.rs", vec![enriched])], &[]);
        assert_eq!(db.seed_cache_len(), Some(2), "replace must apply in place");
        assert_cache_equals_direct(&db, "after modify batch");

        // Dirty rewrite path (re-resolve-only units replace symbols too).
        let mut rewritten = seed_row("b.rs", "three", 1);
        rewritten.container = Some("Mod".to_string());
        write_batch(&db, &[], &[], &[unit("b.rs", vec![rewritten])]);
        assert_cache_equals_direct(&db, "after dirty rewrite batch");

        // Removal.
        write_batch(&db, &["b.rs".to_string()], &[], &[]);
        assert_eq!(db.seed_cache_len(), Some(1), "removal must drop the file");
        assert_cache_equals_direct(&db, "after remove batch");
    }

    /// A symbols write outside the incremental batch (here:
    /// `replace_files_batch`) moves the persisted token without touching the
    /// in-memory snapshot — the next read must miss, return fresh content,
    /// and re-warm the cache.
    #[test]
    fn foreign_symbol_writer_invalidates_then_rewarns() {
        let (_tmp, db) = open_db();
        write_batch(&db, &[], &[], &[]);
        write_batch(
            &db,
            &[],
            &[unit("a.rs", vec![seed_row("a.rs", "one", 1)])],
            &[],
        );
        assert_eq!(db.seed_cache_len(), Some(1));

        db.replace_files_batch(&[unit("c.rs", vec![seed_row("c.rs", "other", 2)])])
            .unwrap();
        assert_eq!(
            db.seed_cache_len(),
            Some(1),
            "foreign writer leaves the stale snapshot in place; the token moved"
        );
        assert_cache_equals_direct(&db, "after foreign writer");
        assert_eq!(
            db.seed_cache_len(),
            Some(2),
            "read after the miss must re-warm with the fresh snapshot"
        );
    }

    /// Within-batch duplicate `symbol_uid`s must fold in the cache exactly
    /// like the SQL `INSERT OR REPLACE` does (last insert wins on the
    /// `symbol_uid` UNIQUE constraint) — reachable from two same-file trait
    /// impls of one method name sharing a uid.
    #[test]
    fn batch_duplicate_uid_folds_like_sql() {
        let (_tmp, db) = open_db();
        write_batch(&db, &[], &[], &[]); // initialize the aggregate baseline
        let mut first = seed_row("a.rs", "fmt_display", 1);
        first.symbol_uid = Some("uid:a.rs:fmt".to_string());
        let mut second = seed_row("a.rs", "fmt_debug", 9);
        second.symbol_uid = Some("uid:a.rs:fmt".to_string());
        write_batch(&db, &[], &[unit("a.rs", vec![first, second])], &[]);

        assert_eq!(
            db.seed_cache_len(),
            Some(1),
            "duplicate uid must fold to one row, like INSERT OR REPLACE"
        );
        let cached = db.resolver_seed_symbols_excluding(&[]).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].name, "fmt_debug", "the LAST insert must win");
        assert_cache_equals_direct(&db, "after duplicate-uid batch");
    }

    #[test]
    fn projection_strips_non_seed_fields() {
        let mut sym = seed_row("a.rs", "one", 3);
        sym.signature = Some("fn one()".into());
        sym.doc = Some("doc".into());
        sym.start_col = 7;
        sym.parser_tier = ParserTier::TreeSitter;
        sym.parser_confidence = 0.9;
        sym.framework_role = Some("hook".into());
        sym.param_types = Some("u32".into());
        let projected = project_seed(&sym);
        assert_eq!(projected.signature, None);
        assert_eq!(projected.doc, None);
        assert_eq!(projected.start_col, 0);
        assert_eq!(projected.framework_role, None);
        assert_eq!(projected.param_types, None);
        assert_eq!(projected.parser_confidence, 0.0);
        assert_eq!(projected.qname, sym.qname);
        assert_eq!(projected.symbol_uid, sym.symbol_uid);
    }
}

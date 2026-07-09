//! Cross-build reuse of the resolver's [`SymbolCatalog`].
//!
//! The incremental resolve phase historically rebuilt the whole catalog
//! (9 lookup maps + the `TypeCatalog`) from every persisted symbol on every
//! build — an O(repo) floor (~0.8s at 278k symbols) even for a single-file
//! batch. This module parks the built catalog on the [`IndexDb`] handle
//! between builds (the only object that survives — the `Indexer` is rebuilt
//! per build, same host rationale as cc-db's seed-row cache) and
//! delta-maintains it:
//!
//! - **take** (phase 4a): validate the parked catalog's token against the
//!   persisted `symbols_seed` aggregate; on a match, remove the batch/removed
//!   files' entries and let the build layer its fresh batch symbols on top.
//! - **fold** (post-write): replace the batch files' in-build entries with
//!   the *final written* units' rows (post-enrichment, seed-projected,
//!   deduplicated exactly like the SQL `INSERT OR REPLACE`), stamp the
//!   post-batch token read inside the write transaction, and park the result
//!   for the next build.
//!
//! # Validity
//!
//! Correctness rests entirely on the `symbols_seed` aggregate (see
//! cc-db's `signature_agg` / `seed_symbol_cache`): equal token ⇒ equal
//! persisted seed-row multiset. Every store carries the token proven inside
//! the producing transaction, every take re-validates against the currently
//! stored token, and a `live-count == token.count` sanity check guards the
//! fold. Any writer outside the incremental batch path (full rebuilds,
//! config-link symbol writes, cross-process mutations) moves the token and
//! the next take simply misses — stale reuse is impossible, only a cold
//! rebuild. In-process, the per-project build gate plus the prepare/commit
//! `index_epoch` guard prevent a taken catalog from being committed against
//! a moved database.
//!
//! # Divergence contract
//!
//! A reused catalog holds the same entry *multiset* a fresh seed load would
//! produce, but not the same bucket *order* (fresh loads order by
//! `(file_path, start_line)`; a reused catalog keeps historical order and
//! appends). Resolution outcomes are order-independent except for
//! equal-score tie-breaks among ambiguous same-name candidates, where a
//! different (equally valid, already confidence-penalized) winner may be
//! picked than a cold rebuild would pick. Entry slots freed by removal are
//! tombstoned, never reused; when tombstones outnumber live entries the fold
//! declines to park and the next build rebuilds fresh (compaction by
//! reconstruction).
//!
//! The cache obeys the seed cache's capacity knob
//! (`CODECORTEX_SEED_CACHE_MAX_SYMBOLS`, `0` disables both).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cc_db::index_db::{FileWriteUnit, IndexDb, SeedTokenSpan};
use cc_db::RowAgg;
use cc_model::symbol::SymbolRecord;

use super::SymbolCatalog;

/// The parked payload: a catalog whose live entries equal the persisted
/// seed rows of the database state identified by `token`.
struct CachedResolverCatalog {
    token: RowAgg,
    catalog: SymbolCatalog,
}

/// Build-side transport: the catalog leaves phase 4 inside this carry (with
/// the basis token its persisted part corresponds to) and rides the
/// `PreparedBuild` into the commit, where [`after_write`] folds it.
pub(crate) struct CatalogCarry {
    pub(crate) basis: RowAgg,
    pub(crate) catalog: SymbolCatalog,
}

/// Take the parked catalog when its token matches the persisted aggregate.
/// Returns the validated basis token alongside the catalog. Misses (empty
/// slot, token mismatch, unreadable token) drop any stale occupant.
pub(crate) fn take_validated(db: &Arc<IndexDb>) -> Option<(RowAgg, SymbolCatalog)> {
    let parked = db.take_resolver_catalog()?;
    let cached = match parked.downcast::<CachedResolverCatalog>() {
        Ok(c) => *c,
        Err(_) => return None,
    };
    let current = db.reads().seed_token().ok().flatten()?;
    if current != cached.token {
        tracing::debug!(
            phase = "resolve",
            step = "catalog_cache",
            "parked catalog token mismatch; rebuilding fresh"
        );
        return None;
    }
    #[cfg(test)]
    test_observability::record_hit(db);
    Some((cached.token, cached.catalog))
}

/// Commit-side hook, called once after `phase_write` has committed all of
/// its writes (batch + config links + metadata). Decides whether a catalog
/// gets parked for the next build:
///
/// - full builds replace the whole symbol table → clear the slot;
/// - incremental builds with a carried catalog → [`fold_incremental`];
/// - incremental pure-removal batches (nothing parsed, so phase 4 never
///   took the catalog) → fold the removals into the still-parked catalog.
pub(crate) fn after_write(
    db: &Arc<IndexDb>,
    full: bool,
    carry: Option<CatalogCarry>,
    final_units: &[FileWriteUnit],
    to_remove: &[String],
    tokens: Option<SeedTokenSpan>,
) {
    if full {
        db.clear_resolver_catalog();
        return;
    }
    match (carry, tokens) {
        (Some(carry), Some(span)) => fold_incremental(db, carry, final_units, span),
        (None, Some(span)) if final_units.is_empty() && !to_remove.is_empty() => {
            fold_removals_on_parked(db, to_remove, span)
        }
        // No-op batches leave the parked catalog untouched (token unmoved);
        // a batch without a carry (basis unknown) can't prove a fold, and
        // any symbol write it performed moved the token anyway.
        (None, _) => {}
        (Some(_), None) => db.clear_resolver_catalog(),
    }
}

/// Fold one committed incremental batch into the carried catalog and park
/// the result. The carried catalog holds `persisted − excluded` plus the
/// *pre-write-phase* batch entries; the written rows are the post-enrichment
/// versions, so the batch files' entries are replaced wholesale with the
/// final units' rows (seed-projected, SQL-order deduplicated).
fn fold_incremental(
    db: &Arc<IndexDb>,
    carry: CatalogCarry,
    final_units: &[FileWriteUnit],
    span: SeedTokenSpan,
) {
    let (Some(pre), Some(post)) = (span.pre, span.post) else {
        db.clear_resolver_catalog();
        return;
    };
    // The batch transaction must have started from the state the catalog
    // was seeded on (the epoch guard already enforces this in-process;
    // this check makes the fold locally provable).
    if carry.basis != pre {
        db.clear_resolver_catalog();
        return;
    }
    // Writes after the batch (config-link units) may also touch symbols;
    // fold only when the stored token still equals the batch's post state.
    if db.reads().seed_token().ok().flatten() != Some(post) {
        db.clear_resolver_catalog();
        return;
    }

    let mut catalog = carry.catalog;
    let batch_files: HashSet<String> = final_units.iter().map(|u| u.rel_path.clone()).collect();
    catalog.remove_files(&batch_files);
    let survivors = sql_folded_survivors(final_units);
    catalog.add_symbols(&survivors);
    catalog.type_catalog_add_symbols(survivors.iter());
    catalog.clear_resolve_cache();

    park_if_consistent(db, catalog, post);
}

/// Pure-removal batches never take the catalog through phase 4 (there is
/// nothing to resolve), so the removal delta is applied to the parked
/// catalog directly.
fn fold_removals_on_parked(db: &Arc<IndexDb>, to_remove: &[String], span: SeedTokenSpan) {
    let (Some(pre), Some(post)) = (span.pre, span.post) else {
        db.clear_resolver_catalog();
        return;
    };
    let Some(parked) = db.take_resolver_catalog() else {
        return;
    };
    let Ok(cached) = parked.downcast::<CachedResolverCatalog>() else {
        return;
    };
    let cached = *cached;
    if cached.token != pre || db.reads().seed_token().ok().flatten() != Some(post) {
        return; // stale basis: stay cold, next build reloads
    }
    let mut catalog = cached.catalog;
    let removed: HashSet<String> = to_remove.iter().cloned().collect();
    catalog.remove_files(&removed);
    park_if_consistent(db, catalog, post);
}

/// Final gate before parking: the live entry count must equal the token's
/// exact row count (the count half of the aggregate is not a hash), the
/// tombstone ratio must be healthy, and the capacity knob must allow it.
fn park_if_consistent(db: &Arc<IndexDb>, catalog: SymbolCatalog, token: RowAgg) {
    let live = catalog.live_len();
    if live as u64 != token.count {
        tracing::warn!(
            phase = "resolve",
            step = "catalog_cache",
            live,
            token_count = token.count,
            "folded catalog row count diverged from the seed aggregate; dropping cache"
        );
        return;
    }
    if catalog.should_compact() || live > cc_db::seed_cache_max_symbols() {
        return;
    }
    db.store_resolver_catalog(Box::new(CachedResolverCatalog { token, catalog }));
}

/// The batch rows as the database keeps them: seed-projected (the only
/// catalog-visible seed projection is `scope_id = None`; every other
/// projected column is either preserved verbatim or not read by the catalog)
/// and deduplicated with the SQL `INSERT OR REPLACE` last-wins semantics
/// over `symbol_id` (PK) and `symbol_uid` (UNIQUE) — `final_units` must be
/// in SQL execution order (normal units then dirty units).
fn sql_folded_survivors(final_units: &[FileWriteUnit]) -> Vec<SymbolRecord> {
    let mut kept: Vec<Option<SymbolRecord>> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut by_uid: HashMap<String, usize> = HashMap::new();
    for unit in final_units {
        for sym in &unit.outcome.symbols {
            let mut row = sym.clone();
            row.scope_id = None;
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
    kept.into_iter().flatten().collect()
}

#[cfg(test)]
pub(crate) fn parked_live_len(db: &Arc<IndexDb>) -> Option<usize> {
    let parked = db.take_resolver_catalog()?;
    let cached = parked.downcast::<CachedResolverCatalog>().ok()?;
    let len = cached.catalog.live_len();
    db.store_resolver_catalog(cached);
    Some(len)
}

#[cfg(test)]
pub(crate) use test_observability::cache_hits;

/// Per-handle hit counters, keyed by the process-unique `instance_id` so
/// concurrently running tests cannot observe each other's hits.
#[cfg(test)]
mod test_observability {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use cc_db::index_db::IndexDb;

    static HITS: Mutex<Option<HashMap<u64, u64>>> = Mutex::new(None);

    pub(super) fn record_hit(db: &Arc<IndexDb>) {
        let mut guard = HITS.lock().expect("hit counter lock");
        *guard
            .get_or_insert_with(HashMap::new)
            .entry(db.admin().instance_id())
            .or_insert(0) += 1;
    }

    pub(crate) fn cache_hits(db: &Arc<IndexDb>) -> u64 {
        HITS.lock()
            .expect("hit counter lock")
            .as_ref()
            .and_then(|m| m.get(&db.admin().instance_id()).copied())
            .unwrap_or(0)
    }
}

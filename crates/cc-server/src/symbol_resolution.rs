//! Shared symbol-name → candidate resolution for graph tools.
//!
//! C3 step A: `trace_path` / `explore_flow` / `type_hierarchy` previously each
//! implemented their own name → UID disambiguation inline. This module hosts a
//! single `resolve()` pipeline; the per-tool differences are expressed
//! explicitly via [`ResolutionOpts`] presets (`for_trace` / `for_flow` /
//! `for_type_hierarchy`) so the strategies stay byte-for-byte identical to the
//! pre-refactor behavior. Step B (strategy convergence) will edit the presets
//! here instead of hunting through three files.
//!
//! Deliberately **not** modeled here (they skip name resolution entirely and
//! stay in the callers):
//! - trace's `from_uid` / `to_uid` override (string `':'` check, no DB lookup)
//! - type_hierarchy's `symbol_uid` override (direct `symbol_rows_by_uids` fetch)

use cc_db::index_db::{IndexDb, SymbolRow};
use cc_model::CcResult;

/// How candidate rows are matched against a file-path filter.
///
/// explore_flow uses substring `contains`; type_hierarchy uses exact equality.
/// This difference is intentional and preserved (step B input).
#[derive(Debug, Clone, Copy)]
pub(crate) enum FileFilter<'a> {
    /// Keep rows whose `file_path` contains the given fragment (explore_flow).
    Contains(&'a str),
    /// Keep rows whose `file_path` equals the given path (type_hierarchy).
    Equals(&'a str),
}

/// Why resolution produced no candidate. Callers map these to their own
/// error/response shapes (type_hierarchy emits a distinct message per reason;
/// explore_flow treats all reasons as "unresolved").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnresolvedReason {
    /// `find_symbol` returned no rows at all.
    NotFound,
    /// Rows existed but the file filter removed all of them.
    FilteredByFile,
    /// Rows existed but the kind filter removed all of them.
    FilteredByKind,
}

/// Outcome of resolving a symbol name against the index.
pub(crate) enum Resolution {
    /// Exactly one candidate (either a single match, or the co-location
    /// heuristic picked a winner among several).
    Unique(SymbolRow),
    /// Multiple candidates remained; rows are in `find_symbol` order
    /// (ORDER BY file_path). Never has fewer than two entries.
    Ambiguous(Vec<SymbolRow>),
    Unresolved(UnresolvedReason),
}

/// Symbol kinds considered "types" by type_hierarchy's kind filter.
pub(crate) const TYPE_KINDS: &[&str] = &[
    "class",
    "interface",
    "enum",
    "struct",
    "trait",
    "abstract_class",
];

/// All knobs needed to replicate the three call sites. Presets below pin the
/// exact pre-refactor combination for each tool; do not mix knobs that a
/// preset never combines (e.g. `file_filter` + `kind_filter`) without checking
/// the pipeline order in [`resolve`].
pub(crate) struct ResolutionOpts<'a> {
    /// Passed through to `IndexDb::find_symbol` (exact name vs LIKE).
    pub exact: bool,
    /// Passed through to `IndexDb::find_symbol` as `top_k`.
    pub max_candidates: usize,
    /// Optional file-path filter, applied after the empty-result check.
    pub file_filter: Option<FileFilter<'a>>,
    /// Optional kind filter, applied after the file filter (presets never set
    /// both; type_hierarchy applies kinds only when no file path is given).
    pub kind_filter: Option<&'static [&'static str]>,
    /// Already-resolved `(query, uid)` pairs for explore_flow's co-location
    /// heuristic: among multiple candidates, prefer one whose directory
    /// matches the directory of any previously resolved symbol.
    pub co_location_resolved: Option<&'a [(String, String)]>,
}

impl<'a> ResolutionOpts<'a> {
    /// trace_path endpoints: exact match, up to 5 candidates, no filters.
    /// Ambiguity is folded to the first candidate by the caller.
    pub(crate) fn for_trace() -> Self {
        Self {
            exact: true,
            max_candidates: 5,
            file_filter: None,
            kind_filter: None,
            co_location_resolved: None,
        }
    }

    /// explore_flow symbols: caller-supplied `exact` / `max_candidates`,
    /// substring file filter, co-location heuristic against the symbols
    /// resolved so far in the same request.
    pub(crate) fn for_flow(
        exact: bool,
        max_candidates: usize,
        file_path_filter: Option<&'a str>,
        already_resolved: &'a [(String, String)],
    ) -> Self {
        Self {
            exact,
            max_candidates,
            file_filter: file_path_filter.map(FileFilter::Contains),
            kind_filter: None,
            co_location_resolved: Some(already_resolved),
        }
    }

    /// type_hierarchy root: exact match, up to 10 candidates. With a file
    /// path: exact-equality file filter and **no** kind filter; without one:
    /// filter to type kinds. Ambiguity surfaces as a candidates response.
    pub(crate) fn for_type_hierarchy(file_path: Option<&'a str>) -> Self {
        Self {
            exact: true,
            max_candidates: 10,
            file_filter: file_path.map(FileFilter::Equals),
            kind_filter: if file_path.is_none() {
                Some(TYPE_KINDS)
            } else {
                None
            },
            co_location_resolved: None,
        }
    }
}

/// Resolve a symbol name to candidate rows according to `opts`.
///
/// Pipeline: `find_symbol` → empty check → file filter → kind filter →
/// single/multi split → (multi only) co-location pick → ambiguous.
pub(crate) fn resolve(db: &IndexDb, name: &str, opts: &ResolutionOpts) -> CcResult<Resolution> {
    let mut rows = db
        .reads()
        .find_symbol(name, opts.exact, opts.max_candidates)?;
    if rows.is_empty() {
        return Ok(Resolution::Unresolved(UnresolvedReason::NotFound));
    }

    if let Some(filter) = &opts.file_filter {
        match filter {
            FileFilter::Contains(fragment) => rows.retain(|r| r.file_path.contains(fragment)),
            FileFilter::Equals(path) => rows.retain(|r| r.file_path == *path),
        }
        if rows.is_empty() {
            return Ok(Resolution::Unresolved(UnresolvedReason::FilteredByFile));
        }
    }

    if let Some(kinds) = opts.kind_filter {
        rows.retain(|r| kinds.contains(&r.kind.as_str()));
        if rows.is_empty() {
            return Ok(Resolution::Unresolved(UnresolvedReason::FilteredByKind));
        }
    }

    if rows.len() == 1 {
        return Ok(Resolution::Unique(rows.into_iter().next().unwrap()));
    }

    // Co-location heuristic (explore_flow): if one candidate shares a
    // directory with an already-resolved symbol, prefer it over the others.
    // A co-located candidate without a UID falls through to Ambiguous,
    // matching the original inline logic.
    if let Some(already_resolved) = opts.co_location_resolved {
        let colocated = rows.iter().find(|r| {
            let candidate_dir = dir_of(&r.file_path);
            already_resolved.iter().any(|(_, resolved_uid)| {
                // Look up the resolved symbol's file_path from the DB
                if let Ok(map) = db
                    .reads()
                    .symbol_rows_by_uids(std::slice::from_ref(resolved_uid))
                {
                    if let Some(resolved_row) = map.get(resolved_uid) {
                        return dir_of(&resolved_row.file_path) == candidate_dir;
                    }
                }
                false
            })
        });
        if let Some(best) = colocated {
            if best.symbol_uid.is_some() {
                return Ok(Resolution::Unique(best.clone()));
            }
        }
    }

    Ok(Resolution::Ambiguous(rows))
}

/// Return the directory portion of a file_path (everything before the last `/`).
fn dir_of(file_path: &str) -> &str {
    match file_path.rfind('/') {
        Some(pos) => &file_path[..pos],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Helper: create an IndexDb and insert symbols with the given
    /// (symbol_id, symbol_uid, name, kind, file_path) rows.
    fn setup_db(symbols: &[(&str, &str, &str, &str, &str)]) -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("resolution.db")).unwrap().0);
        let conn = db.reads().read_conn().unwrap();

        let mut seen_files = std::collections::HashSet::new();
        for (_, _, _, _, file_path) in symbols {
            if seen_files.insert(*file_path) {
                conn.execute(
                    "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
                     VALUES(?1,'Rust','h1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                    rusqlite::params![file_path],
                ).unwrap();
            }
        }

        for (sid, uid, name, kind, file_path) in symbols {
            conn.execute(
                "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
                  start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
                  parent_symbol_id, export_name, is_default_export,
                  framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
                 VALUES(?1,?2,?3,?4,?5,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params![sid, uid, name, kind, file_path],
            ).unwrap();
        }

        (tmp, db)
    }

    // ── for_trace ───────────────────────────────────────────────

    #[test]
    fn trace_unique() {
        let (_tmp, db) = setup_db(&[("s1", "uid_a", "fn_a", "function", "src/lib.rs")]);
        match resolve(&db, "fn_a", &ResolutionOpts::for_trace()).unwrap() {
            Resolution::Unique(row) => assert_eq!(row.symbol_uid.as_deref(), Some("uid_a")),
            _ => panic!("expected Unique"),
        }
    }

    #[test]
    fn trace_ambiguous_preserves_find_symbol_order() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_b", "fn_a", "function", "src/b.rs"),
            ("s2", "uid_a", "fn_a", "function", "src/a.rs"),
        ]);
        match resolve(&db, "fn_a", &ResolutionOpts::for_trace()).unwrap() {
            Resolution::Ambiguous(rows) => {
                // find_symbol orders by file_path, so src/a.rs comes first.
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].symbol_uid.as_deref(), Some("uid_a"));
                assert_eq!(rows[1].symbol_uid.as_deref(), Some("uid_b"));
            }
            _ => panic!("expected Ambiguous"),
        }
    }

    #[test]
    fn trace_unresolved() {
        let (_tmp, db) = setup_db(&[("s1", "uid_a", "fn_a", "function", "src/lib.rs")]);
        match resolve(&db, "missing", &ResolutionOpts::for_trace()).unwrap() {
            Resolution::Unresolved(reason) => assert_eq!(reason, UnresolvedReason::NotFound),
            _ => panic!("expected Unresolved"),
        }
    }

    // ── for_flow ────────────────────────────────────────────────

    #[test]
    fn flow_unique() {
        let (_tmp, db) = setup_db(&[("s1", "uid_a", "fn_a", "function", "src/lib.rs")]);
        let resolved: Vec<(String, String)> = Vec::new();
        let opts = ResolutionOpts::for_flow(true, 10, None, &resolved);
        match resolve(&db, "fn_a", &opts).unwrap() {
            Resolution::Unique(row) => assert_eq!(row.symbol_uid.as_deref(), Some("uid_a")),
            _ => panic!("expected Unique"),
        }
    }

    #[test]
    fn flow_ambiguous_without_co_location_context() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_a1", "fn_a", "function", "src/a.rs"),
            ("s2", "uid_a2", "fn_a", "function", "src/other/a.rs"),
        ]);
        let resolved: Vec<(String, String)> = Vec::new();
        let opts = ResolutionOpts::for_flow(true, 10, None, &resolved);
        match resolve(&db, "fn_a", &opts).unwrap() {
            Resolution::Ambiguous(rows) => assert_eq!(rows.len(), 2),
            _ => panic!("expected Ambiguous"),
        }
    }

    #[test]
    fn flow_co_location_picks_same_directory_candidate() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_b", "fn_b", "function", "src/other/b.rs"),
            ("s2", "uid_a1", "fn_a", "function", "src/a.rs"),
            ("s3", "uid_a2", "fn_a", "function", "src/other/a.rs"),
        ]);
        // fn_b in src/other/ is already resolved → prefer fn_a in src/other/.
        let resolved = vec![("fn_b".to_string(), "uid_b".to_string())];
        let opts = ResolutionOpts::for_flow(true, 10, None, &resolved);
        match resolve(&db, "fn_a", &opts).unwrap() {
            Resolution::Unique(row) => assert_eq!(row.symbol_uid.as_deref(), Some("uid_a2")),
            _ => panic!("expected Unique via co-location"),
        }
    }

    #[test]
    fn flow_file_filter_uses_substring_match() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_a1", "fn_a", "function", "src/a.rs"),
            ("s2", "uid_a2", "fn_a", "function", "src/other/a.rs"),
        ]);
        let resolved: Vec<(String, String)> = Vec::new();
        let opts = ResolutionOpts::for_flow(true, 10, Some("other"), &resolved);
        match resolve(&db, "fn_a", &opts).unwrap() {
            Resolution::Unique(row) => assert_eq!(row.symbol_uid.as_deref(), Some("uid_a2")),
            _ => panic!("expected Unique after file filter"),
        }
    }

    #[test]
    fn flow_unresolved_no_match_and_no_file_match() {
        let (_tmp, db) = setup_db(&[("s1", "uid_a", "fn_a", "function", "src/lib.rs")]);
        let resolved: Vec<(String, String)> = Vec::new();

        let opts = ResolutionOpts::for_flow(true, 10, None, &resolved);
        match resolve(&db, "missing", &opts).unwrap() {
            Resolution::Unresolved(reason) => assert_eq!(reason, UnresolvedReason::NotFound),
            _ => panic!("expected Unresolved"),
        }

        let opts = ResolutionOpts::for_flow(true, 10, Some("nowhere"), &resolved);
        match resolve(&db, "fn_a", &opts).unwrap() {
            Resolution::Unresolved(reason) => assert_eq!(reason, UnresolvedReason::FilteredByFile),
            _ => panic!("expected Unresolved"),
        }
    }

    // ── for_type_hierarchy ──────────────────────────────────────

    #[test]
    fn hierarchy_unique_kind_filter_drops_non_types() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_fn", "Client", "function", "src/a.ts"),
            ("s2", "uid_cls", "Client", "class", "src/b.ts"),
        ]);
        match resolve(&db, "Client", &ResolutionOpts::for_type_hierarchy(None)).unwrap() {
            Resolution::Unique(row) => {
                assert_eq!(row.symbol_uid.as_deref(), Some("uid_cls"));
                assert_eq!(row.kind, "class");
            }
            _ => panic!("expected Unique"),
        }
    }

    #[test]
    fn hierarchy_ambiguous_returns_all_type_candidates() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_c1", "Client", "class", "src/a.ts"),
            ("s2", "uid_c2", "Client", "class", "src/b.ts"),
        ]);
        match resolve(&db, "Client", &ResolutionOpts::for_type_hierarchy(None)).unwrap() {
            Resolution::Ambiguous(rows) => assert_eq!(rows.len(), 2),
            _ => panic!("expected Ambiguous"),
        }
    }

    #[test]
    fn hierarchy_unresolved_reasons() {
        let (_tmp, db) = setup_db(&[("s1", "uid_fn", "Client", "function", "src/a.ts")]);

        match resolve(&db, "missing", &ResolutionOpts::for_type_hierarchy(None)).unwrap() {
            Resolution::Unresolved(reason) => assert_eq!(reason, UnresolvedReason::NotFound),
            _ => panic!("expected Unresolved"),
        }

        // Only a function named Client exists → kind filter removes it.
        match resolve(&db, "Client", &ResolutionOpts::for_type_hierarchy(None)).unwrap() {
            Resolution::Unresolved(reason) => assert_eq!(reason, UnresolvedReason::FilteredByKind),
            _ => panic!("expected Unresolved"),
        }

        // File filter (exact equality) removes everything.
        match resolve(
            &db,
            "Client",
            &ResolutionOpts::for_type_hierarchy(Some("src/elsewhere.ts")),
        )
        .unwrap()
        {
            Resolution::Unresolved(reason) => assert_eq!(reason, UnresolvedReason::FilteredByFile),
            _ => panic!("expected Unresolved"),
        }
    }

    #[test]
    fn hierarchy_file_filter_is_exact_and_skips_kind_filter() {
        let (_tmp, db) = setup_db(&[
            ("s1", "uid_fn", "Client", "function", "src/a.ts"),
            ("s2", "uid_cls", "Client", "class", "src/b.ts"),
        ]);
        // With file_path given, the kind filter is NOT applied, so the
        // function in src/a.ts resolves as Unique.
        match resolve(
            &db,
            "Client",
            &ResolutionOpts::for_type_hierarchy(Some("src/a.ts")),
        )
        .unwrap()
        {
            Resolution::Unique(row) => {
                assert_eq!(row.symbol_uid.as_deref(), Some("uid_fn"));
                assert_eq!(row.kind, "function");
            }
            _ => panic!("expected Unique"),
        }
    }
}

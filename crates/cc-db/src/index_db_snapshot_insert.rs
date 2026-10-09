//! Full-snapshot leaf inserts bounded within one file and one original table loop.
//!
//! No row survives into another file/table. Parent inserts, FTS mirrors, document
//! validation and identity survivor checks remain in their original positions.
//! Existing incremental binders execute each bounded window using their unchanged
//! 64/8/1 tiers, conflict clauses and parameter order.
use crate::index_db::IndexDb;
use crate::sql_util::db_err;
use cc_model::{CcError, CcResult};
use rusqlite::{
    types::{ToSqlOutput, ValueRef},
    Connection, ToSql,
};

const MAX_ROWS: usize = 64;
const MAX_BOUND_BYTES: usize = 64 * 1024;

fn value_bytes(value: ValueRef<'_>) -> usize {
    match value {
        ValueRef::Null => 0,
        ValueRef::Integer(_) | ValueRef::Real(_) => 8,
        ValueRef::Text(value) | ValueRef::Blob(value) => value.len(),
    }
}

// Bound the actual bound-value payload, not a JSON estimate. The model rows
// bind only primitive/optional values; borrowed text stays borrowed.
// SQLite's own statement/index allocations are not an RSS bound.
fn payload_bytes(values: &[&dyn ToSql]) -> CcResult<usize> {
    values.iter().try_fold(0usize, |total, value| {
        let bytes = match value.to_sql().map_err(db_err)? {
            ToSqlOutput::Borrowed(value) => value_bytes(value),
            ToSqlOutput::Owned(value) => value_bytes(ValueRef::from(&value)),
            _ => {
                return Err(CcError::Database(
                    "unsupported snapshot leaf bound value".into(),
                ))
            }
        };
        Ok(total.saturating_add(bytes))
    })
}

fn bounded_rows<'a, T: 'a>(
    rows: impl IntoIterator<Item = &'a T>,
    size: impl Fn(&T) -> CcResult<usize>,
    write: impl Fn(&[&T]) -> CcResult<()>,
) -> CcResult<()> {
    let rows = rows.into_iter();
    if rows.size_hint().1 == Some(0) {
        return Ok(());
    }
    let mut pending = Vec::with_capacity(MAX_ROWS.min(rows.size_hint().0));
    let mut bytes = 0usize;
    for row in rows {
        let row_bytes = size(row)?;
        if !pending.is_empty() && (pending.len() == MAX_ROWS || row_bytes > MAX_BOUND_BYTES - bytes)
        {
            write(&pending)?;
            pending.clear();
            bytes = 0;
        }
        if row_bytes > MAX_BOUND_BYTES {
            // A legal large row keeps the single-row binder. This byte limit
            // limits batching; it does not add a product row-size rejection.
            write(&[row])?;
        } else {
            pending.push(row);
            bytes += row_bytes;
        }
    }
    if !pending.is_empty() {
        write(&pending)?;
    }
    Ok(())
}

/// Borrow the normalized BTreeSet directly; do not collect the whole file or
/// carry rows across file boundaries. Large legal rows keep the single-row path.
pub(crate) fn resolution_dependencies(
    conn: &Connection,
    file_path: &str,
    rows: &std::collections::BTreeSet<cc_model::resolution::ResolutionDependency>,
) -> CcResult<()> {
    bounded_rows(
        rows,
        |dependency| {
            payload_bytes(rusqlite::params![
                file_path,
                dependency.kind.as_str(),
                &dependency.key,
            ])
        },
        |batch| IndexDb::insert_resolution_dependencies_multi(conn, file_path, batch),
    )
}

pub(crate) fn symbols(conn: &Connection, rows: &[cc_model::symbol::SymbolRecord]) -> CcResult<()> {
    bounded_rows(
        rows,
        |s| {
            payload_bytes(rusqlite::params![
                &s.symbol_id,
                &s.file_path,
                &s.name,
                s.kind.as_str(),
                &s.container,
                s.start_line,
                s.end_line,
                s.start_col,
                s.end_col,
                &s.signature,
                &s.doc,
                s.parser_tier.as_str(),
                s.parser_confidence,
                &s.qname,
                &s.parent_symbol_id,
                &s.export_name,
                s.is_default_export as i32,
                &s.symbol_uid,
                &s.framework_role,
                &s.receiver_type,
                &s.param_types,
                &s.return_type,
                s.param_count,
                &s.base_types,
                &s.implements,
            ])
        },
        |batch| IndexDb::insert_symbols_multi(conn, batch, true),
    )
}

pub(crate) fn symbol_refs(
    conn: &Connection,
    rows: &[cc_model::symbol::SymbolRefRecord],
) -> CcResult<()> {
    bounded_rows(
        rows,
        |r| {
            payload_bytes(rusqlite::params![
                &r.ref_id,
                &r.file_path,
                &r.symbol_name,
                &r.container,
                &r.ref_kind,
                r.line,
                r.column,
                &r.target_symbol_id,
                &r.target_file_path,
                &r.target_symbol_uid,
                &r.ref_name,
                r.resolution_kind.as_str(),
                r.resolution_confidence,
                &r.resolution_strategy,
                r.ref_end_line,
                r.ref_end_col,
                r.parser_tier.as_str(),
                r.parser_confidence,
            ])
        },
        |batch| IndexDb::insert_symbol_refs_multi(conn, batch, true),
    )
}

pub(crate) fn call_edges(
    conn: &Connection,
    rows: &[cc_model::edge::CallEdgeRecord],
) -> CcResult<()> {
    bounded_rows(
        rows,
        |e| {
            payload_bytes(rusqlite::params![
                &e.edge_id,
                &e.file_path,
                &e.caller_symbol,
                &e.callee_symbol,
                e.line,
                e.start_col,
                e.end_line,
                e.end_col,
                &e.target_symbol_id,
                &e.target_file_path,
                &e.caller_symbol_id,
                &e.callee_ref_id,
                &e.caller_symbol_uid,
                &e.callee_symbol_uid,
                e.dispatch_kind.as_str(),
                &e.call_kind,
                e.resolution_kind.as_str(),
                e.resolution_confidence,
                &e.resolution_strategy,
                &e.receiver_expr,
                e.arg_count.map(|v| v as i32),
                e.is_optional_chain as i32,
                e.is_awaited as i32,
                e.is_constructor as i32,
                e.parser_tier.as_str(),
                e.parser_confidence,
                &e.synthesized_by,
                &e.synthesis_key,
                &e.registered_file,
                e.registered_line.map(|v| v as i32),
            ])
        },
        |batch| IndexDb::insert_call_edges_multi(conn, batch),
    )
}

pub(crate) fn route_edges(
    conn: &Connection,
    rows: &[cc_model::edge::RouteEdgeRecord],
) -> CcResult<()> {
    bounded_rows(
        rows,
        |r| {
            payload_bytes(rusqlite::params![
                &r.edge_id,
                &r.file_path,
                &r.route_path,
                &r.handler_name,
                &r.method,
                r.line,
                r.start_col,
                r.end_line,
                r.end_col,
                &r.handler_symbol_id,
                &r.handler_symbol_uid,
                &r.handler_expr,
                &r.router_symbol_uid,
                &r.framework,
                &r.route_kind,
                r.confidence,
                r.parser_tier.as_str(),
                &r.resolution_strategy,
                r.resolution_confidence,
            ])
        },
        |batch| IndexDb::insert_route_edges_multi(conn, batch, true),
    )
}

pub(crate) fn semantic_edges(
    conn: &Connection,
    rows: &[cc_model::edge::SemanticEdgeRecord],
) -> CcResult<()> {
    bounded_rows(
        rows,
        |se| {
            payload_bytes(rusqlite::params![
                &se.edge_id,
                &se.file_path,
                &se.source_symbol,
                &se.source_symbol_uid,
                &se.target_symbol,
                &se.target_symbol_uid,
                se.relation_kind.as_str(),
                se.line,
                se.confidence,
                se.parser_tier.as_str(),
            ])
        },
        |batch| IndexDb::insert_semantic_edges_multi(conn, batch),
    )
}

pub(crate) fn dispatch_sites(
    conn: &Connection,
    rows: &[cc_model::dispatch_site::DispatchSiteRecord],
) -> CcResult<()> {
    bounded_rows(
        rows,
        |ds| {
            payload_bytes(rusqlite::params![
                &ds.site_id,
                &ds.file_path,
                ds.line,
                ds.col,
                &ds.enclosing_symbol_uid,
                &ds.receiver_expr,
                ds.site_kind.as_str(),
                &ds.key,
                &ds.handler_expr,
                &ds.handler_symbol_uid,
                ds.confidence,
            ])
        },
        |batch| IndexDb::insert_dispatch_sites_multi(conn, batch),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::StatementStatus;
    use std::cell::RefCell;

    #[test]
    fn bound_payload_measures_sql_values_and_keeps_large_rows_legal() {
        let text = "中文";
        let blob = [0u8, 1, 2];
        assert_eq!(
            payload_bytes(rusqlite::params![
                text,
                &blob[..],
                4i64,
                -0.0f64,
                None::<String>
            ])
            .unwrap(),
            25
        );
        let rows = vec![1usize; 65]
            .into_iter()
            .chain([MAX_BOUND_BYTES, MAX_BOUND_BYTES + 1, 2])
            .collect::<Vec<_>>();
        let observed = RefCell::new(Vec::new());
        bounded_rows(
            &rows,
            |n| Ok(*n),
            |batch| {
                assert!(batch.len() <= MAX_ROWS);
                assert!(
                    batch.len() == 1 || batch.iter().copied().sum::<usize>() <= MAX_BOUND_BYTES
                );
                observed.borrow_mut().extend(batch.iter().map(|r| **r));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*observed.borrow(), rows);
    }

    #[test]
    fn failed_window_stops_before_later_rows_or_tables() {
        let rows = vec![1usize; 130];
        let calls = RefCell::new(Vec::new());
        let result = bounded_rows(
            &rows,
            |_| Ok(1),
            |batch| {
                calls.borrow_mut().push(batch.len());
                Err(CcError::Database("injected execution failure".into()))
            },
        );
        assert!(result.is_err());
        assert_eq!(*calls.borrow(), vec![64]);
    }

    #[test]
    fn semantic_leaf_batches_reduce_actual_sqlite_statement_runs() {
        const HEAD: &str = "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES";
        let db = || {
            let conn = Connection::open_in_memory().unwrap();
            conn.set_prepared_statement_cache_capacity(64);
            conn.execute_batch("CREATE TABLE semantic_edges(edge_id TEXT PRIMARY KEY,file_path TEXT,source_symbol TEXT,source_symbol_uid TEXT,target_symbol TEXT,target_symbol_uid TEXT,relation_kind TEXT,line INTEGER,confidence REAL,parser_tier TEXT)").unwrap();
            conn
        };
        let old = db();
        let new = db();
        let rows: Vec<_> = (0..73)
            .map(|n| cc_model::edge::SemanticEdgeRecord {
                edge_id: format!("e{n}"),
                file_path: "one.py".into(),
                source_symbol: "source".into(),
                source_symbol_uid: None,
                target_symbol: "target".into(),
                target_symbol_uid: Some(format!("uid{n}")),
                relation_kind: cc_model::edge::SemanticRelation::Inherits,
                line: n,
                confidence: 0.5,
                parser_tier: cc_model::ParserTier::Generic,
            })
            .collect();
        let old_sql = format!("{HEAD}(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)");
        for row in &rows {
            IndexDb::execute_cached(
                &old,
                &old_sql,
                rusqlite::params![
                    row.edge_id,
                    row.file_path,
                    row.source_symbol,
                    row.source_symbol_uid,
                    row.target_symbol,
                    row.target_symbol_uid,
                    row.relation_kind.as_str(),
                    row.line,
                    row.confidence,
                    row.parser_tier.as_str()
                ],
            )
            .unwrap();
        }
        semantic_edges(&new, &rows).unwrap();
        let old_runs = old
            .prepare_cached(&old_sql)
            .unwrap()
            .get_status(StatementStatus::Run);
        let mut new_runs = 0;
        for n in [64, 8, 1] {
            let sql = format!("{HEAD}{}", vec!["(?,?,?,?,?,?,?,?,?,?)"; n].join(","));
            new_runs += new
                .prepare_cached(&sql)
                .unwrap()
                .get_status(StatementStatus::Run);
        }
        assert_eq!(old_runs, rows.len() as i32);
        assert!(new_runs > 0 && new_runs < old_runs);
        eprintln!("actual SQLite statement runs: original={old_runs}, snapshot={new_runs}");
        let read = |conn: &Connection| {
            conn.prepare("SELECT * FROM semantic_edges ORDER BY edge_id")
                .unwrap()
                .query_map([], |r| {
                    (0..10)
                        .map(|c| r.get::<_, rusqlite::types::Value>(c))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(read(&old), read(&new));
    }

    fn dependency_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.set_prepared_statement_cache_capacity(64);
        conn.execute_batch(
            "CREATE TABLE resolution_dependencies(
                file_path TEXT NOT NULL,kind TEXT NOT NULL,key TEXT NOT NULL,
                PRIMARY KEY(file_path,kind,key));",
        )
        .unwrap();
        conn
    }

    fn dependency_rows(conn: &Connection) -> Vec<(i64, String, String, String)> {
        let mut statement = conn
            .prepare("SELECT rowid,file_path,kind,key FROM resolution_dependencies ORDER BY rowid")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn dependency_insert_runs(conn: &Connection) -> i32 {
        [64, 8, 1]
            .into_iter()
            .map(|n| {
                let sql = format!(
                    "INSERT INTO resolution_dependencies(file_path,kind,key) VALUES{}",
                    vec!["(?,?,?)"; n].join(",")
                );
                conn.prepare_cached(&sql)
                    .unwrap()
                    .get_status(StatementStatus::Run)
            })
            .sum()
    }

    #[test]
    fn resolution_snapshot_batches_match_original_order_and_statement_work() {
        use cc_model::resolution::{DependencyKind, ResolutionDependency};
        use std::collections::BTreeSet;

        const ORIGINAL: &str =
            "INSERT INTO resolution_dependencies(file_path,kind,key) VALUES(?1,?2,?3)";
        let old = dependency_test_db();
        let new = dependency_test_db();
        let mut expected_rows = 0usize;
        for count in [0usize, 1, 7, 8, 9, 63, 64, 65, 73, 129, 4096] {
            let path = format!("file-{count}.py");
            let mut dependencies = BTreeSet::new();
            for n in (0..count).rev() {
                let kind = match n % 7 {
                    0 => DependencyKind::TargetSurface,
                    1 => DependencyKind::NameBucket,
                    2 => DependencyKind::MissingPath,
                    3 => DependencyKind::ModuleConfig,
                    4 => DependencyKind::PackageFiles,
                    5 => DependencyKind::SymbolInventory,
                    _ => DependencyKind::FileInventory,
                };
                let dependency = ResolutionDependency::new(kind, format!("键-{n:05}"));
                dependencies.insert(dependency.clone());
                dependencies.insert(dependency);
            }
            expected_rows += dependencies.len();
            for dependency in &dependencies {
                IndexDb::execute_cached(
                    &old,
                    ORIGINAL,
                    rusqlite::params![path, dependency.kind.as_str(), dependency.key],
                )
                .unwrap();
            }
            resolution_dependencies(&new, &path, &dependencies).unwrap();
            assert_eq!(dependency_rows(&old), dependency_rows(&new));
        }
        let old_runs = old
            .prepare_cached(ORIGINAL)
            .unwrap()
            .get_status(StatementStatus::Run);
        let new_runs = dependency_insert_runs(&new);
        assert_eq!(old_runs, expected_rows as i32);
        assert!(new_runs > 0 && new_runs < old_runs);
        eprintln!("resolution INSERT runs: original={old_runs}, snapshot={new_runs}");
    }

    #[test]
    fn resolution_snapshot_byte_windows_keep_legal_large_rows_single() {
        use cc_model::resolution::{DependencyKind, ResolutionDependency};
        use std::collections::BTreeSet;

        let db = dependency_test_db();
        // Each legal 4096-byte key counts its repeated file and kind bindings,
        // so this window is byte-limited before the 64-row cap.
        let dependencies: BTreeSet<_> = (0..64)
            .map(|n| {
                ResolutionDependency::new(
                    DependencyKind::NameBucket,
                    format!("{n:04}{}", "x".repeat(4092)),
                )
            })
            .collect();
        let before = dependency_insert_runs(&db);
        resolution_dependencies(&db, "bytes.py", &dependencies).unwrap();
        assert!(dependency_insert_runs(&db) - before > 1);
        assert_eq!(dependency_rows(&db).len(), dependencies.len());

        // The existing byte bound limits batching, not acceptance of a bound
        // value. This helper receives SQL values, not filesystem paths.
        let large_file_value = "p".repeat(MAX_BOUND_BYTES + 1);
        let three: BTreeSet<_> = dependencies.into_iter().take(3).collect();
        let before = dependency_insert_runs(&db);
        resolution_dependencies(&db, &large_file_value, &three).unwrap();
        assert_eq!(dependency_insert_runs(&db) - before, 3);
        let rows = dependency_rows(&db);
        assert_eq!(rows.len(), 67);
        assert!(rows[64..].iter().all(|row| row.1 == large_file_value));
    }
}

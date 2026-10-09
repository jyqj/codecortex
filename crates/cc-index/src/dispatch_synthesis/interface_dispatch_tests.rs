//! Real SQLite controls for the empty-prerequisite allocation fast paths.
//! Deliberately malformed rows pin the original typed-read error boundaries;
//! they are not accepted input or a reason to bypass validation.

use super::compute_interface_dispatch_synthesis;
use crate::dispatch_synthesis::SynthesisConfig;
use crate::synthesis_pipeline::{apply_synthesis_round, EdgeDelta, SynthesisRound};
use crate::test_seed::seed_conn;
use cc_db::index_db::IndexDb;
use cc_model::CallEdgeRecord;
use tempfile::TempDir;

fn setup() -> (TempDir, IndexDb) {
    let temp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&temp.path().join("interface.sqlite3"))
        .unwrap()
        .0;
    seed_conn(&db)
        .execute(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
             VALUES('graph.ts','typescript','fixture',0,0,'2026-10-09')",
            [],
        )
        .unwrap();
    (temp, db)
}

fn symbol(db: &IndexDb, uid: &str, name: &str, kind: &str, container: Option<&str>) {
    seed_conn(db)
        .execute(
            "INSERT INTO symbols(symbol_id,file_path,symbol_uid,name,kind,container,start_line,end_line) \
             VALUES(?1,'graph.ts',?1,?2,?3,?4,1,10)",
            rusqlite::params![uid, name, kind, container],
        )
        .unwrap();
}

fn call(db: &IndexDb, edge_id: &str, caller: &str, synthesized_by: Option<&str>) {
    seed_conn(db)
        .execute(
            "INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,synthesized_by) \
             VALUES(?1,'graph.ts','run',7,?2,'uid:interface_method',?3)",
            rusqlite::params![edge_id, caller, synthesized_by],
        )
        .unwrap();
}

fn interface(db: &IndexDb, kind: &str) {
    symbol(db, "uid:interface", "Service", kind, None);
    symbol(db, "uid:interface_method", "run", "method", Some("Service"));
}

fn implements(db: &IndexDb) {
    seed_conn(db)
        .execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('implements','graph.ts','Worker','uid:worker','Service','uid:interface','implements')",
            [],
        )
        .unwrap();
}

fn assert_cleanup_only(delta: &EdgeDelta) {
    assert_eq!(delta.delete_call_kinds, ["interface_dispatch"]);
    assert!(delta.delete_semantic_prefixes.is_empty());
    assert!(delta.insert_call_edges.is_empty());
    assert!(delta.insert_semantic_edges.is_empty());
}

#[test]
fn empty_prerequisites_remove_stale_edges_without_removing_real_calls() {
    for has_interface in [false, true] {
        let (_temp, db) = setup();
        if has_interface {
            interface(&db, "interface");
        } else {
            symbol(&db, "uid:ordinary", "ordinary", "function", None);
        }
        call(&db, "real", "uid:caller", None);
        call(&db, "stale", "uid:stale", Some("interface_dispatch"));

        let delta =
            compute_interface_dispatch_synthesis(&db, &SynthesisConfig::default(), &[]).unwrap();
        assert_cleanup_only(&delta);
        apply_synthesis_round(
            &db,
            &SynthesisRound {
                deltas: vec![delta],
            },
        )
        .unwrap();
        let rows = db
            .reads()
            .query_json("SELECT edge_id FROM call_edges ORDER BY edge_id", &[])
            .unwrap();
        assert_eq!(rows, vec![serde_json::json!({"edge_id":"real"})]);
    }
}

#[test]
fn no_calls_still_returns_before_reading_symbols_or_implements() {
    let (_temp, db) = setup();
    // The pass excludes its own old rows, so they do not defeat the original
    // no-calls branch. Removing either table must not become a new error.
    call(&db, "stale", "uid:stale", Some("interface_dispatch"));
    seed_conn(&db)
        .execute_batch("DROP TABLE symbols; DROP TABLE semantic_edges;")
        .unwrap();
    let delta =
        compute_interface_dispatch_synthesis(&db, &SynthesisConfig::default(), &[]).unwrap();
    assert_cleanup_only(&delta);
}

#[test]
fn ordinary_symbols_and_empty_interface_uid_do_not_read_implements() {
    for kind in ["function", "interface", "trait"] {
        let (_temp, db) = setup();
        let uid = if kind == "function" {
            "uid:ordinary"
        } else {
            ""
        };
        symbol(&db, uid, "Service", kind, None);
        call(&db, "real", "uid:caller", None);
        seed_conn(&db)
            .execute_batch("DROP TABLE semantic_edges;")
            .unwrap();
        let delta =
            compute_interface_dispatch_synthesis(&db, &SynthesisConfig::default(), &[]).unwrap();
        assert_cleanup_only(&delta);
    }
}

#[test]
fn disabled_pass_does_not_read_or_schedule_cleanup() {
    let (_temp, db) = setup();
    seed_conn(&db)
        .execute_batch("DROP TABLE call_edges;")
        .unwrap();
    let config = SynthesisConfig {
        enabled: false,
        ..SynthesisConfig::default()
    };
    let delta = compute_interface_dispatch_synthesis(&db, &config, &[]).unwrap();
    assert!(delta.delete_call_kinds.is_empty());
    assert!(delta.delete_semantic_prefixes.is_empty());
    assert!(delta.insert_call_edges.is_empty());
    assert!(delta.insert_semantic_edges.is_empty());
}

#[test]
fn malformed_calls_are_not_hidden_by_empty_implements_or_later_errors() {
    let (_temp, db) = setup();
    interface(&db, "interface");
    call(&db, "real", "uid:caller", None);
    seed_conn(&db)
        .execute_batch(
            "UPDATE call_edges SET line=-1; \
             UPDATE symbols SET container=x'80'; \
             DROP TABLE semantic_edges;",
        )
        .unwrap();
    let expected = db
        .symbol_graph_reads()
        .dispatch_call_edges_excluding_synthesized(&["interface_dispatch"])
        .expect_err("negative line must fail typed call decoding")
        .to_string();
    let actual = compute_interface_dispatch_synthesis(&db, &SynthesisConfig::default(), &[])
        .err()
        .expect("call error must precede symbols and implements")
        .to_string();
    assert_eq!(actual, expected);
}

#[test]
fn malformed_symbols_are_not_hidden_by_empty_implements() {
    let (_temp, db) = setup();
    symbol(&db, "uid:ordinary", "ordinary", "function", None);
    call(&db, "real", "uid:caller", None);
    seed_conn(&db)
        .execute("UPDATE symbols SET container=x'80'", [])
        .unwrap();
    let expected = db
        .symbol_graph_reads()
        .symbol_dispatch_rows()
        .expect_err("BLOB container must fail typed symbol decoding")
        .to_string();
    let actual = compute_interface_dispatch_synthesis(&db, &SynthesisConfig::default(), &[])
        .err()
        .expect("ordinary symbols are still decoded")
        .to_string();
    assert_eq!(actual, expected);
}

#[test]
fn malformed_implements_and_query_errors_propagate_after_the_original_guards() {
    for invalid_sql in [
        "UPDATE semantic_edges SET source_symbol_uid=x'80'",
        "DROP TABLE semantic_edges",
    ] {
        let (_temp, db) = setup();
        interface(&db, "interface");
        call(&db, "real", "uid:caller", None);
        implements(&db);
        seed_conn(&db).execute_batch(invalid_sql).unwrap();
        let expected = db
            .edge_reads()
            .semantic_uid_pairs_by_relation("implements")
            .expect_err("malformed implements input must fail")
            .to_string();
        let actual = compute_interface_dispatch_synthesis(&db, &SynthesisConfig::default(), &[])
            .err()
            .expect("implements errors remain visible")
            .to_string();
        assert_eq!(actual, expected);
    }
}

#[test]
fn prior_call_overlay_reaches_implementors_and_preserves_fanout_cap() {
    for kind in ["interface", "trait"] {
        let (_temp, db) = setup();
        interface(&db, kind);
        symbol(&db, "uid:worker", "Worker", "class", None);
        symbol(&db, "uid:worker_method", "run", "method", Some("Worker"));
        implements(&db);
        // There is no current persisted call. The old prior-kind edge must
        // be excluded, and its in-round replacement is the only input.
        call(&db, "prior_old", "uid:old_caller", Some("event_emitter"));
        let prior = EdgeDelta {
            delete_call_kinds: vec!["event_emitter"],
            insert_call_edges: vec![CallEdgeRecord {
                edge_id: "prior_new".into(),
                file_path: "graph.ts".into(),
                callee_symbol: "run".into(),
                caller_symbol_uid: Some("uid:new_caller".into()),
                callee_symbol_uid: Some("uid:interface_method".into()),
                line: 19,
                ..Default::default()
            }],
            ..Default::default()
        };
        for cap in [0, 1] {
            let config = SynthesisConfig {
                event_fanout_cap: cap,
                ..SynthesisConfig::default()
            };
            let delta =
                compute_interface_dispatch_synthesis(&db, &config, std::slice::from_ref(&prior))
                    .unwrap();
            if cap == 0 {
                assert_cleanup_only(&delta);
                continue;
            }
            assert_eq!(delta.insert_call_edges.len(), 1);
            let edge = &delta.insert_call_edges[0];
            assert_eq!(edge.caller_symbol_uid.as_deref(), Some("uid:new_caller"));
            assert_eq!(edge.callee_symbol_uid.as_deref(), Some("uid:worker_method"));
            assert_eq!(edge.line, 19);
            assert_eq!(edge.synthesis_key.as_deref(), Some("uid:interface::run"));
        }
    }
}

//! P4-D V20: release-only build/chunk/document/RSS mechanism observations.
use cc_model::{chunk_policy::ChunkPolicy, source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;
use cc_server::engine::CodeIndex;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{path::Path, time::Instant};

fn source_file(n: usize, changed: bool) -> String {
    format!(
        "export function costMarker{n}(value: number): number {{\n  const base = value + {n};\n  const tax = base * 0.2;\n  const fee = 3;\n  const total = base + tax + fee{};\n  auditCost(total);\n  return total;\n}}\n",
        if changed { " + 1" } else { "" }
    )
}

fn put(root: &Path, path: &str, text: &str) {
    let destination = root.join(path);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(destination, text.as_bytes()).unwrap();
}

fn document_stats(root: &Path) -> Value {
    let connection = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let mut statement = connection
        .prepare("SELECT record_json FROM document_manifest ORDER BY file_path,doc_key")
        .unwrap();
    let records: Vec<cc_model::identity::DocumentRecord> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect();
    let chunks: i64 = connection
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    let ast_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND lower(name) LIKE '%ast%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let source_bytes: usize = records.iter().map(|record| record.source.span.len()).sum();
    let model_input_bytes: usize = records
        .iter()
        .filter_map(|record| record.input.as_ref())
        .map(|input| input.text.len())
        .sum();
    let manifest_bytes: usize = records
        .iter()
        .map(|record| serde_json::to_vec(record).unwrap().len())
        .sum();
    json!({
        "documents": records.len(),
        "chunks": chunks,
        "source_partition_bytes": source_bytes,
        "max_chunk_bytes": records.iter().map(|record| record.source.span.len()).max().unwrap_or(0),
        "model_input_bytes": model_input_bytes,
        "manifest_json_bytes": manifest_bytes,
        "ast_persistence_tables": ast_tables,
        "database_file_bytes": std::fs::metadata(root.join(".codecortex/index.sqlite3")).unwrap().len(),
    })
}

fn large_file_sample(statements: usize, repetition: usize) -> Value {
    let source = format!(
        "export function largeCostTarget(input: number): number {{\n{}  return input;\n}}\n",
        (0..statements)
            .map(|n| format!("  const value{n} = input + {n};\n"))
            .collect::<String>()
    );
    let policy = ChunkPolicy {
        lines: 80,
        bytes: 16_384,
        chars: 16_384,
        estimated_tokens: 4_096,
        merge_min_bytes: 256,
    };
    let parser = ParserRegistry::with_chunk_policy(policy);
    let rss_before = cc_eval::benchmark::sampler::sample("large_file_before", None);
    let started = Instant::now();
    let parsed = parser
        .parse("large.ts", &source, Language::TypeScript)
        .unwrap();
    let elapsed_us = started.elapsed().as_micros();
    let rss_after = cc_eval::benchmark::sampler::sample("large_file_after", None);
    let snapshot = SourceSnapshot::new(source.as_bytes());
    let mut position = 0usize;
    for chunk in &parsed.chunks {
        let proof = chunk.source.as_ref().unwrap();
        assert_eq!(proof.span.start, position);
        assert_eq!(snapshot.slice(proof.span).unwrap(), chunk.text);
        assert!(proof.span.len() <= policy.effective_bytes());
        position = proof.span.end;
    }
    assert_eq!(position, source.len());
    let structure = parsed.source_structure.as_ref().unwrap();
    json!({
        "statements": statements,
        "repetition": repetition,
        "source_bytes": source.len(),
        "parse_count": 1,
        "parse_and_chunk_us": elapsed_us,
        "chunks": parsed.chunks.len(),
        "max_chunk_bytes": parsed.chunks.iter().map(|chunk| chunk.text.len()).max().unwrap_or(0),
        "visited_nodes": structure.visited_nodes,
        "boundaries": structure.boundaries.len(),
        "boundary_json_bytes": serde_json::to_vec(structure).unwrap().len(),
        "source_json_bytes": parsed.chunks.iter().map(|chunk| chunk.source_json().unwrap().unwrap().len()).sum::<usize>(),
        "exact_source_partition": true,
        "rss_before": rss_before,
        "rss_after": rss_after,
    })
}

#[test]
#[ignore = "explicit release P4-D build/chunk/document/RSS mechanism measurements"]
#[allow(clippy::assertions_on_constants)]
fn release_chunk_build_memory_and_document_costs() {
    assert!(!cfg!(debug_assertions), "release profile required");
    let mut project_samples = Vec::new();
    for files in [32usize, 256] {
        for repetition in 0..3 {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            put(
                root,
                ".codecortex.json",
                r#"{"auto_index":{"enabled":false},"indexing":{"chunk_line_budget":6,"chunk_byte_budget":320,"chunk_char_budget":320,"chunk_token_budget":80,"chunk_merge_min_bytes":160}}"#,
            );
            let admitted_bytes: usize = (0..files)
                .map(|n| {
                    let source = source_file(n, false);
                    put(root, &format!("src/f{n}.ts"), &source);
                    source.len()
                })
                .sum();
            let rss_before = cc_eval::benchmark::sampler::sample("before_full_build", None);
            let mut index = CodeIndex::new(Some(root)).unwrap();
            let started = Instant::now();
            let full = index.build_index(true).unwrap();
            let full_us = started.elapsed().as_micros();
            let rss_after = cc_eval::benchmark::sampler::sample("after_full_build", None);
            assert!(full.parse_errors.is_empty());
            assert_eq!(full.files_parsed, files);
            assert_eq!(full.document_changes.files_projected, files);
            assert!(full.resolution_freshness.complete);
            let stats = document_stats(root);
            assert_eq!(stats["source_partition_bytes"], admitted_bytes);
            assert_eq!(stats["ast_persistence_tables"], 0);
            assert!(stats["max_chunk_bytes"].as_u64().unwrap() <= 320);

            let mut noop = Vec::new();
            for _ in 0..5 {
                let started = Instant::now();
                let report = index.build_index(false).unwrap();
                noop.push(started.elapsed().as_micros());
                assert_eq!(report.files_parsed, 0);
                assert_eq!(report.document_changes.files_projected, 0);
            }
            let mut single_update = Vec::new();
            for update in 0..4 {
                put(root, "src/f0.ts", &source_file(0, update % 2 == 0));
                let started = Instant::now();
                let report = index.build_index(false).unwrap();
                single_update.push(json!({
                    "elapsed_us": started.elapsed().as_micros(),
                    "files_parsed": report.files_parsed,
                    "documents": report.document_changes,
                    "chunks_written": report.chunks_total,
                    "phase_timing": report.phase_timing,
                }));
                assert!(report.parse_errors.is_empty());
                assert_eq!(report.files_parsed, 1);
                assert_eq!(report.document_changes.files_projected, 1);
            }
            project_samples.push(json!({
                "files": files,
                "repetition": repetition,
                "admitted_source_bytes": admitted_bytes,
                "full_build_us": full_us,
                "full_files_parsed": full.files_parsed,
                "full_chunks_written": full.chunks_total,
                "full_phase_timing": full.phase_timing,
                "noop_us": noop,
                "single_updates": single_update,
                "document_storage": stats,
                "rss_before": rss_before,
                "rss_after": rss_after,
            }));
        }
    }
    let mut large_files = Vec::new();
    for statements in [1_000usize, 5_000] {
        for repetition in 0..3 {
            large_files.push(large_file_sample(statements, repetition));
        }
    }
    let result = json!({
        "schema_version": 1,
        "status": "passed",
        "project_samples": project_samples,
        "large_file_samples": large_files,
        "memory_method": "native current RSS plus ps stage-boundary snapshots; transient peaks may be missed",
        "storage_method": "persisted chunk/document counts and serialized boundary/source/manifest byte proxies; no AST table is persisted",
        "scope": "release local mechanism observations with 3 full builds per size, 5 no-op and 4 single-file updates; not 100k, p95/p99, cross-platform or global peak certification"
    });
    if let Ok(directory) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join("p4d-cost.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
}

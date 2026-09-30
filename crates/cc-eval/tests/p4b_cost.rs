//! Same-source release ablation of chunk merging and offline text projection.
use cc_index::documents::render::{render, RenderOptions};
use cc_model::{chunk_policy::ChunkPolicy, source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;
use std::time::Instant;
#[test]
#[ignore = "explicit release parser/chunk/projection cost test"]
#[allow(clippy::assertions_on_constants)] // Compiles in debug; the ignored test must run in release.
fn release_chunk_merge_and_projection_costs() {
    assert!(!cfg!(debug_assertions), "release required");
    let mut samples = Vec::new();
    for statements in [150, 1500, 10000] {
        let source = format!(
            "/** cost fixture */\r\nexport function calculate() {{\r\n{}return 1;\r\n}}\r\n",
            "  consume('甲');\r\n".repeat(statements)
        );
        let mut without_count = 0;
        for merge_min_bytes in [0, 256] {
            let policy = ChunkPolicy {
                merge_min_bytes,
                ..Default::default()
            };
            let parser = ParserRegistry::with_chunk_policy(policy);
            for repetition in 0..5 {
                let start = Instant::now();
                let parsed = parser
                    .parse("fixture.ts", &source, Language::TypeScript)
                    .unwrap();
                let parse_us = start.elapsed().as_micros();
                assert_eq!(
                    parsed
                        .chunks
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<String>(),
                    source
                );
                assert!(parsed.source_structure.as_ref().unwrap().complete);
                if merge_min_bytes == 0 {
                    without_count = parsed.chunks.len();
                } else {
                    assert!(parsed.chunks.len() < without_count);
                }
                let start = Instant::now();
                let snapshot = SourceSnapshot::new(source.as_bytes());
                let snapshot_us = start.elapsed().as_micros();
                let start = Instant::now();
                let mut input_bytes = 0;
                let mut estimated_tokens = 0;
                let mut source_json_bytes = 0;
                let mut metadata_truncations = 0;
                for c in &parsed.chunks {
                    let projection = render(&snapshot, c, RenderOptions::default()).unwrap();
                    assert_eq!(
                        &projection.text
                            [projection.source_range.start..projection.source_range.end],
                        c.text
                    );
                    input_bytes += projection.text.len();
                    estimated_tokens += projection.token_estimate as usize;
                    metadata_truncations += usize::from(projection.metadata_truncated);
                    source_json_bytes += c.source_json().unwrap().unwrap().len();
                }
                samples.push(serde_json::json!({"statements":statements,"merge_min_bytes":merge_min_bytes,"repetition":repetition,"source_bytes":source.len(),"chunks":parsed.chunks.len(),"parse_and_chunk_us":parse_us,"snapshot_us":snapshot_us,"render_us":start.elapsed().as_micros(),"model_input_bytes":input_bytes,"estimated_tokens":estimated_tokens,"token_estimator":cc_model::chunk_policy::TOKEN_ESTIMATOR,"source_json_bytes":source_json_bytes,"metadata_truncations":metadata_truncations,"exact_source_union":true}));
            }
        }
    }
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(std::path::Path::new(&path).join("p4b-cost.json"),serde_json::to_vec_pretty(&serde_json::json!({"samples":samples,"scope":"in-process release fixed synthetic functions; merge ablation, not exact tokenization, RSS, whole-index speedup or retrieval holdout; no provider requests"})).unwrap()).unwrap();
    }
}

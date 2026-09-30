//! Release parse/boundary/chunk observations, not a production latency claim.
use cc_model::{source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;
use serde_json::json;
#[test]
#[ignore = "explicit release source partition and boundary cost samples"]
#[allow(clippy::assertions_on_constants)]
fn release_hierarchical_chunks_preserve_all_bytes_and_report_costs() {
    assert!(!cfg!(debug_assertions));
    let mut samples = vec![];
    let parser = ParserRegistry::new();
    for methods in [10, 100, 500] {
        let mut text = String::from("export class CostService {\r\n");
        for n in 0..methods {
            text.push_str(&format!(
                "  /** Handles request {n}. */\r\n  method{n}() {{\r\n"
            ));
            for _ in 0..12 {
                text.push_str("    consume('中文 payload');\r\n");
            }
            text.push_str("  }\r\n");
        }
        text.push_str("}\r\n");
        for repetition in 0..5 {
            let t = std::time::Instant::now();
            let snapshot = SourceSnapshot::new(text.as_bytes());
            let snapshot_us = t.elapsed().as_micros();
            let t = std::time::Instant::now();
            let out = parser
                .parse("cost.ts", &text, Language::TypeScript)
                .unwrap();
            let parse_us = t.elapsed().as_micros();
            let structure = out.source_structure.as_ref().unwrap();
            let mut position = 0;
            for c in &out.chunks {
                let s = c.source.as_ref().unwrap();
                assert_eq!(s.span.start, position);
                assert_eq!(snapshot.slice(s.span).unwrap(), c.text);
                assert!(s.validate(&c.text));
                assert!(c.text.len() <= 16 * 1024);
                position = s.span.end;
            }
            assert_eq!(position, text.len());
            samples.push(json!({"methods":methods,"repetition":repetition,"bytes":text.len(),"snapshot_us":snapshot_us,"parse_and_chunk_us":parse_us,"chunks":out.chunks.len(),"visited_nodes":structure.visited_nodes,"boundaries":structure.boundaries.len(),"complete":structure.complete,"reasons":structure.reasons,"boundary_json_bytes":serde_json::to_vec(structure).unwrap().len(),"chunk_source_json_bytes":out.chunks.iter().map(|c|c.source_json().unwrap().unwrap().len()).sum::<usize>(),"original_partition_exact":true}));
        }
    }
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("p4a-source-cost.json"),serde_json::to_vec_pretty(&json!({"samples":samples,"scope":"release in-process parser incl extraction and chunking; snapshot measured separately; no RSS or production p95 claim"})).unwrap()).unwrap();
    }
}

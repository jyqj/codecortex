//! P4-D V19: fixed-source, fixed-retriever chunk-policy quality ablation.
use std::path::Path;

#[test]
fn chunk_policy_ablation_preserves_quality_and_measures_structure() {
    let plan =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/native/p4d-chunk-ablation.json");
    let result = cc_eval::benchmark::ablation::run_chunk_policy_ablation(&plan).unwrap();
    assert_eq!(result["status"], "passed");
    assert_eq!(result["comparison"]["quality_regressions"], 0);
    assert_eq!(result["comparison"]["source_evidence_failures"], 0);
    assert!(
        result["comparison"]["candidate_document_reduction"]
            .as_i64()
            .unwrap()
            > 0,
        "the controlled factor must change the document partition: {result}"
    );
    if let Ok(directory) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join("p4d-chunk-quality.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
}

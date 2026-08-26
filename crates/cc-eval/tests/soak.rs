//! Long-running resident-memory soak for the MCP server path.
//!
//! Simulates a long-lived agent session: one in-process MCP backend stays
//! attached to a synthetic project while the harness alternates search
//! traffic with single-file incremental rebuilds, sampling process RSS after
//! every cycle. The run FAILS if late-phase RSS keeps climbing over the
//! steady-state plateau — the regression signal for unbounded caches.
//!
//! `#[ignore]`d — run explicitly (duration via env, default 60s):
//!
//! ```sh
//! CODECORTEX_SOAK_SECS=3600 \
//!   cargo test -p cc-eval --test soak soak_mcp_session_rss -- --ignored --nocapture
//! ```
//!
//! `CODECORTEX_SOAK_FILES` overrides the synthetic repo size (default 1000).

use cc_eval::runner::CodeIndexBackend;
use cc_eval::synth::{generate, SynthRepo, SynthSpec};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn touch_file(root: &Path, rel_path: &str, marker: usize) {
    let path = root.join(rel_path);
    let mut source = std::fs::read_to_string(&path).expect("read soak mutation target");
    let comment = if rel_path.ends_with(".py") { "#" } else { "//" };
    source.push_str(&format!("{comment} soak edit marker {marker}\n"));
    std::fs::write(&path, source).expect("write soak mutation target");
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

#[test]
#[ignore = "long-running RSS soak; run explicitly with CODECORTEX_SOAK_SECS"]
fn soak_mcp_session_rss() {
    let duration = Duration::from_secs(env_usize("CODECORTEX_SOAK_SECS", 60) as u64);
    let target_files = env_usize("CODECORTEX_SOAK_FILES", 1_000);

    let tmp = tempfile::tempdir().expect("soak tempdir");
    let root = tmp.path();
    let spec = SynthSpec {
        target_files,
        seed: 0x00C0_FFEE,
    };
    let repo: SynthRepo = generate(root, &spec).expect("synthetic repo generation");
    let backend = CodeIndexBackend::new_unindexed(root).expect("soak backend");
    backend
        .build_index_report(true)
        .expect("initial full index");

    let gt = &repo.ground_truth;
    let edit_target = gt.chain[1].callee_file.clone();
    let searches = [
        json!({ "query": gt.needle_phrase, "mode": "hybrid", "top_k": 10 }),
        json!({ "query": gt.needle_symbol, "mode": "symbol", "top_k": 5 }),
        json!({ "query": "dispatch payload registry bridge", "mode": "hybrid", "top_k": 10 }),
    ];

    let start = Instant::now();
    let mut samples: Vec<u64> = Vec::new();
    let mut cycle = 0usize;
    while start.elapsed() < duration {
        let params = &searches[cycle % searches.len()];
        backend
            .call_tool("search", params)
            .expect("soak search should succeed");
        if cycle.is_multiple_of(3) {
            touch_file(root, &edit_target, cycle);
            backend
                .build_index_report(false)
                .expect("soak incremental build should succeed");
        }
        if cycle.is_multiple_of(5) {
            backend
                .call_tool(
                    "graph_query",
                    &json!({ "query": format!(
                        "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = '{}' RETURN DISTINCT b.name LIMIT 25",
                        gt.chain[0].caller
                    ) }),
                )
                .expect("soak graph_query should succeed");
        }
        let rss = cc_index::process_rss_bytes();
        samples.push(rss);
        if cycle.is_multiple_of(25) {
            eprintln!(
                "[soak] t={:>5.0}s cycle={:<5} rss={:.1} MB",
                start.elapsed().as_secs_f64(),
                cycle,
                mb(rss)
            );
        }
        cycle += 1;
    }

    assert!(
        samples.len() >= 8,
        "soak too short to judge a trend ({} samples); raise CODECORTEX_SOAK_SECS",
        samples.len()
    );

    // Steady-state trend: compare the median of the second quarter (caches
    // warmed) against the median of the final quarter. A leak shows up as a
    // monotone late-phase climb; a plateau stays within noise.
    let quarter = samples.len() / 4;
    let median = |window: &[u64]| -> u64 {
        let mut sorted = window.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    };
    let warmed = median(&samples[quarter..2 * quarter]);
    let tail = median(&samples[3 * quarter..]);
    let peak = *samples.iter().max().unwrap();
    eprintln!(
        "[soak] cycles={} warmed-median={:.1} MB tail-median={:.1} MB peak={:.1} MB",
        cycle,
        mb(warmed),
        mb(tail),
        mb(peak)
    );

    // Allow 25% + 32 MB of drift over the plateau before calling it a leak
    // (allocator slack, cache LRUs filling to capacity).
    let allowed = warmed + warmed / 4 + 32 * 1_048_576;
    assert!(
        tail <= allowed,
        "late-phase RSS climbed past the steady-state plateau: warmed-median {:.1} MB, \
         tail-median {:.1} MB (allowed {:.1} MB) — suspect an unbounded cache",
        mb(warmed),
        mb(tail),
        mb(allowed)
    );
}

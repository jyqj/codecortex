#!/usr/bin/env python3
"""Verify measured receipts, retain adverse observations, emit a compact table."""
import hashlib
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
rows = []
for name in ("measurements-msrv-initial.json", "measurements-msrv.json", "measurements-199.json"):
    data = json.loads((HERE / name).read_text())
    assert data["target_source_sha"] == "b973de5c542db2ba46ddb0798b03d67627e42275"
    assert len(data["cells"]) == 8
    for cell in data["cells"]:
        corrupt = bool(cell["corrupt_suffix"])
        assert cell["cache_files_before"] == cell["cache_files_after"] == 2 * cell["n"]
        assert cell["cache_disk_bytes_before"] == cell["cache_disk_bytes_after"]
        assert len(cell["workers"]) == cell["concurrency"]
        for worker in cell["workers"]:
            assert worker["rows_scanned"] == cell["n"] * cell["repeats"]
            assert worker["max_batch_observed"] <= cell["batch_rows"]
            assert worker["result_count"] == (0 if corrupt else cell["k"] * cell["repeats"])
            assert worker["sqlite_cache_size_kib"] == -2048
            assert worker["sqlite_mmap_bytes"] == 0
        assert cell["rss"]["samples"] > 1
        if not corrupt:
            assert cell["heap_within_envelope"] and cell["rss"]["within_envelope"]
        rows.append({"run":name,"n":cell["n"],"c":cell["concurrency"],
                     "corrupt":cell["corrupt_suffix"],
                     "heap_delta":cell["rust_live_peak_delta_bytes"],
                     "rss_baseline":cell["rss"]["baseline_bytes"],
                     "rss_delta":cell["rss"]["sampled_peak_delta_bytes"],
                     "gap_ms":cell["rss"]["max_observed_gap_ms"],
                     "heap_within_envelope":cell["heap_within_envelope"],
                     "rss_within_envelope":cell["rss"]["within_envelope"],
                     "sqlite_cache_after_max":max(w.get("sqlite_cache_used_bytes_after_scan",0) for w in cell["workers"]) if "initial" not in name else None})
files = ["crates/cc-eval/tests/v16_resource_bounds.rs", "crates/cc-eval/src/benchmark/sampler.rs",
         "crates/cc-semantic/src/vector/exact.rs", "crates/cc-semantic/src/cache.rs",
         "crates/cc-db/src/semantic_manifest_reads.rs", "Cargo.toml", "Cargo.lock"]
summary = {"target_source_sha":"b973de5c542db2ba46ddb0798b03d67627e42275",
           "resource_acceptance":"NOT_PASSED: oversized corrupted artifact reads are unbounded before validation",
           "healthy_scan_submatrix":"18 measured normal cells within fixed envelopes across retained initial/final runs",
           "formal_P7_015_V20":"NOT_RUN; V16 resource subitem only",
           "source_and_final_harness_sha256":{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in files},
           "rows":rows}
(HERE/"summary.json").write_text(json.dumps(summary,indent=2,sort_keys=True)+"\n")
print("run | N | C | corruption | RSS baseline MiB | sampled RSS delta MiB | Rust live delta KiB | SQLite cache after max MiB | gap ms")
for r in rows:
    cache = "not captured" if r["sqlite_cache_after_max"] is None else f'{r["sqlite_cache_after_max"]/1048576:.3f}'
    print(f'{r["run"]} | {r["n"]} | {r["c"]} | {r["corrupt"] or "none"} | {r["rss_baseline"]/1048576:.3f} | {r["rss_delta"]/1048576:.3f} | {r["heap_delta"]/1024:.3f} | {cache} | {r["gap_ms"]}')

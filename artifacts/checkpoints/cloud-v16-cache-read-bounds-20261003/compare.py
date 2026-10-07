#!/usr/bin/env python3
"""Compare frozen PR48 observations with the unchanged matrix after the fix."""
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
BASE = "671352d175a552b2448a8c9618f94bef94d9cad6"
FROZEN = ROOT / "artifacts/checkpoints/cloud-v16-resource-validation-20261002"

def original(path):
    return subprocess.check_output(["git", "show", BASE + ":" + str(path)], cwd=ROOT)

matrix_path = "crates/cc-eval/tests/v16_resource_bounds.rs"
assert (ROOT / matrix_path).read_bytes() == original(matrix_path)
for path in FROZEN.iterdir():
    if path.is_file():
        assert path.read_bytes() == original(path.relative_to(ROOT)), path

rows = []
for name in ("measurements-msrv.json", "measurements-199.json"):
    before = json.loads((FROZEN / name).read_text())["cells"]
    after = json.loads((HERE / name).read_text())["cells"]
    assert len(before) == len(after) == 8
    for a, b in zip(before, after):
        for key in ("n", "concurrency", "dimension", "k", "batch_rows", "repeats", "corrupt_suffix", "rust_heap_envelope_bytes"):
            assert a[key] == b[key], key
        assert a["rss"]["envelope_bytes"] == b["rss"]["envelope_bytes"]
        assert b["heap_within_envelope"] and b["rss"]["within_envelope"]
        assert b["cache_files_before"] == b["cache_files_after"] == 2 * b["n"]
        assert b["cache_disk_bytes_before"] == b["cache_disk_bytes_after"]
        for w in b["workers"]:
            assert w["rows_scanned"] == b["n"] * b["repeats"]
            assert w["max_batch_observed"] <= b["batch_rows"]
            assert w["result_count"] == (0 if b["corrupt_suffix"] else b["k"] * b["repeats"])
        rows.append({"run":name,"n":b["n"],"c":b["concurrency"],"corrupt":b["corrupt_suffix"],
                     "before_heap_delta":a["rust_live_peak_delta_bytes"],"after_heap_delta":b["rust_live_peak_delta_bytes"],
                     "before_sampled_rss_delta":a["rss"]["sampled_peak_delta_bytes"],"after_sampled_rss_delta":b["rss"]["sampled_peak_delta_bytes"],
                     "after_rss_samples":b["rss"]["samples"],"after_max_gap_ms":b["rss"]["max_observed_gap_ms"]})
soaks = {}
for name in ("soak-msrv.json", "soak-199.json"):
    cells = json.loads((HERE / name).read_text())
    assert len(cells) == 3
    for c in cells:
        assert c["reads"] == 10000
        assert c["rust_live_peak_delta_bytes"] <= c["heap_envelope_bytes"]
        assert c["rss"]["samples"] >= 2
        assert c["rss"]["sampled_delta_bytes"] <= c["rss"]["envelope_bytes"]
    soaks[name] = cells
paths = [matrix_path, "crates/cc-semantic/src/cache.rs", "crates/cc-semantic/src/cache_read_bounds_tests.rs",
         "crates/cc-eval/tests/v16_cache_read_soak.rs", "crates/cc-eval/src/benchmark/sampler.rs", "Cargo.lock", "Cargo.toml"]
summary = {"frozen_before_commit":BASE,"production_base":"b973de5c542db2ba46ddb0798b03d67627e42275",
           "unchanged_matrix_marker_note":"raw target_source_sha b973de5 is the frozen harness origin; post-fix source is identified by the cache.rs hash and this PR commit, not that literal marker",
           "status":"PASS bounded cache-read fix and measured submatrix; not full V16/P7-015/V20",
           "frozen_matrix_and_evidence_unchanged":True,
           "cache_before_sha256":hashlib.sha256(original("crates/cc-semantic/src/cache.rs")).hexdigest(),
           "final_file_sha256":{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in paths},
           "before_after":rows,"post_fix_observable_read_soaks":soaks}
(HERE / "comparison.json").write_text(json.dumps(summary,indent=2,sort_keys=True)+"\n")
print("run | N | C | corrupt | before/after Rust live delta bytes | before/after sampled RSS delta bytes | after samples")
for r in rows:
    print(f'{r["run"]} | {r["n"]} | {r["c"]} | {r["corrupt"] or "none"} | {r["before_heap_delta"]}/{r["after_heap_delta"]} | {r["before_sampled_rss_delta"]}/{r["after_sampled_rss_delta"]} | {r["after_rss_samples"]}')

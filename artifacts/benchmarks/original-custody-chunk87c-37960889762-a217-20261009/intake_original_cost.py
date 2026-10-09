#!/usr/bin/env python3
"""Validate retained original streams and summarize; never execute the workload."""
from pathlib import Path
import hashlib
import importlib.util
import json
import re
import statistics
import sys

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / "p8-rebuild-chunk-integration/guard-view"
ORIGINAL = ROOT / "extracted/11632400464"
HEAD = "87c2274e497c3d0c9d40de4b538794318672e028"
TREE = "3399514bf070cbdc4af9e6737c0171bde7d508ba"
SCHEMA = "p8-rebuild-chunk-statements-cost-v1"


def read(name):
    return json.loads((ORIGINAL / name).read_bytes())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    before = {p.name: sha(p) for p in ORIGINAL.iterdir()}
    identity, after = read("identity-before.json"), read("identity-after.json")
    observation, execution = read("cost-observation.json"), read("execution.json")
    assert identity["source_commit"] == identity["expected_source"] == HEAD
    assert identity["source_tree"] == TREE
    assert (identity["run_id"], identity["run_attempt"]) == ("37960889762", "1")
    assert execution["argv"] == identity["argv"]
    assert execution["exit_code"] == 0 and execution["launch_error"] is None
    assert after["error"] is None
    assert all(after[k] is True for k in (
        "observer_inputs_unchanged", "source_unchanged", "toolchain_outputs_unchanged"))
    for name, expected in after["files"].items():
        path = ORIGINAL / name
        assert path.stat().st_size == expected["bytes"] and sha(path) == expected["sha256"]
    assert (ORIGINAL / "source-before.json").read_bytes() == (ORIGINAL / "source-after.json").read_bytes()
    for tool in ("cargo", "rustc"):
        assert (ORIGINAL / f"{tool}-before.txt").read_bytes() == (ORIGINAL / f"{tool}-after.txt").read_bytes()
        assert sha(ORIGINAL / f"{tool}-before.txt") == identity["toolchain_output_sha256"][tool]
    for path, expected in identity["observer_inputs"].items():
        assert sha(SOURCE / path) == expected
    assert sha(SOURCE / "Cargo.lock") == identity["Cargo_lock_sha256"]
    spec = importlib.util.spec_from_file_location("chunk_cost_original_identity", SOURCE / "scripts/p7_build_identity.py")
    helper = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = helper
    spec.loader.exec_module(helper)
    actual_source = helper.source_snapshot(SOURCE)
    assert actual_source == read("source-before.json")

    stdout = (ORIGINAL / "probe.stdout").read_text()
    events = []
    for line in stdout.splitlines():
        if "{" not in line:
            continue
        try:
            value = json.loads(line[line.index("{"):])
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            events.append(value)
    records = [v for v in events if v.get("schema") == SCHEMA]
    assert records == observation["records"]
    assert observation["record_count"] == len(records) == 80
    assert observation["status"] == "observed"
    assert [v for v in events if v.get("reason") == "build-finished"] == [
        {"reason": "build-finished", "success": True}]
    producers = [v for v in events if v.get("reason") == "compiler-artifact"
                 and v.get("target", {}).get("name") == "cc_db"
                 and v.get("target", {}).get("kind") == ["lib"]
                 and v.get("profile", {}).get("test") is True]
    assert producers == [observation["compiler_artifact"]]
    producer = producers[0]
    assert producer["fresh"] is False and producer["features"] == []
    assert producer["profile"]["opt_level"] == "3" and producer["profile"]["debug_assertions"] is False
    assert producer["executable"].startswith(identity["environment"]["CARGO_TARGET_DIR"] + "/")
    assert re.fullmatch(r"[0-9a-f]{64}", observation["test_executable_sha256"])
    assert observation["test_executable_bytes"] > 0
    footers = re.findall(r"^test result: (.+)$", stdout, flags=re.M)
    assert len(footers) == 1 and re.fullmatch(
        r"ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9.]+s", footers[0])

    cases = {"single_plain": 1, "short_plain_6": 6, "mixed_32": 32, "compressed_64": 64}
    assert {(r["case"], r["round"]) for r in records} == {(c, i) for c in cases for i in range(20)}
    for r in records:
        assert r["chunks"] == cases[r["case"]]
        assert r["first"] == ("original" if r["round"] % 2 == 0 else "retained")
        assert r["original_typed_rows_debug_blake3"] == r["retained_typed_rows_debug_blake3"]
        assert re.fullmatch(r"[0-9a-f]{64}", r["original_typed_rows_debug_blake3"])
        assert all(r[k] == r["chunks"] for k in (
            "original_base_rows", "original_fts_rows", "retained_base_rows", "retained_fts_rows"))
        assert r["performance_threshold"] is None
        assert r["timed_scope"] == "chunk-loop including compression selection and both inserts; excludes begin/readback/rollback"
        assert all(re.fullmatch(r"[1-9][0-9]*", r[k]) for k in ("original_ns", "retained_ns"))

    summaries = []
    for case, count in cases.items():
        rows = [r for r in records if r["case"] == case]
        ratios = [int(r["retained_ns"]) / int(r["original_ns"]) for r in rows]
        by_first = {}
        for first in ("original", "retained"):
            selected = [r for r in rows if r["first"] == first]
            rs = [int(r["retained_ns"]) / int(r["original_ns"]) for r in selected]
            by_first[first] = {"pairs": len(rs), "median_ratio": statistics.median(rs),
                               "retained_faster_pairs": sum(r < 1 for r in rs)}
        summaries.append({
            "case": case, "paired_rounds": len(rows), "chunks_each": count,
            "original_median_ns": statistics.median(int(r["original_ns"]) for r in rows),
            "retained_median_ns": statistics.median(int(r["retained_ns"]) for r in rows),
            "paired_retained_over_original_ratio_median": statistics.median(ratios),
            "paired_ratio_min": min(ratios), "paired_ratio_max": max(ratios),
            "retained_faster_pairs": sum(r < 1 for r in ratios), "by_first": by_first,
        })
    assert before == {p.name: sha(p) for p in ORIGINAL.iterdir()}
    result = {
        "schema": "p8-rebuild-chunk-original-cost-intake-v1", "source_commit": HEAD,
        "source_tree": TREE, "run_id": 37960889762, "attempt": 1,
        "artifact_id": 11632400464,
        "original_zip_sha256": "d8ca0b56c96e8bda9ad108216e352a30edb78696853e25619563539cd6c81196",
        "status": "accepted_finite_original_observation", "source_input_count": actual_source["input_count"],
        "actual_original_streams_match_observation": True, "original_files_unchanged": True,
        "actual_cargo_success_and_one_test_footer": True, "unique_paired_records": 80,
        "summary": summaries, "all_original_records_retained": records,
        "limitations": [
            "One release libtest process and runner, 20 alternating paired rounds per case; no independent-host replication.",
            "Chunk-loop timing includes compression selection and both inserts, but excludes begin/readback/rollback and is not full rebuild duration.",
            "Original observer hashed its actual ELF; ZIP does not contain that ELF and local intake does not independently rehash it.",
            "Typed row digests/counts match for all pairs; full typed-row equality is an assertion in the original successful ignored method, not a claim of six ordinary tests passing.",
            "No pooled cross-case speedup, p95/p99 inference, timing threshold, workload replay or original TODO/sample credit."
        ],
        "task_counts": {"done": 163, "remaining": 29, "newly_completed": 0},
    }
    output = ROOT / "root-original-cost-intake-and-statistics.json"
    assert not output.exists()
    output.write_text(json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"output": str(output), "sha256": sha(output), "summary": summaries,
                      "source_input_count": actual_source["input_count"], "accepted_records": 80}, ensure_ascii=False))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Read the one original cost artifact; never execute or re-time the probe."""
from pathlib import Path
import hashlib
import importlib.util
import json
import re
import statistics
import sys

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / "p8-oracle-borrowing-integration/guard-view"
ORIGINAL = ROOT / "extracted/11630255138"
HEAD = "eded24951c3e99693410ce959a627ab575f66bec"
TREE = "b95b5d069cf35f0181c0534ea7855ca48c503cc4"
SCHEMA = "p8-oracle-serialization-cost-v1"


def read(name):
    return json.loads((ORIGINAL / name).read_bytes())


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    before_files = {p.name: sha(p) for p in ORIGINAL.iterdir()}
    identity, after, observation, execution = (
        read("identity-before.json"), read("identity-after.json"),
        read("cost-observation.json"), read("execution.json"),
    )
    assert identity["source_commit"] == identity["expected_source"] == HEAD
    assert identity["source_tree"] == TREE
    assert (identity["run_id"], identity["run_attempt"]) == ("37956976406", "1")
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
    for path, expected in identity["observer_inputs"].items():
        assert sha(SOURCE / path) == expected
    assert sha(SOURCE / "Cargo.lock") == identity["Cargo_lock_sha256"]
    spec = importlib.util.spec_from_file_location("cost_original_identity", SOURCE / "scripts/p7_build_identity.py")
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
    assert observation["record_count"] == len(records) == 100
    assert observation["status"] == "observed"
    assert [v for v in events if v.get("reason") == "build-finished"] == [
        {"reason": "build-finished", "success": True}]
    producers = [v for v in events if v.get("reason") == "compiler-artifact"
                 and v.get("target", {}).get("name") == "cc_eval"
                 and v.get("target", {}).get("kind") == ["lib"]
                 and v.get("profile", {}).get("test") is True]
    assert producers == [observation["compiler_artifact"]]
    producer = producers[0]
    assert producer["fresh"] is False and producer["features"] == ["default"]
    assert producer["profile"]["opt_level"] == "3" and producer["profile"]["debug_assertions"] is False
    assert producer["executable"].startswith(identity["environment"]["CARGO_TARGET_DIR"] + "/")
    assert re.fullmatch(r"[0-9a-f]{64}", observation["test_executable_sha256"])
    assert observation["test_executable_bytes"] > 0
    footers = re.findall(r"^test result: (.+)$", stdout, flags=re.M)
    assert len(footers) == 1 and footers[0].startswith(
        "ok. 1 passed; 0 failed; 0 ignored; 0 measured; 73 filtered out;")

    cases = {"multiple_text", "short_fields", "blob", "empty_text", "null"}
    assert {(r["case"], r["round"]) for r in records} == {(c, i) for c in cases for i in range(20)}
    for r in records:
        assert r["rows"] == 256
        assert r["first"] == ("original" if r["round"] % 2 == 0 else "borrowed")
        assert r["original_digest"] == r["borrowed_digest"]
        assert r["original_streaming_source_blob"] == "4c7fdb879e039af164b3b10ea1491d538c69ac68"
        assert r["timing_threshold"] is None
        assert all(re.fullmatch(r"[1-9][0-9]*", r[k]) for k in ("original_ns", "borrowed_ns"))

    summaries = []
    for case in sorted(cases):
        rows = [r for r in records if r["case"] == case]
        ratios = [int(r["borrowed_ns"]) / int(r["original_ns"]) for r in rows]
        by_first = {first: statistics.median(
            int(r["borrowed_ns"]) / int(r["original_ns"]) for r in rows if r["first"] == first)
            for first in ("original", "borrowed")}
        summaries.append({
            "case": case, "paired_rounds": len(rows), "rows_each": 256,
            "canonical_bytes": sorted({r["canonical_bytes"] for r in rows}),
            "original_median_ns": statistics.median(int(r["original_ns"]) for r in rows),
            "borrowed_median_ns": statistics.median(int(r["borrowed_ns"]) for r in rows),
            "paired_borrowed_over_original_ratio_median": statistics.median(ratios),
            "paired_ratio_min": min(ratios), "paired_ratio_max": max(ratios),
            "borrowed_faster_pairs": sum(r < 1 for r in ratios),
            "paired_ratio_median_by_first": by_first,
        })
    assert before_files == {p.name: sha(p) for p in ORIGINAL.iterdir()}
    result = {
        "schema": "p8-oracle-original-cost-intake-v1", "source_commit": HEAD,
        "source_tree": TREE, "run_id": 37956976406, "attempt": 1,
        "artifact_id": 11630255138, "original_zip_sha256": "6575f9192ecd9191c3761d7fe0a0ddbec6ca8d15c5080bf20b5ad94164f1aa52",
        "status": "accepted_finite_original_observation", "source_input_count": actual_source["input_count"],
        "actual_original_streams_match_observation": True, "original_files_unchanged": True,
        "actual_cargo_success_and_one_test_footer": True, "unique_paired_records": 100,
        "summary": summaries,
        "interpretation": "Four small/blob cases improved in all 20 pairs; multiple_text has mixed/order-dependent results. This does not establish end-to-end parity or scale speedup. Hold default performance integration until the large-row behavior and normal semantic CI are assessed.",
        "limitations": [
            "One release libtest process, one runner, 20 alternating warm paired rounds per case; not 20 independent builds or hosts.",
            "Recorded ns include same SQLite SELECT traversal, row serialization and output retention, not whole indexing/parity duration.",
            "Original observer verified actual ELF and retained its digest; this ZIP does not contain that ELF, so local intake does not independently rehash the ELF.",
            "No p95/p99 reliability, pooled cross-case estimate, new timing threshold or original TODO/sample credit.",
            "No new workload or native probe execution by this intake. Four normal Rust controls remain separate from the single ignored probe."
        ],
        "task_counts": {"done": 163, "remaining": 29, "newly_completed": 0},
    }
    output = ROOT / "root-original-cost-intake-and-statistics.json"
    output.write_text(json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"output": str(output), "sha256": sha(output), "result": result}, ensure_ascii=False))


if __name__ == "__main__":
    main()

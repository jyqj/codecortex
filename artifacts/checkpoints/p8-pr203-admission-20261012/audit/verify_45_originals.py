#!/usr/bin/env python3
"""Read-only replay of the 45 retained originals; no native workload execution.

All published pins are independently fetched GitHub values in remote-facts.json.
Only this audit's output is written. Historical source, ZIPs and raw are read.
"""
import argparse
import collections
import datetime
import hashlib
import json
from pathlib import Path
import sys
import time
import zipfile


def git_blob(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def audit(root):
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    clock = time.monotonic()
    custody_raw = (root / "round48-final-acceptance/complete-45-raw-custody.published.json").read_bytes()
    assert git_blob(custody_raw) == "062cd1b36269c2a4f8c7b5fe0e03b79b9eaf8e1b"
    custody = json.loads(custody_raw)
    assert custody["source"] == "f97c5068d056969705e8387ba1f37d83847f5bee"
    assert custody["run_id"] == 38026411200 and custody["attempt"] == 1
    candidates = list(root.glob("round*-new45-original-artifacts/*.zip"))
    events, matched, leaves = collections.Counter(), {}, 0
    for cell in custody["ordered_cells"]:
        names = {cell["artifact_name"], cell["artifact_name"] + "-original"}
        choices = [p for p in candidates if p.stem in names]
        good = [p for p in choices if p.stat().st_size == cell["whole_bytes"]
                and sha(p.read_bytes()) == cell["whole_sha256"]]
        assert good, (cell["slot"], "no original with the published hash")
        archive = good[0]
        raw = archive.read_bytes()
        assert git_blob(raw) == cell["computed_whole_git_blob"]
        cursor = 0
        for leaf in cell["ordered_original_byte_leaves"]:
            assert leaf["offset"] == cursor
            part = raw[cursor:cursor + leaf["bytes"]]
            assert len(part) == leaf["bytes"]
            assert sha(part) == leaf["sha256"] and git_blob(part) == leaf["git_blob"]
            cursor += len(part)
            leaves += 1
        assert cursor == len(raw)
        with zipfile.ZipFile(archive) as z:
            assert z.testzip() is None
            names = z.namelist()
            receipt_names = [n for n in names if n == "task-shard.json" or n.endswith("/task-shard.json")]
            assert len(receipt_names) == 1
            receipt_raw = z.read(receipt_names[0])
            receipt = json.loads(receipt_raw)
            assert receipt["passed"] is True
            assert receipt["driver_source"]["source_commit"] == custody["source"]
            assert receipt["study"]["run_id"] == "38026411200" and receipt["study"]["run_attempt"] == 1
            raw_names = [n for n in names if n.endswith("/native/raw.jsonl")]
            assert len(raw_names) == 1
            raw_hash = hashlib.sha256()
            cell_events = collections.Counter()
            with z.open(raw_names[0]) as stream:
                for line in stream:
                    raw_hash.update(line)
                    event = json.loads(line)
                    cell_events[event["event"]] += 1
            events.update(cell_events)
            key = tuple(cell["slot"])
            assert key not in matched
            matched[key] = {"archive": str(archive), "archive_bytes": len(raw),
                            "archive_sha256": sha(raw), "receipt_sha256": sha(receipt_raw),
                            "raw_sha256": raw_hash.hexdigest(), "events": dict(cell_events)}
    source = root / "round42-new45-build-original-intake/source-exact-G2"
    expected_sources = {
        "scripts/p8_task_profile_matrix.py": "c858fcb4edea1f91b421367b67bf722f0c2fef3a",
        "scripts/p8_scale_matrix.py": "6d6c6e4362760fb1eb78770efe41b578e6552dc7",
    }
    for relative, expected in expected_sources.items():
        assert git_blob((source / relative).read_bytes()) == expected
    sys.path.insert(0, str(source / "scripts"))
    import p8_task_profile_matrix as task
    matrix_raw = (root / "round47-complete45-original-aggregate-recovery/aggregate-output/task-matrix.json").read_bytes()
    assert sha(matrix_raw) == "307900563e166677db09721100179f8d885ea5e57acb643876a2abbbcac51fca"
    matrix = json.loads(matrix_raw)
    seen, observations, detail = set(), 0, []
    for cell in matrix["cells"]:
        key = tuple(cell["slot"])
        original = matched[key]
        directory = Path(cell["directory"])
        receipt_raw = (directory / "task-shard.json").read_bytes()
        assert sha(receipt_raw) == cell["receipt_sha256"] == original["receipt_sha256"]
        receipt = json.loads(receipt_raw)
        raw_path = directory / "native-shard/native/raw.jsonl"
        assert sha(raw_path.read_bytes()) == original["raw_sha256"]
        # This exact original function only parses raw; no build, binary, or aggregate.
        inspected = task.inspect_raw(raw_path, receipt["plan"])
        assert inspected["measurements"] == cell["measurements"]
        assert key not in seen
        seen.add(key)
        observations += len(inspected["measurements"])
        sample = inspected["measurements"][-1]
        detail.append({"slot": list(key), **original, "engine_ms": sample["engine_ms"],
                       "full_control_ms": sample.get("full_control_ms"),
                       "incremental_builds": sample.get("builds"),
                       "first_build_incomplete": sample.get("first_build_incomplete")})
    assert seen == task.expected_slots() == set(matched)
    assert len(seen) == 45 and observations == 85 and leaves == 118
    return {
        "schema": "codecortex_independent_45_originals_readonly_audit_v1",
        "reviewer": "/root/acceptance_audit", "started_at_utc": started,
        "finished_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "wall_seconds": time.monotonic() - clock, "passed": True,
        "source": custody["source"], "run_id": 38026411200, "attempt": 1,
        "custody_index_git_blob": git_blob(custody_raw),
        "original_matrix_sha256": sha(matrix_raw), "exact_original_driver_blobs": expected_sources,
        "archives_sha_crc_verified": 45, "archive_bytes": sum(v["archive_bytes"] for v in matched.values()),
        "byte_leaves_verified": leaves, "raw_event_counts": dict(events),
        "raw_inspector_calls": 45, "measurements_exactly_equal_published_matrix": observations,
        "all_original_raw_predicates_passed": True,
        "native_binary_executions": 0, "new_measurements": 0, "original_aggregate_executions": 0,
        "newly_closed_original_todos": [], "release_certified": False,
        "scope": "Revalidates original retained task evidence, not performance or release acceptance.",
        "cells": detail,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = audit(args.original_root)
    with args.output.open("x") as out:
        json.dump(result, out, ensure_ascii=False, indent=2)
        out.write("\n")
    print(json.dumps({k: v for k, v in result.items() if k != "cells"}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

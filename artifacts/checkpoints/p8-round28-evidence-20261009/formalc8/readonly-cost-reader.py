import collections
import hashlib
import json
from pathlib import Path

ROOT = Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root/formalc8-100k-failure-reception")
RAW = ROOT / "members/native/raw.jsonl"
EXPECTED = "9bf309d05c2361e18edf41eb0e70208f9697e09630cdae35c2cac8d96976d65e"
EXPECTED_BYTES = 20144893
SOURCE = "c8be5afaac568ffd40ef86d3795423c3b73c9f39"

def digest(path):
    h = hashlib.sha256()
    n = 0
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
            n += len(block)
    return {"bytes": n, "sha256": h.hexdigest()}

def numeric_leaves(value, prefix=""):
    if isinstance(value, dict):
        for key, child in value.items():
            yield from numeric_leaves(child, prefix + "." + key if prefix else key)
    elif type(value) is int:
        yield prefix, value

before = digest(RAW)
assert before == {"bytes": EXPECTED_BYTES, "sha256": EXPECTED}
report = json.loads((ROOT / "members/native/report.json").read_text())
shard = json.loads((ROOT / "members/shard.json").read_text())
assert shard["source_commit"] == SOURCE
assert shard["files"]["native/raw.jsonl"] == before
groups = {}
events = collections.Counter()
parity = []
pending = {}
tail = collections.deque(maxlen=4)
first_report_keys = None
line_count = 0
with RAW.open("rb") as stream:
    for line_count, line in enumerate(stream, 1):
        event = json.loads(line)
        kind = event.get("event")
        events[kind] += 1
        small = {key: event[key] for key in ("event", "label", "sample", "full", "start_us", "end_us", "wall_us") if key in event}
        tail.append(small)
        if kind == "build_started":
            assert event["label"] not in pending
            pending[event["label"]] = small
        elif kind in ("build_finished", "build_failed"):
            assert event["label"] in pending
            started = pending.pop(event["label"])
            assert event["start_us"] == started["start_us"]
            if kind == "build_finished":
                parts = event["label"].split("/")
                group = "full" if event["full"] else parts[2]
                slot = groups.setdefault(group, {"builds": 0, "wall_us": 0, "report_numeric": {}})
                slot["builds"] += 1
                slot["wall_us"] += event["wall_us"]
                if first_report_keys is None:
                    first_report_keys = sorted(event["report"])
                for key, value in numeric_leaves(event["report"]):
                    slot["report_numeric"][key] = slot["report_numeric"].get(key, 0) + value
        if kind in ("cold_parity", "stage_finished"):
            record = {key: event[key] for key in ("event", "label", "sample", "parity_wall_us", "incremental_builds", "incremental_wall_us", "complete", "full_complete", "passed") if key in event}
            comparison = event.get("parity", {})
            record["parity"] = {key: comparison[key] for key in ("status", "equal", "canonical_bytes", "scratch_peak_bytes", "different_tables") if key in comparison}
            record["table_counts"] = [
                {key: table[key] for key in ("table", "equal", "incremental_rows", "full_rows") if key in table}
                for table in comparison.get("tables", [])
            ]
            parity.append(record)
after = digest(RAW)
assert before == after
result = {
    "schema": "c8_retained_failed_raw_readonly_cost_extract_v1",
    "source_commit": SOURCE,
    "run_id": 37902429727,
    "artifact_id": 11617588539,
    "raw_before": before,
    "raw_after": after,
    "raw_lines": line_count,
    "event_counts": dict(events),
    "first_report_keys": first_report_keys,
    "native_report": report,
    "environment": shard["environment"],
    "groups": groups,
    "completed_parity": parity,
    "unfinished_builds": list(pending.values()),
    "last_events": list(tail),
    "scope": "Read existing files only; no writes, subprocess, imports of repo code, DB reads, benchmark, or product execution. Integer report leaves are summed only within each separate field path; timing views overlap and must not be added.",
}
output = json.dumps(result, sort_keys=True, separators=(",", ":"))
assert len(output.encode()) <= 192 * 1024
print(output)

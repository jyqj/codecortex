"""Independent, bounded replay of this optimization's archived evidence."""
import gzip
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
CHECKPOINTS = ROOT / "artifacts/checkpoints"
OLD = CHECKPOINTS / "cloud-p7-release-resource-50k-incomplete-20261003"
NEW = CHECKPOINTS / "cloud-p7-empty-test-population-20261003"
SOURCE = "c70c68f2ff9b4ac40c635652858f56d2d0a06518"
BASE = "8b362e6b9d1334a51d5856ab21223d6e5ec28604"


def load(path):
    return json.loads(path.read_text())


def rpc(path):
    with gzip.open(path, "rt") as stream:
        return [json.loads(line) for line in stream]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


hash_counts = {}
for folder in (OLD, NEW):
    manifest = load(folder / "artifact-manifest.json")
    for row in manifest:
        data = (folder / row["path"]).read_bytes()
        assert hashlib.sha256(data).hexdigest() == row["sha256"], row["path"]
        assert len(data) == row["bytes"], row["path"]
    hash_counts[folder.name] = len(manifest)

old_build = load(OLD / "build-receipt.json")
new_build = load(NEW / "build-receipt.json")
for folder, build in ((OLD, old_build), (NEW, new_build)):
    assert build["build_exit_code"] == 0 and build["guard_stop"] is None
    assert build["profile"] == "release"
    assert build["compiler_artifact"]["profile"]["opt_level"] == "3"
    assert not build["compiler_artifact"]["profile"]["debug_assertions"]
    assert sorted(build["compiler_artifact"]["features"]) == ["semantic", "semantic-http"]
    assert load(folder / "summary.json")["binary_sha256"] == build["binary_sha256"]
assert new_build["source_sha"] == SOURCE
assert not git("diff", SOURCE, "HEAD", "--", ":(glob)crates/*/src/**", "Cargo.toml", "Cargo.lock")
assert not git("diff", old_build["source_sha"], BASE, "--", ":(glob)crates/*/src/**", "Cargo.toml", "Cargo.lock")
production_paths = git("diff", BASE, SOURCE, "--name-only", "--", ":(glob)crates/*/src/**", "Cargo.toml", "Cargo.lock").decode().splitlines()
assert production_paths == ["crates/cc-db/src/index_db_edges.rs"]

old_rows = rpc(OLD / "fixed/n50000/rpc.jsonl.gz")
new_rows = rpc(NEW / "fixed/n50000/rpc.jsonl.gz")


def cold(rows):
    req = next(row for row in rows if row["event"] == "request"
               and row["phase"] == "cold-empty-index-build"
               and row["payload"].get("method") == "tools/call")
    assert req["payload"]["params"]["name"] == "index"
    assert "full" not in req["payload"]["params"]["arguments"]
    responses = [row for row in rows if row["event"] == "response"
                 and row["payload"].get("id") == req["payload"]["id"]]
    return req, responses


_, old_responses = cold(old_rows)
assert not old_responses
assert "_queue.Empty" in load(OLD / "summary.json")["scales"][0]["traceback"]
assert "timeout=300" in (ROOT / "scripts/p7_release_resource_preparation.py").read_text()
new_request, new_responses = cold(new_rows)
assert len(new_responses) == 1
response = new_responses[0]
result = response["payload"]["result"]["structuredContent"]["result"]
assert result["files_added"] == result["files_parsed"] == result["files_scanned"] == 50_000
assert result["files_skipped"] == 0 and not result["parse_errors"]
wire_ms = (response["time_ns"] - new_request["sent_ns"]) / 1e6
groups = {}
requests = {r["payload"]["id"]: r for r in new_rows if r["event"] == "request"}
for row in new_rows:
    if row["event"] != "response":
        continue
    request = requests.get(row["payload"].get("id"))
    if not request or not request["phase"].startswith(("warm_distinct_query", "warm_repeated_query")):
        continue
    value = row["payload"]["result"]
    assert not row["payload"].get("error") and not value.get("isError")
    assert value["structuredContent"]["result"]["machine_pack"]["hits"]
    groups[request["phase"]] = groups.get(request["phase"], 0) + 1
assert len(groups) == 8 and all(count == 32 for count in groups.values())
final = load(NEW / "fixed/n50000/final-db.json")
assert final["integrity"] == "ok" and final["foreign_key_errors"] == 0
assert final["counts"]["files"] == final["counts"]["semantic_manifest"] == 50_000
assert load(NEW / "fixed/n50000/final-status.json")["semantic_state"] == "ready"
assert load(NEW / "summary.json")["scales"][0]["product_exit_code"] == 0

print(json.dumps({
    "status": "passed_archived_evidence_replay",
    "review_head": git("rev-parse", "HEAD").decode().strip(),
    "production_source_sha": SOURCE,
    "old_build_source_sha": old_build["source_sha"],
    "old_binary_sha256": old_build["binary_sha256"],
    "new_binary_sha256": new_build["binary_sha256"],
    "verified_hash_counts": hash_counts,
    "production_diff_paths": production_paths,
    "old_300s_failure": "retained, no cold RPC response, no query cells",
    "new_cold_wire_ms_from_rpc": wire_ms,
    "new_cold_files_scanned_parsed_added": 50_000,
    "new_query_cells": groups,
    "final_ready_files_manifests": 50_000,
    "full_V20": False,
    "independent_live_product_rerun": False,
    "archived_binary_present_in_this_environment": False,
}, indent=2))

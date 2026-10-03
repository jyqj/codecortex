"""Focused independent cold index=false runs, synthetic loopback model only."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("preparation", ROOT / "scripts/p7_release_resource_preparation.py")
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)
parser = argparse.ArgumentParser()
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--receipt", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
args.output.mkdir()  # refuse evidence overwrite
receipt = json.loads(args.receipt.read_text())
assert receipt["profile"] == "release" and receipt["build_exit_code"] == 0
assert hashlib.sha256(args.binary.read_bytes()).hexdigest() == receipt["binary_sha256"]
summary = {"source_sha": receipt["source_sha"], "binary_sha256": receipt["binary_sha256"],
           "profile": "release/semantic-http/default INFO", "scales": [], "full_V20": False}
for count in (1_000, 50_000):
    assert shutil.disk_usage(args.output).free >= 1024**3
    case = args.output / f"n{count}"
    case.mkdir()
    repo = case / "repo"
    (repo / "src").mkdir(parents=True)
    for i in range(count):
        (repo / "src" / f"file_{i:05}.rs").write_text(driver.source(i))
    model_log = (case / "model.log").open("wb")
    model = subprocess.Popen([sys.executable, str(ROOT / "scripts/p7_release_resource_preparation.py"),
                              "--mock", str(case)], stdout=model_log, stderr=model_log)
    product = None
    try:
        driver.wait_file(case / "http-port.json")
        port = json.loads((case / "http-port.json").read_text())["port"]
        config = {
            "auto_index": {"enabled": False}, "indexing": {"max_concurrent_parse": 4},
            "query": {"strategy": "local", "deadline_ms": 30000, "lane_timeout_ms": 20000,
                      "semantic_timeout_ms": 5000, "semantic_top_k": 24},
            "semantic": {"enabled": True, "network_opt_in": True, "allow_query_network": False,
                         "model_id": "fake/resource-preparation", "dimensions": 128,
                         "max_input_tokens": 8192, "max_batch_items": 16,
                         "endpoint": f"http://127.0.0.1:{port}/v1", "allow_http": True,
                         "api_key_ref": "env:P7_RESOURCE_DUMMY", "max_concurrent": 4,
                         "max_concurrent_per_project": 2, "retry_max_attempts": 1,
                         "retry_total_deadline_ms": 120000, "breaker_failure_threshold": 100}}
        driver.write(repo / ".codecortex.json", config)
        product = driver.Product(args.binary.resolve(), repo, case, case / "cache")
        product.model_pid = model.pid
        product.phase = "cold-empty-index-build"
        result = product.tool("index", {"path": str(repo)})  # no full=true or changed_paths
        timing = dict(product.last_timing)
        assert result["files_scanned"] == result["files_parsed"] == result["files_added"] == count
        assert result["files_skipped"] == 0 and not result["parse_errors"]
        driver.wait_file(case / "http-entered")
        held = driver.db_report(repo)
        assert held["counts"]["files"] == count and held["counts"]["edge_tables"]["test_edges"] == 0
        assert held["integrity"] == "ok" and held["foreign_key_errors"] == 0
        driver.write(case / "cold-result.json", {"result": result, "timing": timing, "db": held})
        (case / "http-release").touch()
        product.phase = "backfill-drain"
        deadline = time.monotonic() + 300
        while True:
            status = product.tool("status", {"aspect": "capabilities"})["retrieval"]
            if status["semantic_state"] == "ready":
                break
            assert time.monotonic() < deadline, "backfill did not become ready in 300s"
            time.sleep(.2)
        final = driver.db_report(repo)
        assert final["counts"]["semantic_manifest"] == count
        assert final["integrity"] == "ok" and final["foreign_key_errors"] == 0
        driver.write(case / "final-db.json", final)
        driver.write(case / "final-status.json", status)
        product.phase = "ready"
        query = product.tool("search", {"query": "resource_00000", "retrieval_strategy": "local", "top_k": 5})
        assert query["machine_pack"]["hits"]
        exit_code = product.close()
        product = None
        assert exit_code == 0
        row = {"files": count, "cold_wire_ms": timing["wire_to_response_ms"],
               "files_skipped": result["files_skipped"], "ready_manifests": count,
               "test_edges": 0, "integrity": "ok", "FK_errors": 0, "exit_code": exit_code}
        summary["scales"].append(row)
        print(json.dumps(row), flush=True)
    finally:
        (case / "http-release").touch()
        if product:
            product.close()
        if model.poll() is None:
            model.terminate()
            model.wait(timeout=10)
        model_log.close()
        driver.write(args.output / "summary.json", summary)

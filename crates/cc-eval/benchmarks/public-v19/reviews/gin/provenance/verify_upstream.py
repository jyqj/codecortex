#!/usr/bin/env python3
"""Independent Git provenance/license admission, without any benchmark query reads."""
import argparse
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[6]
AUTHOR = "77d8707110afcb9935d29117ddf6162df4cef277"
SOURCE = "43fe48e8a0f44af783116cdb010725e6bb50255f"
PREFIX = "crates/cc-eval/benchmarks/public-v19/gin/"
URL = "https://github.com/gin-gonic/gin.git"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def audit(upstream):
    def original(path):
        assert path in {"source-manifest.json", "provenance/source-lock.json", "license/LICENSE"} or path.startswith("source/")
        assert ".." not in path
        return subprocess.check_output(["git", "show", f"{AUTHOR}:{PREFIX}{path}"], cwd=ROOT)

    def git(*args):
        return subprocess.check_output(["git", "-C", upstream, *args])

    assert git("remote", "get-url", "origin").decode().strip() == URL
    assert git("rev-parse", "FETCH_HEAD").decode().strip() == SOURCE
    commit = git("cat-file", "commit", SOURCE)
    assert hashlib.sha1(b"commit " + str(len(commit)).encode() + b"\0" + commit).hexdigest() == SOURCE
    tree = git("rev-parse", SOURCE + "^{tree}").decode().strip()
    manifest_raw = original("source-manifest.json")
    lock_raw = original("provenance/source-lock.json")
    manifest, lock = json.loads(manifest_raw), json.loads(lock_raw)
    assert manifest["source_sha"] == lock["source_sha"] == SOURCE
    assert manifest["repository"] == "gin-gonic/gin" and lock["repository"] == "gin-gonic/gin"
    tree_files = {}
    for row in git("ls-tree", "-r", "-z", SOURCE).split(b"\0"):
        if not row:
            continue
        metadata, path = row.split(b"\t", 1)
        mode, kind, blob = metadata.split()
        assert kind == b"blob" and mode in {b"100644", b"100755"}
        tree_files[path.decode()] = blob.decode()
    assert set(tree_files) == {f["path"] for f in manifest["files"]}
    module = git("show", SOURCE + ":go.mod")
    assert module.splitlines()[0] == b"module github.com/gin-gonic/gin"
    license_raw = git("show", SOURCE + ":LICENSE")
    assert license_raw == original("license/LICENSE")
    assert license_raw.startswith(b"The MIT License (MIT)\n")
    assert sha(license_raw) == manifest["license_sha256"] == lock["license_files"][0]["sha256"]
    assert len(license_raw) == lock["license_files"][0]["bytes"]
    inventory = []
    for f in manifest["files"]:
        if not f["admitted"]:
            continue
        path = f["path"]
        assert path.endswith(".go") and not path.endswith("_test.go")
        raw = git("show", SOURCE + ":" + path)
        assert raw == original("source/" + path)
        assert sha(raw) == f["sha256"] and len(raw) == f["bytes"]
        assert b"MIT style" in raw[:250] and b"Code generated" not in raw[:500]
        snapshot_blob = subprocess.check_output(["git", "rev-parse", f"{AUTHOR}:{PREFIX}source/{path}"], cwd=ROOT).decode().strip()
        assert snapshot_blob == tree_files[path]
        inventory.append({"path_sha256": sha(path.encode()), "upstream_and_snapshot_git_blob": tree_files[path], "sha256": sha(raw), "bytes": len(raw), "mit_notice": "verified"})
    assert len(inventory) == 53
    return {"author_current_sha": AUTHOR, "previous_review_sha": "0ecfc52a14e423a1d37b81240667d4c1511076cb", "source_sha": SOURCE, "upstream_url": URL, "upstream_commit_object_sha1_verified": True, "upstream_tree": tree, "upstream_tree_inventory_matches_manifest": True, "source_manifest_sha256": sha(manifest_raw), "source_lock_sha256": sha(lock_raw), "go_module_sha256": sha(module), "declared_and_actual_repository": "gin-gonic/gin", "declared_and_actual_language": "Go", "license": {"declared_and_actual": "MIT", "sha256": sha(license_raw), "bytes": len(license_raw), "git_blob": tree_files["LICENSE"], "snapshot_matches_upstream": True, "notice_preservation": "original per-file notices and full upstream root license retained"}, "inventory": inventory, "decision": "accept_provenance_and_license_admission_for_53_admitted_files", "counts": {"upstream_tree_files": 130, "manifest_files": 130, "admitted_source_files_byte_and_git_blob_equal": 53, "admitted_source_files_with_mit_notice": 53, "excluded_files_not_admitted": 77, "benchmark_query_body_reads": 0, "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "ranking_runs": 0}, "resolved_scope_error_codes": ["U_UPSTREAM_GIT_ORIGIN_NOT_INDEPENDENTLY_REPLAYED"], "still_open_scope_error_codes": ["G_CROSS_REPOSITORY_AND_BLOCKED_COMPONENTS_UNREVIEWED", "H_CUSTODY_BLOCKED_33"], "prior_dev_decisions_and_receipts_mutated": False, "scope": "provenance_license_only; excluded_file_bodies_not_reviewed; no_semantic_reacceptance_or_global_gate_closure", "fetch_command": "git fetch --depth=1 https://github.com/gin-gonic/gin.git 43fe48e8a0f44af783116cdb010725e6bb50255f", "permission_rejections": 0}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream-git", required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = audit(args.upstream_git)
    raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
    path = HERE / "upstream-admission.json"
    if args.check:
        assert path.read_text() == raw
    else:
        path.write_text(raw)
    print(json.dumps({"counts": report["counts"], "decision": report["decision"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

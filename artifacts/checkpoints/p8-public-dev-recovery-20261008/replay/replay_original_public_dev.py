#!/usr/bin/env python3
"""Replay unchanged historical public DEV integrity and V02 validators.

No retrieval, provider, gold rewriting, holdout body access, or automatic fetch.
The recovered archives preserve bytes and original relative suite paths. They
are source subsets, not claimed clean/full upstream checkouts.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time

PREFIX = "artifacts/checkpoints/p8-public-dev-recovery-20261008/"
MANIFEST_PATH = PREFIX + "exact-support-recovery.json"
MANIFEST_SHA256 = "075659bb9d94965e3e11f1125a000e2ab2f82b88aeb282de683e39110875c524"
AUDITOR_SHA256 = "4488cd5fef6b1f11a9a383bd296817c2d2fe79fa66130ca164d605b7932563e3"
AUDITOR_DEP_SHA256 = "dd9971219e0c368f876b7e7b6ce563f0779a0e9cde1e00b5d844a0fb57e6774a"
REQUIRED_COUNTS = {"express": (70, 59, 7, 110), "requests": (91, 83, 20, 141),
                   "gin": (67, 55, 53, 127), "typescript": (73, 59, 20, 98)}
MAX_BYTES = 64 * 1024 * 1024

def require(ok, label):
    if not ok:
        raise ValueError(label)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def read_file(path, maximum=4 * 1024 * 1024):
    path = Path(path)
    for parent in [path, *path.parents]:
        require(not parent.is_symlink(), "symlink input")
    meta = path.stat()
    require(stat.S_ISREG(meta.st_mode) and meta.st_size <= maximum, "nonregular or oversized input")
    raw = path.read_bytes()
    require(len(raw) == meta.st_size, "input changed while reading")
    return raw

def read_relative(root, name, maximum=4 * 1024 * 1024):
    require(isinstance(name, str) and name and not name.startswith("/")
            and "\\" not in name and ":" not in name
            and all(p not in ("", ".", "..", ".git") for p in name.split("/")),
            "unsafe relative path")
    require(not re.search(r"hold[-_]?out|held[-_]?out", name, re.I), "protected body path")
    return read_file(root / name, maximum)

def write_json(path, obj):
    path.write_text(json.dumps(obj, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

def command(out, label, argv, timeout=120):
    require(re.fullmatch(r"[a-z0-9-]+", label) is not None, "command label")
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", GIT_TERMINAL_PROMPT="0",
               GIT_NO_LAZY_FETCH="1", GIT_OPTIONAL_LOCKS="0")
    begin = time.time()
    timed_out = False
    try:
        p = subprocess.run([str(a) for a in argv], stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           timeout=timeout, env=env)
        stdout, stderr, code = p.stdout, p.stderr, p.returncode
    except subprocess.TimeoutExpired as exc:
        stdout, stderr, code, timed_out = exc.stdout or b"", exc.stderr or b"", None, True
    (out / (label + ".stdout")).write_bytes(stdout)
    (out / (label + ".stderr")).write_bytes(stderr)
    receipt = {"argv": [str(a) for a in argv], "exit_code": code, "timed_out": timed_out,
               "timeout_seconds": timeout, "started_unix": begin, "finished_unix": time.time(),
               "stdout_sha256": sha(stdout), "stderr_sha256": sha(stderr)}
    write_json(out / (label + ".json"), receipt)
    require(code == 0 and not timed_out, label + " failed; original diagnostics retained")
    return receipt

def verify_archive(repo):
    raw = read_relative(repo, MANIFEST_PATH)
    require(sha(raw) == MANIFEST_SHA256, "recovery manifest drift")
    m = json.loads(raw)
    files = m["files"]
    require(len(files) == 183 and len({r["entry"] for r in files}) == 183, "input count/duplicate")
    total, snapshot = 0, {}
    for r in files:
        ref, original = r["entry"].split(":", 1)
        require(re.fullmatch("[0-9a-f]{40}", ref) is not None
                and original.startswith("crates/cc-eval/benchmarks/public-v19/"), "fixed original identity")
        expected_path = PREFIX + "original-public-dev/" + ref + "/" + original
        require(r["restored_path"] == expected_path, "archive path binding")
        b = read_relative(repo, expected_path)
        require(len(b) == r["bytes"] and sha(b) == r["sha256"], "exact input byte/hash drift")
        require(hashlib.sha1(b"blob " + str(len(b)).encode() + b"\0" + b).hexdigest()
                == r["git_blob"], "Git blob identity drift")
        total += len(b)
        require(total <= MAX_BYTES, "aggregate byte budget")
        snapshot[expected_path] = sha(b)
    require(total == 3029158, "input byte count")
    return m, snapshot

def replay(repo, evaluator, evaluator_sha, out):
    require(re.fullmatch("[0-9a-f]{64}", evaluator_sha) is not None, "evaluator hash")
    binary = read_file(evaluator, 512 * 1024 * 1024)
    require(sha(binary) == evaluator_sha, "evaluator identity")
    m, before = verify_archive(repo)
    require(sha(read_relative(repo, "scripts/p8_corpus_audit.py")) == AUDITOR_SHA256,
            "original auditor drift")
    require(sha(read_relative(repo, "scripts/p8_release_evidence.py")) == AUDITOR_DEP_SHA256,
            "original auditor dependency drift")
    head = command(out, "head", ["git", "-C", repo, "rev-parse", "HEAD"], 30)
    command(out, "original-public-auditor",
            [sys.executable, repo / "scripts/p8_corpus_audit.py", "--repo-root", repo,
             "--output", out / "original-public-audit.json"], 600)
    audit = json.loads(read_file(out / "original-public-audit.json"))
    require(audit["status"] == "passed_local_audit" and audit["errors"] == {}, "original audit verdict")
    require(audit["totals"]["native_dev_rows"] == 301
            and audit["totals"]["compat_dev_rows"] == 256
            and audit["totals"]["source_files"] == 100
            and audit["totals"]["source_spans"] == 476, "original audit totals")
    for name, expected in REQUIRED_COUNTS.items():
        r = audit["repositories"][name]
        require(tuple(r[k] for k in ("native_dev_rows", "compat_dev_rows",
                                    "source_files_checked", "source_spans_checked")) == expected,
                "original repo count")
    entries = {r["entry"]: r for r in m["files"]}
    require(len(m["suites"]) == 16, "suite count")
    checks = []
    for i, s in enumerate(m["suites"]):
        row = entries[s["entry"]]
        path = repo / row["restored_path"]
        suite = json.loads(read_file(path))
        require(suite["source"]["commit"] is None, "historical snapshot scope")
        require(suite["scoring"] in ("codecortex-native-v1", "oce-compat-v1"), "scoring identity")
        require(suite["queries"] in ("queries.native.dev.jsonl", "queries.compat.dev.jsonl"),
                "DEV query pointer")
        require((suite["top_k"], suite["repetitions"], suite["warmup"],
                 suite["timeout_ms"], suite["seed"]) == (10, 3, 0, 30000, 20261003),
                "original budget drift")
        r = command(out, "v02-" + str(i).zfill(2), [evaluator, "validate", "--suite", path])
        checks.append({"suite_entry": s["entry"], "suite_sha256": row["sha256"], **r})
    _, after = verify_archive(repo)
    require(before == after, "original archives mutated")
    require(sha(read_file(evaluator, 512 * 1024 * 1024)) == evaluator_sha, "evaluator changed")
    result = {"schema_version": 1, "status": "original_public_dev_replay_passed",
              "evaluator_sha256": evaluator_sha, "recovery_manifest_sha256": MANIFEST_SHA256,
              "original_auditor_sha256": AUDITOR_SHA256,
              "native_dev_rows": 301, "compat_dev_rows": 256,
              "source_files": 100, "gold_spans": 476, "suites_validated": checks,
              "original_bytes_unchanged": True, "provider_calls": 0, "ranking_runs": 0,
              "protected_body_reads": 0, "new_gold_semantic_reviews": 0,
              "scope": "Fixed public source subsets only; no full checkout or protected-holdout certification.",
              "canonical_six_repo_projection": "separate reviewed stage, not performed here"}
    write_json(out / "receipt.json", result)
    return result

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--repo-root", type=Path, required=True)
    p.add_argument("--evaluator", type=Path, required=True)
    p.add_argument("--evaluator-sha256", required=True)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    require(not a.output.exists(), "output already exists; retain previous attempt")
    a.output.mkdir(parents=True)
    try:
        result = replay(a.repo_root.resolve(), a.evaluator.absolute(), a.evaluator_sha256, a.output.absolute())
    except Exception as exc:
        write_json(a.output / "failure.json", {"status": "failed", "exception": type(exc).__name__,
                   "message": str(exc), "not_accepted": True})
        raise
    print(json.dumps({"status": result["status"], "suites": len(result["suites_validated"]),
                      "receipt_sha256": sha(read_file(a.output / "receipt.json"))}))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())

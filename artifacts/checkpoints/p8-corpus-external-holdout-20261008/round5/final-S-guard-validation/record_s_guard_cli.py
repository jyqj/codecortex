#!/usr/bin/env python3
"""Record one actual S guard CLI invocation; preserve all completed P evidence."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
BASE = Path("/workspace/scratch/031390cf22eb")
ROOT = BASE / "codecortex-union-validation"
OUT = BASE / "round5-S-guard-validation"
PRODUCT = "1ed3c7df574db6d450f79ef29d5148f68556b3db"
PRODUCT_TREE = "d16aa044e56c9347e6b4ff75d783cc4317f15087"
SOURCE = "714264adc8e3d38f0f25de16669de3fd363adeaa"
TREE = "ecfca123e2dd55a946e4a4722d38a567c432262b"
VERSION = "p8-oracle-compat-source-20261008-v14"
REVIEW = "artifacts/checkpoints/p8-corpus-external-holdout-20261008/round5/independent-source-review.json"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, stderr=subprocess.PIPE)


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n")


def p_evidence():
    directory = BASE / "round5-validation"
    manifest = directory / "validation-artifact-manifest.json"
    raw = manifest.read_bytes()
    assert digest(raw) == "28427c58317a5f5897399f0291a1d4a4e58021c71544d31504e8b3b4ab54dd49"
    contents = json.loads(raw)
    assert contents["source"] == PRODUCT and len(contents["files"]) == 37
    for row in contents["files"]:
        data = (directory / row["path"]).read_bytes()
        assert len(data) == row["bytes"] and digest(data) == row["sha256"]
    binary = directory / "product/codecortex"
    binary_sha = digest(binary.read_bytes())
    assert binary_sha == "f149c9afc5df9d464c68a46d618040ef91a39d847a6c8dc641e7764c52a0dcd1"
    return {"source": PRODUCT, "artifact_manifest": str(manifest),
            "artifact_manifest_sha256": digest(raw), "verified_files": 37,
            "verified_bytes": contents["total_bytes"], "binary_path": str(binary),
            "binary_sha256": binary_sha, "binary_was_not_rebuilt_or_reidentified_as_S": True}


def inventory():
    head = git("rev-parse", "HEAD").decode().strip()
    tree = git("rev-parse", "HEAD^{tree}").decode().strip()
    status = git("status", "--porcelain=v1", "--ignored", "--untracked-files=all").decode()
    assert head == SOURCE and tree == TREE and status == "", (head, tree, status)
    raw = git("ls-tree", "-rz", SOURCE, "--", "Cargo.toml", "Cargo.lock", "crates", "scripts",
              ".github/workflows", "tests/source_integrity", "CONTRIBUTING.md", REVIEW)
    rows = []
    for entry in raw.split(b"\0"):
        if not entry:
            continue
        metadata, relative = entry.split(b"\t", 1)
        mode, kind, oid = metadata.decode().split()
        path = relative.decode()
        assert mode in ("100644", "100755") and kind == "blob"
        target = ROOT / path
        assert all(not part.is_symlink() for part in (target, *list(target.parents)[:len(Path(path).parts) - 1]))
        data = target.read_bytes()
        assert hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest() == oid, path
        group = "installed_review" if path == REVIEW else (
            "source" if path.startswith("crates/") or path in ("Cargo.toml", "Cargo.lock") else "validation_and_policy")
        rows.append({"path": path, "group": group, "mode": mode, "git_blob": oid,
                     "bytes": len(data), "sha256": digest(data)})
    rows.sort(key=lambda row: row["path"])
    counts = {group: sum(row["group"] == group for row in rows)
              for group in ("source", "validation_and_policy", "installed_review")}
    manifest = (json.dumps(rows, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    return {"head": head, "tree": tree, "status": status, "counts": counts,
            "manifest_sha256": digest(manifest), "files": rows}


def main():
    receipt_path = OUT / "S-guard-cli.json"
    assert not receipt_path.exists()
    assert git("rev-parse", "HEAD").decode().strip() == PRODUCT
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == PRODUCT_TREE
    assert git("status", "--porcelain=v1", "--ignored", "--untracked-files=all") == b""
    preserved_before = p_evidence()
    changed = git("diff", "--name-only", PRODUCT, SOURCE).decode().splitlines()
    assert set(changed) == {REVIEW, "scripts/reviewed-source-registry-v14.json", "scripts/verify_reviewed_source_v14.py"}
    assert git("diff", "--name-only", PRODUCT, SOURCE, "--", "Cargo.toml", "Cargo.lock", "crates") == b""
    transition = subprocess.run(["git", "checkout", "--detach", SOURCE], cwd=ROOT,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    assert transition.returncode == 0, transition.stderr
    before = inventory()
    write(OUT / "S-source-before.json", before)
    observer_before = digest(Path(__file__).read_bytes())
    command = [sys.executable, "-B", str(ROOT / "scripts/verify_reviewed_source_v14.py"),
               "--source-version", VERSION]
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    receipt = {"schema_version": 1, "scope": "one actual installed S guard CLI; no full 174 suite or product build",
               "source": SOURCE, "tree": TREE, "source_version": VERSION,
               "cwd": str(ROOT), "argv": command,
               "python": {"executable": sys.executable, "version": sys.version,
                          "executable_sha256": digest(Path(sys.executable).read_bytes())},
               "environment_overrides": {"PYTHONDONTWRITEBYTECODE": "1"},
               "observer_sha256": observer_before,
               "transition": {"from": PRODUCT, "to": SOURCE, "argv": ["git", "checkout", "--detach", SOURCE],
                              "exit_code": transition.returncode, "stdout": transition.stdout, "stderr": transition.stderr,
                              "changed_paths": changed, "crates_and_Cargo_unchanged": True},
               "source_before": str(OUT / "S-source-before.json"),
               "source_before_sha256": digest((OUT / "S-source-before.json").read_bytes()),
               "preserved_P_evidence_before": preserved_before,
               "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
               "status": "running", "guard_invocations": 1,
               "full_174_suite_executed": False}
    write(receipt_path, receipt)
    print(json.dumps({"status": "running", "source": SOURCE, "argv": command, "input_counts": before["counts"]}), flush=True)
    start = time.monotonic()
    with (OUT / "guard-cli.stdout.log").open("wb") as stdout, (OUT / "guard-cli.stderr.log").open("wb") as stderr:
        process = subprocess.run(command, cwd=ROOT, env=environment, stdout=stdout, stderr=stderr)
    elapsed = time.monotonic() - start
    after = inventory()
    write(OUT / "S-source-after.json", after)
    preserved_after = p_evidence()
    unchanged = before == after and observer_before == digest(Path(__file__).read_bytes()) and preserved_before == preserved_after
    receipt.update({"exit_code": process.returncode, "elapsed_seconds": round(elapsed, 6),
                    "finished_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    "source_after": str(OUT / "S-source-after.json"),
                    "source_after_sha256": digest((OUT / "S-source-after.json").read_bytes()),
                    "source_and_guard_inputs_unchanged": before == after,
                    "observer_unchanged": observer_before == digest(Path(__file__).read_bytes()),
                    "preserved_P_evidence_after": preserved_after,
                    "preserved_P_evidence_unchanged": preserved_before == preserved_after,
                    "source_manifest_sha256": before["manifest_sha256"], "input_counts": before["counts"],
                    "installed_trust_inputs": [row for row in before["files"] if row["path"] in (
                        REVIEW, "scripts/reviewed-source-registry-v14.json", "scripts/verify_reviewed_source_v14.py", ".github/workflows/ci.yml")],
                    "stdout": {"path": str(OUT / "guard-cli.stdout.log"), "sha256": digest((OUT / "guard-cli.stdout.log").read_bytes())},
                    "stderr": {"path": str(OUT / "guard-cli.stderr.log"), "sha256": digest((OUT / "guard-cli.stderr.log").read_bytes())},
                    "status": "passed" if process.returncode == 0 and unchanged else "failed"})
    write(receipt_path, receipt)
    print(json.dumps({"status": receipt["status"], "exit_code": process.returncode,
                      "elapsed_seconds": receipt["elapsed_seconds"], "source_unchanged": unchanged,
                      "receipt": str(receipt_path), "receipt_sha256": digest(receipt_path.read_bytes())}), flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())

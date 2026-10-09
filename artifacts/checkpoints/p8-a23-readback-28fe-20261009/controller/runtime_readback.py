"""Read fixed a23 runtime originals through three unchanged trusted audit scripts.

No network, compiler, product/ELF invocation, SQLite opening or measurement.
Transport and safe extraction are the caller's responsibility. Original ZIPs and
extracted inputs must remain read-only; generated reviews/logs are separate.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys
import time

SOURCE = "a23bb72d3c954f385b99fe81ce9189885c208557"
SOURCE_TREE = "58147c952505c44da1f41eb4b9c31643f2303b96"
SOURCE_MANIFEST = "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00"
MAX_JSON_BYTES = 8 * 1024 * 1024
AUDIT_TIMEOUT_SECONDS = 300
ARTIFACTS = json.loads(r'''[{"artifact_id":11592751905,"kind":"mixed","concurrency":1,"bytes":16616365,"sha256":"6cfceee02ed7fcd71039b9870e48457ddb3a7607935c7a5e798d549ee33123da","run_id":37871838957},{"artifact_id":11591493782,"kind":"mixed","concurrency":4,"bytes":16987501,"sha256":"727ab17068655b9ee5cd5677fb3f1a9fb4460c87266dfb0e6628d9a75678502c","run_id":37871838957},{"artifact_id":11593165899,"kind":"mixed","concurrency":8,"bytes":16649537,"sha256":"427906a25d21afae8085328c8fc92d6997763b1ad0f52247118052ec788118b8","run_id":37871838957},{"artifact_id":11592154271,"kind":"mixed","concurrency":16,"bytes":16413256,"sha256":"9ca7ff34acba74969feb840837d48ad89952efe06a9d02733c7c29d92efdf487","run_id":37871838957},{"artifact_id":11594089439,"kind":"soak","concurrency":4,"bytes":51069087,"sha256":"d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086","run_id":37871838957},{"artifact_id":11591821446,"kind":"backfill","bytes":7825602,"sha256":"24fa1251fa006d23df2f801a7d5ba7eb420223c4a7dbd0cce7ab8fdcf6b36e15","run_id":37871838957},{"artifact_id":11591607348,"kind":"lifecycle","bytes":49345173,"sha256":"63aadf94879ea86782e9759828e853c1a030d70014fdc3fbdc127a18ca2eb055","run_id":37871838975}]''')
REVIEWER_ORIGINS = json.loads(r'''{"commit":"4630e635cd762bfbd2726cf1dd49e37803c06034","prefix":"artifacts/checkpoints/p8-a23-closeout-20261009-bfcc/round4-reviewed-progress/review-runtime/","files":[{"name":"runtime-independent-review-v2.py","git_blob":"baeeb5e8ead26b2b76a0453b5c5fae778cb24f80","bytes":20338,"sha256":"a7aafb29e4a45695b4bfa3cfb27c1ab040e305a89cfe3917e2629b2fa960d75d"},{"name":"lifecycle-independent-review-v2.py","git_blob":"c10499b065f948f363b6c274fed0ef1589cfa608","bytes":15626,"sha256":"c501b09549b7c08e922e6cc98919a0c81567b46b8fef1c0515c4e0209f45c121"},{"name":"backfill-independent-review.py","git_blob":"19fe6ad1e63dc8492bfdc6fc1b7f1ece58e23a0e","bytes":9298,"sha256":"08a02b1ed5e39c07071bfa6bc46a04295d6c87f8c47634835d7d4a4a44c74139"}]}''')
SOURCE_HELPERS = json.loads(r'''{"scripts/p8_runtime.py":{"bytes":58896,"sha256":"0656766656ad73b69e6e28db742115ec9b3dd0440c515f694b7568d3cf87c79d","git_blob":"3fe09616768cf35ecc3b76f348ef9eca88d98cd8"},"scripts/p8_runtime_build.py":{"bytes":18068,"sha256":"cf453e1f4312b95dde0cc0338cecab3abf0467c8c3c1428c354824d01c1d20db","git_blob":"95da67c6a270cf4fd96f318cc4ffd494bbad5512"},"scripts/p7_build_identity.py":{"bytes":5940,"sha256":"5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5","git_blob":"4f2213705984effd6aa3cdb75a99cbb03a08c39e"},"scripts/p8_cold_build.py":{"bytes":52649,"sha256":"f910326658b5539c539d1c02cd29a1236e11911b581f1beaf2495a42b53319fa","git_blob":"ed454b6d29e727c87b0b26e4a975a840d0dec310"},"scripts/p8_rollback.py":{"bytes":38818,"sha256":"4aa129fb9f2cd72b591d5c26a3b5e87aae0e2c3c6d03717c199fa612a322de50","git_blob":"61ff1d61c0fc9f14b92daf522faa0d9d5e04be56"},"scripts/p7_stdio_build_receipt.py":{"bytes":9147,"sha256":"e3fd3015d52765f4b61a3ef1f7381b3037b8254c36f1bd26f548d28053e85da2","git_blob":"2b031eb8fc71b0e64ee4bc41f835a49f7c01e45e"},"scripts/p8_backfill.py":{"bytes":8799,"sha256":"c6f75ab31b226b7fea6ef7f14adc66d823015d909f57b0bd47a8e7f10b47a4dd","git_blob":"380b775c0de6141ed8b4f63d2fb4a223bba54221"},"scripts/resource_harness/__init__.py":{"bytes":74,"sha256":"3193f3cffa59c2c6613b3ec007343207598b3cbb0d7c4c7fbcf4513cb9ca3a36","git_blob":"2371ef6525a9815cf327d95d7f8aca1455257e8d"},"scripts/resource_harness/runtime.py":{"bytes":11117,"sha256":"5876d2b0102f6e3a38189b2337c1783500393acde0dc04666d7f97ff34d92cc5","git_blob":"d39eb4ba95f3c3431d1a424c3f6d7f4563664b5c"},"scripts/p8_lifecycle.py":{"bytes":38417,"sha256":"820a63c0dd76a6cfd52dfbf633afe66a72c7eee152cbe08db69971b39d896f5d","git_blob":"4d413c2b23930f2856139a4bfef0f9c114254df3"},"scripts/p8_resources.py":{"bytes":15915,"sha256":"ceb2fb5323688a6c30692f01830e7e3d6e0df4697300eca90003a8ce5b7ae6b1","git_blob":"4ed93b4b1c73002fe592da63ad7d7481a563cad4"}}''')
SCOPE = (
    "Readback of fixed original a23 observations; no new primary execution, "
    "ELF execution, database opening, performance improvement, task closure or "
    "release approval. The unchanged reviewers contain historical Mac scope "
    "wording: that text describes their prior author execution, not this "
    "invocation. This receipt records the actual review host separately."
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def fingerprint(path):
    path = Path(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode), "expected regular file: " + str(path))
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    after = path.lstat()
    require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns),
            "file changed while hashing: " + str(path))
    return {"bytes": after.st_size, "sha256": value.hexdigest()}


def read_json(path):
    path = Path(path)
    require(path.lstat().st_size <= MAX_JSON_BYTES, "JSON exceeds readback bound")
    proof = fingerprint(path)
    raw = path.read_bytes()
    require(len(raw) == proof["bytes"]
            and hashlib.sha256(raw).hexdigest() == proof["sha256"],
            "JSON changed during read")
    return json.loads(raw), proof


def write_json_new(path, value):
    with Path(path).open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write("\n")


def audit_environment():
    env = os.environ.copy()
    for key in ("PYTHONPATH", "PYTHONHOME", "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
        env.pop(key, None)
    env.update(PYTHONDONTWRITEBYTECODE="1", GIT_OPTIONAL_LOCKS="0")
    return env


def git_read(source, *args):
    result = subprocess.run(
        ["git", "-C", str(source), *args], capture_output=True, text=True,
        timeout=30, env=audit_environment(), check=False)
    require(result.returncode == 0, "read-only Git identity check failed: " + result.stderr)
    return result.stdout.strip()


def source_identity(source):
    require(git_read(source, "rev-parse", "--verify", "HEAD") == SOURCE,
            "source checkout is not exact a23")
    require(git_read(source, "rev-parse", "HEAD^{tree}") == SOURCE_TREE,
            "source tree differs")
    helpers = {}
    for relative, expected in SOURCE_HELPERS.items():
        observed = fingerprint(source / relative)
        require(observed == {key: expected[key] for key in ("bytes", "sha256")},
                "trusted source helper changed: " + relative)
        helpers[relative] = observed
    return {"commit": SOURCE, "tree": SOURCE_TREE, "helpers": helpers}


def reviewer_identity(controller):
    result = {}
    for spec in REVIEWER_ORIGINS["files"]:
        path = controller / "reviewers" / spec["name"]
        proof = fingerprint(path)
        require(proof == {key: spec[key] for key in ("bytes", "sha256")},
                "trusted original reviewer changed: " + spec["name"])
        result[spec["name"]] = proof
    return result


def selection(spec):
    identity = spec["artifact_id"]
    if spec["kind"] in ("mixed", "soak"):
        return ("runtime-independent-review-v2.py",
                [str(identity), spec["sha256"], spec["kind"], str(spec["concurrency"])],
                "runtime-" + str(identity) + "-review-v2.json")
    if spec["kind"] == "backfill":
        return ("backfill-independent-review.py", [], "backfill-11591821446-review.json")
    return ("lifecycle-independent-review-v2.py", [], "lifecycle-11591607348-review-v2.json")


def enforce_outcome(spec, report, extracted):
    require(report.get("artifact_id") == spec["artifact_id"]
            and report.get("run_id") == spec["run_id"],
            "review report is for another original artifact/run")
    require(report.get("review_status") == "raw_and_receipt_review_passed"
            and report.get("errors") == [], "original data review failed")
    require(isinstance(report.get("checks"), dict) and bool(report["checks"])
            and all(type(value) is int and value > 0 for value in report["checks"].values()),
            "review check inventory missing")

    if spec["kind"] in ("mixed", "soak"):
        original, proof = read_json(extracted / "p8-runtime" / "report.json")
        plan, plan_proof = read_json(extracted / "p8-runtime" / "plan.json")
        count = 3601 if spec["kind"] == "soak" else 900
        builds = (count + 2) // 3
        reads = count - builds
        require(report.get("source_sha") == SOURCE and report.get("source_input_count") == 1087
                and report.get("observer_count") == 9, "runtime source domain differs")
        require(report.get("profile") == spec["kind"]
                and report.get("configured_concurrency") == spec["concurrency"]
                and report.get("outcomes") == {"build": builds, "read": reads},
                "runtime reviewed workload differs")
        require(original.get("status") == "passed_observation" and original.get("exit_code") == 0
                and original.get("offered") == count and original.get("outcomes") == {"success": count}
                and original.get("failures") == [] and original.get("task_complete") is False
                and original.get("release_approval") is False, "original runtime did not pass")
        require(plan.get("profile") == spec["kind"] and plan.get("concurrency") == spec["concurrency"]
                and plan.get("operations") == count and plan.get("files") == 1000
                and plan.get("offer_interval_ms") == (1000 if spec["kind"] == "soak" else 500),
                "original fixed runtime plan differs")
        if spec["kind"] == "soak":
            cache = report.get("soak_cache_replay")
            require(isinstance(cache, dict) and cache.get("passed") is True
                    and cache.get("expected_offered_reads") == 2400
                    and cache.get("recorded_offered_reads") == 2400
                    and cache.get("validated_reads") == 2400 and cache.get("errors") == [],
                    "complete original soak cache population not verified")
        return {"report": proof, "plan": plan_proof, "status": original["status"],
                "exit_code": original["exit_code"], "offered": count,
                "outcomes": original["outcomes"], "parity_sha256": original.get("parity_sha256"),
                "raw_sha256": original.get("raw_sha256"), "statistics": original.get("statistics")}

    if spec["kind"] == "backfill":
        original, proof = read_json(extracted / "receipt.json")
        require(report.get("source_sha") == SOURCE and report.get("source_input_count") == 1087
                and report.get("observer_count") == 9 and report.get("requests") == 768,
                "backfill source or request population differs")
        cells = report.get("cells", [])
        require(len(cells) == 24 and {(row["seed"], row["phase"], row["concurrency"])
                for row in cells} == {(seed, phase, cap) for seed in (7, 19, 43)
                for phase in ("quiet", "held") for cap in (1, 4, 8, 16)}
                and all(row.get("n") == 32 for row in cells), "backfill cells incomplete")
        require(original.get("status") == "passed_observation" and original.get("exit_code") == 0
                and original.get("build_exit_code") == 0 and original.get("execution_exit_code") == 0
                and original.get("task_complete") is False and original.get("release_approval") is False,
                "original backfill did not pass")
        return {"receipt": proof, "status": original["status"], "exit_code": original["exit_code"],
                "requests": 768, "executable_sha256": original.get("executable_sha256")}

    original, proof = read_json(extracted / "measurement" / "receipt.json")
    source = report.get("source", {})
    require(source.get("source_commit") == SOURCE and source.get("source_tree") == SOURCE_TREE
            and source.get("input_count") == 1087 and source.get("manifest_sha256") == SOURCE_MANIFEST,
            "lifecycle source differs")
    require(report.get("measurement_status") == "complete_observation"
            and report.get("sample_counts") == {"expected": 1230, "recorded": 1230}
            and report.get("sessions") == 431, "lifecycle population differs")
    require(original.get("status") == "complete_observation" and original.get("exit_code") == 0
            and original.get("failures") == [] and original.get("release_certified") is False,
            "original lifecycle did not pass")
    return {"receipt": proof, "status": original["status"], "exit_code": original["exit_code"],
            "sample_counts": original.get("sample_counts"), "evaluator_sha256": original.get("evaluator_sha256")}


def review_runtime(audit_root: Path, source: Path, controller: Path) -> dict:
    """Review all seven fixed originals; continue after individual failure."""
    audit_root, source, controller = (Path(path).resolve(strict=True)
                                     for path in (audit_root, source, controller))
    result = {"schema_version": 1, "passed": False, "scope": SCOPE,
              "actual_review_host": {"system": platform.system(), "release": platform.release(),
                                    "machine": platform.machine(), "python": sys.version},
              "source_before": None, "source_after": None, "reviewers_before": None,
              "reviewers_after": None, "items": [], "errors": [],
              "original_task_completion": False, "remaining_original_todos": 29}
    derived = audit_root / "derived" / "runtime-readback"
    reviews = audit_root / "review-runtime"
    try:
        require((audit_root / "source").resolve(strict=True) == source,
                "audit source link differs from declared checkout")
        require(not derived.exists() and not reviews.exists(),
                "readback destination already exists; do not reuse prior reports")
        derived.parent.mkdir(parents=True, exist_ok=True)
        derived.mkdir()
        reviews.mkdir()
        result["source_before"] = source_identity(source)
        result["reviewers_before"] = reviewer_identity(controller)
    except Exception as error:
        result["errors"].append({"phase": "preflight", "error": type(error).__name__ + ": " + str(error)})
        return result

    for spec in ARTIFACTS:
        item = {"artifact_id": spec["artifact_id"], "run_id": spec["run_id"], "kind": spec["kind"],
                "passed": False, "errors": [], "execution": None, "review_report": None}
        result["items"].append(item)
        archive = audit_root / "raw" / str(spec["artifact_id"]) / "original.zip"
        extracted = archive.parent / "extracted"
        name, arguments, report_name = selection(spec)
        report_path = reviews / report_name
        prefix = derived / str(spec["artifact_id"])
        stdout_path, stderr_path = Path(str(prefix) + ".stdout"), Path(str(prefix) + ".stderr")
        try:
            require(fingerprint(archive) == {key: spec[key] for key in ("bytes", "sha256")},
                    "fixed official ZIP digest or size differs")
            require(extracted.is_dir() and not extracted.is_symlink(), "extracted input root invalid")
            require(not report_path.exists(), "review output was already present")
            require(source_identity(source) == result["source_before"], "source changed before audit")
            require(reviewer_identity(controller) == result["reviewers_before"], "reviewer changed before audit")
            argv = [sys.executable, "-I", "-B", str(controller / "reviewers" / name), *arguments]
            execution = {"argv": argv, "cwd": str(audit_root), "started_unix_ns": time.time_ns(),
                         "timeout_seconds": AUDIT_TIMEOUT_SECONDS, "exit_code": None}
            item["execution"] = execution
            started = time.monotonic_ns()
            try:
                with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
                    process = subprocess.run(argv, cwd=audit_root, env=audit_environment(),
                                             stdout=stdout, stderr=stderr, timeout=AUDIT_TIMEOUT_SECONDS,
                                             check=False)
                    execution["exit_code"] = process.returncode
            finally:
                execution["elapsed_ns"] = time.monotonic_ns() - started
                execution["finished_unix_ns"] = time.time_ns()
            require(execution["exit_code"] == 0, "original audit process returned nonzero")
            report, report_proof = read_json(report_path)
            item["review_report"] = {"path": str(report_path), **report_proof}
            item["key_results"] = report
            item["original_outcome"] = enforce_outcome(spec, report, extracted)
            item["passed"] = True
        except Exception as error:
            item["errors"].append(type(error).__name__ + ": " + str(error))
        finally:
            # Keep a report emitted before a nonzero/timeout even when it cannot pass.
            if report_path.exists() and item["review_report"] is None:
                try:
                    failed_report, failed_proof = read_json(report_path)
                    item["review_report"] = {"path": str(report_path), **failed_proof}
                    item["key_results"] = failed_report
                except Exception as error:
                    item["errors"].append("retain emitted review report: " + str(error))
            for label, path in (("stdout", stdout_path), ("stderr", stderr_path)):
                try:
                    if path.exists():
                        item[label] = {"path": str(path), **fingerprint(path)}
                except Exception as error:
                    item["errors"].append("retain " + label + ": " + str(error))
            try:
                require(source_identity(source) == result["source_before"], "source changed after audit")
                require(reviewer_identity(controller) == result["reviewers_before"], "reviewer changed after audit")
                require(fingerprint(archive) == {key: spec[key] for key in ("bytes", "sha256")},
                        "original ZIP changed after audit")
            except Exception as error:
                item["errors"].append("postflight: " + type(error).__name__ + ": " + str(error))
            if item["errors"]:
                item["passed"] = False
            try:
                write_json_new(Path(str(prefix) + ".json"), item)
            except Exception as error:
                item["passed"] = False
                item["errors"].append("retain item receipt: " + str(error))

    try:
        result["source_after"] = source_identity(source)
        result["reviewers_after"] = reviewer_identity(controller)
        require(result["source_after"] == result["source_before"]
                and result["reviewers_after"] == result["reviewers_before"], "final identity changed")
    except Exception as error:
        result["errors"].append({"phase": "final_identity", "error": type(error).__name__ + ": " + str(error)})
    result["passed"] = not result["errors"] and len(result["items"]) == 7 and all(
        item["passed"] for item in result["items"])
    try:
        write_json_new(derived / "summary.json", result)
    except Exception as error:
        result["passed"] = False
        result["errors"].append({"phase": "retain_summary", "error": type(error).__name__ + ": " + str(error)})
    return result

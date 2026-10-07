#!/usr/bin/env python3
"""Reproduce four original selector methods without their full source-union setup."""
import argparse
import hashlib
import importlib
import json
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import traceback

sys.dont_write_bytecode = True
REVIEWED = "bfc89ce3a5f55ec6505d2801c888a10c7499140e"
FIX = "2da6fd281cb88042e26f34872dfe4d4c61ba7c4d"
AE = "ae906513886bef4501d0a1d2ecffd68ef76c3f5d"
V10 = "48efa5a6058005fe141d7a835ea9f127db9bd28b"
REFS = [AE, V10, "3c8c204216cb54c41850c6da826a68fec3586430",
        "886f90a542a6174a037c79eebbb4f74848fb1f53",
        "eb7cdc55aa94c8d6865bed14fa37fff08080af33"]
METHODS = [
    ("test_reviewed_source", "ReviewedSourceTests",
     "test_ci_only_allows_explicit_selector_and_local_p8_checks", AE),
    ("test_current_source_v2", "CaptureSourceTests",
     "test_ci_selects_explicit_reviewed_version_and_keeps_historical_p0", AE),
    ("test_current_source_v3", "OwnerSourceTests",
     "test_ci_selector_only_change_and_historical_p0_preserved", AE),
    ("test_reviewed_source_v10", "SequentialReviewTests",
     "test_current_workflows_have_only_the_explicit_migration", V10),
]
WATCHED = [
    "tests/source_integrity/" + name + ".py" for name in [
        "test_current_source", "test_current_source_v2", "test_current_source_v3",
        "test_reviewed_source", "test_reviewed_source_v10", "test_reviewed_source_v11",
        "test_historical_integrations_v2", "test_reviewed_base", "test_plan_progress"]
] + [
    "scripts/" + name for name in [
        "verify_current_source.py", "verify_current_source_v2.py", "verify_current_source_v3.py",
        "verify_reviewed_source.py", "verify_reviewed_source_v10.py", "verify_reviewed_source_v11.py",
        "verify_historical_integrations_v2.py", "p0_historical_corpus.py",
        "current-source-registry-v1.json", "current-source-registry-v2.json"]
] + [".github/workflows/ci.yml", ".github/workflows/p7-engineering.yml"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--output-prefix", type=Path, required=True)
    args = parser.parse_args()
    repo = args.repo.resolve()
    commands = []
    phase = "preflight"

    def audit(event, values):
        if event == "subprocess.Popen":
            executable, argv, cwd, _env = values
            commands.append({"phase": phase, "argv": list(argv), "cwd": str(cwd),
                             "executable": str(executable)})

    sys.addaudithook(audit)

    def git(*argv):
        return subprocess.check_output(["git", *argv], cwd=repo)

    def hashes():
        result = {}
        for path in WATCHED:
            current = (repo / path).read_bytes()
            pinned = git("show", REVIEWED + ":" + path)
            if current != pinned:
                raise AssertionError("working bytes differ from reviewed commit: " + path)
            result[path] = hashlib.sha256(current).hexdigest()
        return result

    before = hashes()
    head_before = git("rev-parse", "HEAD").decode().strip()
    shallow = git("rev-parse", "--is-shallow-repository").decode().strip() == "true"
    refs_present = {ref: subprocess.run(
        ["git", "cat-file", "-e", ref + "^{commit}"], cwd=repo,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0 for ref in REFS}
    if not all(refs_present.values()):
        raise AssertionError("this narrow cached-ref reproduction requires the recorded local refs")
    sys.path.insert(0, str(repo / "tests/source_integrity"))
    sys.path.insert(0, str(repo / "scripts"))
    rows = []
    output = ["Independent final historical-selector review", "reviewed_commit=" + REVIEWED,
              "fix_commit=" + FIX, "observed_root_head_before=" + head_before,
              "command=" + shlex.join([sys.executable, "-B", *sys.argv]),
              "mode=original test methods; fresh empty temp roots; no setUpClass/setUp; no helper patching",
              "all_required_refs_already_local=" + str(all(refs_present.values()))]
    for module_name, class_name, method_name, fixture in METHODS:
        identity = module_name + "." + class_name + "." + method_name
        phase = identity
        module = importlib.import_module(module_name)
        case = getattr(module, class_name)(method_name)
        events = []

        def profile(frame, event, _arg):
            if event not in {"call", "return"}:
                return
            filename = frame.f_code.co_filename
            name = frame.f_code.co_name
            if filename == str(repo / "scripts/verify_current_source.py") and name in {"ensure_refs", "blob"}:
                record = {"event": event, "function": name,
                          "caller": frame.f_back.f_code.co_name if frame.f_back else None}
                if name == "ensure_refs":
                    record["refs"] = list(frame.f_locals["refs"])
                else:
                    record.update(ref=frame.f_locals["ref"], path=frame.f_locals["path"])
                events.append(record)

        started = time.monotonic()
        error = None
        with tempfile.TemporaryDirectory(prefix="final-selector-review-") as temporary:
            case.root = Path(temporary)
            sys.setprofile(profile)
            try:
                getattr(case, method_name)()
            except Exception:
                error = traceback.format_exc()
            finally:
                sys.setprofile(None)
        ensure_indexes = [i for i, event in enumerate(events)
                          if event["function"] == "ensure_refs" and event["event"] == "return"
                          and fixture in event["refs"] and event["caller"] == method_name]
        blob_indexes = [i for i, event in enumerate(events)
                        if event["function"] == "blob" and event["event"] == "call"
                        and event["ref"] == fixture and event["path"] == ".github/workflows/ci.yml"]
        ensure_before_read = bool(ensure_indexes and blob_indexes
                                  and min(ensure_indexes) < min(blob_indexes))
        status = "passed" if error is None and ensure_before_read else "failed"
        row = {"method": identity, "status": status, "fixture": fixture,
               "explicit_ensure_return_before_fixture_blob": ensure_before_read,
               "all_original_method_assertions_executed": error is None,
               "duration_seconds": round(time.monotonic() - started, 6),
               "ensure_blob_trace": events, "error": error}
        rows.append(row)
        output.append(f"{status.upper()} {identity} ({row['duration_seconds']:.6f}s) "
                      f"explicit_ensure_before_fixture={ensure_before_read}")
        if error:
            output.append(error)
    phase = "postflight"
    after = hashes()
    head_after = git("rev-parse", "HEAD").decode().strip()
    unchanged = before == after
    fetches = [row for row in commands if row["argv"][:2] == ["git", "fetch"]]
    passed = all(row["status"] == "passed" for row in rows) and unchanged
    report = {
        "schema_version": 1, "reviewer": "/root/pr_audit", "reviewed_commit": REVIEWED,
        "fix_commit": FIX, "observed_root_head_before": head_before,
        "observed_root_head_after": head_after, "verdict": "accepted_scoped" if passed else "rejected",
        "scope": "four_original_historical_selector_methods_and_fixture_ref_order_only",
        "selected_guard": "verify_reviewed_source_v11.py",
        "command": [sys.executable, "-B", *sys.argv], "cwd": str(Path.cwd()),
        "python_version": sys.version, "is_shallow_repository": shallow,
        "refs_present_before_execution": refs_present,
        "git_fetch_commands_observed": fetches, "methods": rows,
        "reviewed_path_count": len(before), "reviewed_sha256": before,
        "reviewed_paths_unchanged_after_execution": unchanged,
        "subprocess_commands": commands,
        "additional_fixture_read_audit": [
            {"scope": "v3 historical BASE before the new ae fixture",
             "result": "normal setUpClass calls approved_union, which ensures BASE/SOURCE/DELIVERY/REVIEW before method execution"},
            {"scope": "old current BASE and historical helpers",
             "result": "verify_ci calls expected_ci, which calls ensure_base before historical_blob"},
            {"scope": "v10/v11 P7 workflow fixture",
             "result": "expected_ci ensures P7_SNAPSHOT before CI read; verify_ci evaluates expected_ci before the subsequent P7 workflow read"},
            {"scope": "v10/v11 review and historical snapshot fixtures",
             "result": "normal setUpClass approved_union/verify_snapshots acquires immutable refs before fixture reads"},
            {"scope": "historical_integrations_v2 fixed snapshots and independent evidence row",
             "result": "class setup ensures TASK_BASE/PACKING_INTEGRATION/E3_INTEGRATION; independent row explicitly ensures its source_commit"},
            {"scope": "remaining source-integrity tests",
             "result": "current v1/v2/v3 use approved-union setup; reviewed_base intentionally uses synthetic Git; plan_progress uses locally authored fixtures"}
        ],
        "blocking_findings": [], "source_edits": [],
        "limitations": [
            "All required refs existed locally, and this repository is not shallow; no actual missing-ref fetch branch or GitHub shallow checkout was exercised.",
            "Original test methods ran directly with fresh temporary roots. Heavy setUpClass and setUp, full 94-test source suite, and full v11 source-proof reconstruction were not run by this reviewer.",
            "Current v11 CI was checked by the original selected.verify_ci calls. This is not a separate v11 source admission review and does not relabel the earlier f99 v10 review.",
            "No Rust/product tests, release certification, model calls, task-state changes, remote changes, or source edits were performed. Final GitHub CI must validate actual shallow-fetch behavior."
        ]
    }
    output.extend(["reviewed_paths_unchanged=" + str(unchanged),
                   "observed_git_fetch_commands=" + str(len(fetches)),
                   "verdict=" + report["verdict"],
                   "LIMIT: cached local refs do not prove the actual GitHub shallow-fetch path; final GitHub CI remains required."])
    args.output_prefix.parent.mkdir(parents=True, exist_ok=True)
    args.output_prefix.with_suffix(".json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    args.output_prefix.with_suffix(".log").write_text("\n".join(output) + "\n")
    print("\n".join(output))
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Execute original CLI failure gates and retain all deliberately invalid raw."""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys

from p7_build_identity import source_snapshot
from p8_cold_build import digest, new_directory, write_json
from p8_recovery import production_fault_test
from p8_rollback import require


CASES = (
    "unmeasurable_latency_cannot_pass_comparison",
    "zero_measurement_gate_is_invalid",
    "cli_quality_and_latency_failures_are_nonzero_and_keep_raw",
    "cli_inconclusive_is_nonzero",
    "cli_lock_failure_preserves_machine_readable_failure_and_raw",
    "cli_bad_policy_is_recorded_without_overwriting_existing_report",
)


def run(root, output, target):
    root, out = root.resolve(strict=True), new_directory(output)
    source = source_snapshot(root)
    require(not target.exists(), "gate execution target must be newly owned")
    env = dict(os.environ)
    env.update(CARGO_TARGET_DIR=str(target.absolute()), CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0",
               CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0")
    report = dict(schema_version=1, status="running", source=source, cases=[],
                  scope="original deterministic artifact/CLI fault controls; no product performance claim",
                  release_approval=False, task_complete=False)
    try:
        cli = None
        for number, name in enumerate(CASES):
            case = out / f"case-{number:02}"
            env["CODECORTEX_GATE_EVIDENCE_DIR"] = str(case / "retained-fixtures")
            result = production_fault_test(root, case, env, "cc-eval", "benchmark_cli", name)
            report["cases"].append(result)
            messages = [json.loads(line) for line in (case / "build/stdout.log").read_text().splitlines() if line.strip()]
            rows = [m for m in messages if m.get("reason") == "compiler-artifact"
                    and m.get("target", {}).get("name") == "cc-eval"
                    and m.get("target", {}).get("kind") == ["bin"] and m.get("executable")]
            require(len(rows) == 1, "original gate CLI build event is missing")
            executable = Path(rows[0]["executable"]).resolve(strict=True)
            if cli is None:
                copied = out / "cc-eval"
                shutil.copy2(executable, copied)
                copied.chmod(0o555)
                cli = dict(path=str(executable), executable_sha256=digest(copied), cargo_artifact=rows[0])
            require(digest(executable) == cli["executable_sha256"], "actual gate CLI changed between cases")
            if name != "zero_measurement_gate_is_invalid":
                require((case / "retained-fixtures").is_dir()
                        and any((case / "retained-fixtures").rglob("*.json")),
                        "original CLI failure fixtures were not retained")
        report["cli"] = cli
        require(source_snapshot(root) == source, "gate source changed during execution")
        report.update(status="passed_original_failure_gates", exit_code=0,
                      observed_controls=["quality_failed", "performance_failed", "inconclusive_nonzero",
                                         "zero_plan_invalid", "zero_latency_inconclusive", "raw_lock_drift_invalid",
                                         "bad_policy_invalid", "output_overwrite_refused"])
    except Exception as error:
        report.update(status="failed", exit_code=2, error=f"{type(error).__name__}: {error}")
    finally:
        report["files"] = {p.relative_to(out).as_posix(): digest(p) for p in sorted(out.rglob("*")) if p.is_file()}
        write_json(out / "report.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", type=Path, required=True)
    args = parser.parse_args()
    result = run(args.source, args.output, args.target)
    print(json.dumps({k: result.get(k) for k in ("status", "exit_code", "error")}, indent=2))
    raise SystemExit(result["exit_code"])

#!/usr/bin/env python3
"""Narrow A+B validation; never edits source, task status, CI or source guards."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    repo, out = args.repo.resolve(), args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    settings = {
        "CARGO_TARGET_DIR": str(args.target_dir.resolve()),
        "CARGO_BUILD_JOBS": "2",
        "CARGO_PROFILE_DEV_DEBUG": "0",
        "CARGO_PROFILE_TEST_DEBUG": "0",
        "CARGO_INCREMENTAL": "0",
        "CARGO_TERM_COLOR": "never",
    }
    env.update(settings)

    def read(*argv):
        return subprocess.check_output(argv, cwd=repo, env=env, text=True).strip()

    def source_manifest():
        names = subprocess.check_output(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
             "--", "crates", "Cargo.toml", "Cargo.lock"], cwd=repo,
        ).decode().split("\0")
        return {name: hashlib.sha256((repo / name).read_bytes()).hexdigest()
                for name in sorted(set(names) - {""})}

    sources = source_manifest()
    manifest_path = out / "source-sha256.json"
    manifest_path.write_text(json.dumps(sources, indent=2) + "\n")
    receipt = {
        "scope": "combined A final validation SQL accounting + B full Python capture revalidation",
        "requested_A_head": "afba69e3665c87d174aa0cbabfc37ccd1670fb27",
        "requested_B_commits": ["4a1f4f6fbb2cd1b17e62ed9bf819bb1b9a52941e",
                                "707b6b64048b9fc119ab1c396eb082be8de2d8a5"],
        "combined_head": read("git", "rev-parse", "HEAD"),
        "combined_tree": read("git", "rev-parse", "HEAD^{tree}"),
        "branch": read("git", "branch", "--show-current"),
        "git_status_before": read("git", "status", "--porcelain"),
        "rustc": read("rustc", "+1.95.0", "--version"),
        "cargo": read("cargo", "+1.95.0", "--version"),
        "environment": settings,
        "source_inputs": len(sources),
        "source_manifest": "source-sha256.json",
        "source_manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        "log_normalization": "trailing blank lines removed only",
        "expected_tests": 27,
        "commands": [],
        "not_run": ["broad_lint", "full_workspace_tests", "source_guard_v4_or_v5",
                    "public_quality", "100k", "remote_CI", "live_provider", "non_Linux"],
        "boundary": "Intermediate combined tree retains v4 guard; parent owns subsequent v5 integration. No task status, CI or source guard edits.",
    }
    cargo = ["cargo", "+1.95.0"]
    commands = [
        ("p7-validation-work", cargo + ["test", "--offline", "--locked", "-p", "cc-eval",
                                        "--test", "p7_validation_work"]),
        ("python-capture", cargo + ["test", "--offline", "--locked", "-p", "cc-index",
                                    "--test", "python_inventory_revalidation",
                                    "--test", "python_inventory_capture"]),
        ("format", cargo + ["fmt", "--all", "--", "--check"]),
    ]
    for name, argv in commands:
        started = time.monotonic()
        log_path = out / f"{name}.log"
        with log_path.open("w") as log:
            result = subprocess.run(argv, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT)
        raw = log_path.read_bytes()
        log_path.write_bytes(raw.rstrip(b"\n") + b"\n" if raw else raw)
        summaries = re.findall(
            r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out",
            log_path.read_text(),
        )
        row = {
            "name": name, "argv": argv, "exit_code": result.returncode,
            "elapsed_seconds": round(time.monotonic() - started, 3),
            "log": log_path.name,
            "log_sha256": hashlib.sha256(log_path.read_bytes()).hexdigest(),
            "test_summaries": [dict(status=items[0], **dict(zip(
                ["passed", "failed", "ignored", "measured", "filtered_out"], map(int, items[1:])
            ))) for items in summaries],
        }
        receipt["commands"].append(row)
        print(json.dumps(row), flush=True)
    after = source_manifest()
    receipt["source_inputs_unchanged"] = sources == after
    receipt["source_mismatches"] = sorted(name for name in sources.keys() | after.keys()
                                          if sources.get(name) != after.get(name))
    receipt["git_status_after"] = read("git", "status", "--porcelain")
    receipt["observed_tests"] = sum(summary["passed"] for row in receipt["commands"]
                                    for summary in row["test_summaries"])
    receipt["passed"] = (
        all(row["exit_code"] == 0 for row in receipt["commands"])
        and receipt["observed_tests"] == receipt["expected_tests"]
        and receipt["source_inputs_unchanged"]
        and receipt["git_status_before"] == receipt["git_status_after"] == ""
    )
    (out / "validation.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({key: receipt[key] for key in
                      ["combined_head", "source_inputs", "source_manifest_sha256", "observed_tests",
                       "source_inputs_unchanged", "passed"]}), flush=True)
    raise SystemExit(0 if receipt["passed"] else 1)


if __name__ == "__main__":
    main()

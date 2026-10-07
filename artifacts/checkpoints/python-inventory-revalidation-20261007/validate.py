#!/usr/bin/env python3
"""Run focused authored capture checks and preserve exact input/log receipts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-dir", required=True)
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    settings = {
        "CARGO_TARGET_DIR": str(Path(args.target_dir).resolve()),
        "CARGO_BUILD_JOBS": "2",
        "CARGO_PROFILE_DEV_DEBUG": "0",
        "CARGO_PROFILE_TEST_DEBUG": "0",
        "CARGO_INCREMENTAL": "0",
    }
    env.update(settings)

    def text(*argv):
        return subprocess.check_output(argv, cwd=root, env=env, text=True).strip()

    tracked = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
         "--", "crates", "Cargo.toml", "Cargo.lock"], cwd=root,
    ).decode().split("\0")
    sources = {name: hashlib.sha256((root / name).read_bytes()).hexdigest()
               for name in sorted(set(tracked) - {""})}
    (out / "source-sha256.json").write_text(json.dumps(sources, indent=2) + "\n")
    cargo = ["cargo", "+1.95.0"]
    tests = ["--test", "python_inventory_revalidation", "--test", "python_inventory_capture",
             "--test", "python_inventory_resource_integration"]
    commands = [
        ("capture-tests", cargo + ["test", "--offline", "--locked", "-p", "cc-index"] + tests),
        ("capture-drift", cargo + ["test", "--offline", "--locked", "-p", "cc-index", "--lib",
                                  "python_inventory_observed_midcapture_mutations_refuse"]),
        ("clippy", cargo + ["clippy", "--offline", "--locked", "-p", "cc-index", "--lib"]
         + tests + ["--", "-D", "warnings"]),
        ("format", cargo + ["fmt", "--all", "--", "--check"]),
        ("diff", ["git", "diff", "--check"]),
    ]
    receipt = {
        "scope": "explicit Python capture receipt and full rederivation; authored local fixtures",
        "base_head": text("git", "rev-parse", "HEAD"),
        "branch": text("git", "branch", "--show-current"),
        "rustc": text("rustc", "+1.95.0", "--version"),
        "cargo": text("cargo", "+1.95.0", "--version"),
        "environment": settings,
        "source_inputs": len(sources),
        "source_manifest": "source-sha256.json",
        "source_manifest_sha256": hashlib.sha256((out / "source-sha256.json").read_bytes()).hexdigest(),
        "log_normalization": "trailing blank lines removed only",
        "commands": [],
        "not_run": ["public_quality", "100k", "remote_ci", "non_linux", "live_provider",
                    "DB_MCP_integration", "source_registry_acceptance"],
        "parent_gate": "P7-014 remains in_progress; no full gate closure",
    }
    for name, argv in commands:
        started = time.monotonic()
        with (out / f"{name}.log").open("w") as log:
            result = subprocess.run(argv, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
        log_path = out / f"{name}.log"
        raw = log_path.read_bytes()
        log_path.write_bytes(raw.rstrip(b"\n") + b"\n" if raw else raw)
        row = {"name": name, "argv": argv, "exit_code": result.returncode,
               "elapsed_seconds": round(time.monotonic() - started, 3), "log": f"{name}.log"}
        receipt["commands"].append(row)
        print(json.dumps(row), flush=True)
    receipt["passed"] = all(row["exit_code"] == 0 for row in receipt["commands"])
    (out / "validation.json").write_text(json.dumps(receipt, indent=2) + "\n")
    raise SystemExit(0 if receipt["passed"] else 1)


if __name__ == "__main__":
    main()

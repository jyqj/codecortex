#!/usr/bin/env python3
"""Run P0 checks with a local SDK/target directory and retain command receipts.
Does not change the system SDK, default Rust toolchain, Git branch, or source files.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdkroot", type=Path)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--product-binary", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.evidence_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env["PATH"] = str(Path.home() / ".cargo/bin") + os.pathsep + env.get("PATH", "")
    env["CARGO_TARGET_DIR"] = str(args.target_dir.resolve())
    env["CARGO_BUILD_JOBS"] = "4"
    if args.sdkroot:
        sdk = args.sdkroot.resolve()
        if not sdk.is_dir():
            raise ValueError("SDK directory missing")
        env["SDKROOT"] = str(sdk)
        env["RUSTFLAGS"] = "-C link-arg=-isysroot -C link-arg=" + str(sdk)
        env["RUSTDOCFLAGS"] = env["RUSTFLAGS"]
    if args.product_binary:
        env["CODECORTEX_BENCH_BINARY"] = str(args.product_binary.resolve())
    env["CODECORTEX_BENCH_OBSERVATIONS"] = str(out / "known-defects")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    files = sorted(list((root / "crates/cc-eval").rglob("*.rs")) + [root / "Cargo.lock", root / "crates/cc-eval/Cargo.toml"])
    source_files = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    receipt = {"head": head, "source_files": source_files, "environment": {k: env.get(k) for k in ["SDKROOT", "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_TARGET_DIR"]}, "commands": [],
               "msrv": {"required": "1.95", "status": "not_run", "reason": "this runner verifies the selected installed toolchain; MSRV requires a separate explicit toolchain job"}}
    commands = [
        ["rustc", "--version"], ["cargo", "--version"],
        ["cargo", "test", "--workspace", "--locked", "--offline"],
        ["cargo", "test", "-p", "cc-eval", "--features", "eval-http", "--locked", "--offline"],
        ["cargo", "build", "-p", "cc-eval", "--features", "eval-http", "--bin", "cc-eval", "--locked", "--offline"],
    ]
    if args.product_binary:
        commands.append(["cargo", "test", "-p", "cc-eval", "--features", "eval-http", "--locked", "--offline", "--test", "benchmark_adapters", "--", "--ignored"])
    failed = False
    for i, command in enumerate(commands):
        log = out / ("command-%02d.log" % i)
        started = time.monotonic()
        with log.open("w") as handle:
            result = subprocess.run(command, cwd=root, env=env, stdout=handle, stderr=subprocess.STDOUT)
        content = log.read_text(errors="replace")
        counts = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;", content)
        item = {"argv": command, "exit_code": result.returncode, "elapsed_seconds": round(time.monotonic() - started, 3), "log": log.name,
                "sha256": hashlib.sha256(log.read_bytes()).hexdigest(), "tests": {"passed": sum(int(a) for a,b,c in counts), "failed": sum(int(b) for a,b,c in counts), "ignored": sum(int(c) for a,b,c in counts)}}
        receipt["commands"].append(item)
        (out / "validation.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps(item), flush=True)
        failed = failed or result.returncode != 0
    after = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    receipt["covered_source_unchanged"] = after == source_files
    failed = failed or after != source_files
    receipt["status"] = "failed" if failed else "passed"
    (out / "validation.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())

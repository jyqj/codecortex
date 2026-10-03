#!/usr/bin/env python3
"""Replay only synthetic parser/resolver tests and bounded checks, offline."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument("--original", type=Path, required=True)
p.add_argument("--pr103", type=Path, required=True)
p.add_argument("--fixed", type=Path, required=True)
p.add_argument("--target-prefix", type=Path, required=True)
p.add_argument("--output", type=Path, required=True)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
test_path = Path("crates/cc-index/src/resolver/type_atom_identifier_tests.rs")
helper_path = Path("crates/cc-index/src/resolver/helpers.rs")
model_path = Path("crates/cc-model/src/resolution.rs")
trailer = '\n#[cfg(test)]\n#[path = "type_atom_identifier_tests.rs"]\nmod type_atom_identifier_tests;\n'
harness = (a.fixed / test_path).read_bytes()
env = os.environ.copy()
env["RUSTUP_HOME"] = "/workspace/.rustup"
env["CARGO_HOME"] = "/workspace/.cargo"
env["PATH"] = "/workspace/.cargo/bin:" + env["PATH"]
receipt = {"scope": "synthetic parse/resolver only; no full-index/corpus/migration acceptance", "runs": []}

def git(tree, *args):
    return subprocess.check_output(["git", "-C", str(tree), *args])

def digest(data):
    return hashlib.sha256(data).hexdigest()

def run(label, tree, command, expected):
    run_env = env | {"CARGO_TARGET_DIR": str(a.target_prefix) + "-" + label.split("-")[0]}
    log = a.output / (label + ".log")
    with log.open("wb") as output:
        result = subprocess.run(command, cwd=tree, env=run_env, stdout=output, stderr=subprocess.STDOUT)
    row = {"label": label, "command": command, "cwd": str(tree), "target": run_env["CARGO_TARGET_DIR"], "exit_code": result.returncode, "expected_exit_code": expected, "log_sha256": digest(log.read_bytes())}
    receipt["runs"].append(row)
    print(label, result.returncode, "expected", expected, flush=True)
    return result.returncode == expected

ok = True
for label, tree, sha in [
    ("original", a.original, "ace2bc7983be2955831c9384e44d1bdd0749c909"),
    ("pr103", a.pr103, "da5b05ee08d84fd336d5a98f25da7cae176daebf"),
    ("fixed", a.fixed, None),
]:
    tree = tree.resolve()
    if sha:
        assert git(tree, "rev-parse", "HEAD").decode().strip() == sha
        original = git(tree, "show", f"{sha}:{helper_path}")
        assert (tree / helper_path).read_bytes() in [original, original + trailer.encode()]
        assert (tree / model_path).read_bytes() == git(tree, "show", f"{sha}:{model_path}")
        (tree / helper_path).write_bytes(original + trailer.encode())
        (tree / test_path).write_bytes(harness)
    assert (tree / test_path).read_bytes() == harness
    receipt[label] = {"base_sha": sha or git(tree, "rev-parse", "HEAD").decode().strip(), "overlay": "test module only" if sha else "local repair", "harness_sha256": digest(harness), "helper_sha256": digest((tree / helper_path).read_bytes()), "model_sha256": digest((tree / model_path).read_bytes()), "cargo_lock_sha256": digest((tree / "Cargo.lock").read_bytes())}
    ok &= run(label, tree, ["cargo", "test", "--offline", "--locked", "-p", "cc-index", "--lib", "type_atom_identifier_tests", "--", "--nocapture"], 0 if label == "fixed" else 101)
for label, args in [
    ("fixed-resolver", ["test", "-p", "cc-index", "--lib", "resolver::"]),
    ("fixed-model", ["test", "-p", "cc-model", "--lib", "resolution::"]),
    ("fixed-clippy", ["clippy", "-p", "cc-index", "-p", "cc-model", "--all-targets", "--", "-D", "warnings"]),
]:
    ok &= run(label, a.fixed.resolve(), ["cargo", args[0], "--offline", "--locked", *args[1:]], 0)
ok &= run("fixed-fmt", a.fixed.resolve(), ["cargo", "fmt", "--all", "--", "--check"], 0)
receipt["rustc"] = subprocess.check_output(["rustc", "--version"], env=env).decode().strip()
receipt["independent_negative_evidence_sha"] = "7f650a5f8338e0d1ad2d8e8592ae31c89b34047c"
receipt["all_expected_exit_codes"] = ok
(a.output / "receipt.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
raise SystemExit(0 if ok else 1)

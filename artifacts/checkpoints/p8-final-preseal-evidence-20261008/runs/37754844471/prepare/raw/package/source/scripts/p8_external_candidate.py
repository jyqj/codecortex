#!/usr/bin/env python3
"""Package actual same-source release builds for the explicit public external run.

No build, retrieval, scorer, question corpus, input-lock refresh, or ref update
is performed here. Product and runner receipts must already be complete.
"""
import argparse
import importlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import zipfile
import hashlib

TEMPLATE_SHA256 = "f8e69dcc9e12e9c22bc63658d7c533dc6c6028e6831c8e79f7857f24feab71dd"
TEMPLATE = "artifacts/benchmarks/p8-external-recovery-20261008/manifest.json"
RECOVERY = "artifacts/benchmarks/p8-external-recovery-20261008/revision-2/recover_external.py"
CONFIG = {"auto_index": {"enabled": False},
          "indexing": {"include_text_files": True, "include_hidden_files": True}}
EXTRA_HELPERS = ("scripts/p7_build_identity.py", "scripts/p7_stdio_build_receipt.py",
                 "scripts/p8_candidate_execution.py", "scripts/p8_external_candidate.py")
MAX_MEMBER = 512 * 1024 * 1024

def need(condition, label):
    if not condition:
        raise ValueError(label)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def json_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True, allow_nan=False) + "\n").encode()

def record(raw):
    return {"bytes": len(raw), "sha256": sha(raw)}

def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   stdin=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                   env=dict(os.environ, GIT_NO_LAZY_FETCH="1",
                                            GIT_TERMINAL_PROMPT="0", GIT_OPTIONAL_LOCKS="0"),
                                   timeout=120)

def regular(path):
    for p in [path, *path.parents]:
        need(not p.is_symlink(), "symlink input")
    meta = path.stat()
    need(stat.S_ISREG(meta.st_mode) and meta.st_size <= MAX_MEMBER, "input shape/size")
    raw = path.read_bytes()
    need(len(raw) == meta.st_size, "input changed while reading")
    return raw

def committed(root, source, name):
    need(not Path(name).is_absolute() and all(p not in ("", ".", "..") for p in name.split("/")),
         "relative source path")
    raw = regular(root / name)
    need(raw == git(root, "show", source + ":" + name), "helper differs from fixed Git bytes")
    return raw

def clean(root, source):
    need(git(root, "rev-parse", "--show-toplevel").decode().strip() == str(root), "checkout root")
    need(git(root, "rev-parse", "HEAD").decode().strip() == source, "candidate HEAD drift")
    need(not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "dirty candidate")
    need(not git(root, "submodule", "status", "--recursive").strip(), "submodules not admitted")

def package(args):
    root = args.source_root.resolve(strict=True)
    source = args.expected_source
    need(re.fullmatch("[0-9a-f]{40}", source) is not None, "full fixed source commit required")
    clean(root, source)
    for name in EXTRA_HELPERS + ("scripts/p8_release_evidence.py",):
        committed(root, source, name)
    need(Path(__file__).resolve() == root / "scripts/p8_external_candidate.py",
         "run the helper from the fixed candidate checkout")
    template_raw = committed(root, source, TEMPLATE)
    need(sha(template_raw) == TEMPLATE_SHA256, "original input template drift")
    template = json.loads(template_raw)
    recovery_raw = committed(root, source, RECOVERY)
    out = args.output.absolute()
    product = args.product_build.absolute()
    runner = args.runner_build.absolute()
    for path in (root, product, runner):
        need(not (out == path or out.is_relative_to(path) or path.is_relative_to(out)),
             "new output must be disjoint from source/build evidence")
    need(not out.exists() and not out.is_symlink(), "new output required")
    out.mkdir(parents=True)
    sys.path.insert(0, str(root / "scripts"))
    candidate = importlib.import_module("p8_candidate_execution")
    need(Path(candidate.__file__).resolve() == root / "scripts/p8_candidate_execution.py",
         "wrong build validator")
    before = candidate.source_snapshot(root)
    expected = {k: v for k, v in before.items() if k != "inputs"}
    builds = {}
    for role, directory, binary in (("product", product, "codecortex"), ("runner", runner, "cc-eval")):
        validated = candidate.validate_build(directory / binary, directory / "build-receipt.json",
                                             root, expected, role)
        receipt = validated["receipt"]
        need(receipt["build_profile"] == "release" and "--release" in receipt["build_command"],
             "new candidate must use actual release invocations")
        profile = receipt["cargo_artifact"]["profile"]
        need(receipt["actual_cargo_profile"] == profile
             and profile["opt_level"] in ("1", "2", "3", "s", "z")
             and profile["debug_assertions"] is False and profile["test"] is False,
             "actual Cargo release artifact is not optimized")
        builder_path = "scripts/p7_stdio_build_receipt.py" if role == "product" else "scripts/p8_candidate_execution.py"
        need(receipt["builder_sha256"] == sha(committed(root, source, builder_path)),
             "receipt builder differs from fixed candidate")
        if role == "product":
            need(receipt["identity_helper_sha256"] == sha(committed(root, source, "scripts/p7_build_identity.py")),
                 "product identity helper drift")
        builds[role] = {k: v for k, v in validated.items() if k != "receipt"}
        builds[role].update(build_profile=receipt["build_profile"], actual_cargo_profile=profile,
                            toolchain=receipt["toolchain"])
    need(builds["product"]["toolchain"] == builds["runner"]["toolchain"], "different build toolchains")
    validation = {"schema_version": 1, "status": "actual_same_source_release_builds_verified",
                  "source": expected, "builds": builds, "configuration_profile": "local-text-hidden",
                  "scope": "Actual build/source/raw identity only; no external retrieval or task acceptance.",
                  "template_sha256": TEMPLATE_SHA256, "recovery_script_sha256": sha(recovery_raw),
                  "packager_sha256": sha(regular(Path(__file__))), "release_certified": False}
    (out / "candidate-build-validation.json").write_bytes(json_bytes(validation))
    manifest = json.loads(template_raw)
    manifest.update(product_source=source, evidence_source=source,
                    configuration_profile="local-text-hidden", engine_config=CONFIG,
                    new_recovery_entry_execution_status="prepared_not_run",
                    candidate_build_validation_status=validation["status"],
                    recovery_script_sha256=sha(recovery_raw))
    members = {}
    def add(name, raw, executable=False):
        need(name not in members, "duplicate ZIP member")
        members[name] = (raw, executable)
        return dict(member=name, executable=executable, **record(raw))
    manifest["tools"] = {}
    for key, directory, binary in (("product", product, "codecortex"), ("evaluator", runner, "cc-eval")):
        for suffix, filename, executable in (
            ("", binary, True), ("_receipt", "build-receipt.json", False),
            ("_raw_build", "cargo-build.jsonl", False),
            ("_raw_stderr", "cargo-build.stderr.log", False),
            ("_source_inputs", "source-inputs.json", False)):
            raw = regular(directory / filename)
            manifest["tools"][key + suffix] = add("builds/" + key + "/" + filename, raw, executable)
    manifest["tools"]["candidate_build_validation"] = add("candidate-build-validation.json",
                                                           json_bytes(validation))
    manifest["helpers"] = {}
    for name in sorted(set(template["helpers"]) | set(EXTRA_HELPERS) | {RECOVERY}):
        raw = committed(root, source, name)
        # Existing scorer, parser, audit and policy bytes remain fixed. The sole
        # old helper deliberately revised for the new mode is p8_compat.py.
        if name in template["helpers"] and name != "scripts/p8_compat.py":
            need(record(raw) == {k: template["helpers"][name][k] for k in ("bytes", "sha256")},
                 "original scorer/helper/policy bytes changed")
        manifest["helpers"][name] = add("source/" + name, raw)
    archive = out / "candidate-tools.zip"
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        for name, (raw, executable) in sorted(members.items()):
            info = zipfile.ZipInfo(name, date_time=(2026, 10, 8, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = ((stat.S_IFREG | (0o755 if executable else 0o644)) << 16)
            z.writestr(info, raw)
    with zipfile.ZipFile(archive) as z:
        need(len(z.namelist()) == len(members) and set(z.namelist()) == set(members), "ZIP inventory")
        for name, (raw, _) in members.items():
            need(z.read(name) == raw, "ZIP byte mismatch")
    for role, directory, binary in (("product", product, "codecortex"), ("runner", runner, "cc-eval")):
        again = candidate.validate_build(directory / binary, directory / "build-receipt.json", root, expected, role)
        need(all(again[k] == builds[role][k] for k in
                 ("receipt_sha256", "raw_build_sha256", "binary_sha256", "source_manifest_sha256")),
             "build evidence drift while packaging")
    need(candidate.source_snapshot(root) == before, "candidate source drift")
    clean(root, source)
    manifest["archive"] = {"kind": "fresh_local_actual_build_bundle",
                           "repository": "jyqj/codecortex", **record(regular(archive))}
    manifest_path = out / "manifest.json"
    manifest_path.write_bytes(json_bytes(manifest))
    evidence = {"schema_version": 1, "status": "fresh_candidate_packaged",
                "source": expected, "archive": manifest["archive"],
                "manifest": record(regular(manifest_path)),
                "recovery_script_sha256": sha(recovery_raw),
                "members": {name: dict(executable=executable, **record(raw))
                            for name, (raw, executable) in sorted(members.items())},
                "ranking": "not_run", "release_certified": False}
    (out / "package-receipt.json").write_bytes(json_bytes(evidence))
    return evidence

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root", type=Path, required=True)
    p.add_argument("--expected-source", required=True)
    p.add_argument("--product-build", type=Path, required=True)
    p.add_argument("--runner-build", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    result = package(args)
    print(json.dumps({"status": result["status"], "manifest_sha256": result["manifest"]["sha256"],
                      "archive_sha256": result["archive"]["sha256"],
                      "source_commit": result["source"]["source_commit"], "ranking": "not_run"}))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Preserve and verify the actual release artifacts used by P8 observations."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys

from p7_build_identity import git, source_snapshot
from p7_stdio_build_receipt import compiler_environment, toolchain_identity
from p8_cold_build import digest, json_bytes, new_directory, write_json


TARGETS = {
    "codecortex": ("cc-server", "src/main.rs", "binary", "cargo_artifact"),
    "p8-oracle": ("cc-eval", "src/bin/p8-oracle.rs", "oracle", "oracle_artifact"),
    "p8-runtime-statistics": ("cc-eval", "src/bin/p8-runtime-statistics.rs", "statistics", "statistics_artifact"),
}
OBSERVER_FILES = (
    "scripts/p8_runtime.py", "scripts/p8_runtime_build.py", "scripts/p7_build_identity.py",
    "scripts/p8_cold_build.py", "scripts/p8_rollback.py",
    "scripts/p7_stdio_build_receipt.py", "scripts/p8_backfill.py",
    "scripts/resource_harness/__init__.py", "scripts/resource_harness/runtime.py",
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def regular(path):
    path = Path(path)
    require(stat.S_ISREG(path.lstat().st_mode), "missing or nonregular evidence: " + str(path))
    return path


def observer_snapshot(root):
    """Bind every executed local module to this exact checkout's committed blob."""
    root = Path(root).resolve(strict=True)
    require(Path(__file__).resolve() == root / "scripts/p8_runtime_build.py",
            "runtime receipt module belongs to another checkout")
    main = sys.modules.get("__main__")
    main_path = Path(getattr(main, "__file__", ""))
    if main_path.name in ("p8_runtime.py", "p8_runtime_build.py", "p8_backfill.py"):
        require(main_path.resolve() == root / "scripts" / main_path.name,
                "runtime entry point belongs to another checkout")
    commit = git(root, "rev-parse", "--verify", "HEAD^{commit}").decode().strip()
    files = {}
    for relative in OBSERVER_FILES:
        path = regular(root / relative)
        require(path.resolve() == root / relative, "observer path traverses a symlink")
        spec = commit + ":" + relative
        entry = git(root, "ls-tree", commit, "--", relative).decode().strip()
        require("\t" in entry, "observer is not in the committed source: " + relative)
        metadata, found = entry.split("\t", 1)
        mode, kind, blob = metadata.split()
        require(found == relative and kind == "blob" and mode in ("100644", "100755"),
                "observer is not a regular committed blob: " + relative)
        expected = hashlib.sha256(git(root, "cat-file", "blob", spec)).hexdigest()
        require(digest(path) == expected, "observer differs from its committed blob: " + relative)
        module_name = relative.removeprefix("scripts/").removesuffix(".py").replace("/", ".")
        if module_name.endswith(".__init__"):
            module_name = module_name.removesuffix(".__init__")
        module = sys.modules.get(module_name)
        if module is not None:
            require(Path(module.__file__).resolve() == path,
                    "loaded observer module belongs to another checkout: " + module_name)
        files[relative] = dict(git_blob=blob, git_mode=mode, bytes=path.stat().st_size, sha256=expected)
    return dict(source_commit=commit, files=files,
                manifest_sha256=hashlib.sha256(json_bytes(files)).hexdigest())


def artifact_inventory(root, exclude="seal.json"):
    """Include every retained file; the seal itself is the only exclusion."""
    root = Path(root)
    require(root.is_dir() and not root.is_symlink(), "artifact root must be an owned directory")
    files = {}
    for path in sorted(root.rglob("*")):
        mode = path.lstat().st_mode
        require(stat.S_ISREG(mode) or stat.S_ISDIR(mode), "nonregular runtime artifact: " + str(path))
        if stat.S_ISREG(mode) and (exclude is None or path != root / exclude):
            files[path.relative_to(root).as_posix()] = dict(bytes=path.stat().st_size, sha256=digest(path))
    return files


def seal_output(root):
    root = Path(root)
    require(not (root / "seal.json").exists(), "runtime artifacts are already sealed")
    value = dict(schema_version=1, artifact_inventory=artifact_inventory(root),
                 scope="all retained files after owned processes, SQLite probes and replay have stopped")
    write_json(root / "seal.json", value)
    return value


def verify_output(root):
    root = Path(root).resolve(strict=True)
    value = json.loads(regular(root / "seal.json").read_text())
    require(value.get("schema_version") == 1
            and value.get("artifact_inventory") == artifact_inventory(root),
            "runtime artifact inventory changed after sealing")
    return value


def checked_artifact(messages, root, target, name):
    package, source, _, _ = TARGETS[name]
    rows = [row for row in messages if row.get("reason") == "compiler-artifact"
            and row.get("target", {}).get("name") == name
            and row.get("target", {}).get("kind") == ["bin"]]
    require(len(rows) == 1, "exactly one original Cargo artifact required: " + name)
    value = rows[0]
    profile = value.get("profile", {})
    require(value.get("features") == [] and profile.get("opt_level") == "3"
            and profile.get("test") is False and profile.get("debug_assertions") is False,
            "runtime artifact must be the actual release default profile: " + name)
    require(value.get("fresh") is False, "runtime artifact was reused instead of built in the private target: " + name)
    require(value.get("manifest_path") == str(root / "crates" / package / "Cargo.toml")
            and value.get("target", {}).get("src_path") == str(root / "crates" / package / source),
            "runtime Cargo event belongs to another source checkout: " + name)
    require(isinstance(value.get("executable"), str) and value["executable"],
            "original Cargo executable is missing: " + name)
    executable = regular(Path(value["executable"]))
    require(executable.is_absolute() and executable.resolve().is_relative_to(target)
            and executable.name == name,
            "runtime executable is outside the recorded target or has the wrong name")
    return value


def command_for(target):
    return ["cargo", "build", "--release", "--locked", "--offline", "--no-default-features",
            "-p", "cc-server", "--bin", "codecortex", "-p", "cc-eval",
            "--bin", "p8-oracle", "--bin", "p8-runtime-statistics",
            "--message-format=json-render-diagnostics", "--target-dir", str(target)]


def private_build_environment(root, target):
    """Reserve a new target and override compiler/build-dir configuration.

    Cargo can reuse a different worktree's same-mtime workspace artifact from a
    shared target. A source snapshot cannot prove that such bytes were rebuilt.
    Reserving a previously absent target also rejects accidental retry reuse.
    """
    target = Path(target)
    require(target.is_absolute() and not target.is_symlink(), "Cargo target must be an absolute private path")
    target.mkdir(parents=True, exist_ok=False)
    environment, overrides = compiler_environment(root)
    environment.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_BUILD_DIR=str(target))
    return environment, overrides


def verify_toolchain(receipt):
    before, after = receipt.get("toolchain_before"), receipt.get("toolchain_after")
    require(isinstance(before, dict) and set(before) == {"cargo", "rustc"} and before == after,
            "runtime actual compiler identity is missing or changed")
    for name, value in before.items():
        invocation = Path(value.get("invocation", ""))
        require(invocation.is_absolute() and invocation.is_file()
                and os.access(invocation, os.X_OK)
                and value.get("command") == [str(invocation), "--version", "--verbose"]
                and value.get("executable") == str(invocation.resolve(strict=True))
                and value.get("executable_sha256") == digest(invocation)
                and isinstance(value.get("version"), str) and bool(value["version"]),
                "runtime original compiler invocation or bytes changed: " + name)
    require(receipt.get("compiler_environment") == {
        "RUSTC": before["rustc"]["invocation"], "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": ""},
        "runtime Cargo compiler selection is not bound to the recorded invocation")
    require(receipt.get("target_initially_absent") is True
            and receipt.get("build_dir") == receipt.get("target_dir"),
            "runtime build target was not new and private")
    return before


def verify_receipt(root, receipt_path, binaries):
    """Current same-checkout/same-job proof, including the actual copy source.

    This deliberately fails when the original Cargo target was removed or the
    artifact came from another job. Downloaded files may still have their seal
    verified; that does not grant them this current-build execution contract.
    """
    root, receipt_path = Path(root).resolve(strict=True), regular(receipt_path).resolve()
    receipt = json.loads(receipt_path.read_text())
    require(receipt.get("schema_version") == 2 and receipt.get("status") == "passed"
            and receipt.get("build_exit_code") == 0, "runtime build receipt is not successful")
    source, observer = source_snapshot(root), observer_snapshot(root)
    require(receipt.get("source_before") == receipt.get("source_after") == source,
            "runtime source differs from the exact build")
    require(receipt.get("observer_before") == receipt.get("observer_after") == observer
            and observer["source_commit"] == source["source_commit"],
            "runtime observer differs from the exact build")
    target = Path(receipt.get("target_dir", "")).resolve(strict=True)
    require(target.is_dir() and receipt.get("target_dir") == str(target)
            and receipt.get("build_command") == command_for(target),
            "runtime Cargo invocation or target is not the recorded locked build")
    toolchain = verify_toolchain(receipt)
    directory = receipt_path.parent
    verify_output(directory)
    for filename, field in (("product-build.jsonl", "cargo_log_sha256"),
                            ("product-build.stderr", "stderr_sha256")):
        require(digest(regular(directory / filename)) == receipt.get(field),
                "runtime original Cargo log changed")
    for when in ("before", "after"):
        require(json.loads(regular(directory / f"source-{when}.json").read_text()) == source,
                "runtime original source manifest differs")
    messages = [json.loads(line) for line in (directory / "product-build.jsonl").read_text().splitlines() if line.strip()]
    require(any(row.get("reason") == "build-finished" and row.get("success") is True for row in messages),
            "runtime Cargo build success event missing")
    require(set(binaries) == set(TARGETS) == set(receipt.get("artifacts", {})),
            "runtime requires the complete product/oracle/statistics artifact set")
    identities = {}
    for name, binary in binaries.items():
        binary = regular(binary).resolve()
        require(binary == directory / name, "runtime copied artifact is outside the sealed build directory")
        artifact = checked_artifact(messages, root, target, name)
        stored = receipt["artifacts"][name]
        original = regular(artifact["executable"])
        copied = dict(path=str(original), bytes=original.stat().st_size, sha256=digest(original))
        actual = dict(binary_path=str(binary), binary_sha256=digest(binary), binary_bytes=binary.stat().st_size,
                      copy_source=copied, cargo_artifact=artifact)
        require(stored == actual and actual["binary_sha256"] == copied["sha256"]
                and actual["binary_bytes"] == copied["bytes"],
                "runtime retained bytes are not the actual original Cargo copy: " + name)
        _, _, prefix, event_field = TARGETS[name]
        require(receipt.get(prefix + "_path") == str(binary)
                and receipt.get(prefix + "_sha256") == actual["binary_sha256"]
                and receipt.get(event_field) == artifact,
                "runtime legacy artifact fields disagree: " + name)
        identities[name] = actual
    for relative, expected in observer["files"].items():
        archived = regular(directory / "observer-source" / relative)
        require(archived.stat().st_size == expected["bytes"] and digest(archived) == expected["sha256"],
                "archived runtime observer differs")
    return dict(source=source, observer=observer, artifacts=identities, toolchain=toolchain,
                receipt_path=str(receipt_path), receipt_sha256=digest(receipt_path),
                build_seal_sha256=digest(directory / "seal.json"),
                verification_scope="original Cargo target, copied artifacts and committed observer available in this job")


def build(root, output):
    root, out = Path(root).resolve(strict=True), new_directory(output)
    target = Path(os.environ.get("CARGO_TARGET_DIR", out.with_name(out.name + "-cargo-target"))).absolute().resolve()
    command = command_for(target)
    receipt = dict(schema_version=2, status="building", build_command=command, build_exit_code=None,
                   target_dir=str(target), cold_build_claim=False, artifacts={})
    error = None
    try:
        require(not target.is_relative_to(out), "Cargo target must be outside the retained build evidence")
        environment, overrides = private_build_environment(root, target)
        tools_before = toolchain_identity(root, environment)
        receipt.update(target_initially_absent=True, build_dir=str(target), compiler_environment=overrides,
                       toolchain_before=tools_before)
        before, observer_before = source_snapshot(root), observer_snapshot(root)
        require(before["source_commit"] == observer_before["source_commit"], "source changed during observer binding")
        receipt.update(source_before=before, observer_before=observer_before)
        write_json(out / "source-before.json", before)
        for relative in OBSERVER_FILES:
            destination = out / "observer-source" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(root / relative, destination)
        with (out / "product-build.jsonl").open("xb") as stdout, (out / "product-build.stderr").open("xb") as stderr:
            result = subprocess.run(command, cwd=root, env=environment, stdout=stdout, stderr=stderr)
        receipt.update(build_exit_code=result.returncode, cargo_log_sha256=digest(out / "product-build.jsonl"),
                       stderr_sha256=digest(out / "product-build.stderr"))
        after, observer_after = source_snapshot(root), observer_snapshot(root)
        write_json(out / "source-after.json", after)
        receipt.update(source_after=after, observer_after=observer_after,
                       toolchain_after=toolchain_identity(root, environment),
                       compiler=tools_before["rustc"]["version"], cargo=tools_before["cargo"]["version"])
        require(before == after and observer_before == observer_after
                and receipt["toolchain_before"] == receipt["toolchain_after"] and result.returncode == 0,
                "runtime build failed or source/observer changed; original logs retained")
        messages = [json.loads(line) for line in (out / "product-build.jsonl").read_text().splitlines() if line.strip()]
        require(any(row.get("reason") == "build-finished" and row.get("success") is True for row in messages),
                "Cargo success event missing")
        for name, (_, _, prefix, event_field) in TARGETS.items():
            artifact = checked_artifact(messages, root, target, name)
            executable = regular(artifact["executable"])
            original = dict(path=str(executable), bytes=executable.stat().st_size, sha256=digest(executable))
            copied = out / name
            shutil.copy2(executable, copied)
            copied.chmod(0o555)
            require(digest(copied) == original["sha256"] and digest(executable) == original["sha256"],
                    "runtime original artifact changed while copying")
            receipt["artifacts"][name] = dict(binary_path=str(copied), binary_sha256=digest(copied),
                                              binary_bytes=copied.stat().st_size, copy_source=original,
                                              cargo_artifact=artifact)
            receipt.update({prefix + "_path": str(copied), prefix + "_sha256": digest(copied), event_field: artifact})
        require(source_snapshot(root) == before and observer_snapshot(root) == observer_before,
                "source/observer changed before build evidence was sealed")
        receipt["status"] = "passed"
    except Exception as caught:
        error = caught
        receipt.update(status="failed", error=f"{type(caught).__name__}: {caught}")
    finally:
        write_json(out / "build-receipt.json", receipt)
        seal_output(out)
    if error is not None:
        raise error
    verify_receipt(root, out / "build-receipt.json", {name: out / name for name in TARGETS})
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        receipt = build(args.source, args.output)
        print(json.dumps({key: value for key, value in receipt.items()
                          if key.endswith("sha256") or key == "build_exit_code"}, indent=2))
    except Exception as error:
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        raise SystemExit(2)

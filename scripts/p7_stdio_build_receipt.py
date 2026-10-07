#!/usr/bin/env python3
"""Build a fixed dev-profile stdio product with source/toolchain/feature identity."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from p7_build_identity import file_sha256, json_bytes, source_snapshot, verify_cargo_artifact


def compiler_environment(root):
    """Select one compiler invocation for both its version query and Cargo.

    RUSTC takes precedence over Cargo config. Empty wrapper variables override
    configured wrappers. Keep the legacy build argv unchanged for its consumer.
    Rustup proxy basenames are significant: do not replace rustc with rustup.
    """
    selected = os.environ.get("RUSTC") or shutil.which("rustc")
    if not selected:
        raise ValueError("missing build tool: rustc")
    candidate = Path(selected)
    if candidate.is_absolute():
        invocation = candidate
    elif candidate.parent != Path("."):
        invocation = Path(root) / candidate
    else:
        found = shutil.which(selected)
        if not found:
            raise ValueError("missing selected RUSTC executable")
        invocation = Path(found)
    invocation = Path(os.path.abspath(invocation))
    if not invocation.is_file() or not os.access(invocation, os.X_OK):
        raise ValueError("selected RUSTC is not executable")
    overrides = {"RUSTC": str(invocation), "RUSTC_WRAPPER": "",
                 "RUSTC_WORKSPACE_WRAPPER": ""}
    return dict(os.environ, **overrides), overrides


def toolchain_identity(root, environment):
    result = {}
    for name in ("cargo", "rustc"):
        executable = (environment["RUSTC"] if name == "rustc"
                      else shutil.which(name, path=environment.get("PATH")))
        if executable is None:
            raise ValueError("missing build tool: " + name)
        command = [executable, "--version", "--verbose"]
        value = subprocess.run(command, cwd=root, env=environment,
                               capture_output=True, text=True, check=True)
        result[name] = {"command": command, "invocation": str(Path(executable).absolute()),
                        "executable": str(Path(executable).resolve()),
                        "executable_sha256": file_sha256(executable),
                        "version": value.stdout.strip()}
    return result


def build(root, output, package_kind, offline):
    # Every invocation owns a new evidence directory. Failed/repeated builds
    # cannot overwrite an older valid receipt or its raw build log.
    output.mkdir(parents=True, exist_ok=False)
    command = ["cargo", "build", "-p", "cc-server", "--bin", "codecortex",
               "--no-default-features", "--locked",
               "--message-format=json-render-diagnostics"]
    if package_kind == "semantic":
        command.extend(["--features", "semantic"])
    if offline:
        command.append("--offline")
    result = None
    try:
        before = source_snapshot(root)
        environment, compiler_overrides = compiler_environment(root)
        # A same-mtime checkout in a shared Cargo target can incorrectly reuse
        # a different worktree's workspace artifact. This run starts with an
        # absent private target, so source snapshots cannot bless that reuse.
        target_dir = output / "cargo-target"
        if target_dir.exists() or target_dir.is_symlink():
            raise ValueError("build target must be new and private to this receipt")
        environment["CARGO_TARGET_DIR"] = str(target_dir)
        environment["CARGO_BUILD_BUILD_DIR"] = str(target_dir)
        tools_before = toolchain_identity(root, environment)
        runner_inputs = {str(path): file_sha256(path) for path in (
            Path(__file__).resolve(), Path(__file__).with_name("p7_build_identity.py").resolve())}
        (output / "source-inputs.json").write_bytes(json_bytes(before["inputs"]))
        with (output / "cargo-build.jsonl").open("wb") as log, \
                (output / "cargo-build.stderr.log").open("wb") as errors:
            result = subprocess.run(command, cwd=root, env=environment, stdout=log, stderr=errors)
        if result.returncode:
            (output / "build-failure.json").write_bytes(json_bytes({
                "build_command": command, "build_exit_code": result.returncode,
                "source_before": {k: v for k, v in before.items() if k != "inputs"},
                "toolchain": tools_before,
                "compiler_environment": compiler_overrides,
                "cargo_target_dir": str(target_dir),
                "cargo_build_dir": str(target_dir),
            }))
            return result.returncode
        after = source_snapshot(root)
        if before != after or tools_before != toolchain_identity(root, environment):
            raise ValueError("source or compiler identity changed during the build")
        if any(file_sha256(path) != digest for path, digest in runner_inputs.items()):
            raise ValueError("receipt builder changed during the build")
        artifacts = []
        with (output / "cargo-build.jsonl").open() as log:
            for line in log:
                message = json.loads(line)
                if (message.get("reason") == "compiler-artifact"
                        and message.get("target", {}).get("name") == "codecortex"
                        and message.get("target", {}).get("kind") == ["bin"]):
                    artifacts.append(message)
        if len(artifacts) != 1:
            raise ValueError("expected exactly one Cargo codecortex binary artifact")
        artifact = artifacts[0]
        executable = verify_cargo_artifact(root, artifact, package_kind)
        if not executable.is_relative_to(target_dir):
            raise ValueError("Cargo product is outside this receipt's private target")
        snapshot = output / executable.name
        temporary = snapshot.with_suffix(".tmp")
        shutil.copy2(executable, temporary)
        digest = file_sha256(temporary)
        if digest != file_sha256(executable):
            raise ValueError("product changed while copying its build snapshot")
        temporary.replace(snapshot)
        receipt = {
            "schema_version": 1, "package_kind": package_kind,
            "build_command": command, "build_exit_code": 0,
            "binary_path": str(snapshot), "binary_sha256": digest,
            "cargo_artifact": artifact,
            "source_identity_version": 1,
            "source_before": {k: v for k, v in before.items() if k != "inputs"},
            "source_after": {k: v for k, v in after.items() if k != "inputs"},
            "source_manifest": "source-inputs.json", "toolchain": tools_before,
            "compiler_environment": compiler_overrides,
            "cargo_target_dir": str(target_dir), "cargo_target_initially_absent": True,
            "cargo_build_dir": str(target_dir),
            "build_profile": "dev", "actual_cargo_profile": artifact["profile"],
            "builder_sha256": runner_inputs[str(Path(__file__).resolve())],
            "identity_helper_sha256": runner_inputs[str(Path(__file__).with_name("p7_build_identity.py").resolve())],
            "scope": "Committed crate/Cargo content, compiler invocation explicitly selected for Cargo with wrappers disabled, feature artifact and copied binary. Compiler proxies are identified by their invocation, resolved bytes and reported version; no behavior or hermetic-build certification.",
        }
        receipt_path = output / "build-receipt.json"
        receipt_path.write_bytes(json_bytes(receipt))
        print(json.dumps({"binary": str(snapshot), "receipt": str(receipt_path),
                          "package_kind": package_kind, "features": artifact["features"],
                          "sha256": digest, "source_commit": before["source_commit"],
                          "source_inputs": before["input_count"], "profile": "dev"}))
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        (output / "build-failure.json").write_bytes(json_bytes({
            "build_command": command,
            "build_exit_code": result.returncode if result is not None else None,
            "identity_validation_failed": True, "error": str(error),
        }))
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-kind", choices=("default", "semantic"), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.output_dir.resolve()
    return build(root, output, args.package_kind, args.offline)


if __name__ == "__main__":
    sys.exit(main())

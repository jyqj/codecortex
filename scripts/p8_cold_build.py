#!/usr/bin/env python3
"""P8-012: explicit eight-cell cold builds with byte-bound, replayable receipts.

Inventory is the default. --execute starts only locally available selected cells;
missing platforms/toolchains and resource refusals remain not_run (exit 2).
Dependency downloads are disabled, and a new output directory is mandatory.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import time


PLATFORMS = ("linux", "macos")
TOOLCHAINS = ("1.95", "stable")
PACKAGES = ("default", "semantic")
BUILD_INPUTS = ("Cargo.toml", "Cargo.lock", "crates", ".cargo", "rust-toolchain", "rust-toolchain.toml")


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2) + "\n").encode()


def write_json(path, value):
    Path(path).write_bytes(json_bytes(value))


def new_directory(path):
    # Resolve only the parent: resolving an existing final symlink would lose
    # the evidence that this output name is already occupied.
    path = Path(path).absolute()
    path = path.parent.resolve(strict=True) / path.name
    path.mkdir()  # O_EXCL-style ownership: never erase or reuse prior receipts.
    return path


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def source_identity(root, expected_commit=None):
    """Verify committed Cargo/crate inputs including skipped/assume-unchanged files.

    This is a source-byte binding, not a claim of a hermetic compiler or a
    sandboxed Cargo build script. Dependency-cache and external config identities
    are recorded separately by the build receipt.
    """
    root = Path(root).resolve(strict=True)
    commit = git(root, "rev-parse", "HEAD")
    if expected_commit and commit != expected_commit:
        raise ValueError("source commit differs from requested commit")
    for options in (("--others", "--exclude-standard"),
                    ("--others", "--ignored", "--exclude-standard")):
        unknown = subprocess.check_output(["git", "ls-files", "-z", *options,
                                           "--", *BUILD_INPUTS], cwd=root)
        if unknown:
            raise ValueError("unbound untracked/ignored files in build inputs")
    entries = subprocess.check_output(["git", "ls-tree", "-r", "-z", "HEAD",
                                       "--", *BUILD_INPUTS], cwd=root)
    algorithm = git(root, "rev-parse", "--show-object-format")
    manifest = []
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        meta, raw_path = entry.split(b"\t", 1)
        mode, kind, expected_blob = meta.decode().split()
        name = os.fsdecode(raw_path)
        path = root / name
        if kind != "blob" or mode not in ("100644", "100755"):
            raise ValueError(f"non-regular/unbound build input: {name}")
        if not stat.S_ISREG(path.lstat().st_mode) or path.resolve() != path:
            raise ValueError(f"symlink/non-regular build input: {name}")
        if bool(path.stat().st_mode & stat.S_IXUSR) != (mode == "100755"):
            raise ValueError(f"committed build file mode differs: {name}")
        data = path.read_bytes()
        actual_blob = hashlib.new(algorithm, b"blob " + str(len(data)).encode()
                                  + b"\0" + data).hexdigest()
        if actual_blob != expected_blob:
            raise ValueError(f"committed build bytes differ: {name}")
        manifest.append(dict(path=name, size=len(data), sha256=hashlib.sha256(data).hexdigest(),
                             git_blob=expected_blob, git_mode=mode))
    names = {entry["path"] for entry in manifest}
    if not {"Cargo.toml", "Cargo.lock", "crates/cc-server/Cargo.toml"} <= names:
        raise ValueError("Cargo lock/workspace/product inputs are required")
    return dict(source_root=str(root), source_commit=commit,
                source_tree=git(root, "rev-parse", "HEAD^{tree}"),
                input_count=len(manifest),
                manifest_sha256=hashlib.sha256(json_bytes(manifest)).hexdigest()), manifest


def host_platform():
    return {"Linux": "linux", "Darwin": "macos"}.get(platform.system(), "unsupported")


def probe(command, env=None):
    try:
        result = subprocess.run(command, env=env, text=True, capture_output=True,
                                timeout=30, check=False)
        return dict(argv=list(map(str, command)), exit_code=result.returncode,
                    stdout=result.stdout, stderr=result.stderr)
    except (OSError, subprocess.TimeoutExpired) as error:
        return dict(argv=list(map(str, command)), exit_code=None,
                    error=f"{type(error).__name__}: {error}")


def discover_toolchain(rustup, channel, label):
    # Some rustup versions resolve an uninstalled tracking channel by syncing
    # it. Inspect the local inventory first; never install/update a toolchain.
    inventory = probe([str(rustup), "toolchain", "list"])
    probes, tools = [inventory], {}
    if inventory["exit_code"] != 0:
        return None, probes, "toolchain_inventory_unavailable"
    installed = [line.split()[0] for line in inventory["stdout"].splitlines() if line.strip()]
    if not any(name == channel or name.startswith(channel + "-") for name in installed):
        return None, probes, "toolchain_not_installed"
    for name in ("cargo", "rustc"):
        result = probe([str(rustup), "which", "--toolchain", channel, name])
        probes.append(result)
        if result["exit_code"] != 0:
            return None, probes, "toolchain_unavailable"
        path = Path(result["stdout"].strip()).resolve(strict=True)
        version = probe([str(path), "--version", "--verbose"])
        probes.append(version)
        if version["exit_code"] != 0:
            return None, probes, "toolchain_version_unavailable"
        tools[name] = dict(path=str(path), sha256=digest(path),
                           version_verbose=version["stdout"].strip())
    version = re.search(r"^release: (\S+)$", tools["rustc"]["version_verbose"], re.M)
    host = re.search(r"^host: (\S+)$", tools["rustc"]["version_verbose"], re.M)
    if not version or not host:
        return None, probes, "compiler_identity_incomplete"
    if label == "1.95" and version.group(1) != "1.95.0":
        return None, probes, "msrv_version_mismatch"
    if label == "stable" and not re.fullmatch(r"\d+\.\d+\.\d+", version.group(1)):
        return None, probes, "stable_compiler_is_prerelease"
    expected_os = "apple-darwin" if host_platform() == "macos" else "linux"
    if expected_os not in host.group(1):
        return None, probes, "compiler_host_mismatch"
    tools.update(channel=channel, matrix_toolchain=label, host=host.group(1),
                 rustc_release=version.group(1))
    return tools, probes, None


def cargo_config_identity(root, env):
    candidates = []
    for parent in (root, *root.parents):
        candidates.extend(parent / ".cargo" / name for name in ("config", "config.toml"))
    cargo_home = Path(env.get("CARGO_HOME", Path.home() / ".cargo"))
    candidates.extend(cargo_home / name for name in ("config", "config.toml"))
    return [dict(path=str(path.resolve()), sha256=digest(path))
            for path in dict.fromkeys(candidates) if path.is_file()]


def cargo_artifact(log, root, target, package):
    messages = [json.loads(line) for line in Path(log).read_text().splitlines() if line.strip()]
    if not any(row.get("reason") == "build-finished" and row.get("success") is True
               for row in messages):
        raise ValueError("Cargo did not report a successful build-finished event")
    artifacts = [row for row in messages if row.get("reason") == "compiler-artifact"
                 and row.get("target", {}).get("name") == "codecortex"
                 and row.get("target", {}).get("kind") == ["bin"]
                 and row.get("executable")]
    if len(artifacts) != 1:
        raise ValueError("exactly one codecortex Cargo binary artifact is required")
    artifact = artifacts[0]
    expected_features = [] if package == "default" else ["semantic"]
    if sorted(artifact.get("features", [])) != expected_features:
        raise ValueError("Cargo artifact features differ from the requested package")
    if artifact.get("fresh") is not False:
        raise ValueError("fresh/warm Cargo artifacts cannot prove a cold build")
    if Path(artifact.get("manifest_path", "")).resolve() != root / "crates/cc-server/Cargo.toml":
        raise ValueError("Cargo artifact is not bound to the selected product manifest")
    executable = Path(artifact["executable"])
    if (target.is_symlink() or target.resolve() != target
            or executable.resolve(strict=True) != executable
            or not executable.resolve(strict=True).is_relative_to(target)):
        raise ValueError("Cargo executable escaped the new target directory")
    if not executable.is_file() or executable.stat().st_size == 0:
        raise ValueError("Cargo executable is empty or absent")
    return artifact


def validate_cell(receipt_path):
    """Strict local replay: changed/missing source, compiler, logs or binary fail."""
    receipt_path = Path(receipt_path).resolve(strict=True)
    record = json.loads(receipt_path.read_text())
    if record.get("schema_version") != 1 or record.get("status") != "passed":
        raise ValueError("a passed v1 cold-build receipt is required")
    cell = record["cell"]
    if cell["platform"] not in PLATFORMS or cell["toolchain"] not in TOOLCHAINS or cell["package"] not in PACKAGES:
        raise ValueError("unknown cold-build matrix cell")
    if record["build_exit_code"] != 0 or record.get("stop_reason") is not None:
        raise ValueError("a failed/stopped build cannot pass")
    if record.get("target_initially_absent") is not True:
        raise ValueError("a new target directory is required")
    command = record["command"]
    for argument in ("--locked", "--offline", "--no-default-features", "--target-dir", "--target"):
        if argument not in command:
            raise ValueError(f"missing required build argument: {argument}")
    root = Path(record["source_before"]["source_root"])
    target = receipt_path.parent / "target"
    if command[command.index("--target-dir") + 1] != str(target):
        raise ValueError("command target directory differs from receipt ownership")
    expected_features = ["semantic"] if cell["package"] == "semantic" else []
    actual_features = [command[command.index("--features") + 1]] if "--features" in command else []
    if expected_features != actual_features:
        raise ValueError("command features differ from the matrix cell")
    identity, manifest = source_identity(root, record["source_before"]["source_commit"])
    if identity != record["source_before"] or identity != record["source_after"]:
        raise ValueError("source changed before, during or after the build")
    if digest(receipt_path.parent / "source-inputs.json") != identity["manifest_sha256"]:
        raise ValueError("source manifest bytes changed")
    if manifest != json.loads((receipt_path.parent / "source-inputs.json").read_text()):
        raise ValueError("source manifest does not enumerate the verified source")
    tools = record["toolchain"]
    if tools != record["toolchain_after"]:
        raise ValueError("compiler identity changed during the build")
    if tools["matrix_toolchain"] != cell["toolchain"]:
        raise ValueError("toolchain identity differs from matrix label")
    if cell["toolchain"] == "1.95" and tools["rustc_release"] != "1.95.0":
        raise ValueError("non-MSRV compiler cannot pass the MSRV cell")
    if cell["toolchain"] == "stable" and not re.fullmatch(r"\d+\.\d+\.\d+", tools["rustc_release"]):
        raise ValueError("prerelease compiler cannot pass the stable cell")
    if ("apple-darwin" if cell["platform"] == "macos" else "linux") not in tools["host"]:
        raise ValueError("compiler host differs from the matrix platform")
    if command[0] != tools["cargo"]["path"] or command[command.index("--target") + 1] != tools["host"]:
        raise ValueError("selected Cargo/target do not match the toolchain")
    if record["compiler_environment"] != dict(RUSTC=tools["rustc"]["path"], RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER=""):
        raise ValueError("Cargo compiler invocation is not explicitly bound")
    for name in ("cargo", "rustc"):
        if digest(tools[name]["path"]) != tools[name]["sha256"]:
            raise ValueError(f"{name} bytes changed")
    for name, expected in record["logs_sha256"].items():
        if name not in ("cargo-build.jsonl", "cargo-stderr.log") or digest(receipt_path.parent / name) != expected:
            raise ValueError("build log digest mismatch")
    if set(record["logs_sha256"]) != {"cargo-build.jsonl", "cargo-stderr.log"}:
        raise ValueError("both Cargo logs must be bound")
    if record["cargo_configs"] != record["cargo_configs_after"]:
        raise ValueError("Cargo configuration changed during the build")
    for config in record["cargo_configs"]:
        if digest(config["path"]) != config["sha256"]:
            raise ValueError("Cargo configuration bytes changed")
    artifact = cargo_artifact(receipt_path.parent / "cargo-build.jsonl", root, target, cell["package"])
    profile = record["profile"]
    if profile not in ("dev", "release") or ("--release" in command) != (profile == "release"):
        raise ValueError("command profile differs from the receipt")
    actual_profile = artifact.get("profile", {})
    if (actual_profile.get("test") is not False
            or actual_profile.get("opt_level") != ("3" if profile == "release" else "0")
            or actual_profile.get("debug_assertions") is not (profile == "dev")):
        raise ValueError("Cargo artifact profile does not match the declared build")
    expected_binary = target / tools["host"] / ("release" if profile == "release" else "debug") / "codecortex"
    if artifact["executable"] != str(expected_binary):
        raise ValueError("Cargo executable path does not match the selected target/profile")
    if artifact != record["cargo_artifact"] or digest(artifact["executable"]) != record["binary_sha256"]:
        raise ValueError("Cargo artifact or binary digest changed")
    return record


def build_cell(root, output, cell, tools, profile, jobs, timeout_seconds):
    target = output / "target"
    record = dict(schema_version=1, cell=cell, status="failed", stop_reason=None,
                  target_initially_absent=not target.exists(), toolchain=tools,
                  runner_sha256=digest(__file__), profile=profile)
    if target.exists() or target.is_symlink():
        raise ValueError("cold-build target must not already exist")
    target.mkdir()
    try:
        record["source_before"], manifest = source_identity(root)
        write_json(output / "source-inputs.json", manifest)
        env = dict(os.environ)
        for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "CARGO_BUILD_RUSTC",
                     "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"):
            env.pop(name, None)
        compiler_env = dict(RUSTC=tools["rustc"]["path"], RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="")
        env.update(compiler_env, CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS=str(jobs))
        record["compiler_environment"] = compiler_env
        record["cargo_configs"] = cargo_config_identity(root, env)
        record["dependency_cache"] = dict(path=env.get("CARGO_HOME"), reused=True,
                                            network="Cargo --offline", hermetic=False)
        command = [tools["cargo"]["path"], "build", "-p", "cc-server", "--bin", "codecortex",
                   "--no-default-features", "--locked", "--offline", "--target-dir", str(target),
                   "--target", tools["host"], "--message-format=json-render-diagnostics"]
        if cell["package"] == "semantic":
            command.extend(["--features", "semantic"])
        if profile == "release":
            command.append("--release")
        record["command"] = command
        started = time.monotonic()
        with (output / "cargo-build.jsonl").open("x") as stdout, (output / "cargo-stderr.log").open("x") as stderr:
            process = subprocess.Popen(command, cwd=root, env=env, stdout=stdout, stderr=stderr,
                                       start_new_session=True)
            record["pid"] = process.pid
            try:
                record["build_exit_code"] = process.wait(timeout=timeout_seconds)
            except BaseException as error:
                record["stop_reason"] = type(error).__name__
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=10)
                record["build_exit_code"] = process.returncode
                raise
        record["wall_seconds"] = time.monotonic() - started
        record["logs_sha256"] = {name: digest(output / name)
                                  for name in ("cargo-build.jsonl", "cargo-stderr.log")}
        if record["build_exit_code"] != 0:
            raise RuntimeError("Cargo build failed; stdout and stderr retained")
        record["source_after"], _ = source_identity(root)
        record["cargo_configs_after"] = cargo_config_identity(root, env)
        record["toolchain_after"] = json.loads(json.dumps(tools))
        for name in ("cargo", "rustc"):
            record["toolchain_after"][name]["sha256"] = digest(tools[name]["path"])
        artifact = cargo_artifact(output / "cargo-build.jsonl", root, target, cell["package"])
        record["cargo_artifact"] = artifact
        record["binary_sha256"] = digest(artifact["executable"])
        record["status"] = "passed"
        write_json(output / "receipt.json", record)
        validate_cell(output / "receipt.json")
    except BaseException as error:
        record["status"] = "failed"
        record["error"] = f"{type(error).__name__}: {error}"
        for name in ("cargo-build.jsonl", "cargo-stderr.log"):
            if (output / name).is_file():
                record.setdefault("logs_sha256", {})[name] = digest(output / name)
        if isinstance(error, (KeyboardInterrupt, SystemExit)):
            record["stop_reason"] = "cancelled"
    write_json(output / "receipt.json", record)
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--verify", type=Path, metavar="CELL_RECEIPT")
    parser.add_argument("--rustup", default=shutil.which("rustup") or "rustup")
    parser.add_argument("--msrv-channel", default="1.95.0")
    parser.add_argument("--stable-channel", default="stable")
    parser.add_argument("--only-toolchain", choices=TOOLCHAINS)
    parser.add_argument("--only-package", choices=PACKAGES)
    parser.add_argument("--profile", choices=("dev", "release"), default="release")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--timeout-seconds", type=int, default=1800)
    parser.add_argument("--minimum-free-bytes", type=int, default=4 * 1024**3)
    args = parser.parse_args(argv)
    try:
        if args.verify:
            record = validate_cell(args.verify)
            print(json.dumps(dict(status="verified", cell=record["cell"])))
            return 0
        if not args.output_dir:
            parser.error("--output-dir is required unless --verify is used")
        if args.jobs < 1 or args.timeout_seconds < 1 or args.minimum_free_bytes < 0:
            parser.error("jobs/timeout must be positive and free-byte reserve nonnegative")
        output = new_directory(args.output_dir)
        root = args.source_root.resolve(strict=True)
        summary = dict(schema_version=1, task="P8-012", status="incomplete", host=host_platform(),
                       runner_sha256=digest(__file__), execute_requested=args.execute,
                       scope="cold-build receipts only; no test-suite or release certification",
                       cells=[dict(cell=dict(platform=os_name, toolchain=toolchain, package=package),
                                   status="not_run", reason="not_reached")
                              for os_name in PLATFORMS for toolchain in TOOLCHAINS for package in PACKAGES])
        try:
            summary["source"], manifest = source_identity(root)
            write_json(output / "source-inputs.json", manifest)
            discovered = {}
            for label, channel in (("1.95", args.msrv_channel), ("stable", args.stable_channel)):
                tools, probes, reason = discover_toolchain(args.rustup, channel, label)
                discovered[label] = (tools, reason)
                write_json(output / f"toolchain-{label}.json", dict(probes=probes, toolchain=tools, reason=reason))
            for index, row in enumerate(summary["cells"]):
                cell = row["cell"]
                os_label, toolchain, package = cell["platform"], cell["toolchain"], cell["package"]
                tools, unavailable = discovered[toolchain]
                reason = ("platform_unavailable" if os_label != summary["host"] else
                          "cell_not_selected" if (args.only_toolchain and toolchain != args.only_toolchain)
                          or (args.only_package and package != args.only_package) else
                          unavailable or ("execution_not_requested" if not args.execute else
                          "disk_reserve_unavailable" if shutil.disk_usage(output).free < args.minimum_free_bytes else None))
                if reason:
                    summary["cells"][index] = dict(cell=cell, status="not_run", reason=reason,
                                                   toolchain_available=tools is not None)
                    continue
                cell_output = new_directory(output / f"{os_label}-{toolchain}-{package}")
                record = build_cell(root, cell_output, cell, tools, args.profile, args.jobs, args.timeout_seconds)
                summary["cells"][index] = dict(cell=cell, status=record["status"],
                                               receipt=str(cell_output / "receipt.json"),
                                               receipt_sha256=digest(cell_output / "receipt.json"))
                write_json(output / "matrix.json", summary)
                if record.get("stop_reason") == "cancelled":
                    raise KeyboardInterrupt("cancelled build; remaining cells were not started")
            summary["counts"] = {state: sum(c["status"] == state for c in summary["cells"])
                                 for state in ("passed", "failed", "not_run")}
            summary["status"] = ("passed" if summary["counts"]["passed"] == 8 else
                                  "failed" if summary["counts"]["failed"] else "incomplete")
        except BaseException as error:
            summary["status"] = "cancelled" if isinstance(error, (KeyboardInterrupt, SystemExit)) else "invalid"
            summary["error"] = f"{type(error).__name__}: {error}"
        summary["counts"] = {state: sum(c["status"] == state for c in summary["cells"])
                             for state in ("passed", "failed", "not_run")}
        write_json(output / "matrix.json", summary)
        print(json.dumps(dict(status=summary["status"], counts=summary.get("counts"),
                              matrix=str(output / "matrix.json"))))
        return (0 if summary["status"] == "passed" else 1 if summary["status"] == "failed" else
                3 if summary["status"] == "cancelled" else 2)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"P8 cold build refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())

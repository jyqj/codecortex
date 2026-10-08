#!/usr/bin/env python3
"""Build the four source-isolated P7-019 engineering cells at one exact Git SHA.

Only complete tracked Cargo build inputs are snapshotted. Each cell is compiled
in its own source directory and fresh target; only that newly created target may
be discarded. Source, binaries, exact input inventories and logs are retained.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
from datetime import datetime, timezone

FACTORS = {
    "none": [],
    "local": ["local_retrieval"],
    "dense_only": ["semantic_dense"],
    "hybrid": ["local_retrieval", "semantic_dense"],
}
CONTROL_PATH = "crates/cc-eval/src/benchmark/ablation/mechanism_controls.json"


def now():
    return datetime.now(timezone.utc).isoformat()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    with Path(path).open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def command(args, **kwargs):
    return subprocess.check_output(args, **kwargs).decode().strip()


def inventory(root):
    result = {}
    total = 0
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"source symlink rejected: {path}")
        if path.is_file():
            data = path.read_bytes()
            total += len(data)
            if len(data) > 4 * 1024 * 1024 or total > 128 * 1024 * 1024 or len(result) >= 5000:
                raise ValueError("snapshot exceeds runner's exact inventory bounds")
            result[path.relative_to(root).as_posix()] = digest(data)
        elif not path.is_dir():
            raise ValueError(f"non-regular source input: {path}")
    return result


def snapshot(source, head, destination):
    tracked = command(["git", "-C", str(source), "ls-tree", "--name-only", head]).splitlines()
    selected = [name for name in ["Cargo.toml", "Cargo.lock", "crates", ".cargo", "rust-toolchain", "rust-toolchain.toml"] if name in tracked]
    if not {"Cargo.toml", "Cargo.lock", "crates"}.issubset(selected):
        raise ValueError("missing tracked workspace build inputs")
    dirty = command(["git", "-C", str(source), "status", "--porcelain", "--untracked-files=all", "--", *selected])
    if dirty:
        raise ValueError("build inputs must be clean at expected-head; commit changes before building")
    archive = subprocess.check_output(["git", "-C", str(source), "archive", "--format=tar", head, "--", *selected])
    destination.mkdir()
    with tarfile.open(fileobj=io.BytesIO(archive)) as files:
        for member in files:
            relative = Path(member.name)
            if relative.is_absolute() or ".." in relative.parts or member.issym() or member.islnk():
                raise ValueError("unsafe or aliased snapshot entry")
            path = destination / relative
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                path.parent.mkdir(parents=True, exist_ok=True)
                data = files.extractfile(member).read()
                path.write_bytes(data)
                path.chmod(member.mode & 0o777)
            else:
                raise ValueError("non-regular tracked build input")
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--profile", choices=["dev", "release"], default="dev")
    parser.add_argument("--discard-targets", action="store_true", help="discard only target directories freshly created by this invocation, after copying binary and recording receipts")
    parser.add_argument("--prepare-only", action="store_true", help="validate and materialize source controls without compiling; never emits successful build manifest")
    args = parser.parse_args()
    if not 1 <= args.jobs <= 16:
        parser.error("jobs must be in 1..16")
    source = args.source.resolve(strict=True)
    head = command(["git", "-C", str(source), "rev-parse", "HEAD"])
    if head != args.expected_head or len(head) != 40:
        raise ValueError("checkout does not match the exact expected source SHA")
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    output = output.resolve(strict=True)
    reference = output / "reference"
    selected = snapshot(source, head, reference)
    original = inventory(reference)
    controls = json.loads((reference / CONTROL_PATH).read_text())
    if [control["id"] for control in controls] != ["local_retrieval", "semantic_dense"]:
        raise ValueError("unexpected compiled mechanism controls")
    for control in controls:
        path = Path(control["path"])
        if path.is_absolute() or ".." in path.parts:
            raise ValueError("invalid source control path")
        if (reference / path).read_text().count(control["on_text"]) != 1:
            raise ValueError("control must match reference exactly once")
    write_json(output / "reference-inventory.json", original)
    write_json(output / "source-binding.json", {
        "source_commit": head, "started_utc": now(), "selected_tracked_roots": selected,
        "snapshot_scope": "complete tracked Cargo workspace/lock/toolchain/config and all crate files; unrelated docs/history artifacts excluded",
        "reference_inventory_sha256": digest(json.dumps(original, sort_keys=True, separators=(",", ":")).encode()),
        "build_input_worktree_clean": True,
        "attestation": "local recorded provenance; not remote source-to-binary attestation",
    })
    variants = []
    sources = output / "sources"
    products = output / "products"
    logs = output / "builds"
    targets = output / "targets"
    for parent in [sources, products, logs, targets]:
        parent.mkdir()
    for name, enabled in FACTORS.items():
        root = sources / name
        shutil.copytree(reference, root)
        for control in controls:
            if control["id"] not in enabled:
                path = root / control["path"]
                path.write_text(path.read_text().replace(control["on_text"], control["off_text"], 1))
        target = targets / name
        target.mkdir()
        cell = logs / name
        cell.mkdir()
        write_json(cell / "source-inventory.json", inventory(root))
        variants.append({"id": name, "enabled": enabled, "source_root": str(root),
                         "binary": str(products / name), "build_receipt": str(cell / "receipt.json")})
    build = {"schema_version": 1, "source_commit": head, "reference_source": str(reference),
             "controls": controls, "variants": variants}
    if args.prepare_only:
        write_json(output / "prepared-snapshots.json", {"compiled": False, "build": build})
        print(output / "prepared-snapshots.json", flush=True)
        return
    cargo = shutil.which(args.cargo)
    rustc = shutil.which(os.environ.get("RUSTC", "rustc"))
    if not cargo or not rustc:
        raise ValueError("cargo and rustc must be available explicitly through PATH or arguments")
    # Resolve a rustup dispatcher to the actual selected compiler. Record and
    # execute the same compiler bytes, not only the dispatcher's path/version.
    sysroot = Path(command([rustc, "--print", "sysroot"], cwd=source)).resolve(strict=True)
    suffix = ".exe" if os.name == "nt" else ""
    rustc = str((sysroot / "bin" / f"rustc{suffix}").resolve(strict=True))
    if Path(cargo).resolve().stem == "rustup":
        cargo = str((sysroot / "bin" / f"cargo{suffix}").resolve(strict=True))
    cargo_version = command([cargo, "--version"])
    rustc_version = command([rustc, "--version", "--verbose"])
    build_command = [cargo, "build", "--locked", "-p", "cc-server", "--features", "semantic-http", "--bin", "codecortex", "--profile", args.profile, "--jobs", str(args.jobs)]
    environment = dict(os.environ)
    for key in list(environment):
        if key.startswith("CARGO_PROFILE_") or key in ["CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]:
            del environment[key]
    controlled = {"CARGO_INCREMENTAL": "0", "CARGO_PROFILE_DEV_DEBUG": "0",
                  "CARGO_PROFILE_TEST_DEBUG": "0", "RUSTC": rustc,
                  "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": ""}
    environment.update(controlled)
    options = {"command": build_command, "cargo": cargo_version, "rustc": rustc_version,
               "SDKROOT": environment.get("SDKROOT"), "RUSTFLAGS": environment.get("RUSTFLAGS"),
               "profile": args.profile, "features": ["semantic-http"], "jobs": args.jobs,
               "binding": {"source_commit": head, "controlled_environment": controlled,
                           "cargo_executable_sha256": file_digest(cargo), "rustc_executable_sha256": file_digest(rustc),
                           "intermediate_output_rule": "CARGO_BUILD_BUILD_DIR and CARGO_BUILD_TARGET_DIR both equal this cell's CARGO_TARGET_DIR",
                           "source_input": "every regular source byte inventoried before and after cargo; per-cell cwd recorded outside semantic projection"}}
    for variant in variants:
        name = variant["id"]
        root = Path(variant["source_root"])
        target = targets / name
        cell = logs / name
        before = inventory(root)
        current = dict(environment, CARGO_TARGET_DIR=str(target),
                       CARGO_BUILD_BUILD_DIR=str(target), CARGO_BUILD_TARGET_DIR=str(target))
        started = now()
        print(f"building {name}: {root}", flush=True)
        with (cell / "cargo.stdout.log").open("wb") as stdout, (cell / "cargo.stderr.log").open("wb") as stderr:
            result = subprocess.run(build_command, cwd=root, env=current, stdout=stdout, stderr=stderr)
        after = inventory(root)
        write_json(cell / "command.json", {"argv": build_command, "cwd": str(root),
            "target": str(target), "started_utc": started, "finished_utc": now(),
            "exit_code": result.returncode, "source_unchanged": before == after,
            "stdout_sha256": digest((cell / "cargo.stdout.log").read_bytes()),
            "stderr_sha256": digest((cell / "cargo.stderr.log").read_bytes()),
            "compiler_environment": {**controlled, "RUSTFLAGS": environment.get("RUSTFLAGS"), "SDKROOT": environment.get("SDKROOT"),
                "CARGO_TARGET_DIR": str(target), "CARGO_BUILD_BUILD_DIR": str(target), "CARGO_BUILD_TARGET_DIR": str(target)}})
        if result.returncode or before != after:
            raise RuntimeError(f"{name} build failed or source changed; exact logs retained in {cell}")
        executable = "codecortex.exe" if os.name == "nt" else "codecortex"
        product = target / ("debug" if args.profile == "dev" else "release") / executable
        binary = Path(variant["binary"])
        shutil.copy2(product, binary)
        receipt = {"source_files": after, "binary_sha256": digest(binary.read_bytes()),
                   "build_options": {**options, "CARGO_TARGET_DIR": str(target)}, "exit_code": 0}
        write_json(Path(variant["build_receipt"]), receipt)
        if args.discard_targets:
            # This path was newly created above, is a direct child of our newly
            # created output, and is never shared or supplied as an input path.
            if target.parent != targets or target.is_symlink() or not target.is_dir():
                raise ValueError("refusing to discard an unowned target")
            shutil.rmtree(target)
            target.mkdir()  # keep a canonical, distinct target identity for replay
            write_json(target / "position-only.json", {"cell": name, "target": str(target),
                "build_intermediates_retained": False})
    if inventory(reference) != original:
        raise ValueError("reference source changed during the build")
    write_json(output / "mechanism-build.json", build)
    print(output / "mechanism-build.json", flush=True)


if __name__ == "__main__":
    main()

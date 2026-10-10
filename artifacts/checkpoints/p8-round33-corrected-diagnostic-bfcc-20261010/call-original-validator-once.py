#!/usr/bin/env python3
"""Prepared explicit original function caller. No execution during preparation.

Each future invocation calls exactly the selected unchanged function once.
Build validation invokes the preserved ELF's --hash-file as originally defined.
Raw and shard predicates are not replaced by the custody reader. Their original
exceptions and stdout/stderr must be retained by the calling controller.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("build", "raw", "shard"))
    parser.add_argument("--binding", required=True)
    parser.add_argument("--source-root", required=True)
    parser.add_argument("--build-directory")
    parser.add_argument("--shard-directory")
    parser.add_argument("--raw-path")
    parser.add_argument("--plan-path")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    binding = json.loads(Path(args.binding).read_text())
    root = Path(args.source_root).resolve(strict=True)
    assert binding["G"] and binding["run_id"] and binding["run_attempt"], "real identity required"
    for relative, expected in binding["driver_inputs"].items():
        assert digest(root / relative) == expected, "original driver/helper differs"
    # No sys.path search can substitute an unrelated helper: load both fixed paths explicitly.
    def load(name, path):
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        spec.loader.exec_module(module)
        return module
    load("p7_build_identity", root / "scripts/p7_build_identity.py")
    driver = load("original_p8_scale_matrix", root / "scripts/p8_scale_matrix.py")
    if args.mode == "build":
        result, binary = driver.validate_build(args.build_directory, root)
        assert result["source_commit"] == binding["G"]
        result = {"original_build_record": result, "binary": str(binary)}
    elif args.mode == "raw":
        plan = driver.read_json(args.plan_path)
        assert driver.exact_equal(plan, binding["plan"]), "registered plan differs"
        result = driver.inspect_raw(Path(args.raw_path), plan)
    else:
        # Caller supplies the same already accepted original build, not a rebuilt executable.
        build = Path(args.build_directory).resolve(strict=True)
        record = driver.read_json(build / "build.json")
        assert record["source_commit"] == binding["G"]
        assert digest(build / "p8-scale") == record["binary_sha256"]
        result = driver.validate_shard(args.shard_directory, record, build / "p8-scale", digest(build / "build.json"))
    with Path(args.output).open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2, sort_keys=True)
        stream.write("\n")


if __name__ == "__main__":
    main()

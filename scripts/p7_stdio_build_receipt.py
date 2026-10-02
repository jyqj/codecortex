#!/usr/bin/env python3
"""Build the real stdio product and bind Cargo's feature artifact to its bytes."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-kind", choices=("default", "semantic"), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    receipt_path = output / "build-receipt.json"
    receipt_path.unlink(missing_ok=True)  # A failed build cannot leave a valid old receipt.
    command = ["cargo", "build", "-p", "cc-server", "--bin", "codecortex",
               "--no-default-features", "--locked",
               "--message-format=json-render-diagnostics"]
    if args.package_kind == "semantic":
        command.extend(["--features", "semantic"])
    if args.offline:
        command.append("--offline")
    artifacts = []
    with (output / "cargo-build.jsonl").open("w") as log:
        with subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE,
                              text=True) as child:
            for line in child.stdout:
                log.write(line)
                message = json.loads(line)
                if (message.get("reason") == "compiler-artifact"
                        and message.get("target", {}).get("name") == "codecortex"
                        and message.get("target", {}).get("kind") == ["bin"]
                        and message.get("executable")):
                    artifacts.append(message)
            result = child.wait()
    if result:
        return result
    if len(artifacts) != 1:
        raise RuntimeError("expected exactly one Cargo codecortex binary artifact")
    artifact = artifacts[0]
    expected = [] if args.package_kind == "default" else ["semantic"]
    if sorted(artifact["features"]) != expected:
        raise RuntimeError("actual Cargo product features do not match requested package")
    executable = Path(artifact["executable"]).resolve(strict=True)
    snapshot = output / executable.name
    temporary = snapshot.with_suffix(".tmp")
    shutil.copy2(executable, temporary)
    temporary.replace(snapshot)
    digest = hashlib.sha256(snapshot.read_bytes()).hexdigest()
    if digest != hashlib.sha256(executable.read_bytes()).hexdigest():
        raise RuntimeError("product changed while copying its build snapshot")
    receipt = {"schema_version": 1, "package_kind": args.package_kind,
               "build_command": command, "build_exit_code": result,
               "binary_path": str(snapshot), "binary_sha256": digest,
               "cargo_artifact": artifact}
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"binary": str(snapshot), "receipt": str(receipt_path),
                      "package_kind": args.package_kind, "features": artifact["features"],
                      "sha256": digest}))
    return 0


if __name__ == "__main__":
    sys.exit(main())

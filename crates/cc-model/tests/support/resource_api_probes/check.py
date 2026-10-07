"""Bounded external API compilation probes; supply the exact freshly built rlib."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--rustc", required=True)
parser.add_argument("--rlib", required=True)
args = parser.parse_args()
rlib = Path(args.rlib).resolve()
deps = rlib.parent if rlib.parent.name == "deps" else rlib.parent / "deps"
root = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="declaration-api-") as directory:
    for name, expected in [("owned_shared", None), ("mutable_ref", "E0277"),
                           ("custom_provider", "E0277"), ("borrowed_mutation", "E0502")]:
        binary = Path(directory) / name
        result = subprocess.run([args.rustc, "--edition=2021", "--error-format=json",
                                 str(root / (name + ".rs")), "--extern", "cc_model=" + str(rlib),
                                 "-L", "dependency=" + str(deps), "-o", str(binary)],
                                capture_output=True, text=True, timeout=30)
        diagnostics = [json.loads(line) for line in result.stderr.splitlines()]
        errors = [d for d in diagnostics if d.get("level") == "error" and d.get("code")]
        codes = {d["code"]["code"] for d in errors}
        if expected is None:
            assert result.returncode == 0, result.stderr
            subprocess.run([str(binary)], check=True, timeout=10)
            print(name + ": compiled and ran")
        else:
            assert result.returncode != 0 and codes == {expected}, result.stderr
            print(name + ": refused " + expected)
            for error in errors:
                print("  " + error["message"])

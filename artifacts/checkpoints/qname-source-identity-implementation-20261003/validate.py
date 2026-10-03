"""Reproduce bounded acceptance with the official lock and direct Rust 1.95.

No public DEV, provider, GC/WAL, kill or denied-runtime scenario is run.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[3]
out = Path(__file__).resolve().parent
owned = Path("/workspace/scratch/qname-validation")
owned.mkdir(parents=True, exist_ok=True)
toolchain = Path("/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin")
env = dict(os.environ, CARGO_HOME="/workspace/.cargo", RUSTUP_HOME="/workspace/.rustup",
           RUSTC=str(toolchain / "rustc"), RUSTDOC=str(toolchain / "rustdoc"),
           CARGO_TARGET_DIR="/workspace/scratch/qname-target",
           CODECORTEX_CACHE_DIR="/workspace/scratch/qname-cache", CARGO_BUILD_JOBS="5")
env.pop("CODECORTEX_QNAME_DIAGNOSTIC_OUT", None)
cargo = str(toolchain / "cargo")

def run(argv, receipt):
    with (out / receipt).open("w") as log:
        result = subprocess.run(argv, cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        print((out / receipt).read_text())
        raise SystemExit(result.returncode)
    # Keep exact lines, removing only the trailing blank line for diff hygiene.
    p = out / receipt
    text = p.read_text().rstrip()
    p.write_text(text + ("\n" if text else ""))

run([cargo, "fmt", "--all", "--", "--check"], "fmt.log")
run([cargo, "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"], "clippy.log")
run([cargo, "test", "--locked", "-p", "cc-index", "--test", "qname_owner_diagnostic",
     "--test", "qname_identity_proof", "--test", "qname_identity_transaction", "-p", "cc-server",
     "--test", "qname_public_diagnostic", "--test", "qname_identity_lifecycle"], "product-tests.log")
run([cargo, "test", "--locked", "-p", "cc-parsers", "-p", "cc-search", "--lib"], "parser-search-regressions.log")
run([cargo, "test", "--locked", "-p", "cc-db", "--lib", "index_migrate"], "schema-regressions.log")
run([cargo, "test", "--locked", "-p", "cc-db", "--lib", "epoch_rules"], "epoch-regressions.log")
run([cargo, "test", "--locked", "-p", "cc-db", "--test", "semantic_outbox",
     "fifo_physical_index_preserves_existing_current_logical_data"], "fifo-regression.log")

manifest = owned / "native-build.jsonl"
with manifest.open("w") as log:
    subprocess.run([cargo, "build", "--locked", "-p", "cc-eval", "--lib", "--message-format=json"],
                   cwd=repo, env=env, stdout=log, check=True)
records = [json.loads(line) for line in manifest.read_text().splitlines()]
binary = owned / "native-driver"
cmd = [str(toolchain / "rustc"), "--edition=2021", str(out / "native_driver.rs"),
       "-L", "dependency=/workspace/scratch/qname-target/debug/deps", "-o", str(binary)]
for name in ["cc_eval", "cc_server", "serde_json"]:
    found = [r for r in records if r.get("reason") == "compiler-artifact"
             and r["target"]["name"] == name and "lib" in r["target"]["kind"]
             and (name != "serde_json" or "float_roundtrip" in r["features"])]
    assert len(found) == 1, (name, len(found))
    libs = [f for f in found[0]["filenames"] if f.endswith(".rlib")]
    assert len(libs) == 1
    cmd += ["--extern", f"{name}={libs[0]}"]
subprocess.run(cmd, cwd=repo, env=env, check=True)
with tempfile.TemporaryDirectory(prefix="native-fixture-", dir=owned) as root:
    run([str(binary), str(repo), str(out), root], "native.log")
fixture = out / "v24-original-fixture.sqlite3"
assert hashlib.sha256(fixture.read_bytes()).hexdigest() == json.loads(
    (out / "v24-original-fixture.json").read_text())["sha256"]
print("bounded qname acceptance complete; receipts:", out)

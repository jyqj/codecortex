import hashlib, json, os, pathlib, subprocess, sys, time
root = pathlib.Path.cwd()
evidence = pathlib.Path(__file__).parent
label = sys.argv[1]
command = sys.argv[2:]
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def source_map():
    names = subprocess.check_output(["git", "ls-files", "-z", "Cargo.toml", "Cargo.lock", "crates"]).decode().split("\0")
    return {name: sha(root / name) for name in sorted(names) if name}
before = source_map()
manifest = evidence / (label + "-source-sha256.json")
manifest.write_text(json.dumps(before, sort_keys=True, indent=2) + "\n")
started = time.time()
log = evidence / (label + ".log")
with log.open("wb") as output:
    result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT)
executables = {}
for line in log.read_text(errors="replace").splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    if event.get("reason") == "compiler-artifact" and event.get("executable"):
        binary = pathlib.Path(event["executable"])
        executables[str(binary)] = {"sha256": sha(binary), "target": event["target"]["name"], "test": event["profile"]["test"]}
receipt = {
    "schema_version": 1, "label": label,
    "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"]).decode().strip(),
    "source_tree": subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"]).decode().strip(),
    "status_after": subprocess.check_output(["git", "status", "--porcelain"]).decode(),
    "command": command, "cwd": str(root), "exit_code": result.returncode,
    "duration_seconds": round(time.time() - started, 3),
    "source_count": len(before), "source_manifest_sha256": sha(manifest),
    "source_unchanged": before == source_map(),
    "log_sha256": sha(log), "executables": executables,
    "environment": {key: os.environ.get(key) for key in ["CARGO_TARGET_DIR", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG", "CARGO_INCREMENTAL"]}
}
evidence.joinpath(label + "-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps(receipt, indent=2), flush=True)
print("\n".join(line for line in log.read_text(errors="replace").splitlines() if not line.startswith("{"))[-14000:], flush=True)
sys.exit(result.returncode)

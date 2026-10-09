import datetime, hashlib, inspect, json, os, pathlib, platform, subprocess, sys, traceback

SOURCE = pathlib.Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-bfccb8494ba0/source")
OUT = pathlib.Path("/output")
BUILD = pathlib.Path("/evidence/build")
SHARDS = [(11593548201, 50000)]
EXPECTED = "a23bb72d3c954f385b99fe81ce9189885c208557"
sys.dont_write_bytecode = True
sys.path.insert(0, str(SOURCE / "scripts"))
import p8_scale_matrix as matrix

def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def save(name, value):
    with (OUT / name).open("x") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def command(args):
    run = subprocess.run(args, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    return run.stdout.strip()

result = {
    "schema": "p8-scale-original-validator-linux-replay-v1",
    "scope": "Supplemental offline replay of original build and one newly completed original successful 50k index-zero shard only",
    "source_commit": EXPECTED,
    "started_utc": stamp(),
    "new_primary_samples": 0,
    "aggregate_executed": False,
    "release_certification": "not_run",
    "full_100k_certification": "not_run",
    "original_todos_completed_by_this_replay": 0,
    "validator_modified": False,
    "checks": []
}
try:
    assert pathlib.Path(matrix.__file__).resolve() == SOURCE / "scripts/p8_scale_matrix.py"
    assert command(["git", "-C", str(SOURCE), "rev-parse", "HEAD"]) == EXPECTED
    result["environment"] = {
        "python": sys.version,
        "git": command(["git", "--version"]),
        "glibc": command(["getconf", "GNU_LIBC_VERSION"]),
        "uname": command(["uname", "-a"]),
        "machine": platform.machine(),
        "network_interfaces": sorted(p.name for p in pathlib.Path("/sys/class/net").iterdir()),
        "source_mount_readonly": bool(os.statvfs(SOURCE).f_flag & os.ST_RDONLY),
        "git_mount_readonly": bool(os.statvfs("/Users/jin/Desktop/codecortex-rust/.git").f_flag & os.ST_RDONLY),
        "build_mount_readonly": bool(os.statvfs(BUILD).f_flag & os.ST_RDONLY),
        "original_build_mount_readonly": bool(os.statvfs("/original-build").f_flag & os.ST_RDONLY),
        "shard_mounts_readonly": {str(a): bool(os.statvfs("/evidence/" + str(a)).f_flag & os.ST_RDONLY) for a, _ in SHARDS},
        "observer_mount_readonly": bool(os.statvfs("/observer").f_flag & os.ST_RDONLY)
    }
    assert result["environment"]["machine"] == "x86_64"
    assert tuple(map(int, result["environment"]["glibc"].split()[-1].split("."))) >= (2,39)
    result["environment"]["network_policy"] = "Docker NetworkMode none, verified in controller container-before.json; inactive kernel tunnel devices may still be listed in sysfs."
    assert all(result["environment"][k] for k in ("source_mount_readonly", "git_mount_readonly", "build_mount_readonly", "original_build_mount_readonly", "observer_mount_readonly"))
    assert all(result["environment"]["shard_mounts_readonly"].values())
    result["loaded_source_files"] = {x: digest(SOURCE / x) for x in ("scripts/p8_scale_matrix.py", "scripts/p7_build_identity.py")}
    result["original_functions"] = {name: {"module": getattr(matrix, name).__module__, "source_sha256": hashlib.sha256(inspect.getsource(getattr(matrix, name)).encode()).hexdigest()} for name in ("validate_build", "validate_shard", "native_digest")}
    built, binary = matrix.validate_build(BUILD, root=SOURCE)
    build_sha = matrix.file_sha256(BUILD / "build.json")
    result["checks"].append({
        "function": "p8_scale_matrix.validate_build",
        "artifact_id": 11591482043,
        "passed": True,
        "receipt_sha256": build_sha,
        "binary_sha256": built["binary_sha256"],
        "binary_blake3": built["binary_blake3"],
        "source_manifest_sha256": built["source_manifest_sha256"],
        "native_hash_execution_required": ["p8-scale --hash-file p8-scale"]
    })
    save("validate-build-result.json", {"passed": True, "build_receipt_sha256": build_sha, "original_build_record": built, "binary_path": str(binary)})
    print(json.dumps({"stage": "validate_build", "passed": True, "artifact_id": 11591482043}), flush=True)
    for artifact_id, scale in SHARDS:
        shard = matrix.validate_shard(pathlib.Path("/evidence/" + str(artifact_id)), built, binary, build_sha)
        assert shard["plan"]["files"] == [scale] and shard["plan"]["shard"]["index"] == 0
        save("validate-shard-" + str(artifact_id) + "-result.json", shard)
        result["checks"].append({
            "function": "p8_scale_matrix.validate_shard",
            "artifact_id": artifact_id,
            "scale": scale,
            "shard_index": 0,
            "passed": True,
            "receipt_sha256": shard["receipt_sha256"],
            "sample_count": len(shard["measurements"]),
            "native_hash_execution_required": ["p8-scale --hash-file native/plan.json", "p8-scale --hash-file native/raw.jsonl"]
        })
        print(json.dumps({"stage": "validate_shard", "passed": True, "artifact_id": artifact_id, "samples": len(shard["measurements"])}), flush=True)
    assert result["loaded_source_files"] == {x: digest(SOURCE / x) for x in result["loaded_source_files"]}
    assert command(["git", "-C", str(SOURCE), "rev-parse", "HEAD"]) == EXPECTED
    result["status"] = "passed"
    result["original_validator_calls_completed"] = 2
    result["original_native_hash_calls_completed_by_unmodified_functions"] = 3
    result["shards_replayed"] = 1
    result["full_matrix_shards_required"] = 150
    result["full_matrix_acceptance"] = "not_established_by_this_scoped_replay"
except BaseException as exc:
    result["status"] = "failed"
    result["error"] = {"type": type(exc).__name__, "message": str(exc)}
    traceback.print_exc()
    raise
finally:
    result["completed_utc"] = stamp()
    save("replay-result.json", result)

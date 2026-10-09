import datetime, hashlib, json, pathlib, subprocess, sys, traceback

BASE = pathlib.Path.cwd()
ROOT = BASE / "review-scale/replay-linux"
LOG = ROOT / "controller-v2"
LOG.mkdir(exist_ok=False)
SOURCE = BASE / "source"
GIT = pathlib.Path("/Users/jin/Desktop/codecortex-rust/.git")
DOCKER = "/usr/local/bin/docker"
IMAGE = "docker.io/library/python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
NAME = "p8-scale-replay-bfccb8494ba0-0333"

def save(name, value):
    (LOG / name).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")

def inventory(directory):
    values = {}
    for path in sorted(directory.rglob("*")):
        assert not path.is_symlink()
        if path.is_file():
            values[path.relative_to(directory).as_posix()] = {"bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    return values

def inspect_container():
    run = subprocess.run([DOCKER, "container", "inspect", NAME], check=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return json.loads(run.stdout)[0]

def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

receipt = {"schema": "p8-scale-linux-replay-execution-receipt-v1", "started_utc": stamp(), "status": "incomplete", "image": IMAGE, "container_name": NAME, "cloud_task_created": False, "github_workflow_triggered": False}
try:
    selection = json.loads((ROOT / "environment/manifest-selection.json").read_text())
    selected_manifest = json.loads((ROOT / "environment/selected-manifest.json").read_text())
    image = json.loads((ROOT / "environment/image-inspect.json").read_text())[0]
    assert IMAGE.endswith(selection["selected_descriptor"]["digest"])
    verified = json.loads((ROOT / "environment/verified-image.json").read_text())
    assert verified["status"] == "passed" and verified["raw_config_digest_verified"] is True
    assert image["Id"] == selection["selected_descriptor"]["digest"]
    assert image["Descriptor"]["digest"] == selection["selected_descriptor"]["digest"]
    assert "sha256:" + hashlib.sha256((ROOT / "environment/selected-config.raw.json").read_bytes()).hexdigest() == selected_manifest["config"]["digest"]
    assert image["Architecture"] == "amd64" and image["Os"] == "linux"
    prep = json.loads((ROOT / "preparation-receipt-v2.json").read_text())
    assert hashlib.sha256((ROOT / "observer-v2/replay.py").read_bytes()).hexdigest() == prep["observer_sha256"]
    assert inventory(pathlib.Path(prep["build_original_directory"])) == prep["original_build_inventory"]
    assert inventory(pathlib.Path(prep["build_replay_directory"])) == prep["original_build_inventory"]
    args = [DOCKER, "create", "--pull=never", "--platform=linux/amd64", "--name", NAME,
            "--hostname", "p8-scale-validator-replay", "--network=none", "--read-only",
            "--cap-drop=ALL", "--security-opt=no-new-privileges", "--pids-limit=128",
            "--memory=2g", "--cpus=2", "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m",
            "--env", "PYTHONDONTWRITEBYTECODE=1", "--env", "GIT_OPTIONAL_LOCKS=0",
            "--env", "GIT_CONFIG_GLOBAL=/dev/null", "--env", "GIT_CONFIG_NOSYSTEM=1",
            "--env", "GIT_CONFIG_COUNT=1", "--env", "GIT_CONFIG_KEY_0=safe.directory",
            "--env", "GIT_CONFIG_VALUE_0=" + str(SOURCE)]
    mounts = [
        (SOURCE, str(SOURCE), True), (GIT, str(GIT), True),
        (ROOT / "inputs/build", "/evidence/build", True),
        (BASE / "raw/11591482043/extracted", "/original-build", True),
        (ROOT / "observer-v2", "/observer", True), (ROOT / "output-v2", "/output", False)
    ]
    for artifact in (11591367982, 11591464502, 11591693031):
        mounts.append((BASE / ("review-scale/raw-" + str(artifact) + "/extracted"), "/evidence/" + str(artifact), True))
    for source, target, readonly in mounts:
        args.extend(["--mount", "type=bind,src=" + str(source) + ",dst=" + target + (",readonly" if readonly else "")])
    args.extend([IMAGE, "python3", "-B", "/observer/replay.py"])
    save("create-command.json", {"argv": args, "source": str(SOURCE), "original_inputs_readonly": True, "only_output_bind_writable": "/output"})
    run = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    (LOG / "create.stdout").write_text(run.stdout)
    (LOG / "create.stderr").write_text(run.stderr)
    assert run.returncode == 0, "docker create failed: " + run.stderr
    receipt["container_id"] = run.stdout.strip()
    before = inspect_container()
    save("container-before.json", before)
    assert before["HostConfig"]["NetworkMode"] == "none" and before["HostConfig"]["ReadonlyRootfs"] is True
    assert all(m["RW"] == (m["Destination"] == "/output") for m in before["Mounts"] if m["Type"] == "bind")
    print(json.dumps({"stage": "container_created", "network": "none", "readonly_root": True, "container_id": receipt["container_id"]}), flush=True)
    with (LOG / "run.stdout").open("w") as out, (LOG / "run.stderr").open("w") as err:
        run = subprocess.run([DOCKER, "start", "--attach", NAME], stdout=out, stderr=err)
    after = inspect_container()
    save("container-after.json", after)
    receipt["docker_start_exit_code"] = run.returncode
    receipt["container_exit_code"] = after["State"]["ExitCode"]
    receipt["container_oom_killed"] = after["State"]["OOMKilled"]
    receipt["container_finished_at"] = after["State"]["FinishedAt"]
    receipt["original_build_bytes_unchanged"] = inventory(pathlib.Path(prep["build_original_directory"])) == prep["original_build_inventory"]
    receipt["replay_build_bytes_unchanged"] = inventory(pathlib.Path(prep["build_replay_directory"])) == prep["original_build_inventory"]
    receipt["original_build_mode_unchanged"] = oct((pathlib.Path(prep["build_original_directory"]) / "p8-scale").stat().st_mode) == prep["mode_restoration"]["original_extracted_mode"]
    receipt["original_shards_unchanged"] = {aid: inventory(pathlib.Path(value["original_directory"])) == value["inventory"] for aid, value in prep["shards"].items()}
    receipt["observer_unchanged"] = hashlib.sha256((ROOT / "observer-v2/replay.py").read_bytes()).hexdigest() == prep["observer_sha256"]
    replay = json.loads((ROOT / "output-v2/replay-result.json").read_text())
    receipt["replay_result"] = {"status": replay["status"], "sha256": hashlib.sha256((ROOT / "output-v2/replay-result.json").read_bytes()).hexdigest(), "checks": replay["checks"]}
    assert run.returncode == 0 and after["State"]["ExitCode"] == 0 and not after["State"]["OOMKilled"]
    assert replay["status"] == "passed"
    assert all(receipt[k] for k in ("original_build_bytes_unchanged", "replay_build_bytes_unchanged", "original_build_mode_unchanged", "observer_unchanged"))
    assert all(receipt["original_shards_unchanged"].values())
    receipt["status"] = "passed"
    print(json.dumps({"stage": "replay_complete", "status": "passed", "container_exit_code": receipt["container_exit_code"], "replay_result_sha256": receipt["replay_result"]["sha256"], "environment": replay["environment"], "checks": replay["checks"]}), flush=True)
except BaseException as exc:
    receipt["status"] = "failed"
    receipt["error"] = {"type": type(exc).__name__, "message": str(exc)}
    traceback.print_exc()
    raise
finally:
    receipt["completed_utc"] = stamp()
    save("execution-receipt.json", receipt)

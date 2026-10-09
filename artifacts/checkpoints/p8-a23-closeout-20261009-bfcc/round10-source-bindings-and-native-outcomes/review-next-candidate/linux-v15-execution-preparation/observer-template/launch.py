"""Preparation template. Running main requires an explicitly populated future-G binding."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

DOCKER = "/usr/local/bin/docker"
IMAGE = "docker.io/library/python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
IMAGE_ID = "sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
COMMON_GIT = Path("/Users/jin/Desktop/codecortex-rust/.git")

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binding", type=Path, required=True)
    args = parser.parse_args()
    binding = json.loads(args.binding.read_text())
    source = Path(binding["source_dir"])
    execution = Path(binding["execution_dir"])
    assert source.is_absolute() and source.is_dir() and not source.is_symlink()
    assert execution.is_absolute() and not execution.exists()
    assert re.fullmatch("[0-9a-f]{40}", binding["head"])
    assert source != execution and source not in execution.parents
    # Plain Git reads; no fetch, checkout, config write or guard import.
    env_host = dict(os.environ, GIT_NO_LAZY_FETCH="1", GIT_TERMINAL_PROMPT="0",
                    GIT_OPTIONAL_LOCKS="0")
    def git(*argv):
        return subprocess.check_output(["git", *argv], cwd=source, env=env_host)
    assert git("rev-parse", "HEAD").decode().strip() == binding["head"]
    assert git("rev-parse", "HEAD^{tree}").decode().strip() == binding["tree"]
    assert git("status", "--porcelain=v1", "--untracked-files=all") == b""
    assert Path(git("rev-parse", "--git-common-dir").decode().strip()).resolve() == COMMON_GIT
    for path, key in [("scripts/verify_reviewed_source_v15.py", "guard_sha256"),
                      ("scripts/reviewed-source-registry-v15.json", "registry_sha256"),
                      (binding["review_path"], "review_sha256")]:
        assert sha((source / path).read_bytes()) == binding[key]
    image_run = subprocess.run([DOCKER, "image", "inspect", IMAGE], capture_output=True, check=True)
    image = json.loads(image_run.stdout)[0]
    assert image["Id"] == IMAGE_ID and image["Architecture"] == "amd64" and image["Os"] == "linux"
    execution.mkdir()
    control, observer, output = [execution / n for n in ("controller", "observer", "output")]
    for path in (control, observer, output):
        path.mkdir()
    (output / "tmp").mkdir()
    observer_bytes = Path(__file__).with_name("observer.py").read_bytes()
    (observer / "controller.py").write_bytes(observer_bytes)
    (observer / "binding.json").write_text(json.dumps(binding, indent=2, sort_keys=True) + "\n")
    (control / "image-inspect.json").write_bytes(image_run.stdout)
    def save(name, value):
        (control / name).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    name = binding["container_name"]
    assert re.fullmatch("[A-Za-z0-9][A-Za-z0-9_.-]+", name)
    env = {
        "PYTHONDONTWRITEBYTECODE": "1", "GIT_OPTIONAL_LOCKS": "0",
        "GIT_NO_LAZY_FETCH": "1", "GIT_TERMINAL_PROMPT": "0",
        "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "safe.directory",
        "GIT_CONFIG_VALUE_0": str(source),
        "TMPDIR": "/output/tmp", "TMP": "/output/tmp", "TEMP": "/output/tmp",
    }
    argv = [DOCKER, "create", "--pull=never", "--platform=linux/amd64",
            "--name", name, "--hostname", "p8-v15-source-verifier",
            "--network=none", "--read-only", "--cap-drop=ALL",
            "--security-opt=no-new-privileges", "--pids-limit=128",
            "--memory=2g", "--cpus=2", "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m",
            "--workdir", str(source)]
    for key, value in env.items():
        argv.extend(["--env", key + "=" + value])
    mounts = [(source, str(source), True), (COMMON_GIT, str(COMMON_GIT), True),
              (observer, "/observer", True), (output, "/output", False)]
    for host, destination, readonly in mounts:
        argv.extend(["--mount", "type=bind,src=" + str(host) + ",dst=" + destination +
                     (",readonly" if readonly else "")])
    argv += [IMAGE, "/usr/local/bin/python3", "/observer/controller.py"]
    save("create-command.json", {"argv": argv, "observer_sha256": sha(observer_bytes),
         "binding_sha256": sha((observer / "binding.json").read_bytes()),
         "scope": "Source proof only. No scale/performance/181 suite execution."})
    created = subprocess.run(argv, capture_output=True)
    (control / "create.stdout").write_bytes(created.stdout)
    (control / "create.stderr").write_bytes(created.stderr)
    save("create-exit.json", {"exit_code": created.returncode})
    assert created.returncode == 0, created.stderr.decode(errors="replace")
    container_id = created.stdout.decode().strip()
    def inspect():
        data = subprocess.check_output([DOCKER, "container", "inspect", container_id])
        return json.loads(data)[0]
    before = inspect()
    save("container-before.json", before)
    assert before["Image"] == IMAGE_ID
    assert before["HostConfig"]["NetworkMode"] == "none"
    assert before["HostConfig"]["ReadonlyRootfs"] is True
    actual = {m["Destination"]: m for m in before["Mounts"] if m["Type"] == "bind"}
    assert set(actual) == {destination for _, destination, _ in mounts}
    for host, destination, readonly in mounts:
        assert actual[destination]["Source"] == str(host)
        assert actual[destination]["RW"] == (not readonly)
    receipt = {"status": "running", "container_id": container_id, "container_name": name,
               "image": IMAGE, "head": binding["head"],
               "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
               "docker_start_exit_code": None, "container_exit_code": None}
    save("execution-receipt.json", receipt)
    try:
        with (control / "attach.stdout").open("xb") as out, (control / "attach.stderr").open("xb") as err:
            attached = subprocess.run([DOCKER, "start", "--attach", container_id],
                                      stdout=out, stderr=err, timeout=3720)
        receipt["docker_start_exit_code"] = attached.returncode
    except subprocess.TimeoutExpired:
        receipt["host_attach_timeout"] = True
        # Stop only this newly created, known container; never global Docker changes.
        stopped = subprocess.run([DOCKER, "stop", "--time=10", container_id], capture_output=True)
        (control / "stop.stdout").write_bytes(stopped.stdout)
        (control / "stop.stderr").write_bytes(stopped.stderr)
        receipt["stop_exit_code"] = stopped.returncode
    finally:
        after = inspect()
        save("container-after.json", after)
        receipt.update(container_exit_code=after["State"]["ExitCode"],
                       container_running=after["State"]["Running"],
                       container_oom_killed=after["State"]["OOMKilled"],
                       finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
        child_path = output / "receipt.json"
        if child_path.exists():
            child = json.loads(child_path.read_text())
            receipt["observer_receipt_sha256"] = sha(child_path.read_bytes())
            receipt["original_child_exit_code"] = child.get("child_exit_code")
            receipt["observer_status"] = child.get("status")
        else:
            child = {}
            receipt["original_child_exit_code"] = None
        receipt["head_after"] = git("rev-parse", "HEAD").decode().strip()
        receipt["status_after"] = git("status", "--porcelain=v1", "--untracked-files=all").decode()
        receipt["status"] = "passed" if (
            receipt["docker_start_exit_code"] == 0 and receipt["container_exit_code"] == 0
            and not receipt["container_running"] and not receipt["container_oom_killed"]
            and child.get("status") == "passed" and child.get("child_exit_code") == 0
            and child.get("source_before_after_equal") is True
            and receipt["head_after"] == binding["head"] and receipt["status_after"] == ""
        ) else "not_passed"
        save("execution-receipt.json", receipt)
        print(json.dumps(receipt), flush=True)
    return 0 if receipt["status"] == "passed" else 1

if __name__ == "__main__":
    sys.exit(main())

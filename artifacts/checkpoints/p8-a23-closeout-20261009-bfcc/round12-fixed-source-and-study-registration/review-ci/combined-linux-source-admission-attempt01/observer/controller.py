"""External observer only; invokes the unmodified original v15 CLI once."""
import ast
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import stat
import subprocess
import sys

OUT = Path("/output")
BINDING = json.loads(Path("/observer/binding.json").read_text())
ROOT = Path(BINDING["source_dir"])
GIT = Path("/Users/jin/Desktop/codecortex-rust/.git")
PYTHON = "/usr/local/bin/python3"
COMMAND = [PYTHON, "scripts/verify_reviewed_source_v15.py", "--source-version",
           "p8-completion-source-20261009-v15"]

def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def save(name, value):
    (OUT / name).write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")

def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)

def tree(ref):
    result = {}
    for row in git("ls-tree", "-r", "-z", ref).split(b"\0"):
        if row:
            info, path = row.decode().split("\t", 1)
            mode, kind, oid = info.split()
            result[path] = {"mode": mode, "kind": kind, "blob": oid}
    return result

def checked(path):
    rel = Path(path)
    assert not rel.is_absolute() and ".." not in rel.parts
    current = ROOT
    assert current.is_dir() and not current.is_symlink()
    for part in rel.parts:
        current /= part
        assert not current.is_symlink()
    assert stat.S_ISREG(current.stat().st_mode)
    return current

def snapshot(registry):
    head = git("rev-parse", "HEAD").decode().strip()
    current = tree(head)
    product = tree(BINDING["product"])
    declared = {**registry["complete_inputs"], **registry["validation_inputs"]}
    paths = set(declared) | {
        "scripts/verify_reviewed_source_v15.py",
        "scripts/reviewed-source-registry-v15.json",
        ".github/workflows/ci.yml", BINDING["review_path"],
    } | set(BINDING.get("additional_inputs", {}))
    files = {}
    for path in sorted(paths):
        p = checked(path)
        raw = p.read_bytes()
        actual_sha = sha(raw)
        obj = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        assert current[path]["kind"] == "blob" and current[path]["blob"] == obj
        mode = "100755" if p.stat().st_mode & 0o111 else "100644"
        assert current[path]["mode"] == mode
        if path in declared:
            assert actual_sha == declared[path]
            assert current[path] == product[path]
        if path in BINDING.get("additional_inputs", {}):
            assert actual_sha == BINDING["additional_inputs"][path]
        files[path] = {"bytes": len(raw), "sha256": actual_sha,
                       "mode": mode, "blob": obj}
    return {
        "head": head, "tree": git("rev-parse", "HEAD^{tree}").decode().strip(),
        "parents": git("show", "-s", "--format=%P", "HEAD").decode().strip().split(),
        "review_parents": git("show", "-s", "--format=%P", BINDING["review"]).decode().strip().split(),
        "status_porcelain": git("status", "--porcelain=v1", "--untracked-files=all").decode(),
        "tracked_index_sha256": sha(git("ls-files", "-s", "-z")),
        "complete_product_count": len(registry["complete_inputs"]),
        "validation_count": len(registry["validation_inputs"]), "files": files,
    }

def main():
    receipt = {"schema": "original-v15-linux-source-proof-v1", "status": "starting",
               "started_at": utc(), "source_platform": "Linux amd64 container on Mac host",
               "command": COMMAND, "cwd": str(ROOT), "child_exit_code": None,
               "scope": "Original source admission only; no benchmark, performance, release or TODO credit.",
               "binding_sha256": sha(Path("/observer/binding.json").read_bytes()),
               "outer_child_timeout_seconds": 3600, "internal_guard_modified": False}
    save("receipt.json", receipt)
    result_code = 1
    try:
        for key in ("head", "tree", "product", "review"):
            assert re.fullmatch("[0-9a-f]{40}", BINDING[key])
        for key in ("guard_sha256", "registry_sha256", "review_sha256"):
            assert re.fullmatch("[0-9a-f]{64}", BINDING[key])
        assert sys.flags.optimize == 0 and not os.environ.get("PYTHONOPTIMIZE")
        assert not os.environ.get("PYTHONPATH")
        assert sys.platform == "linux" and platform.machine() == "x86_64"
        assert os.environ["GIT_NO_LAZY_FETCH"] == "1"
        assert os.environ["PYTHONDONTWRITEBYTECODE"] == "1"
        assert os.environ["TMPDIR"] == "/output/tmp"
        assert OUT.is_dir() and (OUT / "tmp").is_dir()
        runtime = {
            "python": sys.version, "executable": sys.executable,
            "git": git("--version").decode().strip(), "platform": platform.platform(),
            "machine": platform.machine(), "optimize": sys.flags.optimize,
            "source_readonly": bool(os.statvfs(ROOT).f_flag & os.ST_RDONLY),
            "git_readonly": bool(os.statvfs(GIT).f_flag & os.ST_RDONLY),
            "observer_readonly": bool(os.statvfs("/observer").f_flag & os.ST_RDONLY),
            "output_readonly": bool(os.statvfs(OUT).f_flag & os.ST_RDONLY),
            "network_policy": "Host controller must assert Docker NetworkMode none before start; no interface-name heuristic.",
        }
        save("runtime.json", runtime)
        assert runtime["source_readonly"] and runtime["git_readonly"]
        assert runtime["observer_readonly"] and not runtime["output_readonly"]
        guard_path = checked("scripts/verify_reviewed_source_v15.py")
        registry_path = checked("scripts/reviewed-source-registry-v15.json")
        assert sha(guard_path.read_bytes()) == BINDING["guard_sha256"]
        assert sha(registry_path.read_bytes()) == BINDING["registry_sha256"]
        assert sha(checked(BINDING["review_path"]).read_bytes()) == BINDING["review_sha256"]
        constants = {}
        for node in ast.parse(guard_path.read_text()).body:
            if isinstance(node, ast.Assign) and isinstance(node.value, ast.Constant):
                for target in node.targets:
                    if isinstance(target, ast.Name):
                        constants[target.id] = node.value.value
        assert constants["BASE"] == "7354db236c9d9850a75f31672697ae9eab44565e"
        assert constants["PRODUCT"] == BINDING["product"]
        assert constants["REVIEW"] == BINDING["review"]
        assert constants["REVIEW_PATH"] == BINDING["review_path"]
        assert constants["REGISTRY_SHA256"] == BINDING["registry_sha256"]
        registry = json.loads(registry_path.read_bytes())
        before = snapshot(registry)
        save("source-before.json", before)
        assert before["head"] == BINDING["head"] and before["tree"] == BINDING["tree"]
        assert before["parents"] == [BINDING["review"]]
        assert before["review_parents"] == [BINDING["product"]]
        assert before["status_porcelain"] == ""
        receipt.update(status="running", child_started_at=utc())
        with (OUT / "original-v15.stdout").open("xb") as stdout, (OUT / "original-v15.stderr").open("xb") as stderr:
            process = subprocess.Popen(COMMAND, cwd=ROOT, env=dict(os.environ),
                                       stdout=stdout, stderr=stderr, start_new_session=True)
            receipt["child_pid"] = process.pid
            save("receipt.json", receipt)
            timed_out = False
            try:
                code = process.wait(timeout=3600)
            except subprocess.TimeoutExpired:
                timed_out = True
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    code = process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    code = process.wait()
        receipt.update(child_exit_code=code, child_finished_at=utc(), timed_out=timed_out)
        # Persist the actual child terminal before later source/evidence checks.
        save("receipt.json", receipt)
        after = snapshot(registry)
        save("source-after.json", after)
        receipt["source_before_after_equal"] = before == after
        for stream in ("stdout", "stderr"):
            raw = (OUT / ("original-v15." + stream)).read_bytes()
            receipt[stream] = {"bytes": len(raw), "sha256": sha(raw)}
        assert code == 0 and not timed_out
        assert before == after
        receipt["status"] = "passed"
        result_code = 0
    except BaseException as error:
        receipt["status"] = "not_passed"
        receipt["observer_error"] = {"type": type(error).__name__, "message": str(error)}
        for stream in ("stdout", "stderr"):
            path = OUT / ("original-v15." + stream)
            if path.exists():
                raw = path.read_bytes()
                receipt[stream] = {"bytes": len(raw), "sha256": sha(raw)}
        raise
    finally:
        receipt["finished_at"] = utc()
        save("receipt.json", receipt)
        print(json.dumps({"status": receipt["status"], "child_exit_code": receipt["child_exit_code"],
                          "source_head": BINDING["head"]}), flush=True)
    return result_code

if __name__ == "__main__":
    sys.exit(main())

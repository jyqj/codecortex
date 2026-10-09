#!/usr/bin/env python3
"""Independent offline replay of nine fixed original G4 platform artifacts."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import sys
import time
import zipfile

if sys.flags.optimize:
    raise SystemExit("optimized Python is forbidden")
HERE = Path(__file__).resolve().parent
CONTROL = HERE.parents[2]
SOURCE = Path(os.environ["GITHUB_WORKSPACE"]) / "source"
OUT = Path(os.environ["RUNNER_TEMP"]) / "p8-G4-offline-platform"
REG = json.loads((HERE / "registration.json").read_text())
G = "260f596582f2d82b8d7c707b61a6b8b6a43b069f"
RUN = 37854847808
RESERVE = 512 * 1024**2

def require(ok, message):
    if not ok:
        raise ValueError(message)

def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x") as stream:
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write("\n")

def free():
    state = os.statvfs(OUT.parent)
    return state.f_bavail * state.f_frsize

def head(root):
    return subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()

def get_api(relative, path, maximum):
    require(relative.startswith("/repos/jyqj/codecortex/"), "foreign API URL")
    require(free() >= RESERVE + maximum, "not_run: insufficient filesystem reserve")
    require(not path.exists(), "refuse to replace retained input")
    path.parent.mkdir(parents=True, exist_ok=True)
    argv = ["curl", "--fail", "--location", "--silent", "--show-error",
            "--proto", "=https", "--proto-redir", "=https",
            "--retry", "0", "--connect-timeout", "30", "--max-time", "240",
            "--max-filesize", str(maximum),
            "--header", "Accept: application/vnd.github+json",
            "--header", "X-GitHub-Api-Version: 2022-11-28",
            "--header", "Authorization: Bearer " + os.environ["GH_TOKEN"],
            "https://api.github.com" + relative, "--output", str(path)]
    # No token, redirect URL, or effective URL is printed or recorded.
    with path.with_suffix(path.suffix + ".curl.stderr").open("xb") as error:
        try:
            completed = subprocess.run(argv, stdout=subprocess.DEVNULL, stderr=error, timeout=250)
        except subprocess.TimeoutExpired:
            # The original exception includes argv and its Authorization value.
            # Preserve the partial transfer, but never serialize that exception.
            raise ValueError("GitHub transfer timed out for " + relative) from None
    require(completed.returncode == 0, "GitHub transfer failed for " + relative)
    require(path.stat().st_size <= maximum, "API transfer exceeded bound")
    return path

def paged(kind):
    rows = []
    total = None
    page = 1
    while True:
        path = get_api("/repos/jyqj/codecortex/actions/runs/%d/%s?per_page=100&page=%d" %
                       (RUN, kind, page), OUT / "api" / ("%s-page-%03d.json" % (kind, page)), 16 * 1024**2)
        value = json.loads(path.read_text())
        require(isinstance(value.get("total_count"), int), "missing pagination total")
        if total is None:
            total = value["total_count"]
        require(total == value["total_count"], "pagination changed; preserve without retry")
        batch = value[kind]
        require(isinstance(batch, list) and len(batch) <= 100, "invalid API page")
        rows.extend(batch)
        require(len(rows) <= total, "duplicate or extra API page")
        if len(rows) == total:
            break
        require(len(batch) == 100 and page < 100, "missing API page")
        page += 1
    require(len({row["id"] for row in rows}) == len(rows), "duplicate API identity")
    result = {"total_count": total, kind: rows}
    write(OUT / (kind + ".json"), result)
    return result

def invoke(name, argv, cwd=None):
    directory = OUT / "commands" / name
    directory.mkdir(parents=True, exist_ok=False)
    write(directory / "command.json", {"argv": argv, "cwd": str(cwd) if cwd else None,
                                       "source_commit": G, "scope": "offline replay only"})
    started = time.monotonic()
    with (directory / "stdout.log").open("xb") as stdout, (directory / "stderr.log").open("xb") as stderr:
        completed = subprocess.run(argv, cwd=cwd, stdout=stdout, stderr=stderr, timeout=900,
                                   env={**{k: v for k, v in os.environ.items() if k not in ("GH_TOKEN", "GITHUB_TOKEN")}, "PYTHONDONTWRITEBYTECODE": "1"})
    result = {"exit_code": completed.returncode, "wall_seconds": time.monotonic() - started}
    write(directory / "result.json", result)
    require(completed.returncode == 0, "original offline checker failed: " + name)

def safe_infos(archive):
    with zipfile.ZipFile(archive) as zipped:
        infos = zipped.infolist()
    require(len({entry.filename for entry in infos}) == len(infos), "duplicate ZIP name")
    for entry in infos:
        path = PurePosixPath(entry.filename)
        require(not entry.is_dir() and not path.is_absolute() and ".." not in path.parts
                and path.as_posix() == entry.filename and "\\" not in entry.filename,
                "unsafe ZIP member")
        require(stat.S_IFMT(entry.external_attr >> 16) in (0, stat.S_IFREG)
                and not entry.flag_bits & 1, "encrypted/nonregular ZIP member")
    return infos

def extract_verified(archive, destination):
    destination.mkdir(parents=True, exist_ok=False)
    members = {}
    with zipfile.ZipFile(archive) as zipped:
        for info in safe_infos(archive):
            target = destination / info.filename
            target.parent.mkdir(parents=True, exist_ok=True)
            count = 0
            digest = hashlib.sha256()
            with zipped.open(info) as source, target.open("xb") as output:
                while block := source.read(1024 * 1024):
                    digest.update(block)
                    count += len(block)
                    output.write(block)
            require(count == info.file_size, "ZIP expanded count differs")
            members[info.filename] = {"bytes": count, "sha256": digest.hexdigest(),
                                      "crc32": "%08x" % info.CRC, "extracted": True}
    return members

def check_originals(destination, members):
    for name, row in members.items():
        path = destination / name
        require(path.is_file() and not path.is_symlink() and path.stat().st_size == row["bytes"]
                and sha(path) == row["sha256"], "original expanded bytes changed")

def run():
    require(os.environ.get("GITHUB_REPOSITORY") == "jyqj/codecortex"
            and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "wrong controller scope/attempt")
    require(head(CONTROL) == os.environ["GITHUB_SHA"], "controller checkout differs from event")
    require(head(SOURCE) == G, "not original G4 source")
    require(REG["source_commit"] == G and REG["run_id"] == RUN
            and REG["run_attempt"] == 1 and REG["reserve_bytes"] == RESERVE, "registration differs")
    sys.path.insert(0, str(SOURCE / "scripts"))
    import p8_cold_build as cold
    before, source_manifest = cold.source_identity(SOURCE, G)
    observer_before = cold.observer_snapshot(SOURCE)
    require(before["input_count"] == len(source_manifest) == 1087
            and before["manifest_sha256"] == REG["source_manifest_sha256"]
            and before["source_tree"] == REG["source_tree"], "product source differs")
    require(observer_before["manifest_sha256"] == REG["observer_manifest_sha256"]
            and len(observer_before["inputs"]) == 4, "observer source differs")
    write(OUT / "source-before.json", before)
    write(OUT / "source-inputs.json", source_manifest)
    write(OUT / "observer-before.json", observer_before)
    run_path = get_api("/repos/jyqj/codecortex/actions/runs/%d" % RUN, OUT / "run.json", 16 * 1024**2)
    original_run = json.loads(run_path.read_text())
    require(original_run["id"] == RUN and original_run["head_sha"] == G
            and original_run["run_attempt"] == 1 and original_run["status"] == "completed"
            and original_run["conclusion"] == "success", "original run not successful")
    jobs = paged("jobs")
    artifacts = paged("artifacts")
    specs = REG["artifacts"]
    expected = {(p, t, f) for p in cold.PLATFORMS for t in cold.TOOLCHAINS for f in cold.PACKAGES}
    require(len(specs) == len({s["id"] for s in specs}) == 9, "exact nine artifacts required")
    cells = [s for s in specs if s["kind"] == "cell"]
    collectors = [s for s in specs if s["kind"] == "collector"]
    require(len(cells) == 8 and len(collectors) == 1
            and {tuple(s["cell"][k] for k in ("platform", "toolchain", "package")) for s in cells} == expected,
            "complete original eight-cell population required")
    write(OUT / "registration.json", REG)
    required = sum(s["bytes"] + s["expanded_bytes"] for s in specs) + 64 * 1024**2
    write(OUT / "capacity.json", {"free_bytes": free(), "required_original_bytes_and_outputs": required,
                                 "reserve_bytes": RESERVE})
    require(free() >= RESERVE + required, "not_run: insufficient full original evidence capacity")
    originals = []
    for spec in specs:
        job = [j for j in jobs["jobs"] if j["id"] == spec["job_id"]]
        require(len(job) == 1 and job[0]["name"] == spec["job_name"]
                and job[0]["head_sha"] == G and job[0]["status"] == "completed"
                and job[0]["conclusion"] == "success", "original job identity/status differs")
        item = [a for a in artifacts["artifacts"] if a["id"] == spec["id"]]
        require(len(item) == 1, "registered artifact missing")
        item = item[0]
        require(item["name"] == spec["name"] and item["size_in_bytes"] == spec["bytes"]
                and item["digest"] == "sha256:" + spec["sha256"] and item["expired"] is False
                and item["workflow_run"]["id"] == RUN and item["workflow_run"]["head_sha"] == G,
                "registered artifact identity differs")
        archive = OUT / "zips" / ("%d.zip" % spec["id"])
        get_api("/repos/jyqj/codecortex/actions/artifacts/%d/zip" % spec["id"], archive, spec["bytes"])
        require(archive.stat().st_size == spec["bytes"] and sha(archive) == spec["sha256"],
                "original ZIP bytes differ")
        infos = safe_infos(archive)
        require(len(infos) == spec["members"]
                and sum(i.file_size for i in infos) == spec["expanded_bytes"], "original ZIP inventory differs")
        destination = OUT / ("cells" if spec["kind"] == "cell" else "original-collector") / str(spec["id"])
        destination.parent.mkdir(parents=True, exist_ok=True)
        members = extract_verified(archive, destination)
        write(OUT / "members" / ("%d.json" % spec["id"]), members)
        if spec["kind"] == "collector":
            require(set(members) == {"matrix.json"}, "original collector member differs")
        else:
            bundle = json.loads((destination / "bundle.json").read_text())
            require(bundle["cell"] == spec["cell"], "artifact cell differs from registration")
        originals.append((spec, archive, destination, members))
    # The original CLI creates a new output directory and performs every original
    # portable Cargo/binary/source/observer/stdio predicate. No shim or rewrite.
    invoke("original-cold-collector", [
        sys.executable, "-B", str(SOURCE / "scripts/p8_cold_build.py"),
        "--source-root", str(SOURCE), "--collect-cells", str(OUT / "cells"),
        "--expected-commit", G, "--output-dir", str(OUT / "replay")], SOURCE)
    replay = json.loads((OUT / "replay/matrix.json").read_text())
    original_path = OUT / "original-collector" / str(collectors[0]["id"]) / "matrix.json"
    original = json.loads(original_path.read_text())
    normalized = json.loads(original_path.read_text())
    normalized["source"]["source_root"] = str(SOURCE)
    require(replay == normalized and replay["counts"] == {"passed": 8, "failed": 0, "not_run": 0},
            "original full collector output differs beyond checkout root")
    write(OUT / "collector-comparison.json", {
        "exact_except_source_root": True, "original_sha256": sha(original_path),
        "replay_sha256": sha(OUT / "replay/matrix.json"),
        "original_source_root": original["source"]["source_root"], "replay_source_root": str(SOURCE)})
    for spec, archive, destination, members in originals:
        require(archive.stat().st_size == spec["bytes"] and sha(archive) == spec["sha256"],
                "original ZIP changed during replay")
        check_originals(destination, members)
    after, after_manifest = cold.source_identity(SOURCE, G)
    observer_after = cold.observer_snapshot(SOURCE)
    write(OUT / "source-after.json", after)
    write(OUT / "observer-after.json", observer_after)
    require(before == after and source_manifest == after_manifest and observer_before == observer_after,
            "source or loaded observers changed")
    return {"status": "accepted_scoped_original_G4_platform_replay", "source_commit": G,
            "original_run": RUN, "counts": replay["counts"],
            "original_artifact_ids": [s["id"] for s in specs],
            "product_or_Cargo_execution": False, "TODO_closed": 0, "TODO_remaining": 29}

def main():
    require(not OUT.exists(), "refuse existing receiver output")
    OUT.mkdir()
    started = time.monotonic()
    code = 2
    receipt = {"status": "incomplete_not_certified", "source_commit": G,
               "TODO_closed": 0, "TODO_remaining": 29}
    try:
        receipt = run()
        code = 0
    except BaseException as error:
        receipt["error"] = type(error).__name__ + ": " + str(error)
    finally:
        receipt["exit_code"] = code
        receipt["wall_seconds"] = time.monotonic() - started
        receipt["controller_commit"] = os.environ.get("GITHUB_SHA")
        inventory = {}
        for path in sorted(OUT.rglob("*")):
            if path.is_file():
                inventory[path.relative_to(OUT).as_posix()] = {"bytes": path.stat().st_size, "sha256": sha(path)}
        write(OUT / "file-inventory.json", inventory)
        receipt["file_inventory_sha256"] = sha(OUT / "file-inventory.json")
        write(OUT / "receipt.json", receipt)
    print(json.dumps(receipt, sort_keys=True))
    return code

if __name__ == "__main__":
    raise SystemExit(main())

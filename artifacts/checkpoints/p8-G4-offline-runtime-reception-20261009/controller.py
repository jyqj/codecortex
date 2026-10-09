#!/usr/bin/env python3
"""Receive exactly five prior G4 artifacts and replay their original offline checks.
This external controller is not part of the G4/P/R/G product validation tree.
"""
import hashlib
import json
import os
import re
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
OUT = Path(os.environ["RUNNER_TEMP"]) / "p8-G4-offline-runtime"
REG = json.loads((HERE / "registration.json").read_text())
G = "260f596582f2d82b8d7c707b61a6b8b6a43b069f"
RUN = 37854847820
RESERVE = 512 * 1024**2
LIMIT = 4 * 1024**3
# These hashes are original pre-existing receiver bodies, not rewritten validators.
ORIGINALS = {
    "mixed/review_mixed.py": "e0e6002810aa7722ed070483532a7853e9971f7f45f38ea5355caf9e5920825b",
    "soak/inspect.py": "a9eaa4b0ac030d8b9569d3db5bbea3e9ba624c115bf4ae6966f75320af5add26",
    "soak/cache_wire.py": "63486a0954a6e2d3b12077797e40a3256ce8aa6a179b1233f2947ab3473e4b35",
}

# Explicitly reviewed derivatives; original three receiver bodies remain archived.
DERIVED = {
    "soak/inspect.py": "4fa1e18e355cd303b0ed8df986438f5965fb186ee413249b0ac62a434a6705c1",
    "soak/cache_wire.py": "7ddec02ac351832562a7c0ff367cbccead2291c0fb447b91600c248ded57a5ea"
}

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

def original_name(spec):
    kind = "p8-soak" if spec["kind"] == "soak" else "p8-mixed-c" + str(spec["c"])
    return kind + "-" + G

def location(spec):
    if spec["kind"] == "mixed":
        home = OUT / "mixed" / ("C" + str(spec["c"]))
        archive = OUT / "mixed" / ("C%d-artifact-%d.zip" % (spec["c"], spec["id"]))
    else:
        home = OUT / "soak"
        archive = home / ("artifact-%d.zip" % spec["id"])
    return home, archive

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


def job_log_exclusions():
    result = {}
    for spec in REG["artifacts"]:
        home, _ = location(spec)
        original = home / ("job-%d.log" % spec["job"])
        result[original.relative_to(OUT).as_posix()] = spec["job"]
    return result

def public_job_log_events(raw):
    # This is a deliberately incomplete typed excerpt, never a replay input.
    # No arbitrary log substring, URL or command argument reaches the output.
    events = []
    for number, line in enumerate(raw.decode("utf-8", errors="replace").splitlines(), 1):
        checkout = re.search(r"git checkout --progress --force ([0-9a-f]{40})(?:\s|$)", line)
        exit_code = re.search(r"Process completed with exit code ([0-9]{1,10})\.", line)
        if checkout:
            events.append({"line": number, "event": "checkout", "commit": checkout.group(1)})
        if exit_code:
            events.append({"line": number, "event": "process_exit", "exit_code": int(exit_code.group(1))})
    return events

def record_withheld_job_logs():
    records = {}
    for relative, job in job_log_exclusions().items():
        original = OUT / relative
        record = {"job_id": job,
                  "original_job_api": "https://api.github.com/repos/jyqj/codecortex/actions/jobs/%d/logs" % job,
                  "included_in_upload": False,
                  "reason": "Original Actions job logs may contain signed storage redirects.",
                  "required_for_complete_replay": True,
                  "retained_on_runner": original.is_file()}
        if original.is_file():
            raw = original.read_bytes()
            record.update({"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()})
            public = original.with_suffix(".public-events.json")
            write(public, {"scope": "Incomplete typed excerpt; not a substitute for the original job log.",
                           "original_sha256": record["sha256"],
                           "events": public_job_log_events(raw)})
            record["public_excerpt"] = {"path": public.relative_to(OUT).as_posix(),
                                        "bytes": public.stat().st_size, "sha256": sha(public)}
        records[relative] = record
    write(OUT / "withheld-original-job-logs.json", {
        "scope": "Five raw job-log paths are explicitly excluded from the upload. No raw log is deleted or changed.",
        "complete_independent_replay_requires_original_actions_logs": True,
        "files": records})

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
        require(entry.filename.startswith(("p8-build/", "p8-runtime/")), "unknown original artifact prefix")
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

def adapt():
    # The only substitutions are fixed absolute checkout roots. No predicate is edited.
    mapping = {
        "mixed/review_mixed.py": ("REPO=Path('/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source')",
                                 "REPO=Path(" + repr(str(SOURCE)) + ")"),
        "soak/inspect.py": ("REPO = '/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source'",
                           "REPO = " + repr(str(SOURCE))),
    }
    records = {}
    for relative, expected in ORIGINALS.items():
        original = HERE / "originals" / relative
        require(sha(original) == expected, "original receiver body changed")
        variant = original
        if relative in DERIVED:
            variant = HERE / "derived" / relative
            require(sha(variant) == DERIVED[relative]
                    and REG["derived_receivers"][relative]["sha256"] == DERIVED[relative],
                    "registered receiver derivative changed")
        raw = variant.read_text()
        body = raw
        if relative in mapping:
            old, new = mapping[relative]
            require(raw.count(old) == 1 and new not in raw, "path mapping ambiguity")
            body = raw.replace(old, new)
            require(body.replace(new, old) == raw, "non-path receiver mutation")
        target = OUT / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("x") as output:
            output.write(body)
        records[relative] = {"original_sha256": expected, "derived_sha256": DERIVED.get(relative),
                             "selected_receiver_sha256": sha(variant), "mapped_sha256": sha(target),
                             "only_checkout_root_mapping_after_registered_derivation": relative in mapping}
    write(OUT / "receiver-mapping.json", records)

def run():
    require(os.environ.get("GITHUB_REPOSITORY") == "jyqj/codecortex"
            and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "wrong controller scope/attempt")
    require(head(CONTROL) == os.environ["GITHUB_SHA"], "controller checkout differs from event")
    require(head(SOURCE) == G, "not original G4 source")
    require(REG["source"] == G and REG["run_id"] == RUN and REG["run_attempt"] == 1
            and REG["reserve_bytes"] == RESERVE and REG["expanded_ceiling_bytes"] == LIMIT,
            "registration changed")
    sys.path.insert(0, str(SOURCE / "scripts"))
    import p7_build_identity as identity
    import p8_runtime_build as builder
    source_before = identity.source_snapshot(SOURCE)
    observer_before = builder.observer_snapshot(SOURCE)
    require(source_before["source_commit"] == G
            and source_before["source_tree"] == REG["source_tree"]
            and source_before["input_count"] == len(source_before["inputs"]) == 1087,
            "fixed product snapshot mismatch")
    require(observer_before["source_commit"] == G and len(observer_before["files"]) == 7,
            "fixed observer snapshot mismatch")
    write(OUT / "source-before.json", source_before)
    write(OUT / "observer-before.json", observer_before)
    run_path = get_api("/repos/jyqj/codecortex/actions/runs/%d" % RUN, OUT / "run.json", 16 * 1024**2)
    workflow_run = json.loads(run_path.read_text())
    require(workflow_run["id"] == RUN and workflow_run["head_sha"] == G
            and workflow_run["run_attempt"] == 1, "wrong original workflow run")
    jobs = paged("jobs")
    artifacts = paged("artifacts")
    require(len(REG["artifacts"]) == 5 and len({s["id"] for s in REG["artifacts"]}) == 5
            and {s["c"] for s in REG["artifacts"] if s["kind"] == "mixed"} == {1, 4, 8, 16}
            and sum(s["kind"] == "soak" for s in REG["artifacts"]) == 1, "registered population")
    write(OUT / "registration.json", REG)
    total_expanded = 0
    preflight = []
    for spec in REG["artifacts"]:
        job = [j for j in jobs["jobs"] if j["id"] == spec["job"]]
        expected_job = "soak" if spec["kind"] == "soak" else "mixed (%d)" % spec["c"]
        require(len(job) == 1 and job[0]["name"] == expected_job and job[0]["head_sha"] == G
                and job[0]["status"] == "completed" and job[0]["conclusion"] == "success",
                "original selected job not successful")
        item = [a for a in artifacts["artifacts"] if a["id"] == spec["id"]]
        require(len(item) == 1, "missing selected artifact")
        item = item[0]
        require(item["name"] == original_name(spec) and item["expired"] is False
                and item["size_in_bytes"] == spec["bytes"] and item["digest"] == "sha256:" + spec["sha256"]
                and item["workflow_run"]["id"] == RUN and item["workflow_run"]["head_sha"] == G,
                "artifact registration identity mismatch")
        home, archive = location(spec)
        home.mkdir(parents=True, exist_ok=False)
        get_api("/repos/jyqj/codecortex/actions/artifacts/%d/zip" % spec["id"], archive, spec["bytes"])
        require(archive.stat().st_size == spec["bytes"] and sha(archive) == spec["sha256"],
                "original ZIP size/digest mismatch")
        infos = safe_infos(archive)
        expanded = sum(info.file_size for info in infos)
        total_expanded += expanded
        require(total_expanded <= LIMIT, "expanded original evidence exceeds receiver capacity")
        preflight.append({"artifact_id": spec["id"], "zip_bytes": spec["bytes"],
                          "expanded_bytes": expanded, "members": len(infos)})
        get_api("/repos/jyqj/codecortex/actions/jobs/%d/logs" % spec["job"],
                home / ("job-%d.log" % spec["job"]), 64 * 1024**2)
    # Original scripts additionally retain one statistics executable plus plan/raw output.
    # Conservatively reserve each complete expanded artifact again for those derived copies.
    needed = total_expanded * 2 + 64 * 1024**2
    write(OUT / "capacity.json", {"free_bytes": free(), "required_for_expansion_and_replay": needed,
                                 "reserve_bytes": RESERVE, "expanded_ceiling_bytes": LIMIT,
                                 "artifacts": preflight})
    require(free() >= RESERVE + needed, "not_run: cannot retain all expanded originals and replay outputs")
    mixed_specs = []
    inventories = []
    for spec in REG["artifacts"]:
        home, archive = location(spec)
        destination = home / "originals"
        members = extract_verified(archive, destination)
        write(home / ("member-hashes.json" if spec["kind"] == "mixed" else "transport-member-hashes.json"), {"members": members})
        inventories.append((spec, destination, members))
        for prefix in ("p8-build", "p8-runtime"):
            require((destination / prefix / "seal.json").is_file(), "mandatory original seal absent")
        invoke("seal-%d" % spec["id"], [sys.executable, "-B", str(SOURCE / "scripts/p8_runtime.py"),
               "verify", "--output", str(destination / "p8-runtime"),
               "--build-output", str(destination / "p8-build")], SOURCE)
        if spec["kind"] == "mixed":
            mixed_specs.append({"c": spec["c"], "id": spec["id"]})
        else:
            expected = {**spec, "artifact_id": spec["id"], "source": G, "run_id": RUN,
                        "run_attempt": 1, "job_id": spec["job"]}
            write(home / "expected-artifact.json", expected)
            write(home / "run.json", workflow_run)
            write(home / "jobs.json", jobs)
            write(home / "artifacts.json", artifacts)
    write(OUT / "mixed" / "expected-artifacts.json", mixed_specs)
    adapt()
    invoke("original-mixed-review", [sys.executable, "-B", str(OUT / "mixed/review_mixed.py")])
    invoke("original-soak-review", [sys.executable, "-B", str(OUT / "soak/inspect.py")])
    for spec, destination, members in inventories:
        _, archive = location(spec)
        require(sha(archive) == spec["sha256"] and archive.stat().st_size == spec["bytes"],
                "original ZIP changed after replay")
        check_originals(destination, members)
    after = identity.source_snapshot(SOURCE)
    observer_after = builder.observer_snapshot(SOURCE)
    write(OUT / "source-after.json", after)
    write(OUT / "observer-after.json", observer_after)
    require(after == source_before and observer_after == observer_before, "source/observer changed")
    return {"status": "accepted_scoped_original_G4_offline_replay", "source_commit": G,
            "original_run": RUN, "original_artifact_ids": [s["id"] for s in REG["artifacts"]],
            "original_sources_relabelled": False, "new_product_workload": False,
            "TODO_closed": 0, "TODO_remaining": 29}

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
        record_withheld_job_logs()
        receipt["withheld_original_job_logs_sha256"] = sha(OUT / "withheld-original-job-logs.json")
        receipt["complete_replay_requires_original_actions_logs"] = True
        exclusions = job_log_exclusions()
        inventory = {}
        for path in sorted(OUT.rglob("*")):
            if path.is_file():
                relative = path.relative_to(OUT).as_posix()
                inventory[relative] = {"bytes": path.stat().st_size, "sha256": sha(path),
                                       "included_in_upload": relative not in exclusions}
        write(OUT / "file-inventory.json", inventory)
        receipt["file_inventory_sha256"] = sha(OUT / "file-inventory.json")
        write(OUT / "receipt.json", receipt)
    print(json.dumps(receipt, sort_keys=True))
    return code

if __name__ == "__main__":
    raise SystemExit(main())

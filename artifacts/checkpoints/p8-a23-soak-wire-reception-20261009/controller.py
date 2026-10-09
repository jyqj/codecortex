#!/usr/bin/env python3
"""Receive one fixed a23 original soak; no product or new measurement."""
import base64
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
OUT = Path(os.environ["RUNNER_TEMP"]) / "p8-a23-soak-wire-reception"
REG = json.loads((HERE / "registration.json").read_text())
G = "a23bb72d3c954f385b99fe81ce9189885c208557"
RUN = 37871838957
JOB = 113631481157
AID = 11594089439
RESERVE = 512 * 1024**2
LIMIT = 4 * 1024**3

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

def record_withheld_job_log():
    original = OUT / "job-113631481157.log"
    record = {"job_id":JOB, "included_in_upload":False,
              "reason":"Original Actions job logs may contain signed storage redirects.",
              "required_for_complete_replay":True,
              "original_job_api":"https://api.github.com/repos/jyqj/codecortex/actions/jobs/113631481157/logs",
              "retained_on_runner":original.is_file()}
    if original.is_file():
        raw = original.read_bytes()
        record.update(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
        public = OUT / "job-113631481157.public-events.json"
        write(public, {"scope":"Incomplete typed excerpt, not a substitute for the original log.",
                       "original_sha256":record["sha256"], "events":public_job_log_events(raw)})
        record["public_excerpt"] = {"path":public.name, "bytes":public.stat().st_size,
                                    "sha256":sha(public)}
    write(OUT / "withheld-original-job-log.json",record)

def install_checkers():
    for relative, identity in REG["receiver_inputs"].items():
        path = HERE / relative
        require(path.is_file() and not path.is_symlink()
                and path.stat().st_size == identity["bytes"] and sha(path) == identity["sha256"],
                "registered receiver input differs: " + relative)
    raw = (HERE / "originals/runtime-independent-review-v2.py").read_text()
    old = "source=(root/'source').resolve()"
    new = "source=Path(" + repr(str(SOURCE)) + ").resolve()"
    require(raw.count(old) == 1 and new not in raw, "original checker checkout-path ambiguity")
    body = raw.replace(old,new)
    old_scope = REG["peer_checker_original_scope"]
    new_scope = REG["peer_checker_replay_scope"]
    require(body.count(old_scope) == 1 and new_scope not in body, "original checker scope-text ambiguity")
    body = body.replace(old_scope,new_scope)
    require(body.replace(new_scope,old_scope).replace(new,old) == raw,
            "unexpected original checker predicate change")
    destination = OUT / "runtime-independent-review.py"
    with destination.open("x") as stream:
        stream.write(body)
    write(OUT / "checker-adaptation.json",{
        "original_blob":REG["receiver_inputs"]["originals/runtime-independent-review-v2.py"]["git_blob"],
        "original_sha256":sha(HERE/"originals/runtime-independent-review-v2.py"),
        "adapted_sha256":sha(destination),
        "changes":["actual read-only source checkout path","replay host/scope prose only"],
        "inverse_exact":True})
    for relative in ("cache_wire.py","supplement.py"):
        with (OUT/relative).open("xb") as stream:
            stream.write((HERE/relative).read_bytes())
    (OUT/"reference").mkdir()
    with (OUT/"reference/peer-soak-review.json").open("xb") as stream:
        stream.write((HERE/"reference/peer-soak-review.json").read_bytes())

def run():
    require(os.environ.get("GITHUB_REPOSITORY") == "jyqj/codecortex"
            and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "wrong controller repository/attempt")
    require(head(CONTROL) == os.environ["GITHUB_SHA"], "controller differs from event commit")
    require(head(SOURCE) == G, "not fixed original a23 source")
    require(REG["source"] == G and REG["run_id"] == RUN and REG["job_id"] == JOB
            and REG["run_attempt"] == 1 and REG["reserve_bytes"] == RESERVE
            and REG["expanded_ceiling_bytes"] == LIMIT, "registration identity differs")
    initial_self = sha(Path(__file__))
    initial_registration = sha(HERE/"registration.json")
    sys.path.insert(0,str(SOURCE/"scripts"))
    import p7_build_identity as identity
    import p8_runtime_build as builder
    source_before, observer_before = identity.source_snapshot(SOURCE), builder.observer_snapshot(SOURCE)
    require(source_before["source_commit"] == G and source_before["source_tree"] == REG["source_tree"]
            and source_before["input_count"] == len(source_before["inputs"]) == 1087
            and source_before["manifest_sha256"] == REG["source_manifest_sha256"],
            "full original product source binding")
    require(observer_before["source_commit"] == G and len(observer_before["files"]) == 9
            and observer_before["manifest_sha256"] == REG["observer_manifest_sha256"],
            "full original observer binding")
    write(OUT/"source-before.json",source_before)
    write(OUT/"observer-before.json",observer_before)
    write(OUT/"registration.json",REG)
    workflow_run=json.loads(get_api("/repos/jyqj/codecortex/actions/runs/%d" % RUN,
                                   OUT/"run.json",16*1024**2).read_text())
    require(workflow_run["id"] == RUN and workflow_run["head_sha"] == G
            and workflow_run["run_attempt"] == 1, "wrong original workflow run")
    jobs, artifacts = paged("jobs"), paged("artifacts")
    job = [j for j in jobs["jobs"] if j["id"] == JOB]
    require(len(job) == 1 and job[0]["head_sha"] == G and job[0]["run_attempt"] == 1
            and job[0]["name"] == "soak" and job[0]["status"] == "completed"
            and job[0]["conclusion"] == "success", "original soak job not successful")
    spec = REG["artifact"]
    selected = [a for a in artifacts["artifacts"] if a["id"] == AID]
    require(len(selected) == 1, "missing original artifact")
    item = selected[0]
    require(item["name"] == "p8-soak-"+G and item["expired"] is False
            and item["size_in_bytes"] == spec["bytes"] and item["digest"] == "sha256:"+spec["sha256"]
            and item["workflow_run"]["id"] == RUN and item["workflow_run"]["head_sha"] == G,
            "exact original artifact identity")
    home = OUT/"raw"/str(AID)
    home.mkdir(parents=True)
    archive = home/"original.zip"
    get_api("/repos/jyqj/codecortex/actions/artifacts/%d/zip" % AID,archive,spec["bytes"])
    require(archive.stat().st_size == spec["bytes"] and sha(archive) == spec["sha256"],
            "full original ZIP digest")
    infos = safe_infos(archive)
    expanded = sum(info.file_size for info in infos)
    require(len(infos) == REG["original_member_count"] and expanded == REG["original_member_bytes"]
            and expanded <= LIMIT, "original ZIP population/capacity differs")
    required = expanded*2 + 64*1024**2
    write(OUT/"capacity.json",{"free_bytes":free(),"required_for_expansion_and_replay":required,
          "reserve_bytes":RESERVE,"expanded_ceiling_bytes":LIMIT,"actual_expanded_bytes":expanded,
          "actual_member_count":len(infos)})
    require(free() >= RESERVE+required, "not_run: cannot retain originals and offline outputs")
    log = get_api("/repos/jyqj/codecortex/actions/jobs/%d/logs" % JOB,
                  OUT/"job-113631481157.log",64*1024**2)
    log_text=log.read_text()
    require("git checkout --progress --force "+G in log_text
            and re.search(r"git log -1 --format=%H\n[^\n]*"+G,log_text),
            "actual original job fixed-head checkout")
    destination=home/"extracted"
    members=extract_verified(archive,destination)
    write(home/"member-hashes.json",{"members":members})
    for prefix in ("p8-build","p8-runtime"):
        require((destination/prefix/"seal.json").is_file(), "mandatory original seal absent")
    install_checkers()
    invoke("original-complete-checker",[sys.executable,"-B",str(OUT/"runtime-independent-review.py"),
           str(AID),spec["sha256"],"soak","4"],OUT)
    invoke("full-wire-and-statistics-supplement",[sys.executable,"-B",str(OUT/"supplement.py"),
           str(OUT),str(SOURCE)],OUT)
    require(sha(archive) == spec["sha256"] and archive.stat().st_size == spec["bytes"],
            "original ZIP changed after replay")
    check_originals(destination,members)
    source_after, observer_after=identity.source_snapshot(SOURCE),builder.observer_snapshot(SOURCE)
    write(OUT/"source-after.json",source_after)
    write(OUT/"observer-after.json",observer_after)
    require(source_before == source_after and observer_before == observer_after,
            "source/observer changed during offline replay")
    require(sha(Path(__file__)) == initial_self and sha(HERE/"registration.json") == initial_registration,
            "receiver/registration changed")
    for relative,row in REG["receiver_inputs"].items():
        require(sha(HERE/relative) == row["sha256"], "original receiver input changed")
    return {"status":"accepted_scoped_original_a23_soak_offline_replay",
            "source_commit":G,"original_run":RUN,"original_artifact_id":AID,
            "original_sources_relabelled":False,"new_product_workload":False,
            "TODO_closed":0,"TODO_remaining":29}

def emit_report_frames(relative):
    path = OUT / relative
    if not path.is_file():
        return
    raw_report = path.read_bytes()
    # Same safe byte framing as controller 0b5b23bb.  No JSON reserialization.
    chunk_bytes = 16 * 1024
    chunk_count = (len(raw_report) + chunk_bytes - 1) // chunk_bytes
    header = dict(schema="p8-safe-full-intake-log-frames-v1", profile="a23-soak:" + relative,
                  report_bytes=len(raw_report), report_sha256=hashlib.sha256(raw_report).hexdigest(),
                  chunk_bytes=chunk_bytes, chunk_count=chunk_count)
    print("\nBEGIN P8_COMPLETE_SAFE_INTAKE_JSON", flush=True)
    print(json.dumps(dict(kind="header", **header), sort_keys=True), flush=True)
    for index, offset in enumerate(range(0, len(raw_report), chunk_bytes)):
        chunk = raw_report[offset:offset + chunk_bytes]
        print(json.dumps(dict(kind="chunk", index=index, count=chunk_count,
                              bytes=len(chunk), sha256=hashlib.sha256(chunk).hexdigest(),
                              payload_base64=base64.b64encode(chunk).decode("ascii")), sort_keys=True), flush=True)
    print(json.dumps(dict(kind="complete", **header), sort_keys=True), flush=True)
    print("END P8_COMPLETE_SAFE_INTAKE_JSON\n", flush=True)

def main():
    require(not OUT.exists(), "refuse existing receiver output")
    OUT.mkdir()
    started=time.monotonic()
    code=2
    receipt={"status":"incomplete_not_certified","source_commit":G,
             "TODO_closed":0,"TODO_remaining":29}
    accepted=None
    try:
        accepted=run()
    except BaseException as error:
        receipt["error"]=type(error).__name__+": "+str(error)
    try:
        record_withheld_job_log()
        receipt["withheld_original_job_log_sha256"]=sha(OUT/"withheld-original-job-log.json")
        receipt["complete_replay_requires_original_actions_log"]=True
        inventory={}
        for path in sorted(OUT.rglob("*")):
            if path.is_file():
                relative=path.relative_to(OUT).as_posix()
                inventory[relative]={"bytes":path.stat().st_size,"sha256":sha(path),
                    "included_in_upload":relative != "job-113631481157.log"}
        write(OUT/"file-inventory.json",inventory)
        receipt["file_inventory_sha256"]=sha(OUT/"file-inventory.json")
        if accepted is not None:
            receipt.update(accepted)
            code=0
    except BaseException as error:
        receipt.update(status="incomplete_not_certified",
                       finalization_error=type(error).__name__+": "+str(error))
        code=2
    receipt.update(exit_code=code,wall_seconds=time.monotonic()-started,
                   controller_commit=os.environ.get("GITHUB_SHA"))
    write(OUT/"receipt.json",receipt)
    print(json.dumps(receipt,sort_keys=True))
    for relative in (
        "receipt.json", "file-inventory.json", "capacity.json", "checker-adaptation.json",
        "withheld-original-job-log.json", "job-113631481157.public-events.json",
        "commands/original-complete-checker/command.json",
        "commands/original-complete-checker/result.json",
        "commands/full-wire-and-statistics-supplement/command.json",
        "commands/full-wire-and-statistics-supplement/result.json",
        "review-runtime/runtime-11594089439-review-v2.json",
        "wire-supplement/inspection.json", "wire-supplement/cache-wire-binding.json",
        "wire-supplement/statistics-command.json", "wire-supplement/statistics-execution.json",
        "wire-supplement/statistics.json",
    ):
        emit_report_frames(relative)
    return code

if __name__ == "__main__":
    raise SystemExit(main())

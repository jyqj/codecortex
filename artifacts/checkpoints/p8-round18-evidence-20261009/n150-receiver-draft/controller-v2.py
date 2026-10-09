#!/usr/bin/env python3
"""DRAFT: receive original a23 N150 only; never run a product workload or Cargo.
The original a23 aggregate/validators/statistics are imported without modification.
Download and credential-free offline replay are separate workflow steps.
"""
import argparse, contextlib, datetime, hashlib, inspect, json, os, re, resource, shutil, stat, subprocess, sys, time, zipfile, zlib
from pathlib import Path, PurePosixPath

SOURCE = "a23bb72d3c954f385b99fe81ce9189885c208557"
TREE = "58147c952505c44da1f41eb4b9c31643f2303b96"
MANIFEST = "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00"
RUN, ATTEMPT = 37872522779, 1
REPOSITORY = "jyqj/codecortex"
BRANCH = "task/p8-a23-scale-reception-20261009"
SCALES = (1000, 5000, 10000, 50000, 100000)
HEADROOM, OUTPUT_ALLOWANCE = 512 * 1024**2, 128 * 1024**2
MAX_ZIP, MAX_EXPANDED_PER_ZIP = 1024**3, 1024**3
SDK_PATHS = ["/usr/local/lib/android", "/usr/share/dotnet"]
OBSERVERS = {
    "scripts/p8_scale_matrix.py": "f169b0f26cf2a87d3e2548d734431c530cbf63d353ac054045ec8be80889f957",
    "scripts/p7_build_identity.py": "5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5",
    "scripts/p8_runner_capacity.py": "2e37954df30b153ad82b49a5841a977451288c1730d03bfd2d0d7a2ace412939",
    ".github/workflows/p8-scale.yml": "4258476dd37d8af223aaa7ad45d7c97d7298be1ba4ec5ffd9af9949785ed8f3d",
}
KNOWN_ARTIFACTS = {
    "p8-scale-shard-50000-0-37872522779": {
        "id": 11593548201,
        "bytes": 2964759,
        "sha256": "d0163f3421ea9abf3d544edad56a9386aec2db55fef0aa11155e30e10a27b06d"
    },
    "p8-scale-capacity-100000-0-37872522779": {
        "id": 11592230167,
        "bytes": 786,
        "sha256": "91e99d0cc13917232dc9c722ee6f1828919da414ea6d7e5d39f0f610440c7266"
    },
    "p8-scale-capacity-1000-0-37872522779": {
        "id": 11591807078,
        "bytes": 783,
        "sha256": "7fff46b5a357b34a81e9d386f1453a1343101eff0055f67457aa2664b273877b"
    },
    "p8-scale-shard-10000-0-37872522779": {
        "id": 11591693031,
        "bytes": 853798,
        "sha256": "d5a73580784d5bb247adf0d0e6d189f45215600df62bc94e4ca88bdf0ab6a444"
    },
    "p8-scale-build-37872522779": {
        "id": 11591482043,
        "bytes": 9214568,
        "sha256": "d84d66e9048aa2849d037af9df06fcc34423ca7a4e7937f3e79e4d77b441afed"
    },
    "p8-scale-shard-5000-0-37872522779": {
        "id": 11591464502,
        "bytes": 583603,
        "sha256": "cfc34907c79a13ceede8ad7a892af2d0861ddd2b978ba36105ad26fba2da010f"
    },
    "p8-scale-shard-1000-0-37872522779": {
        "id": 11591367982,
        "bytes": 952631,
        "sha256": "a5eeda7a03131fd0d5cebfd26bbcdfb0ff07db32f303593219665f0aa7d2b5b6"
    },
    "p8-scale-capacity-10000-0-37872522779": {
        "id": 11591237661,
        "bytes": 783,
        "sha256": "86dcdfb7624c6da5a35609600b2653d71d1a51f9cb0c1596e0ab27c05aa60f2b"
    },
    "p8-scale-capacity-50000-0-37872522779": {
        "id": 11591232618,
        "bytes": 784,
        "sha256": "58c71192c4fa0ae398f2b9cad462865da64f83ab435a67934c4a6bd33fb83a63"
    },
    "p8-scale-capacity-5000-0-37872522779": {
        "id": 11591128037,
        "bytes": 785,
        "sha256": "04ffc744eb89e5ef14740947e1e6618e95b29909832677298eb1cf1eb444b392"
    }
}

def require(ok, message):
    if not ok:
        raise ValueError(message)

def digest(path):
    h=hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024**2), b""):
            h.update(block)
    return h.hexdigest()

def regular_tree_path(path, directory=True):
    require(path.is_absolute() and path.resolve(strict=True)==path, "noncanonical actual path")
    for parent in (path,*path.parents):
        mode=parent.lstat().st_mode
        require(not stat.S_ISLNK(mode), "symlink in actual path")
        require(stat.S_ISDIR(mode) if parent != path or directory else stat.S_ISREG(mode),
                "nonregular actual path")

def json_load(path):
    regular_tree_path(path, False)
    return json.loads(path.read_bytes())

def json_bytes(value):
    return (json.dumps(value,ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()

def save_new(path, value):
    with path.open("xb") as stream:
        stream.write(json_bytes(value))

def zip_inventory(path, entry):
    regular_tree_path(path,False)
    require(path.stat().st_size==entry["bytes"] and digest(path)==entry["sha256"],"original ZIP identity differs")
    result={}
    with zipfile.ZipFile(path) as archive:
        infos=archive.infolist()
        require(0<len(infos)<=20000,"empty or unbounded ZIP")
        for info in infos:
            name=PurePosixPath(info.filename)
            require(not name.is_absolute() and str(name)==info.filename and ".." not in name.parts and
                    "\\" not in info.filename and "\x00" not in info.filename,"unsafe ZIP member")
            require(not info.is_dir() and stat.S_ISREG(info.external_attr>>16) and not info.flag_bits&1 and
                    info.filename not in result,"nonregular, encrypted or duplicate ZIP member")
            require(info.file_size>=0,"negative member size")
            count,crc,h=0,0,hashlib.sha256()
            with archive.open(info) as stream:
                for block in iter(lambda:stream.read(1024**2),b""):
                    count+=len(block)
                    require(count<=info.file_size,"expanded ZIP exceeds declared bytes")
                    crc=zlib.crc32(block,crc);h.update(block)
            require(count==info.file_size and crc&0xffffffff==info.CRC,"ZIP CRC/size differs")
            result[info.filename]={"bytes":count,"sha256":h.hexdigest()}
        for name in result:
            require(not any(str(p) in result for p in PurePosixPath(name).parents if str(p)!="."),
                    "ZIP file overlaps parent")
    return result

def expand(path, target, members, original):
    require(not target.exists(),"refuse existing expansion")
    target.mkdir()
    with zipfile.ZipFile(path) as archive:
        for name in members:
            output=target/name
            output.parent.mkdir(parents=True,exist_ok=True)
            with archive.open(name) as source,output.open("xb") as destination:
                shutil.copyfileobj(source,destination,1024**2)
    require(original.inventory(target)==members,"full actual Path expansion differs")
    return target

def capacity_review(directory, scale, observer, matrix, github_job):
    """Check the separately retained observer receipts; no SDK commands run."""
    receipt = matrix.read_json(directory / "receipt.json")
    require(receipt.get("profile") == matrix.CAPACITY_PROFILE and receipt.get("scale") == scale and
            receipt.get("status") == "capacity_available" and receipt.get("exit_code") == 0 and
            receipt.get("artifact_state") == "sealed_after_commands_finished" and
            receipt.get("source_commit") == SOURCE and receipt.get("observer_sha256") == observer,
            "capacity receipt did not complete at a23")
    require(receipt.get("files") == matrix.inventory(directory, ("receipt.json",)), "capacity files changed after sealing")
    context = receipt.get("context", {})
    for key, value in {"GITHUB_REPOSITORY": "jyqj/codecortex", "GITHUB_RUN_ID": str(RUN),
                       "GITHUB_RUN_ATTEMPT": str(ATTEMPT), "GITHUB_JOB": github_job, "RUNNER_OS": "Linux",
                       "RUNNER_ENVIRONMENT": "github-hosted", "ImageOS": "ubuntu24", "P8_EXPECTED_SOURCE": SOURCE}.items():
        require(context.get(key) == value, "capacity context mismatch: " + key)
    require(isinstance(context.get("RUNNER_NAME"), str) and context["RUNNER_NAME"] and
            isinstance(context.get("ImageVersion"), str) and context["ImageVersion"], "capacity host/image missing")
    need = scale * 256 * 1024 + 4 * 1024**3
    require(receipt.get("required_free_bytes") == need and receipt.get("allow_preparation") is True,
            "capacity requirement/invocation changed")
    for state in (receipt.get("initial"), receipt.get("final")):
        require(isinstance(state, dict) and all(type(state.get(key)) is int and state[key] >= 0
                for key in ("total_bytes", "used_bytes", "free_bytes")), "invalid capacity disk observation")
    require(receipt["final"]["free_bytes"] >= need, "registered capacity unavailable")
    steps = receipt.get("steps")
    require(isinstance(steps, list) and len(steps) <= 2, "invalid capacity step list")
    for index, step in enumerate(steps):
        require(step.get("target") == SDK_PATHS[index] and type(step.get("existed")) is bool,
                "unregistered capacity target/order")
        if not step["existed"]:
            require("directory_size" not in step and "removal" not in step, "nonexistent SDK has command")
            continue
        for field, operation, suffix in (("directory_size", ["du", "-sx", "--block-size=1", "--", SDK_PATHS[index]], "size"),
                                         ("removal", ["rm", "-rf", "--one-file-system", "--preserve-root=all", "--", SDK_PATHS[index]], "remove")):
            command = step[field]
            label = f"sdk-{index}-{suffix}"
            require(command == matrix.read_json(directory / (label + ".command.json")), "capacity command differs from original receipt")
            require(command.get("operation") == operation and command.get("argv") ==
                    ["sudo", "-n", "timeout", "--signal=TERM", "--kill-after=5s", "300s", *operation] and
                    command.get("exit_code") == 0 and command.get("status") == "completed" and
                    command.get("cleanup_complete") is True and command.get("stdout") == label + ".stdout" and
                    command.get("stderr") == label + ".stderr", "capacity command failed/unregistered/unsealed")
            require((directory / command["stdout"]).is_file() and (directory / command["stderr"]).is_file(), "missing capacity command output")
    return {"scale": scale, "receipt_sha256": digest(directory / "receipt.json"), "context": context,
            "required_free_bytes": need, "final_free_bytes": receipt["final"]["free_bytes"], "status": "passed"}


def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def file_row(path):
    return {"bytes": path.stat().st_size, "sha256": digest(path)}

def safe_error(error):
    # No argv, stderr, headers, signed URLs, or exception message is published.
    frames = []
    tb = error.__traceback__
    while tb:
        frames.append({"file": Path(tb.tb_frame.f_code.co_filename).name,
                       "function": tb.tb_frame.f_code.co_name, "line": tb.tb_lineno})
        tb = tb.tb_next
    return {"type": type(error).__name__, "frames": frames}

def inventory(root):
    rows = {}
    for path in sorted(root.rglob("*")):
        require(not path.is_symlink(), "symlink in retained evidence")
        if path.is_file():
            regular_tree_path(path, False)
            rows[path.relative_to(root).as_posix()] = file_row(path)
        else:
            require(path.is_dir(), "nonregular retained evidence")
    return rows

def source_identity(root, matrix):
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == SOURCE,
            "wrong actual source HEAD")
    snap = matrix.source_snapshot(root)
    require(snap["source_commit"] == SOURCE and snap["source_tree"] == TREE and
            snap["input_count"] == 1087 and snap["manifest_sha256"] == MANIFEST,
            "wrong complete original source inventory")
    driver = matrix.driver_snapshot(root)
    observed = {}
    for relative, wanted in OBSERVERS.items():
        path = root / relative
        regular_tree_path(path, False)
        body = path.read_bytes()
        committed = subprocess.check_output(["git", "show", SOURCE + ":" + relative], cwd=root)
        require(body == committed and hashlib.sha256(body).hexdigest() == wanted, "original observer differs")
        observed[relative] = file_row(path)
    return {"source": snap, "driver": driver, "observers": observed}

def gh_to_file(endpoint, target, timeout, limit=16 * 1024**2):
    require(re.fullmatch(r"repos/jyqj/codecortex/actions/[A-Za-z0-9_/?=&-]+", endpoint),
            "unregistered API endpoint")
    require(not target.exists(), "refuse existing transport output")
    require(type(limit) is int and limit > 0, "invalid transport byte bound")
    require(shutil.disk_usage(target.parent).free >= limit + HEADROOM + OUTPUT_ALLOWANCE, "bounded transfer would consume reserve")
    started = time.monotonic_ns()
    def child_limits():
        resource.setrlimit(resource.RLIMIT_FSIZE, (limit, limit))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    # The token stays in this step's environment, never in argv or evidence.
    try:
        with target.open("xb") as stream:
            result = subprocess.run(["gh", "api", endpoint], stdin=subprocess.DEVNULL,
                                    stdout=stream, stderr=subprocess.DEVNULL,
                                    timeout=timeout, check=False, preexec_fn=child_limits)
    except (OSError, subprocess.TimeoutExpired):
        raise RuntimeError("API transfer failed; partial bytes retained") from None
    require(result.returncode == 0, "API transfer failed; partial bytes retained")
    return {"endpoint": endpoint, "exit_code": result.returncode,
            "elapsed_ns": time.monotonic_ns() - started, "output_limit_bytes": limit, **file_row(target)}

def api_json(endpoint, target):
    receipt = gh_to_file(endpoint, target, 180)
    require(target.stat().st_size <= 16 * 1024**2, "API JSON size budget")
    save_new(target.with_suffix(".transfer.json"), receipt)
    return json_load(target)

def pages(kind, target):
    rows = []
    count = None
    for page in range(1, 12):
        suffix = "&filter=all" if kind == "jobs" else ""
        data = api_json(f"repos/{REPOSITORY}/actions/runs/{RUN}/{kind}?per_page=100&page={page}" + suffix,
                        target / f"{kind}-{page}.json")
        current = data["total_count"]
        require(type(current) is int and 0 <= current <= 1000, "API population bound")
        if count is None:
            count = current
        require(count == current and len(data[kind]) <= 100, "API pagination changed")
        rows.extend(data[kind])
        if len(rows) >= count:
            require(len(rows) == count and len({x["id"] for x in rows}) == count,
                    "incomplete or duplicate API pages")
            return rows
    raise ValueError("incomplete API pagination")

def metadata_snapshot(target):
    target.mkdir()
    run = api_json(f"repos/{REPOSITORY}/actions/runs/{RUN}", target / "run.json")
    jobs, artifacts = pages("jobs", target), pages("artifacts", target)
    return {"run": run, "jobs": jobs, "artifacts": artifacts}

def expected_entries():
    expected = {f"p8-scale-build-{RUN}": {"role": "build", "job_name": "build", "github_job": "build"}}
    for scale in SCALES:
        for index in range(30):
            job = "preflight" if index == 0 else "measure"
            for role in ("capacity", "shard"):
                expected[f"p8-scale-{role}-{scale}-{index}-{RUN}"] = {
                    "role": role, "scale": scale, "index": index,
                    "job_name": f"{job} ({scale}, {index})", "github_job": job}
    return expected

def register(snapshot):
    run, jobs, artifacts = snapshot["run"], snapshot["jobs"], snapshot["artifacts"]
    require(run["id"] == RUN and run["head_sha"] == SOURCE and run["run_attempt"] == ATTEMPT and
            run["event"] == "pull_request" and run["path"] == ".github/workflows/p8-scale.yml" and
            run["status"] == "completed", "original source/attempt/run is not fixed and terminal")
    expected = expected_entries()
    matrix_name = f"p8-scale-matrix-{RUN}"
    by_name = {x["name"]: x for x in artifacts}
    require(len(by_name) == len(artifacts) and set(expected) <= set(by_name) and
            set(by_name) <= set(expected) | {matrix_name}, "original 301-artifact population incomplete or duplicate")
    jobs_by_name = {j["name"]: j for j in jobs}
    required_jobs = {v["job_name"] for v in expected.values()}
    require(len(jobs_by_name) == len(jobs) and required_jobs <= set(jobs_by_name) and
            set(jobs_by_name) <= required_jobs | {"aggregate"}, "original producer job topology differs")
    for job in jobs:
        require(job["run_id"] == RUN and job["head_sha"] == SOURCE and job["run_attempt"] == ATTEMPT and
                job["status"] == "completed", "original job source/attempt/terminal state differs")
    if matrix_name in by_name:
        require("aggregate" in jobs_by_name, "matrix has no original producer job")
        expected[matrix_name] = {"role": "matrix", "job_name": "aggregate", "github_job": "aggregate"}
    entries = []
    for name, row in sorted(expected.items()):
        meta = by_name[name]
        require(meta["workflow_run"]["id"] == RUN and meta["workflow_run"]["head_sha"] == SOURCE and
                meta.get("expired") is False and type(meta["id"]) is int and meta["id"] > 0 and type(meta["size_in_bytes"]) is int and
                0 < meta["size_in_bytes"] <= MAX_ZIP and
                re.fullmatch(r"sha256:[0-9a-f]{64}", meta["digest"]), "artifact metadata identity differs")
        item = {**row, "name": name, "artifact_id": meta["id"], "bytes": meta["size_in_bytes"],
                "sha256": meta["digest"][7:], "job_id": jobs_by_name[row["job_name"]]["id"]}
        if name in KNOWN_ARTIFACTS:
            known = KNOWN_ARTIFACTS[name]
            require((item["artifact_id"], item["bytes"], item["sha256"]) ==
                    (known["id"], known["bytes"], known["sha256"]), "previously observed original artifact changed")
        entries.append(item)
    require(len({e["artifact_id"] for e in entries}) == len(entries), "duplicate artifact identity")
    return {"schema": 1, "run_id": RUN, "attempt": ATTEMPT, "source": SOURCE,
            "source_tree": TREE, "source_manifest": MANIFEST, "artifacts": entries,
            "remote_run_conclusion": run["conclusion"],
            "remote_jobs": [{"id": j["id"], "name": j["name"], "status": j["status"],
                             "conclusion": j["conclusion"]} for j in jobs],
            "remote_matrix_present": matrix_name in by_name,
            "remote_aggregate_job_present": "aggregate" in jobs_by_name,
            "topology": {"required_producer_jobs": 151, "full_topology_with_aggregate": 152,
                         "observed_jobs": len(jobs), "required_artifacts": 301,
                         "observed_artifacts": len(entries)},
            "remote_job_green_is_not_measurement_qualification": True}

def download(out, private):
    metadata = out / "metadata"
    metadata.mkdir()
    require(os.environ.get("GH_TOKEN"), "download credentials absent")
    before = metadata_snapshot(metadata / "before")
    # This exact raw snapshot survives a refusal; no future IDs are guessed.
    registration = register(before)
    save_new(out / "registration.json", registration)
    originals = out / "originals"
    originals.mkdir()
    jobs_dir = private / "original-job-logs"
    jobs_dir.mkdir()
    need = sum(e["bytes"] for e in registration["artifacts"])
    require(shutil.disk_usage(out).free >= need + HEADROOM + OUTPUT_ALLOWANCE, "ZIP download would consume reserve")
    artifacts = []
    for entry in registration["artifacts"]:
        path = originals / (str(entry["artifact_id"]) + ".zip")
        require(shutil.disk_usage(out).free >= entry["bytes"] + HEADROOM + OUTPUT_ALLOWANCE,
                "next ZIP would consume reserve")
        transfer = gh_to_file(f"repos/{REPOSITORY}/actions/artifacts/{entry['artifact_id']}/zip", path, 1800, entry["bytes"])
        require(path.stat().st_size == entry["bytes"] and digest(path) == entry["sha256"], "original ZIP digest/size differs")
        with zipfile.ZipFile(path) as archive:
            require(sum(info.file_size for info in archive.infolist()) <= MAX_EXPANDED_PER_ZIP,
                    "declared ZIP expanded transport budget")
        members = zip_inventory(path, entry)
        require(sum(v["bytes"] for v in members.values()) <= MAX_EXPANDED_PER_ZIP,
                "ZIP expanded transport budget")
        proof = {"entry": entry, "transfer": transfer, "members": members,
                 "expanded_bytes": sum(v["bytes"] for v in members.values())}
        save_new(originals / (str(entry["artifact_id"]) + ".inventory.json"), proof)
        artifacts.append(proof)
        print(json.dumps({"stage": "original_zip_verified", "id": entry["artifact_id"],
                          "role": entry["role"], "members": len(members)}), flush=True)
    withheld = []
    # Raw logs can contain signed storage URLs. Retain on the VM, never upload
    # or print them. Unavailable logs remain an explicit custody limitation.
    for job in before["jobs"]:
        path = jobs_dir / (str(job["id"]) + ".log")
        require(shutil.disk_usage(out).free >= HEADROOM + OUTPUT_ALLOWANCE, "job-log transfer would consume reserve")
        try:
            receipt = gh_to_file(f"repos/{REPOSITORY}/actions/jobs/{job['id']}/logs", path, 180, 32 * 1024**2)
            withheld.append({"job_id": job["id"], "status": "received", **receipt,
                             "included_in_upload": False, "provider_log_required_for_full_log_replay": True})
        except Exception as error:
            withheld.append({"job_id": job["id"], "status": "unavailable_or_partial",
                             "error": safe_error(error), "partial_file": file_row(path) if path.exists() else None,
                             "included_in_upload": False})
    save_new(out / "withheld-original-job-logs.json", withheld)
    after = metadata_snapshot(metadata / "after")
    require(register(after) == registration, "terminal original registration changed during reception")
    save_new(out / "download-complete.json", {"status": "passed", "registration_sha256": digest(out / "registration.json"),
             "artifacts": len(artifacts), "zip_bytes": sum(e["bytes"] for e in registration["artifacts"]),
             "expanded_bytes": sum(x["expanded_bytes"] for x in artifacts),
             "all_zip_members_crc_size_sha256_checked": True,
             "job_logs_all_received": all(x["status"] == "received" for x in withheld)})
    return {"status": "transport_complete_not_measurement_qualified", "artifacts": len(artifacts)}

def canonical_matrix(value, directories):
    require(type(value) is dict and type(value.get("evidence")) is list and len(value["evidence"]) == 150,
            "matrix evidence population differs")
    value = json.loads(json.dumps(value))
    seen, relocations = set(), []
    for row in value["evidence"]:
        path = PurePosixPath(row["directory"])
        require(path.is_absolute() and ".." not in path.parts and path.name in directories and
                path.name not in seen and set(row) == {"directory", "receipt_sha256"},
                "matrix evidence directory is not an exact registered artifact")
        seen.add(path.name)
        replacement = str(directories[path.name])
        relocations.append({"original": row["directory"], "replay": replacement,
                            "receipt_sha256": row["receipt_sha256"]})
        row["directory"] = replacement
    require(seen == set(directories), "matrix evidence path population differs")
    return value, relocations


def saved_pages(directory, kind):
    """Read only numbered raw API pages, never their *.transfer.json receipts."""
    numbered = []
    for path in directory.iterdir():
        match = re.fullmatch(re.escape(kind) + r"-([1-9][0-9]*)\.json", path.name)
        if match:
            numbered.append((int(match.group(1)), path))
    numbered.sort()
    require(numbered and [number for number, _ in numbered] == list(range(1, len(numbered) + 1)),
            "saved API pages are missing or noncontiguous")
    rows, total = [], None
    for number, path in numbered:
        data = json_load(path)
        count = data["total_count"]
        require(type(count) is int and 0 <= count <= 1000, "saved API population bound")
        if total is None:
            total = count
        require(count == total and type(data[kind]) is list and
                len(data[kind]) == max(0, min(100, total - (number - 1) * 100)),
                "saved API page population changed")
        rows.extend(data[kind])
    require(len(numbered) == max(1, (total + 99) // 100) and len(rows) == total and
            len({row["id"] for row in rows}) == total, "saved API pagination is incomplete or duplicated")
    return rows

def replay(out, root, matrix):
    require(not any(os.environ.get(k) for k in ("GH_TOKEN", "GITHUB_TOKEN", "ACTIONS_RUNTIME_TOKEN")),
            "replay step received transport credentials")
    require(json_load(out / "download-complete.json")["status"] == "passed", "original transport did not finish")
    registration = json_load(out / "registration.json")
    before_api = {"run": json_load(out / "metadata/before/run.json"), "jobs": [], "artifacts": []}
    for kind in ("jobs", "artifacts"):
        before_api[kind] = saved_pages(out / "metadata/before", kind)
    require(register(before_api) == registration, "saved registration is not original API metadata")
    review = out / "review"
    original_dir = out / "expanded"
    original_dir.mkdir()
    proofs = {}
    for entry in registration["artifacts"]:
        artifact = entry["artifact_id"]
        proof = json_load(out / "originals" / (str(artifact) + ".inventory.json"))
        require(proof["entry"] == entry, "artifact proof identity differs")
        archive = out / "originals" / (str(artifact) + ".zip")
        require(zip_inventory(archive, entry) == proof["members"], "original ZIP/member inventory changed")
        proofs[entry["name"]] = proof
    required = sum(p["expanded_bytes"] for p in proofs.values())
    require(shutil.disk_usage(out).free >= required + HEADROOM + OUTPUT_ALLOWANCE,
            "complete actual expansion would consume reserve")
    directories = {}
    for entry in registration["artifacts"]:
        name = entry["name"]
        archive = out / "originals" / (str(entry["artifact_id"]) + ".zip")
        members = proofs[name]["members"]
        require(sum(v["bytes"] for v in members.values()) == proofs[name]["expanded_bytes"],
                "expanded-byte inventory differs")
        require(shutil.disk_usage(out).free >= proofs[name]["expanded_bytes"] + HEADROOM + OUTPUT_ALLOWANCE,
                "next actual expansion would consume reserve")
        directories[name] = expand(archive, original_dir / name, members, matrix)
    build = directories[f"p8-scale-build-{RUN}"]
    (build / "p8-scale").chmod(0o755)  # New copy only; no original byte change.
    call = {"function": "p8_scale_matrix.validate_build", "arguments": {"build_directory": str(build), "root": str(root)},
            "started_utc": stamp(), "source": SOURCE, "status": "started"}
    save_new(review / "validate-build-command.json", call)
    built, binary = matrix.validate_build(build, root=root)
    require(built["source_manifest_sha256"] == MANIFEST, "original build source manifest differs")
    save_new(review / "validate-build-result.json", {"passed": True, "completed_utc": stamp(),
             "build_receipt_sha256": digest(build / "build.json"), "binary_sha256": built["binary_sha256"],
             "binary_blake3": built["binary_blake3"]})
    capacities = []
    for entry in registration["artifacts"]:
        if entry["role"] == "capacity":
            result = capacity_review(directories[entry["name"]], entry["scale"],
                                     OBSERVERS["scripts/p8_runner_capacity.py"], matrix, entry["github_job"])
            result.update(index=entry["index"], artifact_id=entry["artifact_id"], job_id=entry["job_id"])
            capacities.append(result)
    require(len(capacities) == 150, "capacity population differs")
    save_new(review / "capacity-results.json", capacities)
    shards = {e["name"]: directories[e["name"]] for e in registration["artifacts"] if e["role"] == "shard"}
    require(len(shards) == 150, "shard population differs")
    args = {"build_directory": str(build), "directories": [str(shards[n]) for n in sorted(shards)],
            "output": str(review / "independent-matrix"), "repetitions": 30, "shard_count": 30,
            "capacity_profile": matrix.CAPACITY_PROFILE}
    save_new(review / "aggregate-command.json", {"function": "p8_scale_matrix.aggregate",
             "arguments": args, "started_utc": stamp(), "scope": "original unmodified whole-population offline replay"})
    started = time.monotonic_ns()
    with (review / "aggregate.stdout").open("x") as stdout, (review / "aggregate.stderr").open("x") as stderr:
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = matrix.aggregate(build, [shards[n] for n in sorted(shards)], review / "independent-matrix",
                                      repetitions=30, shard_count=30, capacity_profile=matrix.CAPACITY_PROFILE)
    save_new(review / "aggregate-result.json", {"completed_utc": stamp(), "elapsed_ns": time.monotonic_ns() - started,
             "returned_passed": result.get("passed"), "status": result.get("status"),
             "matrix_sha256": digest(review / "independent-matrix/matrix.json")})
    # Original aggregate itself rejects any failed/missing/duplicate shard and
    # owns all population/statistical predicates; no successes-only filtering.
    require(result.get("passed") is True and result.get("status") == "complete_measurement_coverage" and
            result["sample_count"] == 1500 and len(result["groups"]) == 50 and
            all(g["n"] == 30 for g in result["groups"]) and len(result["evidence"]) == 150,
            "original full-population aggregate did not pass")
    remote = {"present": registration["remote_matrix_present"], "run_conclusion": registration["remote_run_conclusion"],
              "actions_full_success_claimed": False}
    matrix_name = f"p8-scale-matrix-{RUN}"
    if matrix_name in directories:
        remote_directory = directories[matrix_name]
        require(set(matrix.inventory(remote_directory)) == {"matrix.json"}, "unexpected remote matrix member population")
        original_matrix = matrix.read_json(remote_directory / "matrix.json")
        remote.update(original_matrix_sha256=digest(remote_directory / "matrix.json"),
                      original_status=original_matrix.get("status"), original_passed=original_matrix.get("passed"))
        if original_matrix.get("passed") is True:
            normalized, relocations = canonical_matrix(original_matrix, shards)
            require(matrix.exact_equal(normalized, result), "successful remote matrix differs beyond directory relocation")
            save_new(review / "remote-matrix-directory-relocations.json", relocations)
            remote["successful_remote_matrix_equal_after_directory_relocation"] = True
        else:
            remote["failed_remote_matrix_retained_unchanged"] = True
    else:
        remote["scope"] = "remote matrix not delivered; original terminal API preserved; independent original aggregate owns qualification"
    save_new(review / "remote-matrix-comparison.json", remote)
    for entry in registration["artifacts"]:
        name = entry["name"]
        require(matrix.inventory(directories[name]) == proofs[name]["members"], "expanded original bytes changed")
        archive = out / "originals" / (str(entry["artifact_id"]) + ".zip")
        require(file_row(archive) == {"bytes": entry["bytes"], "sha256": entry["sha256"]}, "retained original ZIP changed")
    return {"status": "accepted_original_a23_full_measurement_coverage",
            "source": SOURCE, "primary_shards": 150, "groups": 50, "samples": 1500,
            "remote_outcome": remote, "native_scope": "original --hash-file only",
            "release_certification": "not_run", "task_statuses_changed": False}

def main():
    require(not sys.flags.optimize, "optimized Python is forbidden")
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("download", "replay"))
    parser.add_argument("--source-root", required=True, type=Path)
    args = parser.parse_args()
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY and os.environ.get("GITHUB_EVENT_NAME") == "push" and
            os.environ.get("GITHUB_REF") == "refs/heads/" + BRANCH and os.environ.get("GITHUB_RUN_ATTEMPT") == "1" and
            os.environ.get("RUNNER_OS") == "Linux" and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted" and
            os.environ.get("ImageOS") == "ubuntu24", "first-push hosted offline controller required")
    root = args.source_root.resolve(strict=True)
    regular_tree_path(root)
    temp = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
    regular_tree_path(temp)
    out, private = temp / "a23-scale-intake", temp / "a23-scale-intake-private"
    if args.mode == "download":
        out.mkdir()
        private.mkdir()
        (out / "review").mkdir()
    else:
        regular_tree_path(out)
        regular_tree_path(private)
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(root / "scripts"))
    import p8_scale_matrix as matrix
    review = out / "review"
    script = Path(__file__).resolve(strict=True)
    script_before = digest(script)
    before = None
    report = {"schema": "a23-original-N150-reception-v1", "mode": args.mode, "started_utc": stamp(),
              "status": "failed_or_incomplete", "source": SOURCE, "source_tree": TREE, "original_run": RUN,
              "original_attempt": ATTEMPT, "controller_commit": os.environ["GITHUB_SHA"],
              "controller_run": os.environ["GITHUB_RUN_ID"], "controller_attempt": 1,
              "controller_sha256": script_before, "new_measurements": 0, "TODO_closed": 0, "TODO_remaining": 29}
    okay = False
    try:
        require(shutil.disk_usage(out).free >= HEADROOM + OUTPUT_ALLOWANCE, "initial reception reserve unavailable")
        if args.mode == "download":
            with (review / "loaded-controller.py").open("xb") as stream:
                stream.write(script.read_bytes())
        else:
            require((review / "loaded-controller.py").read_bytes() == script.read_bytes(), "loaded controller body changed")
        before = source_identity(root, matrix)
        save_new(review / (args.mode + "-source-before.json"), before)
        report["original_functions"] = {name: hashlib.sha256(inspect.getsource(getattr(matrix, name)).encode()).hexdigest()
            for name in ("validate_build", "validate_shard", "inspect_raw", "combine", "aggregate", "native_digest")}
        report["result"] = download(out, private) if args.mode == "download" else replay(out, root, matrix)
        okay = True
    except BaseException as error:
        report["error"] = safe_error(error)
    finally:
        try:
            after = source_identity(root, matrix)
            save_new(review / (args.mode + "-source-after.json"), after)
            require(before == after and digest(script) == script_before, "source/controller changed")
            report["source_and_observer_unchanged"] = True
        except BaseException as error:
            okay = False
            report["final_source_error"] = safe_error(error)
        report["completed_utc"] = stamp()
        report["status"] = "passed" if okay else "failed_or_incomplete"
        report["uploaded_inventory"] = {prefix: inventory(out / prefix) for prefix in ("metadata", "originals", "review")
                                        if (out / prefix).exists()}
        report["custody"] = {"original_zips_and_members_preserved": "see per-artifact inventory",
                            "expanded_copies": "VM-only; exact originals reconstructible from retained ZIPs",
                            "raw_job_logs": "VM-only, hashes and explicit availability in withheld-original-job-logs.json",
                            "signed_urls_printed_or_uploaded": False}
        save_new(out / (args.mode + "-receipt.json"), report)
        print(json.dumps({"mode": args.mode, "status": report["status"], "receipt_sha256": digest(out / (args.mode + "-receipt.json")),
                          "TODO_closed": 0, "TODO_remaining": 29}), flush=True)
    return 0 if okay else 1

if __name__ == "__main__":
    raise SystemExit(main())

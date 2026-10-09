#!/usr/bin/env python3
"""DRAFT: retain fixed275e originals, then replay the unchanged full150 consumer.
No study trigger, Cargo, provider, product workload or automatic retry.
"""
import argparse, contextlib, datetime, hashlib, inspect, json, os, platform, re, resource, shutil, stat, subprocess, sys, time, zipfile, zlib
from pathlib import Path, PurePosixPath

SOURCE = "275e8799d4947d297329073eaa3ca675d3fd0777"
TREE = "5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46"
MANIFEST = "593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83"
RUN, ATTEMPT = 37896198208, 1
REPOSITORY = "jyqj/codecortex"
SOURCE_BRANCH = "task/p8-surface-window-controls-20261009"
REGISTRATION_BLOB = "ccc71f4d8c305a71b611d120d3a8dd6420b6dc2f"
BINDING_BLOB = "6509839e68ef2ae1bb6cb9b1ce9662899b6b31c8"
SCALES = (1000, 5000, 10000, 50000, 100000)
HEADROOM, OUTPUT_ALLOWANCE = 512 * 1024**2, 128 * 1024**2
MAX_ZIP, MAX_EXPANDED_PER_ZIP = 1024**3, 1024**3
SDK_PATHS = ["/usr/local/lib/android", "/usr/share/dotnet"]
PROJECT = Path("/Users/jin/Desktop/codecortex-rust")
SOURCE_ROOT = PROJECT / "artifacts/checkpoints/p8-round19-evidence-intake-20261009/platform275e-author/evidence/source"
OWNED_ROOT = PROJECT / "artifacts/checkpoints/p8-round20-formal275e-full-consumer"
REMOTE_SHARDS_ROOT = PurePosixPath("/home/runner/work/_temp/p8-scale-shards")
OBSERVERS = {
    "scripts/p8_scale_matrix.py": "f169b0f26cf2a87d3e2548d734431c530cbf63d353ac054045ec8be80889f957",
    "scripts/p7_build_identity.py": "5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5",
    "scripts/p8_runner_capacity.py": "2e37954df30b153ad82b49a5841a977451288c1730d03bfd2d0d7a2ace412939",
    ".github/workflows/p8-scale.yml": "4258476dd37d8af223aaa7ad45d7c97d7298be1ba4ec5ffd9af9949785ed8f3d",
}
KNOWN_ARTIFACTS = {
    "p8-scale-build-37896198208": {
        "id": 11600309282,
        "bytes": 9218801,
        "sha256": "9c13733e1bdb207301920f3dddb0bde4dcab278b31e4cdfd0dc8bc1f3c75ab67"
    },
    "p8-scale-shard-10000-0-37896198208": {
        "id": 11601336788,
        "bytes": 853841,
        "sha256": "33944d8b9c895b62064ce89dc5d99a476b78cbcc97c2f9a57ccf190571e20374"
    },
    "p8-scale-shard-1000-0-37896198208": {
        "id": 11601142045,
        "bytes": 952777,
        "sha256": "242eb240b06cccb6ffc6aa210a44a0985f969308661e7ad92b8451735c445171"
    },
    "p8-scale-capacity-5000-0-37896198208": {
        "id": 11601130609,
        "bytes": 786,
        "sha256": "568d505fe7da83f8df0679a2932405dbc5c042fe89d45fdd67066210097f45cf"
    },
    "p8-scale-capacity-100000-0-37896198208": {
        "id": 11601087740,
        "bytes": 786,
        "sha256": "7b830f3444bfa18a386775813730d9f8945f85a76ba30fae92eca8c448270311"
    },
    "p8-scale-shard-5000-0-37896198208": {
        "id": 11600946382,
        "bytes": 583828,
        "sha256": "7d994e6d922d477ee07c5bb7ea478f180d927b7c108a965bff675be54a52cbb2"
    },
    "p8-scale-capacity-50000-0-37896198208": {
        "id": 11600574581,
        "bytes": 785,
        "sha256": "abb0eebb51584c58f9ce243a229498acb7e5e4b51f554bb763b6e602e7f9ce24"
    },
    "p8-scale-capacity-10000-0-37896198208": {
        "id": 11600444517,
        "bytes": 785,
        "sha256": "4fd49d2682ec5279689c4dd47bdf353881e407b869b92d9978b6f5b9504ea66e"
    },
    "p8-scale-capacity-1000-0-37896198208": {
        "id": 11600229155,
        "bytes": 783,
        "sha256": "65c283361ecf9f6b1af395cd2fb51c2c14e8f5d74a2bd94110056ba314e623be"
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

def capacity_review(directory, scale, observer, matrix, github_job):
    """Check the separately retained observer receipts; no SDK commands run."""
    receipt = matrix.read_json(directory / "receipt.json")
    require(receipt.get("profile") == matrix.CAPACITY_PROFILE and receipt.get("scale") == scale and
            receipt.get("status") == "capacity_available" and receipt.get("exit_code") == 0 and
            receipt.get("artifact_state") == "sealed_after_commands_finished" and
            receipt.get("source_commit") == SOURCE and receipt.get("observer_sha256") == observer,
            "capacity receipt did not complete at 275e")
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

def source_identity(root, matrix):
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == SOURCE,
            "wrong actual source HEAD")
    snap = matrix.source_snapshot(root)
    require(snap["source_commit"] == SOURCE and snap["source_tree"] == TREE and
            snap["input_count"] == 1089 and snap["manifest_sha256"] == MANIFEST,
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

def register(snapshot):
    """Describe every delivered original; missing entries never become samples."""
    run, jobs, artifacts = snapshot["run"], snapshot["jobs"], snapshot["artifacts"]
    require(run["id"] == RUN and run["head_sha"] == SOURCE and run["run_attempt"] == ATTEMPT and
            run["event"] == "pull_request" and run["path"] == ".github/workflows/p8-scale.yml" and
            run["workflow_id"] == 378814687 and run["head_branch"] == SOURCE_BRANCH and
            run["created_at"] == "2026-10-09T06:56:25Z", "different registered original run")
    expected = expected_entries()
    matrix_name = f"p8-scale-matrix-{RUN}"
    by_name = {x["name"]: x for x in artifacts}
    require(len(by_name) == len(artifacts) and set(by_name) <= set(expected) | {matrix_name},
            "duplicate or unexpected original artifact")
    jobs_by_name = {j["name"]: j for j in jobs}
    required_jobs = {v["job_name"] for v in expected.values()}
    require(len(jobs_by_name) == len(jobs) and set(jobs_by_name) <= required_jobs | {"aggregate", "preflight", "measure"},
            "duplicate or unexpected original job topology")
    for job in jobs:
        require(job["run_id"] == RUN and job["head_sha"] == SOURCE and job["run_attempt"] == ATTEMPT,
                "original job source/attempt differs")
    missing = sorted(set(expected) - set(by_name))
    if matrix_name in by_name:
        expected[matrix_name] = {"role": "matrix", "job_name": "aggregate", "github_job": "aggregate"}
    entries = []
    for name in sorted(by_name):
        row, meta = expected[name], by_name[name]
        require(row["job_name"] in jobs_by_name, "original artifact lacks its actual producer job")
        require(meta["workflow_run"]["id"] == RUN and meta["workflow_run"]["head_sha"] == SOURCE and
                meta["workflow_run"]["head_branch"] == SOURCE_BRANCH and meta.get("expired") is False and
                type(meta["id"]) is int and meta["id"] > 0 and type(meta["size_in_bytes"]) is int and
                0 < meta["size_in_bytes"] <= MAX_ZIP and re.fullmatch(r"sha256:[0-9a-f]{64}", meta["digest"]),
                "artifact metadata identity differs")
        item = {**row, "name": name, "artifact_id": meta["id"], "bytes": meta["size_in_bytes"],
                "sha256": meta["digest"][7:], "job_id": jobs_by_name[row["job_name"]]["id"]}
        if name in KNOWN_ARTIFACTS:
            known = KNOWN_ARTIFACTS[name]
            require((item["artifact_id"], item["bytes"], item["sha256"]) ==
                    (known["id"], known["bytes"], known["sha256"]), "previously received original artifact changed")
        entries.append(item)
    require(len({e["artifact_id"] for e in entries}) == len(entries), "duplicate artifact identity")
    return {"schema": 1, "run_id": RUN, "attempt": ATTEMPT, "source": SOURCE,
            "source_tree": TREE, "source_manifest": MANIFEST, "artifacts": entries,
            "missing_core_artifacts": missing, "complete_core_301": not missing,
            "remote_run_status": run["status"], "remote_run_conclusion": run["conclusion"],
            "remote_jobs": [{"id": j["id"], "name": j["name"], "status": j["status"],
                             "conclusion": j["conclusion"]} for j in jobs],
            "remote_matrix_present": matrix_name in by_name,
            "remote_aggregate_job_present": "aggregate" in jobs_by_name,
            "topology": {"required_producer_jobs": 151, "observed_jobs": len(jobs),
                         "required_core_artifacts": 301, "observed_artifacts": len(entries)},
            "remote_job_green_is_not_measurement_qualification": True}


def selected_entries_unchanged(before, after):
    newer = {entry["name"]: entry for entry in after["artifacts"]}
    require(all(newer.get(entry["name"]) == entry for entry in before["artifacts"]),
            "a selected original artifact/producer identity changed")
    return sorted(set(newer) - {entry["name"] for entry in before["artifacts"]})


def prepare(out, matrix):
    metadata = out / "metadata"
    metadata.mkdir()
    before = metadata_snapshot(metadata / "before")
    registration = register(before)
    save_new(out / "registration.json", registration)
    originals, expanded = out / "originals", out / "expanded"
    originals.mkdir()
    expanded.mkdir()
    need = sum(e["bytes"] for e in registration["artifacts"])
    require(shutil.disk_usage(out).free >= need + HEADROOM + OUTPUT_ALLOWANCE,
            "all selected ZIPs would consume reserve")
    proofs = []
    for entry in registration["artifacts"]:
        archive = originals / (str(entry["artifact_id"]) + ".zip")
        transfer = gh_to_file(f"repos/{REPOSITORY}/actions/artifacts/{entry['artifact_id']}/zip",
                              archive, 1800, entry["bytes"])
        require(file_row(archive) == {"bytes": entry["bytes"], "sha256": entry["sha256"]},
                "original ZIP bytes differ")
        with zipfile.ZipFile(archive) as package:
            require(sum(info.file_size for info in package.infolist()) <= MAX_EXPANDED_PER_ZIP,
                    "ZIP declares excessive expansion")
        members = zip_inventory(archive, entry)
        expanded_bytes = sum(value["bytes"] for value in members.values())
        require(expanded_bytes <= MAX_EXPANDED_PER_ZIP, "ZIP expansion bound")
        proof = {"entry": entry, "transfer": transfer, "members": members, "expanded_bytes": expanded_bytes}
        save_new(originals / (str(entry["artifact_id"]) + ".inventory.json"), proof)
        proofs.append(proof)
        print(json.dumps({"stage": "original_zip_verified", "id": entry["artifact_id"],
                          "role": entry["role"], "members": len(members)}), flush=True)
    require(shutil.disk_usage(out).free >= sum(p["expanded_bytes"] for p in proofs) + HEADROOM + OUTPUT_ALLOWANCE,
            "all selected expansions would consume reserve")
    for proof in proofs:
        entry = proof["entry"]
        require(shutil.disk_usage(out).free >= proof["expanded_bytes"] + HEADROOM + OUTPUT_ALLOWANCE,
                "next expansion would consume reserve")
        target = expand(originals / (str(entry["artifact_id"]) + ".zip"),
                        expanded / entry["name"], proof["members"], matrix)
        if entry["role"] == "build" and (target / "p8-scale").is_file():
            (target / "p8-scale").chmod(0o755)  # Only this newly created byte-identical copy.
        require(matrix.inventory(target) == proof["members"], "expanded bytes changed")
    after = metadata_snapshot(metadata / "after")
    after_registration = register(after)
    original_bytes(out, registration, matrix, full_crc=False)
    new_names = selected_entries_unchanged(registration, after_registration)
    save_new(out / "after-registration.json", after_registration)
    save_new(out / "transport-complete.json", {
        "status": "transport_complete_full_core" if registration["complete_core_301"] else "transport_complete_partial_originals",
        "registration_sha256": digest(out / "registration.json"),
        "artifacts": len(proofs), "zip_bytes": need,
        "expanded_bytes": sum(p["expanded_bytes"] for p in proofs),
        "complete_core_301": registration["complete_core_301"],
        "new_artifacts_observed_after_selection_not_imported": new_names,
        "all_zip_members_crc_size_sha256_checked": True,
        "original_job_logs": "not downloaded; complete original job/step API retained; provider logs remain separate",
        "measurement_qualification": "not_run"})
    return {"status": "transport_complete_not_measurement_qualified",
            "complete_core_301": registration["complete_core_301"], "artifacts": len(proofs)}


def saved_registration(out):
    before = {"run": json_load(out / "metadata/before/run.json"),
              "jobs": saved_pages(out / "metadata/before", "jobs"),
              "artifacts": saved_pages(out / "metadata/before", "artifacts")}
    after = {"run": json_load(out / "metadata/after/run.json"),
             "jobs": saved_pages(out / "metadata/after", "jobs"),
             "artifacts": saved_pages(out / "metadata/after", "artifacts")}
    registration, later = register(before), register(after)
    require(registration == json_load(out / "registration.json") and
            later == json_load(out / "after-registration.json"), "saved API registration differs")
    newly_published = selected_entries_unchanged(registration, later)
    require(not registration["complete_core_301"] or not newly_published,
            "a published remote matrix is outside this fixed batch; preserve and receive it before final comparison")
    complete = json_load(out / "transport-complete.json")
    require(complete["registration_sha256"] == digest(out / "registration.json") and
            complete["complete_core_301"] is registration["complete_core_301"] and
            complete["status"] == ("transport_complete_full_core" if registration["complete_core_301"] else
                                  "transport_complete_partial_originals"), "transport did not complete")
    return registration


def original_bytes(out, registration, matrix, full_crc):
    originals, expanded = out / "originals", out / "expanded"
    expected_files, expected_dirs, proofs = set(), set(), {}
    for entry in registration["artifacts"]:
        artifact = entry["artifact_id"]
        archive = originals / (str(artifact) + ".zip")
        sidecar = originals / (str(artifact) + ".inventory.json")
        proof = json_load(sidecar)
        require(proof["entry"] == entry and sum(x["bytes"] for x in proof["members"].values()) == proof["expanded_bytes"],
                "original inventory identity changed")
        require(file_row(archive) == {"bytes": entry["bytes"], "sha256": entry["sha256"]},
                "original ZIP changed")
        if full_crc:
            require(zip_inventory(archive, entry) == proof["members"], "original full ZIP/member audit differs")
        require(matrix.inventory(expanded / entry["name"]) == proof["members"], "original expanded member closure differs")
        expected_files.update((archive.name, sidecar.name))
        expected_dirs.add(entry["name"])
        proofs[entry["name"]] = proof
    require({p.name for p in originals.iterdir()} == expected_files and
            all(p.is_file() and not p.is_symlink() for p in originals.iterdir()), "original ZIP inventory has extras/missing entries")
    require({p.name for p in expanded.iterdir()} == expected_dirs and
            all(p.is_dir() and not p.is_symlink() for p in expanded.iterdir()), "expanded original directory set differs")
    return proofs


def compare_remote_matrix(result, registration, directories, shards, matrix, review):
    remote = {"present": registration["remote_matrix_present"],
              "run_status": registration["remote_run_status"], "run_conclusion": registration["remote_run_conclusion"],
              "actions_full_success_claimed": False}
    name = f"p8-scale-matrix-{RUN}"
    if name not in directories:
        remote["scope"] = "original matrix absent in this fixed batch; complete raw original aggregate governs measurement coverage"
        return remote
    directory = directories[name]
    require(set(matrix.inventory(directory)) == {"matrix.json"}, "unexpected remote matrix members")
    raw = matrix.read_json(directory / "matrix.json", limit=16 * 1024**2)
    remote.update(original_matrix_sha256=digest(directory / "matrix.json"),
                  original_status=raw.get("status"), original_passed=raw.get("passed"))
    if raw.get("passed") is True:
        # The only registered relocation is the original Ubuntu RUNNER_TEMP artifact directory.
        require(all(row.get("directory") == str(REMOTE_SHARDS_ROOT / PurePosixPath(row["directory"]).name)
                    for row in raw.get("evidence", [])), "remote directory outside exact relocation map")
        normalized, relocations = canonical_matrix(raw, shards)
        require(matrix.exact_equal(normalized, result), "successful remote matrix differs beyond registered directory relocation")
        save_new(review / "remote-directory-relocations.json", relocations)
        remote["successful_remote_matrix_exact_equal"] = True
    else:
        remote["failed_remote_matrix_retained_unchanged"] = True
    return remote


def replay(out, root, matrix, review):
    require(platform.system() == "Linux" and platform.machine() == "x86_64", "retained ELF needs registered Linux amd64")
    require(not any(os.environ.get(k) for k in ("GH_TOKEN", "GITHUB_TOKEN", "ACTIONS_RUNTIME_TOKEN")),
            "offline replay received credentials")
    registration = saved_registration(out)
    require(registration["complete_core_301"] is True, "missing original core; no partial aggregate")
    before_inventory = inventory(out)
    proofs = original_bytes(out, registration, matrix, full_crc=True)
    directories = {entry["name"]: out / "expanded" / entry["name"] for entry in registration["artifacts"]}
    build = directories[f"p8-scale-build-{RUN}"]
    require(matrix.read_json(build / "source-before.json") == matrix.source_snapshot(root),
            "current complete source differs from original build")
    # Aggregate itself performs original validate_build once. Do not replay all150 twice.
    capacities = []
    capacity_error = None
    try:
        for entry in registration["artifacts"]:
            if entry["role"] == "capacity":
                row = capacity_review(directories[entry["name"]], entry["scale"],
                                      OBSERVERS["scripts/p8_runner_capacity.py"], matrix, entry["github_job"])
                row.update(index=entry["index"], artifact_id=entry["artifact_id"], job_id=entry["job_id"])
                capacities.append(row)
        require(len(capacities) == 150, "capacity population differs")
    except Exception as error:
        capacity_error = safe_error(error)
        raise
    finally:
        save_new(review / "capacity-results.json", {"results": capacities, "error": capacity_error,
                                                   "complete": len(capacities) == 150 and capacity_error is None})
    shards = {e["name"]: directories[e["name"]] for e in registration["artifacts"] if e["role"] == "shard"}
    require(len(shards) == 150, "shard population differs")
    args = {"build_directory": str(build), "directories": [str(shards[n]) for n in sorted(shards)],
            "output": str(review / "independent-matrix"), "repetitions": 30, "shard_count": 30,
            "capacity_profile": matrix.CAPACITY_PROFILE}
    save_new(review / "aggregate-command.json", {"function": "p8_scale_matrix.aggregate",
             "arguments": args, "started_utc": stamp(), "scope": "unchanged original whole-population replay"})
    started = time.monotonic_ns()
    with (review / "aggregate.stdout").open("x") as stdout, (review / "aggregate.stderr").open("x") as stderr:
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = matrix.aggregate(build, [shards[n] for n in sorted(shards)], review / "independent-matrix",
                                      repetitions=30, shard_count=30, capacity_profile=matrix.CAPACITY_PROFILE)
    save_new(review / "aggregate-result.json", {"completed_utc": stamp(), "elapsed_ns": time.monotonic_ns() - started,
             "returned_passed": result.get("passed"), "status": result.get("status"),
             "matrix_sha256": digest(review / "independent-matrix/matrix.json")})
    require(result.get("passed") is True and result.get("status") == "complete_measurement_coverage" and
            result["sample_count"] == 1500 and len(result["groups"]) == 50 and
            all(g["n"] == 30 for g in result["groups"]) and len(result["evidence"]) == 150,
            "original full-population aggregate did not pass")
    remote = compare_remote_matrix(result, registration, directories, shards, matrix, review)
    save_new(review / "remote-matrix-comparison.json", remote)
    original_bytes(out, registration, matrix, full_crc=False)
    require(inventory(out) == before_inventory, "original input tree changed")
    return {"status": "accepted_original_275e_full_measurement_coverage", "source": SOURCE,
            "primary_shards": 150, "capacity_receipts": 150, "groups": 50, "samples": 1500,
            "remote_outcome": remote, "native_scope": "original --hash-file only",
            "release_certification": "not_run", "task_statuses_changed": False}


def main():
    require(not sys.flags.optimize, "optimized Python forbidden")
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("prepare", "replay"))
    parser.add_argument("--evidence-root", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = SOURCE_ROOT
    regular_tree_path(root)
    out = args.evidence_root
    require(out.is_absolute() and out.parent.parent == OWNED_ROOT and
            re.fullmatch(r"batch-[0-9]{3}", out.parent.name) and out.name == "transport",
            "unregistered owned evidence directory")
    regular_tree_path(out.parent)
    if args.mode == "prepare":
        require(args.output is None and not out.exists(), "prepare must create a fresh transport directory")
        out.mkdir()
        review = out / "review"
        review.mkdir()
    else:
        regular_tree_path(out)
        require(args.output == Path("/output"), "unregistered replay output")
        review = args.output
        regular_tree_path(review)
        require(not any(review.iterdir()), "replay output must be fresh")
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(root / "scripts"))
    import p8_scale_matrix as matrix
    script = Path(__file__).resolve(strict=True)
    script_sha = digest(script)
    report = {"schema": "p8-275e-original-full150-consumer-v1", "mode": args.mode,
              "source": SOURCE, "tree": TREE, "run": RUN, "attempt": ATTEMPT,
              "registration_blob": REGISTRATION_BLOB, "binding_blob": BINDING_BLOB,
              "started_utc": stamp(), "controller_sha256": script_sha,
              "status": "failed_or_incomplete", "new_measurements": 0,
              "TODO_closed": 0, "TODO_remaining": 29}
    before, input_before, okay = None, None, False
    try:
        require(shutil.disk_usage(review).free >= HEADROOM + OUTPUT_ALLOWANCE, "reception reserve unavailable")
        before = source_identity(root, matrix)
        save_new(review / "source-before.json", before)
        if args.mode == "replay":
            input_before = inventory(out)
        report["original_functions"] = {
            name: hashlib.sha256(inspect.getsource(getattr(matrix, name)).encode()).hexdigest()
            for name in ("validate_build", "validate_shard", "inspect_raw", "combine", "aggregate", "native_digest")}
        report["result"] = prepare(out, matrix) if args.mode == "prepare" else replay(out, root, matrix, review)
        okay = True
    except BaseException as error:
        report["error"] = safe_error(error)
    finally:
        try:
            after = source_identity(root, matrix)
            save_new(review / "source-after.json", after)
            require(before == after and digest(script) == script_sha, "source/observer/controller changed")
            if input_before is not None:
                require(inventory(out) == input_before, "original receiver inputs changed")
            report["source_observer_controller_and_original_inputs_unchanged"] = True
        except BaseException as error:
            okay = False
            report["final_check_error"] = safe_error(error)
        report["completed_utc"] = stamp()
        report["status"] = "passed" if okay else "failed_or_incomplete"
        report["output_inventory_before_receipt"] = inventory(review)
        save_new(review / "receipt.json", report)
        print(json.dumps({"mode": args.mode, "status": report["status"],
                          "receipt_sha256": digest(review / "receipt.json"),
                          "TODO_closed": 0, "TODO_remaining": 29}), flush=True)
    return 0 if okay else 1


if __name__ == "__main__":
    raise SystemExit(main())

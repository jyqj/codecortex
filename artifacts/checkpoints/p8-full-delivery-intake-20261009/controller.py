"""Fresh hosted, read-only intake of two prior complete receiver deliveries.

Disk-backed standard zipfile transport; unrelated to the Range adapter's path.
Only fixed original artifact downloads use the token. No native product,
statistics, oracle, Cargo, provider or new workload is executed.
"""
import base64
import hashlib
import importlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

if sys.flags.optimize:
    raise SystemExit("optimized Python is forbidden")
HERE = Path(__file__).resolve().parent
CONTROL = HERE.parents[2]
SOURCE = Path(os.environ["GITHUB_WORKSPACE"]) / "source"
RESERVE = 512 * 1024**2
PROFILES = {
    "G4-runtime": dict(source="260f596582f2d82b8d7c707b61a6b8b6a43b069f",
        receiver="9aacfbea2771209134b4f2dd8889cbb4e2a3d099", run=37879784342, job=113656573097,
        artifact=11593443086, bytes=233905529,
        sha256="7bc22b041108af5bf8f29b571fe094c35ede0f92cb56c93ad29bcd4c8f9ff1b6",
        nested_bytes=117552305, module="runtime_intake",
        artifact_name="p8-G4-offline-runtime-reception-37879784342"),
    "G2-platform": dict(source="0272a1fb152fd76a7cfb22386a580629d4038a64",
        receiver="08bfcb4538c4fe390adac6c5a127c0a8795a1e7a", run=37881939531, job=113663338905,
        artifact=11594372339, bytes=137100343,
        sha256="bb5a3ae2a3808a908c00b35b8bbbdffa7b191279703b569f4338f1af1d413f14",
        nested_bytes=68772411, module="platform_intake",
        artifact_name="p8-G2-offline-platform-reception-37881939531"),
}
INPUTS = ("controller.py", "disk_zip.py", "runtime_intake.py", "platform_intake.py", "G2-platform-registration.json")

def require(ok, message):
    if not ok:
        raise ValueError(message)

def sha(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()

def write(path, value):
    with path.open("x") as f:
        json.dump(value, f, sort_keys=True, indent=2)
        f.write("\n")

def free(path):
    state = os.statvfs(path)
    return state.f_bavail * state.f_frsize

def git(root, *args):
    env = {k: v for k, v in os.environ.items() if k not in ("GH_TOKEN", "GITHUB_TOKEN")}
    result = subprocess.run(["git", "-C", str(root), *args], stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, env=env, timeout=30)
    require(result.returncode == 0, "read-only Git identity command failed")
    return result.stdout

def input_snapshot():
    result = {}
    for name in INPUTS:
        path = HERE / name
        require(path.is_file() and not path.is_symlink(), "controller input is not regular")
        body = path.read_bytes()
        relative = path.relative_to(CONTROL).as_posix()
        expected = git(CONTROL, "show", "HEAD:" + relative)
        require(body == expected, "controller input differs from its actual committed bytes")
        result[name] = dict(bytes=len(body), sha256=hashlib.sha256(body).hexdigest(),
                            git_blob=hashlib.sha1(b"blob " + str(len(body)).encode() + b"\0" + body).hexdigest())
    return result

def fetch(relative, destination, maximum, token, transfers):
    require(relative.startswith("/repos/jyqj/codecortex/actions/"), "unregistered API namespace")
    require(not destination.exists() and free(destination.parent) >= RESERVE + maximum,
            "not_run: no fresh output or insufficient unchanged filesystem reserve")
    argv = ["curl", "--fail", "--location", "--silent",
            "--proto", "=https", "--proto-redir", "=https", "--retry", "0",
            "--connect-timeout", "30", "--max-time", "240", "--max-filesize", str(maximum),
            "--header", "Accept: application/vnd.github+json",
            "--header", "X-GitHub-Api-Version: 2022-11-28",
            "--header", "Authorization: Bearer " + token,
            "https://api.github.com" + relative, "--output", str(destination),
            "--write-out", "%{http_code}"]
    record = dict(api=relative, maximum_bytes=maximum, status="incomplete")
    transfers.append(record)
    try:
        result = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=250)
    except subprocess.TimeoutExpired:
        record["status"] = "timeout"
        if destination.is_file():
            record.update(partial_bytes=destination.stat().st_size, partial_sha256=sha(destination))
        raise ValueError("fixed GitHub input transfer timed out") from None
    except OSError:
        record["status"] = "process_start_failed"
        raise ValueError("fixed GitHub transfer process unavailable") from None
    code = result.stdout.decode("ascii", errors="ignore").strip()
    record.update(curl_exit=result.returncode, http_status=code if re.fullmatch(r"[0-9]{3}", code) else None)
    if destination.is_file():
        record.update(bytes=destination.stat().st_size, sha256=sha(destination))
    require(result.returncode == 0 and code == "200" and destination.is_file()
            and destination.stat().st_size <= maximum, "fixed GitHub input transfer failed")
    record["status"] = "downloaded"
    return destination

def pages(run, kind, scratch, token, transfers):
    rows = []
    total = None
    for page in range(1, 101):
        path = fetch("/repos/jyqj/codecortex/actions/runs/%d/%s?per_page=100&page=%d" % (run, kind, page),
                     scratch / ("%s-page-%03d.json" % (kind, page)), 2 * 1024**2, token, transfers)
        item = json.loads(path.read_bytes())
        require(type(item.get("total_count")) is int, "API pagination count missing")
        total = item["total_count"] if total is None else total
        require(item["total_count"] == total and isinstance(item.get(kind), list)
                and len(item[kind]) <= 100, "API population changed or invalid")
        rows.extend(item[kind])
        require(len(rows) <= total, "API duplicate/excessive population")
        if len(rows) == total:
            break
        require(len(item[kind]) == 100, "API incomplete intermediate page")
    require(len(rows) == total and len({x["id"] for x in rows}) == total, "API full unique population")
    return dict(total_count=total, **{kind: rows})

def main(profile):
    require(profile in PROFILES and os.environ.get("GITHUB_REPOSITORY") == "jyqj/codecortex"
            and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "wrong controller scope or attempt")
    spec = PROFILES[profile]
    root = Path(os.environ["RUNNER_TEMP"])
    scratch = root / ("p8-full-intake-inputs-" + profile)
    out = root / ("p8-full-intake-reports-" + profile)
    require(not scratch.exists() and not out.exists(), "refuse existing intake roots")
    scratch.mkdir()
    out.mkdir()
    started = time.monotonic()
    report = dict(schema="p8-fixed-outer-full-intake-v1", status="incomplete_not_certified",
                  profile=profile, measured_source=spec["source"], original_receiver=spec["receiver"],
                  original_run=spec["run"], original_artifact=spec["artifact"],
                  transport="independent fresh-VM disk-backed standard zipfile; not bounded HTTP Range",
                  original_measurements_rerun=False, native_statistics_or_product_or_Cargo_executed=False,
                  raw_originals_relabelled=False, TODO_closed=0, TODO_remaining=29)
    transfers = []
    code = 2
    try:
        require(git(CONTROL, "rev-parse", "HEAD").decode().strip() == os.environ["GITHUB_SHA"],
                "actual intake controller checkout mismatch")
        require(git(SOURCE, "rev-parse", "HEAD").decode().strip() == spec["source"],
                "actual original product checkout mismatch")
        report["controller_commit"] = os.environ["GITHUB_SHA"]
        before = input_snapshot()
        report["controller_inputs_before"] = before
        required = RESERVE + spec["bytes"] + spec["nested_bytes"] + 128 * 1024**2
        capacity = dict(free_bytes=free(root), required_bytes=required, reserve_bytes=RESERVE,
                        registered_outer_bytes=spec["bytes"], registered_nested_bytes=spec["nested_bytes"],
                        allowance_bytes=128 * 1024**2)
        report["capacity_before"] = capacity
        require(capacity["free_bytes"] >= required, "not_run: insufficient pre-registered hosted disk capacity")
        token = os.environ.get("GH_TOKEN", "")
        require(bool(token), "read-only download token unavailable")
        try:
            run_path = fetch("/repos/jyqj/codecortex/actions/runs/%d" % spec["run"],
                             scratch / "original-run.json", 2 * 1024**2, token, transfers)
            run = json.loads(run_path.read_bytes())
            jobs = pages(spec["run"], "jobs", scratch, token, transfers)
            artifacts = pages(spec["run"], "artifacts", scratch, token, transfers)
            require(run["id"] == spec["run"] and run["head_sha"] == spec["receiver"]
                    and run["run_attempt"] == 1 and run["status"] == "completed"
                    and run["conclusion"] == "success", "original fixed receiver run identity/result")
            job = [x for x in jobs["jobs"] if x["id"] == spec["job"]]
            item = [x for x in artifacts["artifacts"] if x["id"] == spec["artifact"]]
            require(len(job) == len(item) == 1, "missing/duplicate original job/artifact")
            require(job[0]["name"] == "offline_replay" and job[0]["head_sha"] == spec["receiver"]
                    and job[0]["status"] == "completed" and job[0]["conclusion"] == "success",
                    "original receiver actual job")
            item = item[0]
            require(item["name"] == spec["artifact_name"] and item["expired"] is False
                    and item["size_in_bytes"] == spec["bytes"]
                    and item["digest"] == "sha256:" + spec["sha256"]
                    and item["workflow_run"]["id"] == spec["run"]
                    and item["workflow_run"]["head_sha"] == spec["receiver"],
                    "original fixed delivery API identity")
            archive = fetch("/repos/jyqj/codecortex/actions/artifacts/%d/zip" % spec["artifact"],
                            scratch / "original-outer.zip", spec["bytes"], token, transfers)
            log = fetch("/repos/jyqj/codecortex/actions/jobs/%d/logs" % spec["job"],
                        scratch / "original-job.log", 4 * 1024**2, token, transfers)
        finally:
            token = None
            os.environ.pop("GH_TOKEN", None)
            os.environ.pop("GITHUB_TOKEN", None)
        require(archive.stat().st_size == spec["bytes"] and sha(archive) == spec["sha256"],
                "complete original outer ZIP bytes/SHA")
        log_text = log.read_text()
        checkouts = re.findall(r"git checkout --progress --force ([0-9a-f]{40})", log_text)
        require(spec["receiver"] in checkouts and spec["source"] in checkouts,
                "actual original controller/product checkout evidence")
        safe_receipts = []
        for line in log_text.splitlines():
            at = line.find('{"TODO_closed":')
            if at >= 0:
                try:
                    candidate = json.loads(line[at:])
                except json.JSONDecodeError:
                    continue
                if candidate.get("controller_commit") == spec["receiver"]:
                    safe_receipts.append(candidate)
        require(len(safe_receipts) == 1 and safe_receipts[0]["exit_code"] == 0,
                "one complete original controller stdout receipt")
        report["original_job_log"] = dict(bytes=log.stat().st_size, sha256=sha(log),
            api="/repos/jyqj/codecortex/actions/jobs/%d/logs" % spec["job"],
            included_in_public_outputs=False, reason="raw log can contain signed storage redirects",
            safe_checkout_commits=checkouts, safe_original_controller_receipt=safe_receipts[0])
        report["original_api"] = dict(run=run, jobs=jobs, artifacts=artifacts)
        module = importlib.import_module(spec["module"])
        evidence = module.inspect(archive, SOURCE, scratch)
        require(evidence["status"] == "accepted_scoped_original_receiver_delivery"
                and evidence["receipt"] == safe_receipts[0], "actual complete intake and original stdout receipt")
        after = input_snapshot()
        require(before == after and free(root) >= RESERVE, "controller input changed or disk reserve exhausted")
        report.update(status="accepted_scoped_full_original_delivery_intake", evidence=evidence,
                      controller_inputs_after=after, capacity_after=dict(free_bytes=free(root), reserve_bytes=RESERVE),
                      raw_durability_scope="Original immutable Actions artifacts remain the raw source. Downloads and nested copies are unchanged on this fresh VM; the report artifact contains no duplicate giant ZIP or raw signed-URL job log.")
        code = 0
    except BaseException as error:
        report["error_type"] = type(error).__name__
        # Source locations only: never exception arguments, argv, source text,
        # token values or transfer URLs. This preserves safe failure diagnosis.
        locations = []
        trace = error.__traceback__
        while trace is not None:
            code_object = trace.tb_frame.f_code
            locations.append(dict(file=Path(code_object.co_filename).name,
                                  function=code_object.co_name, line=trace.tb_lineno))
            trace = trace.tb_next
        report["error_locations"] = locations
        if isinstance(error, ValueError):
            report["error"] = str(error)
    finally:
        os.environ.pop("GH_TOKEN", None)
        os.environ.pop("GITHUB_TOKEN", None)
        report.update(transfers=transfers, exit_code=code, wall_seconds=time.monotonic() - started)
        write(out / "complete-safe-report.json", report)
        raw_report = (out / "complete-safe-report.json").read_bytes()
        # Frame the actual file bytes, without reserializing parsed JSON.
        # 16KiB bytes yield at most 21848 base64 chars plus small metadata.
        chunk_bytes = 16 * 1024
        chunk_count = (len(raw_report) + chunk_bytes - 1) // chunk_bytes
        header = dict(schema="p8-safe-full-intake-log-frames-v1", profile=profile,
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
    return code

if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("one fixed profile required")
    raise SystemExit(main(sys.argv[1]))

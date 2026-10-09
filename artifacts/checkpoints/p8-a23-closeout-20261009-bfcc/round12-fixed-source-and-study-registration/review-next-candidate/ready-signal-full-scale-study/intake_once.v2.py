#!/usr/bin/env python3
"""Finite read-only GitHub observation or one original artifact intake; never dispatch or rerun."""
import argparse, datetime, hashlib, json, os, re, shutil, stat, subprocess, sys, zipfile
from pathlib import Path, PurePosixPath
GH = "/opt/homebrew/bin/gh"
REPO = "jyqj/codecortex"

def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()

def write_new(path, data):
    with path.open("x", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")

def api(endpoint, output, calls):
    argv = [GH, "api", endpoint]
    record = {"argv": argv, "started_at": utc(), "exit_code": None}
    calls.append(record)
    child = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    timed_out = False
    try:
        stdout, stderr = child.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        timed_out = True
        child.kill()
        stdout, stderr = child.communicate()
    record.update(exit_code=child.returncode, finished_at=utc(), timed_out=timed_out,
                  stderr=stderr.decode("utf-8", "replace"))
    with output.open("xb") as f:
        f.write(stdout)
    stderr_path = output.with_name(output.name + ".stderr")
    with stderr_path.open("xb") as f:
        f.write(stderr)
    record["response_sha256"] = sha(output)
    record["stderr_raw"] = {"path": stderr_path.name, "bytes": len(stderr), "sha256": sha(stderr_path)}
    if timed_out or child.returncode != 0:
        raise RuntimeError("GitHub GET failed; original response and integer exit retained")
    return json.loads(stdout)

def checked_binding(path):
    b = json.loads(path.read_text())
    if b.get("repository") != REPO or not re.fullmatch(r"[0-9a-f]{40}", b.get("source_G") or ""):
        raise ValueError("A real public pushed G binding is required; pending template cannot execute")
    if type(b.get("run_id")) is not int or b["run_id"] <= 0 or b.get("expected_run_attempt") != 1:
        raise ValueError("Root must bind the actual single original dispatch run and attempt1")
    if b.get("product_P") != "35a7b3412e5fdee9e8ff88951735c06b0e371465":
        raise ValueError("Unexpected product identity")
    if b.get("dispatch_owner") != "root" or b.get("event") != "workflow_dispatch":
        raise ValueError("Only the root-owned original workflow_dispatch is admitted")
    return b

def verify_run(run, binding):
    expected = {"id": binding["run_id"], "head_sha": binding["source_G"],
                "run_attempt": binding["expected_run_attempt"], "event": "workflow_dispatch",
                "head_branch": binding["branch"]}
    if any(run.get(k) != v for k, v in expected.items()):
        raise ValueError("Run identity/attempt/event differs; preserve metadata, no credit")
    if run.get("path") != ".github/workflows/p8-scale.yml":
        raise ValueError("Unexpected workflow path")

def validate_members(z):
    seen = set()
    for member in z.infolist():
        name = member.filename
        original_name = member.orig_filename
        if "\x00" in original_name or original_name != name:
            raise ValueError("NUL-truncated or changed archive member name")
        p = PurePosixPath(name)
        normalized = p.as_posix().rstrip("/")
        if not name or "\\" in name or p.is_absolute() or ".." in p.parts or normalized in ("", "."):
            raise ValueError("Unsafe archive member path")
        if name.rstrip("/") != normalized or normalized in seen:
            raise ValueError("Noncanonical or duplicate archive member path")
        seen.add(normalized)
        kind = stat.S_IFMT(member.external_attr >> 16)
        if kind not in (0, stat.S_IFREG, stat.S_IFDIR):
            raise ValueError("Archive special files/symlinks are not extracted")
        if member.flag_bits & 1:
            raise ValueError("Encrypted member is not accepted")
        yield member, p

def extract_original(zip_path, destination):
    with zipfile.ZipFile(zip_path) as z:
        members = list(validate_members(z))
        total = sum(m.file_size for m, _ in members)
        if total > shutil.disk_usage(destination.parent).free - 1024 ** 3:
            raise ValueError("Insufficient receiver disk; retain ZIP for later intake, no measurement change")
        destination.mkdir(exist_ok=False)
        inventory = []
        for m, p in members:
            target = destination.joinpath(*p.parts)
            item = {"path": m.filename, "zip_bytes": m.file_size, "compressed_bytes": m.compress_size,
                    "crc32": m.CRC, "external_attr": m.external_attr, "zip_datetime": list(m.date_time)}
            if m.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                item["directory"] = True
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with z.open(m) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                item.update(bytes=target.stat().st_size, sha256=sha(target),
                            extracted_mode=stat.S_IMODE(target.stat().st_mode))
                if item["bytes"] != m.file_size:
                    raise ValueError("Extracted member length mismatch")
            inventory.append(item)
    return inventory

def observe(binding, out, calls):
    run = api(f"/repos/{REPO}/actions/runs/{binding['run_id']}", out / "run.json", calls)
    verify_run(run, binding)
    totals = {}
    for kind, key in (("jobs", "jobs"), ("artifacts", "artifacts")):
        page = 1
        values = []
        pages = None
        while pages is None or page <= pages:
            response = api(f"/repos/{REPO}/actions/runs/{binding['run_id']}/{kind}?per_page=100&page={page}",
                           out / f"{kind}-page-{page}.json", calls)
            if pages is None:
                first_total = response["total_count"]
                pages = max(1, (first_total + 99) // 100)
                if pages > 10:
                    raise ValueError("More than10pages: preserve current response and request bounded manual intake")
            values.extend(response[key])
            page += 1
        totals[kind] = {"observed_items": len(values), "pages": pages,
                        "reported_total_at_first_page": first_total, "snapshot_is_not_atomic": True}
        if kind == "jobs":
            totals[kind]["statuses"] = [{"id": j["id"], "name": j["name"], "status": j["status"],
                                        "conclusion": j.get("conclusion"), "started_at": j.get("started_at"),
                                        "completed_at": j.get("completed_at"), "steps": j.get("steps", [])}
                                       for j in values]
        else:
            totals[kind]["items"] = [{"id": x["id"], "name": x["name"], "size_in_bytes": x["size_in_bytes"],
                                      "digest": x.get("digest"), "created_at": x["created_at"],
                                      "expired": x["expired"]} for x in values]
    write_new(out / "observation.json", {"run_id": binding["run_id"], "head": binding["source_G"],
              "run_status": run["status"], "run_conclusion": run.get("conclusion"), **totals,
              "measurement_or_todo_credit": False})

def verify_zip_identity(original, meta, out):
    digest = sha(original)
    expected = meta.get("digest")
    verified = bool(expected and expected == "sha256:" + digest)
    actual_size = original.stat().st_size
    official_size = meta.get("size_in_bytes")
    size_matches = type(official_size) is int and official_size >= 0 and actual_size == official_size
    write_new(out / "zip-receipt.json", {"bytes": actual_size, "sha256": digest,
              "github_digest": expected, "github_digest_matches": verified,
              "api_size_in_bytes": official_size, "size_matches_api": size_matches})
    if not size_matches:
        raise ValueError("ZIP size differs from official metadata; original retained, not extracted")
    if expected and not verified:
        raise ValueError("ZIP digest mismatch; original bytes retained, not extracted")
    return digest, verified

def receive(binding, artifact_id, out, calls):
    run = api(f"/repos/{REPO}/actions/runs/{binding['run_id']}", out / "run-before.json", calls)
    verify_run(run, binding)
    meta = api(f"/repos/{REPO}/actions/artifacts/{artifact_id}", out / "github-metadata.json", calls)
    origin = meta.get("workflow_run", {})
    if meta["id"] != artifact_id or origin.get("id") != binding["run_id"] or origin.get("head_sha") != binding["source_G"]:
        raise ValueError("Artifact source/run identity mismatch")
    if meta.get("expired"):
        raise ValueError("Original artifact expired; no replacement is requested")
    pattern = rf"p8-scale-(?:build|matrix|(?:shard|capacity)-(?:1000|5000|10000|50000|100000)-(?:[0-9]|[12][0-9]))-{binding['run_id']}"
    if not re.fullmatch(pattern, meta["name"]):
        raise ValueError("Unexpected artifact name for the unchanged study protocol")
    argv = [GH, "api", f"/repos/{REPO}/actions/artifacts/{artifact_id}/zip"]
    record = {"argv": argv, "started_at": utc(), "exit_code": None}
    calls.append(record)
    part = out / "original.zip.part"
    with part.open("xb") as so, (out / "download.stderr").open("xb") as se:
        child = subprocess.Popen(argv, stdout=so, stderr=se)
        timed_out = False
        try:
            code = child.wait(timeout=120)
        except subprocess.TimeoutExpired:
            timed_out = True
            child.kill()
            code = child.wait()
        record.update(exit_code=code, timed_out=timed_out, finished_at=utc())
    write_new(out / "download-exit.json", record)
    if timed_out or code != 0:
        raise RuntimeError("Original ZIP transfer failed; partial bytes preserved without automatic retry")
    original = out / "original.zip"
    part.rename(original)
    digest, verified = verify_zip_identity(original, meta, out)
    members = extract_original(original, out / "extracted")
    write_new(out / "members.json", {"members": members, "total_uncompressed_bytes": sum(x.get("bytes", 0) for x in members)})
    after = api(f"/repos/{REPO}/actions/runs/{binding['run_id']}", out / "run-after.json", calls)
    verify_run(after, binding)
    if sha(original) != digest:
        raise ValueError("Original ZIP changed during extraction")
    return {"artifact_id": artifact_id, "name": meta["name"], "zip_sha256": digest,
            "github_digest_verified": verified, "member_count": len(members),
            "raw_semantic_validation": "pending; transport intake is not validate_build/validate_shard/aggregate",
            "measurement_or_todo_credit": False}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("observe", "receive"))
    parser.add_argument("--binding", type=Path, required=True)
    parser.add_argument("--artifact-id", type=int)
    parser.add_argument("--attempt", type=int, default=1)
    args = parser.parse_args()
    b = checked_binding(args.binding)
    if args.attempt < 1:
        parser.error("attempt must be a positive explicit transport attempt")
    if args.mode == "receive":
        if not args.artifact_id or args.artifact_id <= 0:
            parser.error("receive needs an actual artifact ID")
        out = Path(b["raw_root"]) / str(b["run_id"]) / str(args.artifact_id) / f"attempt-{args.attempt:02d}"
    else:
        out = Path(b["observation_root"]) / datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    out.mkdir(parents=True, exist_ok=False)
    write_new(out / "binding-used.json", b)
    receipt = {"mode": args.mode, "started_at": utc(), "binding_sha256": sha(args.binding),
               "run_id": b["run_id"], "head": b["source_G"], "status": "running", "calls": [],
               "sleep_or_poll_loop": False, "automatic_retry": False, "dispatch_or_rerun": False}
    code = 0
    try:
        if args.mode == "observe":
            observe(b, out, receipt["calls"])
            receipt["status"] = "observation_recorded"
        else:
            receipt.update(receive(b, args.artifact_id, out, receipt["calls"]))
            receipt["status"] = ("transport_received" if receipt["github_digest_verified"]
                                 else "transport_received_digest_unavailable")
    except Exception as error:
        receipt.update(status="failed_retained", error=repr(error))
        code = 1
    receipt.update(finished_at=utc(), process_exit_code=code)
    write_new(out / "transport-receipt.json", receipt)
    print(json.dumps({"output": str(out), "status": receipt["status"], "exit_code": code}))
    return code

if __name__ == "__main__":
    sys.exit(main())

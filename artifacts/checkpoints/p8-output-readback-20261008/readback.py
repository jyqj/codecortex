#!/usr/bin/env python3
"""Read exact public recovery artifacts without rerunning the evaluator."""
import argparse
import base64
import hashlib
import json
from pathlib import Path, PurePosixPath
import stat
import zipfile

PINS = [
    {"kind":"metadata", "artifact_id":11531074232, "name":"p8-public-metadata-37735653177", "bytes":1604, "sha256":"75dadcab573c78d22f8e8ee66e77470d0f875418e287335ffbb7bafb17f6c84b", "files":3, "prefix":"artifacts/checkpoints/p8-public-dev-recovery-20261008/runs/37735653177/raw/"},
    {"kind":"external", "artifact_id":11531039777, "name":"p8-external-recovery-37735653177", "bytes":575989, "sha256":"41c71bbb969174216a988005df0f1aaad472a8c85e595ffc9722a0fd1c5e4fc8", "files":100, "prefix":"artifacts/benchmarks/p8-external-recovery-20261008/runs/37735653177/raw/"},
]
RUN = 37735653177
HEAD = "3512d14c20a80ab833ee634c1d2f0a2d83f03d9c"

def need(ok, message):
    if not ok:
        raise ValueError(message)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

parser = argparse.ArgumentParser()
parser.add_argument("--input", type=Path, required=True)
parser.add_argument("--part", type=int, choices=(0, 1), required=True)
args = parser.parse_args()
jobs = json.loads((args.input / "jobs.json").read_bytes())
by_name = {j["name"]: j for j in jobs["jobs"]}
need(len(by_name) == 2 and set(by_name) == {"external-public", "public-metadata"}, "original job set changed")
for name, expected in (("external-public", "failure"), ("public-metadata", "success")):
    job = by_name[name]
    need(job["head_sha"] == HEAD and job["status"] == "completed" and job["conclusion"] == expected, "original terminal result drift")
    steps = {s["name"]: s["conclusion"] for s in job["steps"]}
    if name == "external-public":
        need(steps["Recover inputs and execute original compat and native runner"] == "failure", "original gate failure missing")
        need(steps["Preserve actual public evidence and external-body references"] == "success", "original external upload failed")
    else:
        need(steps["Verify actual fixed public metadata"] == "success", "original metadata execution failed")
records, files, total = [], {}, 0
for pin in PINS:
    metadata = json.loads((args.input / (pin["kind"] + ".json")).read_bytes())
    need(metadata["id"] == pin["artifact_id"] and metadata["name"] == pin["name"] and not metadata["expired"], "artifact identity changed")
    need(metadata["workflow_run"]["id"] == RUN and metadata["workflow_run"]["head_sha"] == HEAD, "artifact source changed")
    need(metadata["size_in_bytes"] == pin["bytes"] and metadata["digest"] == "sha256:" + pin["sha256"], "metadata digest changed")
    raw_zip = (args.input / (pin["kind"] + ".zip")).read_bytes()
    need(len(raw_zip) == pin["bytes"] and sha(raw_zip) == pin["sha256"], "archive bytes changed")
    with zipfile.ZipFile(args.input / (pin["kind"] + ".zip")) as archive:
        infos = archive.infolist()
        need(len(infos) == pin["files"] and len({x.filename for x in infos}) == pin["files"], "complete original member set required")
        need(sum(x.file_size for x in infos) <= 10 * 1024 * 1024, "expanded artifact exceeds bound")
        for info in sorted(infos, key=lambda x:x.filename):
            name = info.filename
            path = PurePosixPath(name)
            need(name and not path.is_absolute() and "\\" not in name and all(x not in ("", ".", "..") for x in name.split("/")), "unsafe member path")
            need(not info.is_dir() and not stat.S_ISLNK(info.external_attr >> 16), "regular members required")
            raw = archive.read(info)
            need(len(raw) == info.file_size, "member size mismatch")
            target = pin["prefix"] + name
            need(target not in files, "duplicate target")
            files[target] = raw
            total += len(raw)
            need(total <= 12 * 1024 * 1024, "total expanded size exceeds bound")
            encoded = base64.b64encode(raw).decode("ascii")
            record = {"path":target, "artifact":pin["artifact_id"], "member":name, "bytes":len(raw), "sha256":sha(raw), "encoding":"base64", "encoded_characters":len(encoded), "chunks":(len(encoded)+7999)//8000}
            records.append(record)
records.sort(key=lambda x:x["path"])
loads = [0, 0]
for record in sorted(records, key=lambda x:(-x["encoded_characters"], x["path"])):
    part = min(range(2), key=lambda x:(loads[x], x))
    record["part"] = part
    loads[part] += record["encoded_characters"]
need(max(loads) <= 8 * 1024 * 1024, "per-part readback exceeds bound")
manifest = {"schema_version":1, "source_run":RUN, "source_head":HEAD, "original_external_conclusion":"failure", "original_metadata_conclusion":"success", "artifacts":PINS, "files":records, "file_count":len(records), "total_bytes":total, "part":args.part, "parts":2, "part_encoded_characters":loads, "scope":"Exact public artifact readback only; original gate failure retained; no evaluator or heldout execution"}
for record in records:
    if record["part"] != args.part:
        continue
    encoded = base64.b64encode(files[record["path"]]).decode("ascii")
    print("P8_READBACK_FILE " + json.dumps(record, ensure_ascii=True), flush=True)
    for index, start in enumerate(range(0, len(encoded), 8000)):
        print("P8_READBACK_CHUNK " + json.dumps({"path":record["path"], "index":index, "content":encoded[start:start+8000]}, ensure_ascii=True), flush=True)
print("P8_READBACK_MANIFEST " + json.dumps(manifest, ensure_ascii=True), flush=True)

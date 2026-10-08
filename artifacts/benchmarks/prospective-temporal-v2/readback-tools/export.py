#!/usr/bin/env python3
"""Archive-only readback of actual completed Actions artifacts. No queries, builds, scorer or imports from measured source."""
import argparse, base64, hashlib, json, os, stat, zipfile
from pathlib import Path, PurePosixPath
PARTS = 16
parser = argparse.ArgumentParser()
parser.add_argument("--part", type=int, choices=range(PARTS), required=True)
args = parser.parse_args()
root = Path(os.environ["READBACK_ROOT"])
pins = json.loads((Path(__file__).parent / "pins.json").read_bytes())
def need(value, reason):
    if not value:
        raise ValueError(reason)
def sha(value):
    return hashlib.sha256(value).hexdigest()
def oid(value):
    return hashlib.sha1(b"blob " + str(len(value)).encode() + b"\0" + value).hexdigest()
def jb(value):
    return (json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode()
def safe(name):
    return bool(name) and not PurePosixPath(name).is_absolute() and "\\" not in name and all(part not in ("", ".", "..") for part in name.split("/"))
files, selection = {}, {}
def add(name, value, origin):
    need(safe(name) and name not in files, "unique safe export path")
    files[name] = (value, origin)
need(isinstance(pins, dict) and 1 <= len(pins) <= 4, "bounded explicit archive pins")
for key, pin in sorted(pins.items()):
    need(safe(key) and "/" not in key and safe(pin["export_prefix"]), "safe pinned metadata names")
    directory = root / key
    prefix = pin["export_prefix"] + "/"
    meta_raw = (directory / "artifact.json").read_bytes()
    run_raw = (directory / "run.json").read_bytes()
    jobs_raw = (directory / "jobs.json").read_bytes()
    meta, run = json.loads(meta_raw), json.loads(run_raw)
    need(meta["id"] == pin["artifact_id"] and meta["name"] == pin["artifact_name"] and not meta["expired"], "actual artifact identity")
    need(meta["workflow_run"]["id"] == pin["run_id"] and meta["workflow_run"]["head_sha"] == pin["workflow_head"], "actual artifact origin")
    need(meta["size_in_bytes"] == pin["zip_bytes"] and meta["digest"] == "sha256:" + pin["zip_sha256"], "actual service ZIP digest")
    need(run["id"] == pin["run_id"] and run["head_sha"] == pin["workflow_head"] and run["status"] == "completed" and run["conclusion"] == pin["conclusion"] and run["run_attempt"] == pin["run_attempt"], "actual completed attempt identity")
    archive = (directory / "original.zip").read_bytes()
    need(len(archive) == pin["zip_bytes"] and sha(archive) == pin["zip_sha256"], "complete original ZIP bytes")
    for name, value in (("artifact.json", meta_raw), ("run.json", run_raw), ("jobs.json", jobs_raw)):
        add(prefix + "metadata/" + name, value, {"kind": "original_service_metadata", "artifact": key})
    chunks = []
    for index, start in enumerate(range(0, len(archive), 786432)):
        value = archive[start:start + 786432]
        name = prefix + "original-archive/part-" + str(index).zfill(3) + ".bin"
        add(name, value, {"kind": "lossless_original_zip_chunk", "artifact": key, "offset": start})
        chunks.append({"path": name, "offset": start, "bytes": len(value), "sha256": sha(value)})
    add(prefix + "original-archive/manifest.json", jb({"original_pin": pin, "original_zip_bytes": len(archive), "original_zip_sha256": sha(archive), "chunks": chunks, "format": "Concatenate all raw chunks in this exact order"}), {"kind": "lossless_zip_restoration_manifest", "artifact": key})
    seen, members, exported, omitted = set(), [], [], []
    with zipfile.ZipFile(directory / "original.zip") as zipped:
        infos = zipped.infolist()
        for info in sorted(infos, key=lambda item: item.filename):
            name = info.filename
            need(safe(name.rstrip("/")) and name not in seen, "safe unique original member")
            seen.add(name)
            value = zipped.read(info)
            mode = info.external_attr >> 16
            item = {"path": name, "bytes": len(value), "sha256": sha(value), "git_blob": oid(value), "zip_mode": mode, "directory": info.is_dir(), "crc": info.CRC}
            members.append(item)
            regular = not info.is_dir() and stat.S_IFMT(mode) in (0, stat.S_IFREG)
            readable = False
            if regular and len(value) <= 8 * 1024 * 1024 and b"\0" not in value:
                try:
                    value.decode("utf-8")
                    readable = True
                except UnicodeDecodeError:
                    pass
            if readable:
                add(prefix + "raw/" + name, value, {"kind": "exact_original_utf8_member", "artifact": key, "member": name})
                exported.append(name)
            else:
                omitted.append({"path": name, "reason": "original bytes/mode retained in complete lossless ZIP; not duplicated as a bounded ordinary UTF8 review file"})
    inventory = {"pin": pin, "file_count": len(members), "total_member_bytes": sum(item["bytes"] for item in members), "files": members, "scope": "Every original ZIP member is hashed directly from the preserved original archive; no extraction, execution, mode substitution or score calculation."}
    add(prefix + "metadata/original-member-inventory.json", jb(inventory), {"kind": "actual_complete_original_member_inventory", "artifact": key})
    selection[key] = {"pin": pin, "original_members": len(members), "original_bytes": inventory["total_member_bytes"], "exported_plain_members": exported, "not_duplicated_as_plain_files": omitted, "every_original_byte_preserved": True}
    add(prefix + "selection.json", jb(selection[key]), {"kind": "complete_archive_selection", "artifact": key})
records = []
for name, (value, origin) in sorted(files.items()):
    encoded_chars = ((len(value) + 2) // 3) * 4
    records.append({"path": name, "bytes": len(value), "sha256": sha(value), "git_blob": oid(value), "encoded_characters": encoded_chars, "chunks": (encoded_chars + 7999) // 8000, **origin})
loads = [0] * PARTS
for item in sorted(records, key=lambda item: (-item["encoded_characters"], item["path"])):
    part = min(range(PARTS), key=lambda index: (loads[index], index))
    item["part"] = part
    loads[part] += item["encoded_characters"]
need(max(loads) <= 6 * 1024 * 1024, "bounded per-job original archive readback")
for item in records:
    if item["part"] != args.part:
        continue
    encoded = base64.b64encode(files[item["path"]][0]).decode("ascii")
    print("P8_PT_FILE " + json.dumps(item), flush=True)
    for index, start in enumerate(range(0, len(encoded), 8000)):
        print("P8_PT_CHUNK " + json.dumps({"path": item["path"], "index": index, "content": encoded[start:start + 8000]}), flush=True)
print("P8_PT_MANIFEST " + json.dumps({"schema_version": 1, "scope": "Archive-only actual completed artifact readback; no queries, scorer, product or source imports", "original_pins": pins, "files": records, "file_count": len(records), "total_bytes": sum(item["bytes"] for item in records), "part": args.part, "parts": PARTS, "part_encoded_characters": loads}), flush=True)

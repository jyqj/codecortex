#!/usr/bin/env python3
"""Reassemble and verify the exact worker evidence; no tests or samples are executed."""
import argparse, hashlib, io, json, pathlib, zipfile
p = argparse.ArgumentParser()
p.add_argument("--output-root", type=pathlib.Path, required=True)
args = p.parse_args()
here = pathlib.Path(__file__).resolve().parent
manifest = json.loads((here / "manifest.json").read_text())
parts = []
for item in manifest["parts"]:
    name = item["name"]
    if pathlib.PurePosixPath(name).name != name:
        raise ValueError("unsafe part name")
    data = (here / name).read_bytes()
    if len(data) != item["bytes"] or hashlib.sha256(data).hexdigest() != item["sha256"]:
        raise ValueError("part hash mismatch: " + name)
    parts.append(data)
archive = b"".join(parts)
if len(archive) != manifest["bytes"] or hashlib.sha256(archive).hexdigest() != manifest["sha256"]:
    raise ValueError("archive hash mismatch")
root = args.output_root.resolve()
with zipfile.ZipFile(io.BytesIO(archive)) as z:
    if z.testzip() is not None:
        raise ValueError("ZIP CRC mismatch")
    transfer = json.loads(z.read("transfer-manifest.json"))
    checked = []
    for item in transfer["files"]:
        name = item.get("repo_path", item.get("repo_relative_path"))
        rel = pathlib.PurePosixPath(name)
        if rel.is_absolute() or ".." in rel.parts or not name.startswith("artifacts/checkpoints/independent-v24-migration-20261003/"):
            raise ValueError("unsafe evidence path")
        if item["mode"] != "100644":
            raise ValueError("unexpected mode")
        data = z.read(name)
        blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        if len(data) != item["bytes"] or hashlib.sha256(data).hexdigest() != item["sha256"] or blob != item["git_blob_sha"]:
            raise ValueError("evidence hash mismatch: " + name)
        target = root.joinpath(*rel.parts)
        if any(parent.is_symlink() for parent in [target, *target.parents]):
            raise ValueError("symlink destination")
        if target.exists() and target.read_bytes() != data:
            raise ValueError("refusing to overwrite different bytes: " + name)
        checked.append((target, data))
    for target, data in checked:
        target.parent.mkdir(parents=True, exist_ok=True)
        if not target.exists():
            with target.open("xb") as f:
                f.write(data)
    print(json.dumps({"files": len(checked), "archive_sha256": manifest["sha256"], "original_local_commit": manifest["source_local_commit"], "all_bytes_verified": True}))

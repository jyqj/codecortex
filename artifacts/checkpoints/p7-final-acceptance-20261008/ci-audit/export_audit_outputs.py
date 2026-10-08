#!/usr/bin/env python3
"""Emit exact UTF-8 review files as bounded JSON chunks for connector readback."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import stat

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--prefix", required=True)
args = parser.parse_args()
root = args.root.resolve(strict=True)
prefix = PurePosixPath(args.prefix)
assert not prefix.is_absolute() and ".." not in prefix.parts
records = []
total = 0
for path in sorted(root.rglob("*")):
    assert not path.is_symlink(), "symlink output"
    if not path.is_file():
        continue
    assert stat.S_ISREG(path.stat().st_mode), "non-regular output"
    raw = path.read_bytes()
    total += len(raw)
    assert total <= 25 * 1024 * 1024, "audit export exceeds fixed total bound"
    text = raw.decode("utf-8")
    assert text.encode("utf-8") == raw
    relative = str(prefix / path.relative_to(root).as_posix())
    record = {"path":relative, "bytes":len(raw), "sha256":sha(raw),
              "characters":len(text), "chunks":(len(text)+7999)//8000,
              "mode":"100755" if path.stat().st_mode & 0o111 else "100644"}
    records.append(record)
    print("P7_AUDIT_EXPORT_FILE " + json.dumps(record, ensure_ascii=True), flush=True)
    for number, start in enumerate(range(0, len(text), 8000)):
        print("P7_AUDIT_EXPORT_CHUNK " + json.dumps({
            "path":relative, "index":number, "content":text[start:start+8000]
        }, ensure_ascii=True), flush=True)
manifest = {"schema_version":1, "files":records, "file_count":len(records),
            "total_bytes":total, "scope":"exact original audit outputs; not task approval"}
print("P7_AUDIT_EXPORT_MANIFEST " + json.dumps(manifest, ensure_ascii=True), flush=True)

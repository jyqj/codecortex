#!/usr/bin/env python3
"""Reassemble the exact selected lifecycle evidence container; never overwrite."""
import argparse
import hashlib
import json
from pathlib import Path

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--parts-dir", type=Path,
                        help="Optional directory containing the three part basenames.")
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name("publication-manifest.json").read_text())
    record = manifest["transport_adjustment"]
    repo = Path(__file__).resolve().parents[4]
    chunks = []
    for entry in record["parts"]:
        path = (args.parts_dir / entry["file_name"] if args.parts_dir
                else repo / entry["repository_path"])
        if path.is_symlink() or not path.is_file():
            raise SystemExit("Missing regular part: " + str(path))
        data = path.read_bytes()
        if len(data) != entry["bytes"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
            raise SystemExit("Part identity mismatch: " + str(path))
        chunks.append(data)
    data = b"".join(chunks)
    actual = hashlib.sha256(data).hexdigest()
    if len(data) != record["reassembled_bytes"] or actual != record["reassembled_sha256"]:
        raise SystemExit("Reassembled identity mismatch")
    with args.output.open("xb") as handle:
        handle.write(data)
    print(json.dumps({"status": "reassembled", "bytes": len(data), "sha256": actual,
                      "output": str(args.output)}))

if __name__ == "__main__":
    main()

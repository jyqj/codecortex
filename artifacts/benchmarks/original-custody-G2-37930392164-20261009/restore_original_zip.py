#!/usr/bin/env python3
"""Restore one unchanged original Actions ZIP from a checked-out custody tree.

Example:
  python restore_original_zip.py manifests/11609403178.json --output /tmp/11609403178.zip

Each chunk and the complete ZIP are checked. Existing output is never replaced.
No network requests, archive extraction, or benchmark execution are performed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib


def restore(manifest_path: pathlib.Path, output: pathlib.Path) -> dict:
    manifest_path = manifest_path.resolve(strict=True)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "codecortex-original-actions-zip-custody-v1":
        raise ValueError("unsupported custody manifest")
    root = manifest_path.parent.parent.resolve(strict=True)
    expected_size = manifest["original_zip_bytes"]
    expected_digest = manifest["original_zip_sha256"]
    if not isinstance(expected_size, int) or expected_size < 0:
        raise ValueError("invalid original ZIP size")
    if len(expected_digest) != 64 or any(c not in "0123456789abcdef" for c in expected_digest):
        raise ValueError("invalid original ZIP SHA256")
    chunks = manifest["chunks"]
    if not isinstance(chunks, list) or not chunks:
        raise ValueError("nonempty ordered chunks are required")
    output = output.absolute()
    if os.path.lexists(output):
        raise FileExistsError(f"refusing to replace {output}")
    partial = output.with_name(output.name + ".partial")
    total = 0
    whole = hashlib.sha256()
    created = False
    try:
        with partial.open("xb") as destination:
            created = True
            for chunk in chunks:
                if chunk["offset"] != total or not isinstance(chunk["bytes"], int) or chunk["bytes"] <= 0:
                    raise ValueError("noncontiguous or invalid chunk")
                relative = pathlib.PurePosixPath(chunk["path"])
                if relative.is_absolute() or ".." in relative.parts or relative.parts[:1] != ("chunks",):
                    raise ValueError("unsafe chunk path")
                source = (root / relative).resolve(strict=True)
                if not source.is_relative_to(root / "chunks") or not source.is_file():
                    raise ValueError("chunk must be a regular file inside custody chunks")
                digest = hashlib.sha256()
                git = hashlib.sha1(f"blob {chunk['bytes']}\0".encode())
                copied = 0
                with source.open("rb") as original:
                    while data := original.read(1024 * 1024):
                        copied += len(data)
                        if copied > chunk["bytes"]:
                            raise ValueError("oversized chunk")
                        digest.update(data)
                        git.update(data)
                        whole.update(data)
                        destination.write(data)
                if copied != chunk["bytes"] or digest.hexdigest() != chunk["sha256"] or git.hexdigest() != chunk["git_blob"]:
                    raise ValueError("chunk byte count or digest mismatch")
                total += copied
            if total != expected_size or whole.hexdigest() != expected_digest:
                raise ValueError("reconstructed original ZIP digest mismatch")
            destination.flush()
            os.fsync(destination.fileno())
        # A hard link creates the final name exclusively, including if another
        # process creates that name after the initial check. It never replaces it.
        os.link(partial, output)
        partial.unlink()
        created = False
    except BaseException:
        if created:
            partial.unlink(missing_ok=True)
        raise
    return {"artifact_id": manifest["artifact_id"], "output": str(output),
            "bytes": total, "sha256": whole.hexdigest(), "native_execution": False}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    arguments = parser.parse_args()
    print(json.dumps(restore(arguments.manifest, arguments.output), sort_keys=True, indent=2))


if __name__ == "__main__":
    main()

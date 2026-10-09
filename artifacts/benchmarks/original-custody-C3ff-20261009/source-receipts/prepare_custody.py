#!/usr/bin/env python3
"""Plan byte-preserving custody of already accepted original Actions ZIPs.

This hashes existing files. It neither extracts archives nor reruns a benchmark.
The private plan contains local paths; only public manifests may be published.
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import stat

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = pathlib.Path(__file__).resolve().parent
INPUT = ROOT / "original-C3ff-platform-gates/final-delivery-navigation-draft/benchmark-navigation-and-raw-custody-draft.json"
CHUNK_BYTES = 2 * 1024 * 1024
PREFIX = "artifacts/benchmarks/original-custody-C3ff-20261009"


def canonical(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def write_new(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as output:
        output.write(canonical(value))


def main() -> None:
    original_input = INPUT.read_bytes()
    draft = json.loads(original_input)
    records = []
    chunks = {}
    private_sources = []
    candidates = [item for item in draft["artifacts"] if item.get("local_zip_present_and_size_matches")]
    candidates.sort(key=lambda item: (item["expires_at"], item["artifact_id"]))
    for item in candidates:
        source = ROOT / item["local_original_zip"]
        before = source.stat()
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"not a regular original file: {source}")
        if before.st_size != item["zip_bytes"]:
            raise ValueError(f"original ZIP size changed: {source}")
        expected = item["official_zip_digest"].removeprefix("sha256:")
        if len(expected) != 64:
            raise ValueError("an official SHA256 is required")
        whole = hashlib.sha256()
        ordered = []
        offset = 0
        with source.open("rb") as original:
            while data := original.read(CHUNK_BYTES):
                whole.update(data)
                digest = hashlib.sha256(data).hexdigest()
                oid = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
                entry = {"offset": offset, "bytes": len(data), "sha256": digest,
                         "git_blob": oid, "path": f"chunks/{digest}.bin"}
                ordered.append(entry)
                previous = chunks.setdefault(digest, {
                    "bytes": len(data), "sha256": digest, "git_blob": oid,
                    "repository_path": f"{PREFIX}/chunks/{digest}.bin",
                    "local_original_zip": str(source), "offset": offset,
                })
                if previous["bytes"] != len(data) or previous["git_blob"] != oid:
                    raise ValueError("inconsistent content-addressed chunk")
                offset += len(data)
        after = source.stat()
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (
            after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns
        ):
            raise ValueError(f"original file changed while hashing: {source}")
        if whole.hexdigest() != expected or offset != item["zip_bytes"]:
            raise ValueError(f"official original ZIP digest mismatch: {source}")
        record = {
            "schema": "codecortex-original-actions-zip-custody-v1",
            "artifact_id": item["artifact_id"], "artifact_name": item["name"],
            "run_id": item["run_id"], "scope": item["scope"],
            "source_commit": item["source_commit"],
            "actual_execution_checkout": item["actual_execution_checkout"],
            "original_zip_filename": f"{item['artifact_id']}.zip",
            "original_zip_bytes": item["zip_bytes"], "original_zip_sha256": expected,
            "official_artifact_locator": item["actions_locator"],
            "official_created_at": item["created_at"], "official_expires_at": item["expires_at"],
            "chunk_bytes": CHUNK_BYTES, "chunks": ordered,
            "reconstruction": "concatenate all chunk bytes in ascending offset; no ZIP repacking",
            "acceptance_scope": item["acceptance_scope"],
            "acceptance_reviews": item["review_records"],
            "new_native_execution": False, "new_task_completion": False,
        }
        local_manifest = OUT / "manifests" / f"{item['artifact_id']}.json"
        write_new(local_manifest, record)
        payload = local_manifest.read_bytes()
        records.append({
            "artifact_id": item["artifact_id"], "run_id": item["run_id"], "scope": item["scope"],
            "bytes": item["zip_bytes"], "sha256": expected,
            "manifest": f"manifests/{item['artifact_id']}.json",
            "manifest_sha256": hashlib.sha256(payload).hexdigest(),
            "manifest_bytes": len(payload), "chunks": len(ordered),
        })
        private_sources.append({"artifact_id": item["artifact_id"], "local_path": str(source),
                                "dev": before.st_dev, "inode": before.st_ino,
                                "bytes": before.st_size, "mtime_ns": before.st_mtime_ns,
                                "sha256": expected})
    catalog = {
        "schema": "codecortex-original-actions-custody-catalog-v1",
        "source_commit": draft["source_commit"], "source_tree": draft["source_tree"],
        "artifacts": records, "artifact_count": len(records),
        "original_zip_bytes": sum(r["bytes"] for r in records),
        "unique_chunk_count": len(chunks), "unique_chunk_bytes": sum(c["bytes"] for c in chunks.values()),
        "input_manifest_sha256": hashlib.sha256(original_input).hexdigest(),
        "transport_only": True, "new_native_execution": False, "new_task_completion": False,
        "publication_state": "planned; confirm the published Git tree before claiming durable custody",
    }
    write_new(OUT / "catalog-planned.json", catalog)
    write_new(OUT / "private-transport-plan.json", {
        "repository_prefix": PREFIX, "source_stat_seals": private_sources,
        "chunks": list(chunks.values()), "catalog": catalog,
    })
    print(json.dumps({key: catalog[key] for key in (
        "artifact_count", "original_zip_bytes", "unique_chunk_count", "unique_chunk_bytes"
    )}, sort_keys=True))


if __name__ == "__main__":
    main()

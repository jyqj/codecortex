#!/usr/bin/env python3
"""Read-only verification of this checkpoint's original disk/archive contents.

No member is extracted to a filesystem path and no payload is executed.
The original manifest and archive bytes are pinned before archive parsing.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import stat
import tarfile

MANIFEST_SHA256 = "abaf329b6cff036c1e592faaff712aabc1be79f66d3bee0438191d16549a7c38"
ARCHIVE_SHA256 = "c7f7c705306f1a3b6a1452651bbde99e350a93252a5ed0381f1576d9d1bb1218"
ARCHIVE_BYTES = 12557
RAW_BYTES = 162940


def require(condition, message):
    if not condition:
        raise ValueError(message)


def safe_parts(name):
    require(isinstance(name, str) and name, "empty/non-string relative path")
    require("\\" not in name and ":" not in name, f"unsafe relative path: {name!r}")
    require(not any(ord(c) < 32 or ord(c) == 127 for c in name), "control character in path")
    parts = name.split("/")
    require(all(part not in ("", ".", "..") for part in parts), f"unsafe relative path: {name!r}")
    return parts


def regular_path(root, name):
    """Reject symlinks in every component; missing paths may exist in the archive."""
    parts = safe_parts(name)
    path = root
    for index, part in enumerate(parts):
        path = path / part
        try:
            mode = path.lstat().st_mode
        except FileNotFoundError:
            return None
        require(not stat.S_ISLNK(mode), f"symlink disk path: {name}")
        if index + 1 == len(parts):
            require(stat.S_ISREG(mode), f"non-regular disk file: {name}")
        else:
            require(stat.S_ISDIR(mode), f"non-directory disk ancestor: {name}")
    return path


def digest_stream(stream, expected_size):
    digest = hashlib.sha256()
    size = 0
    while block := stream.read(1024 * 1024):
        size += len(block)
        require(size <= expected_size, "stream exceeds declared size")
        digest.update(block)
    require(size == expected_size, "stream shorter than declared size")
    return digest.hexdigest()


def disk_digest(path, expected_size=None):
    size = path.stat().st_size
    if expected_size is not None:
        require(size == expected_size, f"unexpected size: {path.name}")
    with path.open("rb") as stream:
        return digest_stream(stream, size)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate manifest key: {key}")
        result[key] = value
    return result


def verify(root):
    root = root.resolve(strict=True)
    manifest_path = regular_path(root, "checkpoint-files.json")
    require(manifest_path is not None, "missing original manifest")
    with manifest_path.open("rb") as stream:
        manifest_bytes = stream.read(1024 * 1024 + 1)
    require(len(manifest_bytes) <= 1024 * 1024, "oversized original manifest")
    require(hashlib.sha256(manifest_bytes).hexdigest() == MANIFEST_SHA256,
            "original manifest changed")
    records = json.loads(manifest_bytes, object_pairs_hook=unique_object)
    require(isinstance(records, dict) and len(records) == 33, "unexpected manifest shape")
    for name, record in records.items():
        safe_parts(name)
        require(isinstance(record, dict) and set(record) == {"bytes", "sha256", "storage"},
                f"invalid record shape: {name}")
        require(type(record["bytes"]) is int and 0 <= record["bytes"] <= 16 * 1024 * 1024,
                f"invalid byte length: {name}")
        require(record["storage"] in ("archive", "disk"), f"invalid storage kind: {name}")
    manifest = {name: record["sha256"] for name, record in records.items()}
    for name, digest in manifest.items():
        safe_parts(name)
        require(isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest),
                f"invalid manifest digest: {name}")
    expected_archive = {name for name, record in records.items() if record["storage"] == "archive"}
    require(len(expected_archive) == 2, "unexpected archive membership")

    archive_path = regular_path(root, "raw-observations.tar.gz")
    require(archive_path is not None, "missing raw archive")
    with archive_path.open("rb") as stream:
        archive_bytes = stream.read(ARCHIVE_BYTES + 1)
    require(len(archive_bytes) == ARCHIVE_BYTES, "unexpected archive size")
    require(hashlib.sha256(archive_bytes).hexdigest() == ARCHIVE_SHA256, "archive bytes changed")
    require(archive_bytes[:4] == b"\x1f\x8b\x08\x00" and archive_bytes[4:8] == b"\x00" * 4,
            "noncanonical gzip header")
    archived = set()
    raw_bytes = 0
    previous_name = ""
    # Stream members rather than extracting them, with a fixed total payload
    # bound in addition to the complete archive hash checked above.
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r|gz") as archive:
        for member in archive:
            safe_parts(member.name)
            require(member.name in expected_archive, f"extra member: {member.name}")
            require(member.name not in archived, f"duplicate member: {member.name}")
            require(member.name > previous_name, "members are not sorted")
            require(member.type == tarfile.REGTYPE and member.sparse is None
                    and not member.pax_headers and not member.linkname
                    and member.offset_data - member.offset == tarfile.BLOCKSIZE,
                    f"non-plain regular member: {member.name}")
            require(member.mode == 0o644 and member.uid == member.gid == member.mtime == 0
                    and member.uname == member.gname == "",
                    f"noncanonical tar metadata: {member.name}")
            require(member.size == records[member.name]["bytes"], "unexpected member size")
            raw_bytes += member.size
            require(raw_bytes <= RAW_BYTES, "oversized total payload")
            payload = archive.extractfile(member)
            require(payload is not None, f"unreadable member: {member.name}")
            with payload:
                require(digest_stream(payload, member.size) == manifest[member.name],
                        f"member digest mismatch: {member.name}")
            archived.add(member.name)
            previous_name = member.name
    require(archived == expected_archive, "missing archive member")
    require(raw_bytes == RAW_BYTES, "unexpected total payload size")

    disk_files = mirrored = 0
    for name, expected_digest in manifest.items():
        path = regular_path(root, name)
        if path is not None:
            require(disk_digest(path, records[name]["bytes"]) == expected_digest, f"disk digest mismatch: {name}")
            disk_files += 1
            mirrored += name in archived
        else:
            require(name in archived, f"missing original file: {name}")
    return {
        "storage_verified": True,
        "original_manifest_sha256": MANIFEST_SHA256,
        "archive_sha256": ARCHIVE_SHA256,
        "original_files_verified": len(manifest),
        "archive_members_verified": len(archived),
        "disk_files_verified": disk_files,
        "mirrored_files_verified": mirrored,
        "raw_content_bytes": raw_bytes,
        "archive_bytes": ARCHIVE_BYTES,
        "filesystem_extraction": False,
        "payload_execution": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkpoint", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    try:
        result = verify(args.checkpoint)
    except (OSError, ValueError, tarfile.TarError) as error:
        parser.exit(1, f"storage verification failed: {error}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

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
import zipfile
import collections

MANIFEST_SHA256 = '4f92083f0f7bab440f02878eb1b4c3fdf9b61652cb353cd9719e51cdfb50c93b'
ARCHIVE_SHA256 = 'ad05f7ca5da7df3b82642d784a8d649430b253040af3fcd711504b34dd544524'
ARCHIVE_BYTES = 1006455
RAW_BYTES = 7295471


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
    require(isinstance(records, dict) and len(records) == 50, "unexpected manifest shape")
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
    require(len(expected_archive) == 46, "unexpected archive membership")

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



def verify_uploaded_zip_members(root):
    # Re-pin the exact bytes being parsed; no filesystem extraction occurs.
    raw = (root / "raw-observations.tar.gz").read_bytes()
    require(len(raw) == ARCHIVE_BYTES and hashlib.sha256(raw).hexdigest() == ARCHIVE_SHA256,
            "archive changed before nested ZIP verification")
    results = []
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        def read(name):
            stream = archive.extractfile(name)
            require(stream is not None, "missing indexed archive original")
            with stream:
                return stream.read()
        for run, artifact_id, head in [
            (37657882000, 11499237680, "eb7cdc55aa94c8d6865bed14fa37fff08080af33"),
            (37661087236, 11502030128, "b951f27d3ed50b7755bc2456c6425355f753ec17"),
        ]:
            prefix = str(run) + "/"
            payload = read(prefix + f"p7-engineering-{artifact_id}.zip")
            index = json.loads(read(prefix + "artifact-members.json"), object_pairs_hook=unique_object)
            api = json.loads(read(prefix + "artifacts-api.json"))
            require(api.get("isError") is False, "artifact API error")
            artifacts = api["structuredContent"]["artifacts"]
            require(len(artifacts) == 1 and artifacts[0]["id"] == artifact_id, "artifact identity mismatch")
            meta = artifacts[0]
            require(meta["workflow_run"]["id"] == run and meta["workflow_run"]["head_sha"] == head,
                    "artifact run/head mismatch")
            require(len(payload) == meta["size_in_bytes"] and
                    "sha256:" + hashlib.sha256(payload).hexdigest() == meta["digest"],
                    "ZIP differs from GitHub artifact metadata")
            members, content = set(), {}
            with zipfile.ZipFile(io.BytesIO(payload)) as zipped:
                for entry in zipped.infolist():
                    safe_parts(entry.filename)
                    require(entry.filename not in members and entry.filename in index, "extra/duplicate ZIP member")
                    require(not entry.is_dir() and stat.S_ISREG(entry.external_attr >> 16)
                            and not entry.flag_bits & 1, "unsafe ZIP member type")
                    require(0 <= entry.file_size <= 4 * 1024 * 1024, "oversized ZIP member")
                    data = zipped.read(entry)
                    require(len(data) == index[entry.filename]["size_bytes"] and
                            hashlib.sha256(data).hexdigest() == index[entry.filename]["sha256"],
                            "ZIP member differs from original member index")
                    members.add(entry.filename)
                    content[entry.filename] = data
            require(members == set(index) and len(members) == 26, "incomplete ZIP member set")
            require(content["source-before.json"] == content["source-after.json"], "source changed within job")
            before = json.loads(content["source-before.json"])
            expected = json.loads(read(prefix + "expected-source-head.json"))
            require(expected["source_commit"] == head, "wrong independently hashed Git head")
            for key in ["input_count", "inputs", "manifest_sha256", "source_tree"]:
                require(before[key] == expected[key], "snapshot mismatch: " + key)
            require(before["input_count"] == len(before["inputs"]) == 776, "wrong complete source count")
            actual = json.loads(read(prefix + "actual-rust-test-index.json"))
            all_cases, original = [], []
            for name, groups in actual.items():
                lines = content[name].decode().splitlines()
                for group in groups:
                    for case in group["test_cases"]:
                        line = lines[case["log_line"] - 1]
                        match = re.fullmatch(r"test (\S+) \.\.\. (ok|FAILED|ignored)(?:, (.*))?", line)
                        require(match is not None and group["target"] + "::" + match[1] == case["identity"]
                                and match[2] == case["result"], "test index does not match raw log line")
                    counts = collections.Counter(c["result"] for c in group["test_cases"])
                    require(tuple(counts[k] for k in ["ok", "FAILED", "ignored"]) ==
                            tuple(group["summary"][k] for k in ["passed", "failed", "ignored"]),
                            "test result index/summary mismatch")
                    all_cases.extend(group["test_cases"])
                    if name in ["deadline-cache.log", "query-encoding.log", "execution-deadline.log"]:
                        original.extend(group["test_cases"])
            wanted = json.loads(read(prefix + "expected-original58.json"))["test_cases"]
            require(len(original) == len(wanted) == 58, "missing original test identity")
            require(collections.Counter(c["identity"] for c in original) ==
                    collections.Counter(c["identity"] for c in wanted) and
                    len({c["identity"] for c in original}) == 58 and
                    all(c["result"] == "ok" for c in original), "original58 mismatch or non-passing result")
            totals = collections.Counter(c["result"] for c in all_cases)
            require(totals == {"ok": 95, "ignored": 1}, "unexpected observed Rust scope")
            python = json.loads(read(prefix + "actual-python-test-index.json"))
            lines = content["input-lock.log"].decode().splitlines()
            for case in python:
                match = re.fullmatch(r"test_\S+ \(([^)]+)\) \.\.\. (ok|FAIL|ERROR|skipped.*)",
                                     lines[case["log_line"] - 1])
                require(match is not None and match[1] == case["identity"] and match[2] == case["result"],
                        "Python index does not match raw log line")
            require(len(python) == 15 and all(c["result"] == "ok" for c in python), "Python scope differs")
            results.append({"run": run, "zip_members_verified": len(members),
                            "zip_sha256": hashlib.sha256(payload).hexdigest(),
                            "source_inputs": before["input_count"], "manifest_sha256": before["manifest_sha256"],
                            "original58_passed": len(original), "rust_passed": totals["ok"],
                            "python_passed": len(python), "ignored": totals["ignored"],
                            "raw_test_index_lines_verified": len(all_cases) + len(python)})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkpoint", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    try:
        result = verify(args.checkpoint)
        result["uploaded_artifacts"] = verify_uploaded_zip_members(args.checkpoint.resolve(strict=True))
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile) as error:
        parser.exit(1, f"storage verification failed: {error}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

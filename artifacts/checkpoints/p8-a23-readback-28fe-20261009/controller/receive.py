#!/usr/bin/env python3
"""Read fixed A23 GitHub artifacts without executing their product binaries."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import stat
import struct
import zlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

SOURCE = "a23bb72d3c954f385b99fe81ce9189885c208557"
SOURCE_TREE = "58147c952505c44da1f41eb4b9c31643f2303b96"
REPO = "jyqj/codecortex"
API = "https://api.github.com/repos/" + REPO
MAX_EXPANDED_BYTES = 3 * 1024 * 1024 * 1024
MAX_MEMBERS = 50000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def write_new(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write("\n")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        return None


def validate_locator(location, token=None):
    require(isinstance(location, str) and location and
            not any(ord(char) <= 32 or ord(char) == 127 for char in location),
            "artifact locator has invalid characters")
    parsed = urllib.parse.urlsplit(location)
    require(parsed.scheme == "https" and parsed.hostname and not parsed.username and
            not parsed.password and not parsed.fragment,
            "artifact locator must be credential-free HTTPS")
    if token:
        require(token not in location and token not in urllib.parse.unquote(location),
                "artifact locator contains an authentication credential")
    return parsed


class HTTPSRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, token):
        super().__init__()
        self.token = token

    def redirect_request(self, request, fp, code, message, headers, newurl):
        try:
            validate_locator(newurl, self.token)
            result = super().redirect_request(request, fp, code, message, headers, newurl)
            if result is not None:
                result.remove_header("Authorization")
                result.remove_header("Cookie")
            return result
        except Exception as error:
            raise RuntimeError("Artifact redirect rejected (" + type(error).__name__ +
                               "); locator not logged") from None


def api_request(path, token):
    require(path.startswith("/") and "?" not in path and "://" not in path,
            "unexpected GitHub API relative path")
    request = urllib.request.Request(
        API + path,
        headers={"Authorization": "Bearer " + token,
                 "Accept": "application/vnd.github+json",
                 "X-GitHub-Api-Version": "2022-11-28",
                 "User-Agent": "codecortex-fixed-a23-readback"},
    )
    return urllib.request.build_opener(NoRedirect()).open(request, timeout=90)


def api_json(path, token, destination):
    try:
        with api_request(path, token) as response:
            raw = response.read(2 * 1024 * 1024 + 1)
        require(len(raw) <= 2 * 1024 * 1024, "GitHub metadata exceeds transport bound")
        value = json.loads(raw)
        with Path(destination).open("xb") as output:
            output.write(raw)
        return value
    except urllib.error.HTTPError as error:
        raise RuntimeError("GitHub metadata request returned HTTP " + str(error.code)) from None
    except (urllib.error.URLError, TimeoutError, OSError):
        raise RuntimeError("GitHub metadata transport failed; no authenticated URL logged") from None


def archive_response(artifact_id, token):
    try:
        try:
            return api_request("/actions/artifacts/" + str(artifact_id) + "/zip", token)
        except urllib.error.HTTPError as error:
            if error.code not in (301, 302, 303, 307, 308):
                code = error.code
                error.close()
                raise RuntimeError("GitHub archive request returned HTTP " + str(code)) from None
            location = error.headers.get("Location", "")
            error.close()
        validate_locator(location, token)
        # A new request carries no bearer/cookie. All later redirect locations
        # are validated and still receive no authentication header.
        request = urllib.request.Request(
            location, headers={"User-Agent": "codecortex-fixed-a23-readback"})
        return urllib.request.build_opener(HTTPSRedirect(token)).open(request, timeout=90)
    except Exception as error:
        raise RuntimeError("Archive connection failed (" + type(error).__name__ +
                           "); authenticated and signed locators not logged") from None


def member_payload(raw, item, output):
    """Read the exact compressed member and require true EOF, size and CRC."""
    raw.seek(item.header_offset)
    header = raw.read(30)
    require(len(header) == 30, "truncated ZIP local header")
    signature, version, flags, method, mt, md, crc, csize, usize, nlen, elen = struct.unpack(
        "<4s5H3I2H", header)
    require(signature == b"PK\x03\x04" and flags == item.flag_bits and
            method == item.compress_type, "ZIP local/central header mismatch")
    name = raw.read(nlen)
    extra = raw.read(elen)
    require(len(name) == nlen and len(extra) == elen, "truncated ZIP local name/extra")
    require(name.decode("utf-8" if flags & 0x800 else "cp437") == item.orig_filename,
            "ZIP local/central filename mismatch")
    require(method in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
            "unsupported original ZIP compression method")
    if not flags & 8:
        require(crc == item.CRC, "ZIP local/central CRC mismatch")
    data_start = raw.tell()
    require(data_start + item.compress_size <= os.fstat(raw.fileno()).st_size,
            "compressed ZIP member lies beyond original bytes")
    decoder = zlib.decompressobj(-15) if method == zipfile.ZIP_DEFLATED else None
    count = 0
    actual_crc = 0
    digest = hashlib.sha256()

    def consume(data):
        nonlocal count, actual_crc
        count += len(data)
        require(count <= item.file_size, "true ZIP expansion exceeds declared member size")
        require(not item.is_dir() or not data, "directory ZIP member contains payload")
        actual_crc = zlib.crc32(data, actual_crc)
        digest.update(data)
        if output is not None:
            output.write(data)

    remaining = item.compress_size
    while remaining:
        block = raw.read(min(65536, remaining))
        require(bool(block), "truncated compressed ZIP member")
        remaining -= len(block)
        if decoder is None:
            consume(block)
            continue
        pending = block
        while pending:
            previous = len(pending)
            data = decoder.decompress(pending, min(1048576, item.file_size - count + 1))
            pending = decoder.unconsumed_tail
            consume(data)
            require(not decoder.unused_data, "bytes remain after the DEFLATE end marker")
            require(data or len(pending) < previous, "DEFLATE stream made no progress")
    if decoder is not None:
        require(decoder.eof and not decoder.unused_data and not decoder.unconsumed_tail,
                "DEFLATE stream did not end exactly at the member boundary")
    require(count == item.file_size and actual_crc & 0xFFFFFFFF == item.CRC,
            "actual complete ZIP member length/CRC mismatch")
    return {"bytes": count, "sha256": digest.hexdigest(),
            "zip_crc32": f"{item.CRC:08x}",
            "original_zip_mode": item.external_attr >> 16,
            "actual_compressed_eof_verified": True,
            "local_header_offset": item.header_offset,
            "compressed_data_start": data_start,
            "compressed_data_end": data_start + item.compress_size}


def safe_extract(archive, destination):
    destination = Path(destination)
    destination.mkdir()
    result = {}
    with zipfile.ZipFile(archive) as zipped, Path(archive).open("rb") as raw:
        members = zipped.infolist()
        require(len(members) <= MAX_MEMBERS, "ZIP member transport bound exceeded")
        require(sum(item.file_size for item in members) <= MAX_EXPANDED_BYTES,
                "ZIP expansion transport bound exceeded")
        names = set()
        for item in members:
            name = item.filename
            parts = name.rstrip("/").split("/")
            require(name and name == item.orig_filename and not name.startswith("/") and
                    "\\" not in name and "\x00" not in name and
                    all(part not in ("", ".", "..") for part in parts) and
                    ":" not in parts[0], "unsafe ZIP member path")
            canonical = "/".join(parts)
            require(canonical not in names, "duplicate ZIP member")
            names.add(canonical)
            kind = stat.S_IFMT(item.external_attr >> 16)
            require(kind in (0, stat.S_IFREG, stat.S_IFDIR), "non-regular ZIP member")
            require(not (item.flag_bits & 1), "encrypted ZIP member")
            require(kind != stat.S_IFDIR or item.is_dir(), "ZIP directory mode mismatch")
            require(not item.is_dir() or item.file_size == 0,
                    "directory ZIP member declares payload")
        ranges = []
        for item in members:
            target = destination.joinpath(*PurePosixPath(item.filename).parts)
            if item.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                record = member_payload(raw, item, None)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("xb") as output:
                    record = member_payload(raw, item, output)
            ranges.append((record["local_header_offset"], record["compressed_data_end"]))
            result[item.filename] = record
        ranges.sort()
        require(all(left[1] <= right[0] for left, right in zip(ranges, ranges[1:])),
                "ZIP local member byte ranges overlap")
    return result


def freeze_inputs(directory):
    for path in Path(directory).rglob("*"):
        require(not path.is_symlink(), "input symlink appeared")
        if path.is_file():
            path.chmod(0o444)
    for path in sorted(Path(directory).rglob("*"), key=lambda x: len(x.parts), reverse=True):
        if path.is_dir():
            path.chmod(0o555)
    Path(directory).chmod(0o555)


def inventory(directory):
    root = Path(directory)
    result = {}
    for path in sorted(root.rglob("*")):
        require(not path.is_symlink(), "input symlink appeared during readback")
        if path.is_file():
            result[path.relative_to(root).as_posix()] = {
                "bytes": path.stat().st_size, "sha256": sha(path)}
    return result


def receive_one(spec, root, token):
    artifact_id = spec["id"]
    transport = root / "transport"
    original = root / "raw" / str(artifact_id)
    original.mkdir(parents=True)
    metadata = api_json("/actions/artifacts/" + str(artifact_id), token,
                        transport / (str(artifact_id) + "-metadata.json"))
    require(metadata["id"] == artifact_id and metadata["expired"] is False,
            "artifact identity/expiry mismatch")
    require(metadata["name"] == spec["name"] and
            metadata["size_in_bytes"] == spec["bytes"] and
            metadata["digest"] == "sha256:" + spec["sha256"],
            "official archive length or digest differs from fixed registration")
    run = metadata["workflow_run"]
    require(run["id"] == spec["run_id"] and run["head_sha"] == SOURCE and
            run["repository_id"] == spec["repository_id"] == 1249213794 and
            run["head_repository_id"] == spec["head_repository_id"] == 1249213794,
            "artifact run/source mismatch")
    archive = original / "original.zip"
    try:
        with archive_response(artifact_id, token) as response, archive.open("xb") as output:
            count = 0
            while block := response.read(1024 * 1024):
                count += len(block)
                require(count <= spec["bytes"], "download exceeds registered ZIP length")
                output.write(block)
    except (urllib.error.URLError, TimeoutError, OSError):
        raise RuntimeError("Archive byte transport failed; partial bytes retained") from None
    require(archive.stat().st_size == spec["bytes"] and sha(archive) == spec["sha256"],
            "downloaded ZIP differs from the fixed original")
    members = safe_extract(archive, original / "extracted")
    write_new(transport / (str(artifact_id) + "-members.json"), members)
    frozen = inventory(original)
    write_new(transport / (str(artifact_id) + "-input-before.json"), frozen)
    freeze_inputs(original)
    return {"artifact_id": artifact_id, "run_id": spec["run_id"], "passed": True,
            "zip_sha256": spec["sha256"], "zip_bytes": spec["bytes"],
            "members": len(members),
            "expanded_bytes": sum(item["bytes"] for item in members.values()),
            "input_inventory": frozen}


def source_identity(source):
    environment = dict(os.environ, GIT_OPTIONAL_LOCKS="0")
    def git(*args):
        completed = subprocess.run(["git", "-C", str(source), *args],
                                   check=True, capture_output=True, text=True,
                                   timeout=90, env=environment)
        return completed.stdout.strip()
    require(git("rev-parse", "HEAD") == SOURCE, "measured source commit differs")
    require(git("rev-parse", "HEAD^{tree}") == SOURCE_TREE, "measured source tree differs")
    require(git("status", "--porcelain", "--untracked-files=no") == "",
            "measured source has tracked modifications")
    require(not list((source / "scripts").rglob("*.pyc")) and
            not list((source / "scripts").rglob("__pycache__")),
            "source contains stale Python bytecode")
    require(sha(source / "scripts/p7_build_identity.py") ==
            "5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5",
            "source identity helper differs before import")
    sys.path.insert(0, str(source / "scripts"))
    from p7_build_identity import source_snapshot
    snapshot = source_snapshot(source)
    require(snapshot["source_commit"] == SOURCE and snapshot["input_count"] == 1087 and
            snapshot["manifest_sha256"] ==
            "4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00",
            "actual native source manifest differs")
    return {"commit": SOURCE, "tree": SOURCE_TREE, "snapshot": snapshot}


def self_check():
    cases = []
    with tempfile.TemporaryDirectory(prefix="a23-readback-fixtures-") as temporary:
        base = Path(temporary)
        valid = base / "valid.zip"
        with zipfile.ZipFile(valid, "w", compression=zipfile.ZIP_STORED) as zipped:
            zipped.writestr("nested/a.txt", "known bytes")
        rows = safe_extract(valid, base / "valid")
        require(rows["nested/a.txt"]["sha256"] == hashlib.sha256(b"known bytes").hexdigest(),
                "valid fixture digest changed")
        cases.append("valid_nested_member")
        for index, name in enumerate(("../escape", "/absolute", "a\\escape", "C:/escape")):
            archive = base / ("bad-path-" + str(index) + ".zip")
            with zipfile.ZipFile(archive, "w") as zipped:
                zipped.writestr(name, b"x")
            try:
                safe_extract(archive, base / ("bad-path-" + str(index)))
            except ValueError:
                cases.append("rejected_path_" + str(index))
            else:
                raise AssertionError("unsafe path fixture accepted")
        link = base / "link.zip"
        with zipfile.ZipFile(link, "w") as zipped:
            item = zipfile.ZipInfo("link")
            item.create_system = 3
            item.external_attr = (stat.S_IFLNK | 0o777) << 16
            zipped.writestr(item, "../escape")
        try:
            safe_extract(link, base / "link")
        except ValueError:
            cases.append("rejected_symlink")
        else:
            raise AssertionError("symlink fixture accepted")
        corrupt = base / "corrupt.zip"
        data = valid.read_bytes()
        require(data.count(b"known bytes") == 1, "CRC fixture data is ambiguous")
        corrupt.write_bytes(data.replace(b"known bytes", b"wrong bytes"))
        try:
            safe_extract(corrupt, base / "corrupt")
        except (zipfile.BadZipFile, ValueError):
            cases.append("rejected_corrupt_crc")
        else:
            raise AssertionError("corrupt CRC fixture accepted")
        directory_crc = base / "directory-crc.zip"
        with zipfile.ZipFile(directory_crc, "w") as zipped:
            zipped.writestr("empty/", b"")
        damaged = bytearray(directory_crc.read_bytes())
        central = damaged.index(b"PK\x01\x02")
        struct.pack_into("<I", damaged, 14, 1)
        struct.pack_into("<I", damaged, central + 16, 1)
        directory_crc.write_bytes(damaged)
        try:
            safe_extract(directory_crc, base / "directory-crc")
        except (zipfile.BadZipFile, ValueError):
            cases.append("rejected_directory_crc")
        else:
            raise AssertionError("nonzero empty-directory CRC accepted")
        directory_payload = base / "directory-payload.zip"
        with zipfile.ZipFile(directory_payload, "w") as zipped:
            zipped.writestr("payload/", b"hidden")
        try:
            safe_extract(directory_payload, base / "directory-payload")
        except ValueError:
            cases.append("rejected_directory_payload")
        else:
            raise AssertionError("directory payload accepted")
        prefix = base / "deflate-prefix.zip"
        with zipfile.ZipFile(prefix, "w", compression=zipfile.ZIP_DEFLATED) as zipped:
            zipped.writestr("data", b"head-hidden-tail")
        positive = safe_extract(prefix, base / "deflate-positive")
        require(positive["data"]["sha256"] == hashlib.sha256(b"head-hidden-tail").hexdigest(),
                "valid DEFLATE fixture changed")
        cases.append("valid_complete_deflate_member")
        damaged = bytearray(prefix.read_bytes())
        central = damaged.index(b"PK\x01\x02")
        prefix_crc = zlib.crc32(b"head") & 0xFFFFFFFF
        struct.pack_into("<I", damaged, 14, prefix_crc)
        struct.pack_into("<I", damaged, 22, 4)
        struct.pack_into("<I", damaged, central + 16, prefix_crc)
        struct.pack_into("<I", damaged, central + 24, 4)
        prefix.write_bytes(damaged)
        try:
            safe_extract(prefix, base / "deflate-prefix")
        except (ValueError, zipfile.BadZipFile, zlib.error):
            cases.append("rejected_declared_prefix_with_hidden_deflate_tail")
        else:
            raise AssertionError("truncated DEFLATE prefix accepted")
    print(json.dumps({"schema": "a23-readback-transport-controls-v1",
                      "passed": True, "cases": cases, "native_executions": 0}))
    return 0


def stage_upload(audit_root, controller, destination):
    """Hard-link only regular files; never traverse source or raw-view symlinks."""
    destination = Path(destination).resolve()
    destination.mkdir(parents=True, exist_ok=False)
    record = {"schema": "a23-readback-upload-custody-v1",
              "regular_files": {}, "excluded_symlink_views": [],
              "missing_roots": [], "errors": []}
    for label, origin in (("audit", Path(audit_root)), ("controller", Path(controller))):
        if not origin.exists():
            record["missing_roots"].append(label)
            continue
        require(not origin.is_symlink(), "upload origin must not be a symlink")
        require(destination != origin.resolve() and origin.resolve() not in destination.parents,
                "upload destination must be outside copied trees")
        for current, directories, files in os.walk(origin, followlinks=False):
            current = Path(current)
            kept = []
            for name in sorted(directories):
                entry = current / name
                if entry.is_symlink():
                    record["excluded_symlink_views"].append(
                        label + "/" + entry.relative_to(origin).as_posix())
                else:
                    require(entry.is_dir(), "unexpected upload directory entry")
                    kept.append(name)
            directories[:] = kept
            for name in sorted(files):
                original = current / name
                relative = Path(label) / original.relative_to(origin)
                if original.is_symlink():
                    record["excluded_symlink_views"].append(relative.as_posix())
                    continue
                require(stat.S_ISREG(original.lstat().st_mode),
                        "upload input is not a regular file")
                target = destination / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                before = {"bytes": original.stat().st_size, "sha256": sha(original)}
                try:
                    os.link(original, target)
                except OSError as error:
                    import errno
                    if error.errno != errno.EXDEV:
                        raise
                    shutil.copyfile(original, target)
                require(target.stat().st_size == before["bytes"] and sha(target) == before["sha256"],
                        "upload staging changed a regular file")
                record["regular_files"][relative.as_posix()] = before
    record["passed"] = bool(record["regular_files"] and not record["errors"])
    write_new(destination / "upload-custody.json", record)
    print(json.dumps({"schema": record["schema"], "passed": record["passed"],
                      "regular_file_count": len(record["regular_files"]),
                      "excluded_symlink_views": record["excluded_symlink_views"],
                      "missing_roots": record["missing_roots"],
                      "manifest_sha256": sha(destination / "upload-custody.json")}))
    return 0 if record["passed"] else 2


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-check", action="store_true")
    parser.add_argument("--stage-upload", action="store_true")
    parser.add_argument("--upload", type=Path)
    parser.add_argument("--group", choices=("runtime", "platform"))
    parser.add_argument("--source", type=Path)
    parser.add_argument("--controller", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.self_check:
        return self_check()
    if args.stage_upload:
        require(args.output and args.controller and args.upload,
                "staging requires output/controller/upload")
        return stage_upload(args.output, args.controller, args.upload)
    require(args.group and args.source and args.controller and args.output,
            "group/source/controller/output are required")
    source = args.source.resolve(strict=True)
    controller = args.controller.resolve(strict=True)
    root = args.output.resolve()
    require(root != source and source not in root.parents and
            root != controller and controller not in root.parents,
            "readback output must be separate from source/controller")
    root.mkdir(parents=True, exist_ok=False)
    (root / "transport").mkdir()
    (root / "raw").mkdir()
    (root / "source").symlink_to(source, target_is_directory=True)
    receipt = {"schema": "a23-original-readback-execution-v1", "group": args.group,
               "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
               "controller_commit": os.environ.get("GITHUB_SHA"),
               "run_id": os.environ.get("GITHUB_RUN_ID"),
               "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
               "platform": platform.platform(), "python": sys.version,
               "passed": False, "transport": [], "errors": [],
               "scope": "Actual Linux data readback only. Inherited Mac scope strings in "
                        "unchanged historical reviewer code describe its original author run, "
                        "not this execution. No product, statistics or oracle ELF, Cargo build, "
                        "performance workload, fault injection or native CLI replay is run. "
                        "No original TODO status is changed.",
               "original_tasks": {"total": 192, "done": 163, "remaining": 29, "newly_closed": 0}}
    before = None
    controller_before = None
    try:
        protocol = json.loads((controller / "protocol.json").read_text())
        require(protocol["source_commit"] == SOURCE and protocol["source_tree"] == SOURCE_TREE,
                "readback protocol source mismatch")
        specifications = protocol["groups"][args.group]
        require(len({item["id"] for item in specifications}) == len(specifications),
                "duplicate registered artifact")
        require(not list(controller.rglob("*.pyc")) and
                not list(controller.rglob("__pycache__")),
                "controller contains stale Python bytecode")
        controller_before = inventory(controller)
        write_new(root / "controller-inputs-before.json", controller_before)
        before = source_identity(source)
        write_new(root / "source-before.json", before)
        token = os.environ.pop("GH_TOKEN", "")
        require(bool(token), "read-only GitHub artifact token unavailable")
        for specification in specifications:
            try:
                item = receive_one(specification, root, token)
            except Exception as error:
                item = {"artifact_id": specification["id"], "passed": False,
                        "error_type": type(error).__name__, "error": str(error),
                        "transport_status": "incomplete_transport"}
                original = root / "raw" / str(specification["id"])
                if original.exists():
                    try:
                        item["input_inventory"] = inventory(original)
                        write_new(root / "transport" /
                                  (str(specification["id"]) + "-input-before.json"),
                                  item["input_inventory"])
                        freeze_inputs(original)
                        item["partial_inputs_frozen"] = True
                    except Exception as custody_error:
                        item["partial_inputs_frozen"] = False
                        item["custody_error"] = type(custody_error).__name__
                        receipt["errors"].append({
                            "type": "PartialTransportCustodyFailure",
                            "artifact_id": specification["id"]})
            receipt["transport"].append(item)
        token = None
        module_name = args.group + "_readback"
        path = controller / (module_name + ".py")
        specification = importlib.util.spec_from_file_location(module_name, path)
        require(specification is not None and specification.loader is not None,
                "readback module is unavailable")
        module = importlib.util.module_from_spec(specification)
        source_code = path.read_bytes()
        exec(compile(source_code, str(path), "exec"), module.__dict__)
        review = getattr(module, "review_" + args.group)(root, source, controller)
        require(isinstance(review, dict) and type(review.get("passed")) is bool,
                "readback module has no explicit boolean result")
        receipt["review"] = review
    except Exception as error:
        receipt["errors"].append({"type": type(error).__name__, "message": str(error)})
    finally:
        os.environ.pop("GH_TOKEN", None)
        try:
            after = source_identity(source)
            write_new(root / "source-after.json", after)
            receipt["source_unchanged"] = before is not None and before == after
            receipt["source_manifest_sha256"] = after["snapshot"]["manifest_sha256"]
        except Exception as error:
            receipt["source_unchanged"] = False
            receipt["errors"].append({"type": type(error).__name__, "message": str(error)})
        for item in receipt["transport"]:
            if "input_inventory" in item:
                artifact_id = item["artifact_id"]
                try:
                    after = inventory(root / "raw" / str(artifact_id))
                    write_new(root / "transport" / (str(artifact_id) + "-input-after.json"), after)
                    item["input_unchanged"] = after == item.pop("input_inventory")
                except Exception as error:
                    item["input_unchanged"] = False
                    receipt["errors"].append({"type": type(error).__name__, "message": str(error)})
        try:
            after = inventory(controller)
            write_new(root / "controller-inputs-after.json", after)
            receipt["controller_unchanged"] = controller_before is not None and controller_before == after
        except Exception as error:
            receipt["controller_unchanged"] = False
            receipt["errors"].append({"type": type(error).__name__, "message": str(error)})
        receipt["passed"] = bool(
            not receipt["errors"] and receipt.get("source_unchanged") is True and
            receipt.get("controller_unchanged") is True and receipt["transport"] and
            all(item["passed"] and item.get("input_unchanged") is True
                for item in receipt["transport"]) and
            receipt.get("review", {}).get("passed") is True)
        receipt["finished_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        write_new(root / "execution-receipt.json", receipt)
        # Original full receipts and all input bytes are also uploaded as artifacts.
        print("A23_READBACK_EXECUTION_RECEIPT_BEGIN")
        print(json.dumps(receipt, ensure_ascii=False, sort_keys=True))
        print("A23_READBACK_EXECUTION_RECEIPT_END")
    return 0 if receipt["passed"] else 2


if __name__ == "__main__":
    raise SystemExit(main())

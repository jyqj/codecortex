#!/usr/bin/env python3
"""Freeze and archive bounded local P8 inputs; never certify a release or call models."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import stat
import subprocess
import sys
import threading
import uuid


SCHEMA = 1
SCOPE = "local_engineering_only"
MAX_JSON = 16 * 1024 * 1024
MAX_FILE = 1024 * 1024 * 1024
MAX_TOTAL = 2 * 1024 * 1024 * 1024
MAX_FILES = 20000
MAX_GIT = 8 * 1024 * 1024
ENV_ALLOWLIST = ("LANG", "LC_ALL", "TZ", "CARGO_BUILD_JOBS", "RAYON_NUM_THREADS")
GATES = {"passed_local": 0, "passed": 0,
         "baseline_recorded_not_quality_certified": 0,
         "failed": 1, "inconclusive": 1, "gate_failed": 1,
         "invalid_measurement": 2, "cancelled": 3}
SECRET_KEY = re.compile(r"(?i)(api[_-]?key|secret|password|authorization|credential|"
                        r"access[_-]?token|refresh[_-]?token|private[_-]?key)")


class Invalid(ValueError):
    """Invalid inputs or integrity drift, mapped to benchmark exit code 2."""


def require(condition, message):
    if not condition:
        raise Invalid(message)


def canonical(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True,
                       allow_nan=False) + "\n").encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def name(value):
    """One portable relative path; no control chars, backslashes, or git pathspecs."""
    require(isinstance(value, str) and 0 < len(value) <= 1024, "invalid relative path")
    parts = value.split("/")
    require(not value.startswith("/") and "\\" not in value
            and all(part not in ("", ".", "..") for part in parts)
            and not any(ord(c) < 32 or ord(c) == 127 for c in value),
            "unsafe relative path")
    require(len(parts) <= 32 and all(len(p) <= 255 for p in parts), "path depth/length limit")
    require(not any(p == ".git" or p.startswith(".env") or
                    re.search(r"hold[-_]?out|held[-_]?out", p, re.I) for p in parts),
            "protected metadata, credential, or holdout path")
    return value


def path(value, *, exists=True):
    raw = os.fspath(value)
    require(".." not in raw.split(os.sep), "path traversal not allowed")
    result = Path(os.path.abspath(raw))
    name("/".join(result.parts[1:]))
    # Check every component before resolving, so symlinks cannot hide behind resolve().
    current = Path(result.anchor)
    for part in result.parts[1:]:
        current /= part
        try:
            info = current.lstat()
        except FileNotFoundError:
            require(not exists, "input path does not exist")
            break
        require(not stat.S_ISLNK(info.st_mode), "symlink path not allowed")
    require(not exists or result.exists(), "input path does not exist")
    return result


def disjoint(output, inputs):
    for item in inputs:
        require(not output.is_relative_to(item) and not item.is_relative_to(output),
                "output overlaps an input or would archive itself")


class Budget:
    def __init__(self):
        self.bytes = 0
        self.files = 0

    def add(self, size):
        self.files += 1
        self.bytes += size
        require(self.files <= MAX_FILES and self.bytes <= MAX_TOTAL, "input budget exceeded")


def file_record(source, destination=None, *, budget=None, limit=MAX_FILE, _chunks=None):
    source = path(source)
    info = source.lstat()
    require(stat.S_ISREG(info.st_mode), "input must be a regular file")
    require(info.st_size <= limit, "file size limit exceeded")
    if budget is not None:
        budget.add(info.st_size)
    descriptor = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
                         | getattr(os, "O_NONBLOCK", 0))
    writer = None
    try:
        with os.fdopen(descriptor, "rb") as stream:
            before = os.fstat(stream.fileno())
            require(stat.S_ISREG(before.st_mode) and
                    (before.st_dev, before.st_ino) == (info.st_dev, info.st_ino),
                    "input replaced while opening")
            if destination is not None:
                destination.parent.mkdir(parents=True, exist_ok=True)
                writer = destination.open("xb")
            h = hashlib.sha256()
            count = 0
            while chunk := stream.read(1024 * 1024):
                count += len(chunk)
                require(count <= limit and count <= info.st_size, "input grew during read")
                h.update(chunk)
                if _chunks is not None:
                    _chunks.append(chunk)
                if writer is not None:
                    writer.write(chunk)
            after = os.fstat(stream.fileno())
        current = path(source).lstat()
        identity = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns,
                              stat.S_IMODE(s.st_mode))
        require(identity(info) == identity(after) == identity(current) and count == info.st_size,
                "input changed during read")
        if destination is not None:
            destination.chmod(0o755 if info.st_mode & 0o111 else 0o644)
        return {"sha256": h.hexdigest(), "bytes": count,
                "executable": bool(info.st_mode & 0o111)}
    finally:
        if writer is not None:
            writer.flush()
            os.fsync(writer.fileno())
            writer.close()


def read_bytes(source, *, limit):
    chunks = []
    file_record(source, limit=limit, _chunks=chunks)
    return b"".join(chunks)


def read_json(source):
    data = read_bytes(source, limit=MAX_JSON)
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    try:
        value = json.loads(data, object_pairs_hook=unique,
                           parse_constant=lambda _: (_ for _ in ()).throw(Invalid("nonfinite JSON")))
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise Invalid("invalid JSON input") from exc
    require(isinstance(value, dict), "JSON object required")
    return value


def write_json(target, value):
    data = canonical(value)
    require(len(data) <= MAX_JSON, "manifest size limit exceeded")
    with target.open("xb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def git(root, *args):
    # Only local plumbing commands; stdout and wall time are bounded. No hooks/builds.
    env = {**os.environ, "GIT_OPTIONAL_LOCKS": "0", "GIT_TERMINAL_PROMPT": "0"}
    with subprocess.Popen(["git", "--no-pager", "-c", "core.fsmonitor=false", "-C", str(root), *args],
                          stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, env=env) as process:
        timed_out = threading.Event()
        def stop():
            timed_out.set()
            process.kill()
        timer = threading.Timer(30, stop)
        timer.daemon = True
        timer.start()
        try:
            output = process.stdout.read(MAX_GIT + 1)
            if len(output) > MAX_GIT:
                process.kill()
            code = process.wait()
            require(not timed_out.is_set() and len(output) <= MAX_GIT and code == 0,
                    "git inventory failed, timed out, or exceeded its output limit")
            return output
        finally:
            timer.cancel()


def source_identity(root, prefixes, destination=None, budget=None):
    prefixes = [name(p) for p in prefixes]
    require(git(root, "rev-parse", "--show-toplevel").decode().strip() == str(root),
            "source-root must be the Git worktree root")
    specs = [":(literal)" + p for p in prefixes]
    head = git(root, "rev-parse", "HEAD").decode().strip()
    tree = git(root, "rev-parse", "HEAD^{tree}").decode().strip()
    committed, index = {}, {}
    for row in git(root, "ls-tree", "-r", "-z", "HEAD", "--", *specs).split(b"\0"):
        if not row:
            continue
        meta, raw_name = row.split(b"\t", 1)
        mode, kind, oid = meta.decode().split()
        require(kind == "blob" and mode in ("100644", "100755"), "unbound symlink/submodule")
        committed[name(raw_name.decode())] = {"mode": mode, "oid": oid}
    for row in git(root, "ls-files", "--stage", "-z", "--", *specs).split(b"\0"):
        if not row:
            continue
        meta, raw_name = row.split(b"\t", 1)
        mode, oid, stage = meta.decode().split()
        require(stage == "0" and mode in ("100644", "100755"), "unmerged/symlink/submodule input")
        index[name(raw_name.decode())] = {"mode": mode, "oid": oid}
    # No --exclude-standard: even ignored additions inside the declared source scope count.
    others = {name(p.decode()) for p in git(root, "ls-files", "--others", "-z", "--", *specs)
              .split(b"\0") if p}
    names = sorted(committed.keys() | index.keys() | others)
    require(0 < len(names) <= MAX_FILES, "empty or oversized source scope")
    entries = {}
    budget = budget or Budget()
    for relative in names:
        original = path(root / relative, exists=False)
        record = {"head": committed.get(relative), "index": index.get(relative)}
        if original.exists():
            record["content"] = file_record(original,
                destination / relative if destination else None, budget=budget)
        else:
            record["content"] = None  # Retain tracked deletions, including staged deletions.
        entries[relative] = record
    require(any(v["content"] is not None for v in entries.values()), "all source inputs deleted")
    return {"head": head, "tree": tree, "prefixes": prefixes, "entries": entries}


def environment():
    values = {}
    for key in ENV_ALLOWLIST:
        value = os.environ.get(key)
        require(value is None or (len(value) <= 160 and
                re.fullmatch(r"[A-Za-z0-9_./:+,@= -]*", value)), "invalid allowlisted environment value")
        values[key] = value
    return {"system": platform.system(), "release": platform.release(),
            "machine": platform.machine(), "python": platform.python_version(),
            "cpu_count": os.cpu_count(), "variables": values,
            "capture_policy": "explicit_noncredential_allowlist_v1"}


def no_secrets(value, depth=0):
    require(depth <= 32, "JSON nesting limit")
    if isinstance(value, dict):
        for key, child in value.items():
            require(not SECRET_KEY.search(key), "credential-bearing config fields not admitted")
            no_secrets(child, depth + 1)
    elif isinstance(value, list):
        for child in value:
            no_secrets(child, depth + 1)


def model_declaration(value):
    no_secrets(value)
    require(set(value) <= {"mode", "model_id", "revision", "encoding_space"},
            "unknown model declaration field")
    require(value.get("mode") in ("disabled", "fake"), "live models are not admitted to this profile")
    if value["mode"] == "disabled":
        require(set(value) == {"mode"}, "disabled model must not claim an encoding identity")
    else:
        for field in ("model_id", "revision", "encoding_space"):
            require(isinstance(value.get(field), str) and
                    re.fullmatch(r"[A-Za-z0-9_.:/+-]{1,160}", value[field]),
                    "fake model identity must be explicit")
    return value


def reserve(output):
    output.parent.mkdir(parents=True, exist_ok=True)
    path(output.parent)
    output.mkdir()  # Exclusive claim: even an existing empty directory is not overwritten.
    write_json(output / "INCOMPLETE.json", {"scope": SCOPE, "state": "incomplete"})


def tree_files(root):
    root = path(root)
    require(root.is_dir(), "artifact root must be a directory")
    result, pending, count = [], [root], 0
    while pending:
        directory = pending.pop()
        with os.scandir(directory) as children:
            for child in children:
                count += 1
                require(count <= MAX_FILES, "artifact entry limit exceeded")
                relative = name(str(Path(child.path).relative_to(root)))
                require(not child.is_symlink(), "symlink artifact not admitted")
                if child.is_dir(follow_symlinks=False):
                    pending.append(Path(child.path))
                else:
                    require(child.is_file(follow_symlinks=False), "special artifact not admitted")
                    result.append(relative)
    return sorted(result)


def inventory(root, destination=None, budget=None):
    budget = budget or Budget()
    return {relative: file_record(root / relative,
            destination / relative if destination else None, budget=budget)
            for relative in tree_files(root)}


def seal(output, manifest_name, manifest):
    write_json(output / manifest_name, manifest)
    (output / "INCOMPLETE.json").unlink()
    # Immutability is append-only tool behavior, not an ACL/security guarantee.
    # Do not chmod inputs or turn their executable bits into a claimed build receipt.


def freeze(args):
    root = path(args.source_root)
    prefixes = sorted({name(p) for p in args.source_path})
    require(len(prefixes) == len(args.source_path), "duplicate source scope")
    require(len(args.feature) <= 128 and all(re.fullmatch(r"[A-Za-z0-9_+/-]{1,160}", feature)
            for feature in args.feature), "invalid build feature declaration")
    for left, prefix in enumerate(prefixes):
        require(not any(PurePosixPath(prefix).is_relative_to(other)
                        for other in prefixes[:left]), "overlapping source scopes")
    inputs = {role: path(getattr(args, role)) for role in
              ("binary", "config", "corpus", "scoring", "model")}
    corpus_root = path(args.corpus_root) if args.corpus_root else None
    output = path(args.output, exists=False)
    disjoint(output, [root / p for p in prefixes] + list(inputs.values()) +
             ([corpus_root] if corpus_root else []))
    for original in inputs.values():
        name(original.name)
    no_secrets(read_json(inputs["config"]))
    model = model_declaration(read_json(inputs["model"]))
    # Caller-supplied metadata digests are declarations, distinct from the actual tree digest.
    corpus = read_json(inputs["corpus"])
    require(corpus.get("split") in ("dev", "fixture"),
            "only declared development/fixture corpus metadata is admitted")
    for field in ("corpus_sha256", "query_sha256", "gold_sha256"):
        require(isinstance(corpus.get(field), str) and
                re.fullmatch(r"[0-9a-f]{64}", corpus[field]), "corpus metadata digest required")
    before_environment = environment()
    reserve(output)
    budget = Budget()
    source = source_identity(root, prefixes, output / "source", budget)
    records = {}
    for role, original in inputs.items():
        relative = "inputs/" + role
        records[role] = {"original": str(original), "snapshot": relative,
                         **file_record(original, output / relative, budget=budget)}
    corpus_content = {"status": "unverified_corpus_content", "root": None,
                      "files": None, "content_sha256": None}
    if corpus_root:
        files = inventory(corpus_root, output / "corpus", budget)
        require(files, "explicit corpus root must contain files")
        corpus_content = {"status": "verified_explicit_corpus_files", "root": str(corpus_root),
                          "files": files, "content_sha256": digest(files)}
    no_secrets(read_json(output / "inputs/config"))
    require(model_declaration(read_json(output / "inputs/model")) == model,
            "model changed while freezing")
    require(read_json(output / "inputs/corpus") == corpus, "corpus metadata changed while freezing")
    if corpus_root:
        require(inventory(corpus_root) == corpus_content["files"], "corpus changed while freezing")
    require(source_identity(root, prefixes) == source, "source changed while freezing")
    require(environment() == before_environment, "environment changed while freezing")
    for role, original in inputs.items():
        expected = {k: records[role][k] for k in ("sha256", "bytes", "executable")}
        require(file_record(original) == expected, "input changed while freezing")
    manifest = {"schema_version": SCHEMA, "kind": "p8_local_candidate", "scope": SCOPE,
                "release_certified": False, "source_root": str(root), "source": source,
                "inputs": records, "corpus_content": corpus_content,
                "model": model, "environment": before_environment,
                "build": {"profile": args.build_profile, "features": sorted(args.feature),
                          "provenance": "operator_declaration_not_build_proof"}}
    manifest["candidate_sha256"] = digest(manifest)
    seal(output, "candidate.json", manifest)
    verify_candidate(output)
    return {"status": "frozen_local_inputs", "candidate": str(output),
            "candidate_sha256": manifest["candidate_sha256"], "scope": SCOPE,
            "corpus_content_status": corpus_content["status"]}, 0


def checked_manifest(root, filename, kind, digest_field, expected=None):
    root = path(root)
    require(not (root / "INCOMPLETE.json").exists(), "incomplete artifact")
    manifest = read_json(root / filename)
    require(manifest.get("schema_version") == SCHEMA and manifest.get("kind") == kind
            and manifest.get("scope") == SCOPE and manifest.get("release_certified") is False,
            "unsupported artifact schema/scope")
    actual = manifest.get(digest_field)
    require(isinstance(actual, str) and re.fullmatch(r"[0-9a-f]{64}", actual), "invalid manifest digest")
    require(digest({k: v for k, v in manifest.items() if k != digest_field}) == actual,
            "manifest checksum mismatch")
    require(expected is None or actual == expected, "trusted manifest digest mismatch")
    return manifest


def candidate_snapshot(root, manifest):
    expected = {"candidate.json"}
    budget = Budget()
    require(isinstance(manifest.get("inputs"), dict) and isinstance(manifest.get("source"), dict),
            "candidate input/source objects required")
    require(isinstance(manifest["source"].get("entries"), dict)
            and 0 < len(manifest["source"]["entries"]) <= MAX_FILES
            and isinstance(manifest["source"].get("prefixes"), list), "invalid source manifest")
    require(set(manifest["inputs"]) == {"binary", "config", "corpus", "scoring", "model"},
            "candidate input roles mismatch")
    for role, record in manifest["inputs"].items():
        require(isinstance(record, dict), "invalid candidate input record")
        relative = name(record["snapshot"])
        require(relative == "inputs/" + role, "candidate snapshot path mismatch")
        expected.add(relative)
        content = {k: record[k] for k in ("sha256", "bytes", "executable")}
        actual = file_record(root / relative, budget=budget)
        require(actual == content, "candidate input checksum mismatch")
    for relative, record in manifest["source"]["entries"].items():
        relative = name(relative)
        require(isinstance(record, dict), "invalid source record")
        if record["content"] is not None:
            snapshot = "source/" + relative
            expected.add(snapshot)
            actual = file_record(root / snapshot, budget=budget)
            require(actual == record["content"], "candidate source checksum mismatch")
    corpus = manifest.get("corpus_content")
    require(isinstance(corpus, dict), "corpus content declaration required")
    if corpus.get("status") == "verified_explicit_corpus_files":
        require(isinstance(corpus.get("files"), dict) and 0 < len(corpus["files"]) <= MAX_FILES,
                "invalid corpus file inventory")
        require(corpus.get("content_sha256") == digest(corpus["files"]), "corpus inventory checksum mismatch")
        require(isinstance(corpus.get("root"), str), "explicit corpus root required")
        for relative, record in corpus["files"].items():
            snapshot = "corpus/" + name(relative)
            expected.add(snapshot)
            require(file_record(root / snapshot, budget=budget) == record,
                    "candidate corpus checksum mismatch")
    else:
        require(corpus == {"status": "unverified_corpus_content", "root": None,
                           "files": None, "content_sha256": None}, "invalid unverified corpus declaration")
    require(set(tree_files(root)) == expected, "unexpected or missing candidate files")


def verify_candidate(root, expected=None, *, current=True):
    root = path(root)
    manifest = checked_manifest(root, "candidate.json", "p8_local_candidate", "candidate_sha256", expected)
    candidate_snapshot(root, manifest)
    if current:
        actual = source_identity(path(manifest["source_root"]), manifest["source"]["prefixes"])
        require(actual == manifest["source"], "source HEAD/index/content/addition/deletion drift")
        for record in manifest["inputs"].values():
            require(file_record(record["original"]) ==
                    {k: record[k] for k in ("sha256", "bytes", "executable")}, "candidate input drift")
        corpus = manifest["corpus_content"]
        if corpus["status"] == "verified_explicit_corpus_files":
            require(inventory(path(corpus["root"])) == corpus["files"], "corpus content/addition/deletion drift")
        require(environment() == manifest["environment"], "candidate environment drift")
    return manifest


def gate_record(evidence, candidate):
    gate = read_json(evidence / "gate.json")
    require(gate.get("candidate_sha256") == candidate["candidate_sha256"], "gate candidate binding mismatch")
    require(gate.get("status") in GATES and type(gate.get("exit_code")) is int
            and gate["exit_code"] == GATES[gate["status"]], "gate status/exit code mismatch")
    if gate["status"] == "passed_local":
        require((evidence / "raw").is_dir() and tree_files(evidence / "raw"), "passed_local needs raw evidence")
        for required in ("metrics.json", "report.md"):
            require(file_record(evidence / required)["bytes"] > 0, "passed_local needs metrics/report")
    return {k: gate[k] for k in ("candidate_sha256", "status", "exit_code")}


def checksum_text(output, records):
    lines = [record["sha256"] + "  " + relative + "\n" for relative, record in sorted(records.items())]
    lines.append(file_record(output / "archive.json")["sha256"] + "  archive.json\n")
    return "".join(lines).encode()


def verify_archive(root, expected=None):
    root = path(root)
    manifest = checked_manifest(root, "archive.json", "p8_local_archive", "archive_sha256", expected)
    require(isinstance(manifest.get("files"), dict) and 0 < len(manifest["files"]) <= MAX_FILES,
            "invalid archive inventory")
    expected_names = set(manifest["files"]) | {"archive.json", "checksums.sha256"}
    require(set(tree_files(root)) == expected_names, "unexpected or missing archive files")
    budget = Budget()
    for relative, record in manifest["files"].items():
        require(file_record(root / name(relative), budget=budget) == record, "archive payload checksum mismatch")
    candidate = verify_candidate(root / "candidate", manifest["candidate_sha256"], current=False)
    require(gate_record(root / "evidence", candidate) == manifest["gate"], "archived gate mismatch")
    checksum = root / "checksums.sha256"
    require(read_bytes(checksum, limit=MAX_JSON) == checksum_text(root, manifest["files"]),
            "archive checksum list mismatch")
    return manifest


def update_latest(output, manifest):
    latest = path(output.parent / "latest.json", exists=False)
    if latest.exists():
        old = read_json(latest)
        require(old.get("kind") == "p8_local_latest" and old.get("scope") == SCOPE,
                "refusing to replace an unrelated latest file")
    temporary = output.parent / (".latest-" + uuid.uuid4().hex + ".tmp")
    write_json(temporary, {"schema_version": SCHEMA, "kind": "p8_local_latest", "scope": SCOPE,
                          "run": output.name, "archive_sha256": manifest["archive_sha256"],
                          "candidate_sha256": manifest["candidate_sha256"]})
    try:
        path(latest, exists=False)
        os.replace(temporary, latest)
    finally:
        temporary.unlink(missing_ok=True)


def archive(args):
    candidate_root, evidence = path(args.candidate), path(args.evidence)
    candidate = verify_candidate(candidate_root)
    gate = gate_record(evidence, candidate)
    require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,79}", args.run_id), "invalid run id")
    corpus = candidate["corpus_content"]
    corpus_digest = corpus["content_sha256"] or candidate["inputs"]["corpus"]["sha256"]
    suffix = "-".join(value[:12] for value in
                      (candidate["inputs"]["binary"]["sha256"], corpus_digest,
                       candidate["inputs"]["scoring"]["sha256"]))
    output = path(Path(args.output_root) / (args.run_id + "-" + suffix), exists=False)
    disjoint(output, [candidate_root, evidence] +
             [Path(candidate["source_root"]) / p for p in candidate["source"]["prefixes"]] +
             [Path(v["original"]) for v in candidate["inputs"].values()] +
             ([Path(corpus["root"])] if corpus["root"] else []))
    if args.update_latest:
        disjoint(output.parent / "latest.json", [candidate_root, evidence] +
                 [Path(candidate["source_root"]) / p for p in candidate["source"]["prefixes"]] +
                 [Path(v["original"]) for v in candidate["inputs"].values()] +
                 ([Path(corpus["root"])] if corpus["root"] else []))
    candidate_before = inventory(candidate_root)
    evidence_before = inventory(evidence)
    budget = Budget()
    for record in list(candidate_before.values()) + list(evidence_before.values()):
        budget.add(record["bytes"])
    reserve(output)
    require(inventory(candidate_root, output / "candidate") == candidate_before,
            "candidate changed while archiving")
    copied = inventory(evidence, output / "evidence")
    require(copied == evidence_before == inventory(evidence), "evidence changed while archiving")
    verify_candidate(candidate_root)
    records = inventory(output)
    del records["INCOMPLETE.json"]
    manifest = {"schema_version": SCHEMA, "kind": "p8_local_archive", "scope": SCOPE,
                "release_certified": False, "run_id": output.name,
                "candidate_sha256": candidate["candidate_sha256"], "gate": gate, "files": records}
    manifest["archive_sha256"] = digest(manifest)
    write_json(output / "archive.json", manifest)
    with (output / "checksums.sha256").open("xb") as stream:
        stream.write(checksum_text(output, records))
        stream.flush()
        os.fsync(stream.fileno())
    (output / "INCOMPLETE.json").unlink()
    verify_archive(output)
    corpus_verified = corpus["status"] == "verified_explicit_corpus_files"
    promoted = args.update_latest and gate["status"] == "passed_local" and corpus_verified
    if promoted:
        update_latest(output, manifest)
    return {"status": "archived_local_evidence", "archive": str(output),
            "archive_sha256": manifest["archive_sha256"], "gate": gate,
            "latest_updated": bool(promoted), "corpus_content_status": corpus["status"],
            "scope": SCOPE}, gate["exit_code"]


def parser():
    cli = argparse.ArgumentParser(description=__doc__)
    commands = cli.add_subparsers(dest="command", required=True)
    freeze_cli = commands.add_parser("freeze", help="bind and snapshot explicitly scoped local inputs")
    freeze_cli.add_argument("--source-root", required=True)
    freeze_cli.add_argument("--source-path", required=True, action="append")
    for role in ("binary", "config", "corpus", "scoring", "model"):
        freeze_cli.add_argument("--" + role, required=True)
    freeze_cli.add_argument("--corpus-root", help="explicit dev/fixture file tree to lock; required for latest eligibility")
    freeze_cli.add_argument("--build-profile", choices=("release", "debug", "fixture"), required=True)
    freeze_cli.add_argument("--feature", action="append", default=[])
    freeze_cli.add_argument("--output", required=True)
    verify_cli = commands.add_parser("verify", help="check a candidate against current inputs or an archive offline")
    choice = verify_cli.add_mutually_exclusive_group(required=True)
    choice.add_argument("--candidate")
    choice.add_argument("--archive")
    verify_cli.add_argument("--expected-sha256", help="trusted manifest digest obtained separately")
    archive_cli = commands.add_parser("archive", help="append a checksum-verifiable evidence run")
    archive_cli.add_argument("--candidate", required=True)
    archive_cli.add_argument("--evidence", required=True)
    archive_cli.add_argument("--output-root", required=True)
    archive_cli.add_argument("--run-id", required=True)
    archive_cli.add_argument("--update-latest", action="store_true")
    return cli


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        if args.command == "freeze":
            result, code = freeze(args)
        elif args.command == "archive":
            result, code = archive(args)
        else:
            if args.candidate:
                manifest = verify_candidate(args.candidate, args.expected_sha256)
            else:
                manifest = verify_archive(args.archive, args.expected_sha256)
            result, code = {"status": "integrity_verified", "scope": SCOPE,
                            "release_certified": False,
                            "candidate_sha256": manifest["candidate_sha256"]}, 0
        print(json.dumps(result, sort_keys=True))
        return code
    except KeyboardInterrupt:
        print(json.dumps({"status": "cancelled", "partial_artifacts_retained": True}), file=sys.stderr)
        return 3
    except (Invalid, OSError, KeyError, TypeError, UnicodeError, RecursionError) as exc:
        # Do not print file bodies, environment values, or subprocess stderr.
        message = str(exc) if isinstance(exc, Invalid) else type(exc).__name__
        print(json.dumps({"status": "invalid_input", "reason": message,
                          "partial_artifacts_retained": True}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())

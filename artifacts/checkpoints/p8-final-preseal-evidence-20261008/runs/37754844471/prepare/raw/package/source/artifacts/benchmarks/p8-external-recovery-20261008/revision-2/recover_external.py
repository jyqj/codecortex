#!/usr/bin/env python3
"""Recover pinned public external inputs; use the existing cc-eval and p8_compat.
This file contains no third-party question corpus and never updates a Git ref.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import fnmatch
import hashlib
import importlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import sys
import traceback
import zipfile

SOURCE = None  # Set only after the explicit manifest/source pin is checked.
IGNORE_DIRS = {".git", ".hg", ".svn", ".next", ".turbo", "build", "coverage", "dist",
               "node_modules", "openclaw-retrieval-eval", "target", "tmp"}
IGNORE_SUFFIXES = {".7z", ".a", ".avi", ".bmp", ".class", ".dll", ".dmg", ".exe", ".gif",
 ".gz", ".ico", ".jar", ".jpeg", ".jpg", ".mov", ".mp3", ".mp4", ".o", ".pdf", ".png",
 ".so", ".tar", ".ttf", ".wasm", ".webp", ".woff", ".woff2", ".zip"}
QUERY_FIELDS = ("id", "category", "difficulty", "language", "split", "query_family", "query",
                "path_prefix", "no_answer", "expected_files", "answers", "annotations")

def need(ok, message):
    if not ok:
        raise ValueError(message)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def bytes_record(raw):
    return {"bytes": len(raw), "sha256": sha(raw)}

def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as out:
        out.write((json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True,
                              allow_nan=False) + "\n").encode())

def read_json(path):
    return json.loads(path.read_bytes())

def relative(value):
    p = PurePosixPath(value)
    need(value and not p.is_absolute() and "\\" not in value and
         all(x not in ("", ".", "..") for x in value.split("/")) and
         not any(ord(c) < 32 or ord(c) == 127 for c in value), "unsafe relative path")
    return p

def owned_new(path):
    need(not path.exists() and not path.is_symlink(), "new directory required")
    path.mkdir(parents=True)
    return path

def safe_file(root, name):
    p = root
    for part in relative(name).parts:
        p = p / part
        need(not p.is_symlink(), "symlink source")
    need(p.is_file(), "regular file required")
    return p

def raw_git(root, *args):
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1",
           "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}
    return subprocess.check_output(
        ["git", "-c", "core.hooksPath=" + os.devnull, "-C", str(root), *args],
        stdin=subprocess.DEVNULL, stderr=subprocess.PIPE, env=env, timeout=120)

def checkout_identity(root, pin):
    need(raw_git(root, "rev-parse", "HEAD").decode().strip() == pin["commit"], "HEAD drift")
    need(raw_git(root, "rev-parse", "HEAD^{tree}").decode().strip() == pin["tree"], "tree drift")
    need(not raw_git(root, "status", "--porcelain", "--untracked-files=all").strip(), "dirty checkout")
    need(not raw_git(root, "submodule", "status", "--recursive").strip(), "submodules not admitted")

def clone_pinned(parent, pin, compat, evidence):
    root = parent / pin.get("name", "oce-benchmark")
    owned_new(root)
    commands = [
        ["git", "init", "-q", str(root)],
        ["git", "-C", str(root), "remote", "add", "origin",
         "https://github.com/" + pin["repository"] + ".git"],
        ["git", "-C", str(root), "-c", "core.hooksPath=" + os.devnull,
         "fetch", "--depth=1", "--no-tags", "origin", pin["commit"]],
        ["git", "-C", str(root), "-c", "core.hooksPath=" + os.devnull,
         "-c", "core.autocrlf=false", "-c", "filter.lfs.smudge=",
         "-c", "filter.lfs.required=false", "checkout", "--detach", pin["commit"]],
    ]
    evidence.mkdir(parents=True)
    for i, argv in enumerate(commands):
        receipt = compat.command(argv, evidence / ("%02d.log" % i), 120,
                                 env={**os.environ, "GIT_TERMINAL_PROMPT": "0",
                                      "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull})
        write_json(evidence / ("%02d.json" % i), receipt)
        need(receipt["exit_code"] == 0, "pinned checkout command failed")
    checkout_identity(root, pin)
    return root

def tracked_inventory(root, pin):
    records, common, excluded = [], [], Counter()
    for entry in raw_git(root, "ls-tree", "-r", "-z", "HEAD").split(b"\0"):
        if not entry:
            continue
        header, raw_name = entry.split(b"\t", 1)
        mode, kind, blob = header.decode().split()
        name = raw_name.decode("utf-8")
        need(kind == "blob" and mode in ("100644", "100755"), "symlink/gitlink unsupported")
        p = safe_file(root, name)
        raw = p.read_bytes()
        need(hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest() == blob,
             "checkout differs from pinned Git blob")
        actual_mode = "100755" if p.stat().st_mode & 0o111 else "100644"
        need(mode == actual_mode, "executable-mode drift")
        record = {"path": name, "git_blob": blob, "git_mode": mode, **bytes_record(raw)}
        records.append(record)
        parts = name.split("/")
        reason = None
        if any(x.casefold() in IGNORE_DIRS for x in parts[:-1]):
            reason = "upstream_directory"
        elif p.suffix.casefold() in IGNORE_SUFFIXES:
            reason = "upstream_suffix"
        elif parts[-1] in (".env", ".env.local", ".env.production"):
            reason = "upstream_environment_file"
        elif len(raw) > 1000000:
            reason = "upstream_size"
        elif b"\0" in raw:
            reason = "upstream_nul"
        else:
            try:
                raw.decode("utf-8")
            except UnicodeDecodeError:
                reason = "upstream_utf8"
        if reason is None:
            need(not any(x in (".git", ".codecortex", "node_modules", "target") or
                         x.startswith(".env") for x in parts) and name != ".codecortex.json",
                 "additional cc-eval exclusion requires a declared input amendment")
            need(not raw.startswith(b"version https://git-lfs.github.com/spec/"), "unresolved LFS")
            common.append(record)
        else:
            excluded[reason] += 1
    records.sort(key=lambda x: x["path"])
    common.sort(key=lambda x: x["path"])
    need(len(records) == pin["all_tracked_files"] and
         sum(x["bytes"] for x in records) == pin["all_tracked_bytes"], "tracked inventory drift")
    need(len(common) == pin["common_files"] and
         sum(x["bytes"] for x in common) == pin["common_bytes"], "common inventory drift")
    checkout_identity(root, pin)
    return {"tracked": records, "common": common, "excluded_counts": dict(excluded)}

def extract_tools(archive, work, manifest):
    raw_digest = hashlib.sha256()
    with archive.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            raw_digest.update(chunk)
    need(archive.stat().st_size == manifest["archive"]["bytes"] and
         raw_digest.hexdigest() == manifest["archive"]["sha256"], "fixed artifact ZIP drift")
    tool_root = owned_new(work / "tools")
    helper_root = owned_new(work / "fixed-source")
    extracted = {}
    with zipfile.ZipFile(archive) as z:
        need(len(z.namelist()) == len(set(z.namelist())), "duplicate ZIP member")
        pins = [(name, tool_root / name, pin) for name, pin in manifest["tools"].items()]
        pins += [(name, helper_root / name, pin) for name, pin in manifest["helpers"].items()]
        for name, dest, pin in pins:
            info = z.getinfo(pin["member"])
            need(info.file_size == pin["bytes"] and
                 not stat.S_ISLNK(info.external_attr >> 16), "invalid fixed ZIP member")
            raw = z.read(info)
            need(bytes_record(raw) == {"bytes": pin["bytes"], "sha256": pin["sha256"]},
                 "fixed member content drift")
            dest.parent.mkdir(parents=True, exist_ok=True)
            with dest.open("xb") as f:
                f.write(raw)
            dest.chmod(0o755 if pin["executable"] else 0o644)
            extracted[name] = {"path": str(dest), "member": pin["member"], **bytes_record(raw)}
    sys.path.insert(0, str(helper_root / "scripts"))
    compat = importlib.import_module("p8_compat")
    need(Path(compat.__file__).resolve() == helper_root / "scripts/p8_compat.py", "wrong wrapper module")
    return compat, tool_root, helper_root, extracted

def read_external(oce, pin):
    base = oce / "benchmarks"
    query = base / (pin["name"] + "-retrieval-benchmark.jsonl")
    metadata = base / (pin["name"] + "-retrieval-benchmark.metadata.json")
    for f, kind in [(query, "query"), (metadata, "metadata")]:
        raw = f.read_bytes()
        need(bytes_record(raw) == {"bytes": pin[kind + "_bytes"], "sha256": pin[kind + "_sha256"]},
             "external input digest drift")
        need(hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest() ==
             pin[kind + "_blob"], "external input Git blob drift")
    rows = [json.loads(x) for x in query.read_text(encoding="utf-8-sig").splitlines() if x.strip()]
    meta = read_json(metadata)
    need(meta["repository"]["commit"] == pin["commit"] and len(rows) == meta["questions"] == 100,
         "external question count or target drift")
    need(len({r.get("id", r.get("query_id")) for r in rows}) == 100, "duplicate upstream ID")
    need(sorted(Counter(r["category"] for r in rows).values()) == [10] * 10, "category count drift")
    need(sum(len(r["expected_files"]) for r in rows) == pin["expected_patterns"], "pattern count drift")
    return query, metadata, rows

def match_patterns(rows, paths):
    matches = {}
    for row in rows:
        row_id = row.get("id", row.get("query_id"))
        patterns = [p.replace("\\", "/") for p in row["expected_files"]]
        matched = [[p for p in paths if fnmatch.fnmatchcase(p, pattern)] for pattern in patterns]
        need(all(matched), "expected pattern matches no common input")
        need(all(sum(p in group for group in matched) <= 1 for p in paths), "overlapping gold patterns")
        matches[row_id] = matched
    return matches

def read_rows(path):
    return [json.loads(x) for x in path.read_bytes().splitlines() if x.strip()]

def serde_queries(rows):
    """Fixed Query struct field order and BTreeMap annotations; checked against actual runner."""
    output = []
    for row in rows:
        item = {k: row[k] for k in QUERY_FIELDS}
        item["annotations"] = dict(sorted(item["annotations"].items()))
        output.append(json.dumps(item, ensure_ascii=False, separators=(",", ":"), allow_nan=False))
    return ("\n".join(output) + "\n").encode()

def project_native(rows, matches):
    out = []
    for row in rows:
        native = dict(row)
        native["expected_files"] = []
        native["answers"] = [
            {"id": "g%d" % (i + 1), "primary": i == 0, "grade": 2 if i == 0 else 1,
             "alternatives": [{"path": p, "symbol": None, "span": None} for p in group]}
            for i, group in enumerate(matches[row["id"]])]
        native["annotations"] = {**row["annotations"],
            "native_gold_review": "not_run; mechanical original expected-file order only",
            "projection": "first pattern primary, remaining supporting; matched paths are alternatives"}
        out.append(native)
    return out

def run_cli(compat, argv, output, label, records, expected=0):
    output.mkdir(parents=True, exist_ok=True)
    r = compat.command(argv, output / (label + ".log"), 600,
                       env={**os.environ, "CODECORTEX_BENCH_PROCESS_PROBE": "0",
                            "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1"})
    write_json(output / (label + ".json"), r)
    records.append(r)
    need(r["exit_code"] == expected, "cc-eval command failed: " + label)
    return r

def make_inputs(pin, oce, inputs, tool_root, helper_root, manifest, compat, public):
    root = clone_pinned(inputs, pin, compat, public / "clone" / pin["name"])
    inventory = tracked_inventory(root, pin)
    write_json(public / "sources" / (pin["name"] + ".json"), inventory)
    query, metadata, raw_rows = read_external(oce, pin)
    paths = [x["path"] for x in inventory["common"]]
    matches = match_patterns(raw_rows, paths)
    suite_dir = owned_new(inputs / (pin["name"] + "-suites"))
    imported = suite_dir / "queries.compat.jsonl"
    commands = []
    logs = public / "commands" / pin["name"]
    run_cli(compat, [tool_root / "evaluator", "import-oce", "--queries", query,
                    "--metadata", metadata, "--output", imported], logs, "01-import", commands)
    imported_rows = read_rows(imported)
    need(len(imported_rows) == 100 and all(r["split"] == "dev" for r in imported_rows), "import projection drift")
    import_receipt = read_json(imported.with_suffix(".receipt.json"))
    need(import_receipt["actual_questions"] == 100 and
         import_receipt["reference_commit"] == manifest["upstream"]["commit"], "import receipt identity")
    # Receipt includes source metadata and digests, never question bodies.
    write_json(public / "imports" / (pin["name"] + ".json"), import_receipt)
    native = suite_dir / "queries.native.jsonl"
    native.write_bytes(serde_queries(project_native(imported_rows, matches)))
    source_hashes = {x["path"]: x["sha256"] for x in inventory["common"]}
    suite_entries = []
    for index, profile in enumerate(("compat", "native")):
        q = suite_dir / ("queries." + profile + ".jsonl")
        suite_path = suite_dir / ("suite." + profile + ".json")
        suite = {"schema_version": 1, "name": "external-%s-%s" % (pin["name"], profile),
                 "source": {"root": "../" + pin["name"], "commit": pin["commit"], "digest": "", "files": paths},
                 "queries": q.name, "queries_digest": "",
                 "scoring": compat.PROFILES[profile], **manifest["budget"],
                 "engine_config": manifest["engine_config"]}
        write_json(suite_path, suite)
        run_cli(compat, [tool_root / "evaluator", "freeze", "--suite", suite_path],
                logs, "%02d-freeze-%s" % (2 + index * 2, profile), commands)
        run_cli(compat, [tool_root / "evaluator", "validate", "--suite", suite_path],
                logs, "%02d-validate-%s" % (3 + index * 2, profile), commands)
        locked = read_json(suite_path)
        need(all(locked[k] == v for k, v in manifest["budget"].items()), "freeze budget drift")
        suite_entries.append({"profile": profile, "path": str(suite_path.relative_to(inputs)),
            "sha256": sha(suite_path.read_bytes()), "query_sha256": sha(q.read_bytes()),
            "source_files": source_hashes})
        # Suites have paths/digests/budgets, no query or gold bodies.
        write_json(public / "suites" / pin["name"] / suite_path.name, locked)
    lock = {"schema_version": 1, "dataset": pin["name"], "repository": pin["repository"],
        "commit": pin["commit"], "source_mode": "git_checkout", "input_root": str(inputs),
        "evaluator": {"path": str(tool_root / "evaluator"), "sha256": manifest["tools"]["evaluator"]["sha256"],
            "source_sha": SOURCE, "build_receipt": str(tool_root / "evaluator_receipt"),
            "build_receipt_sha256": manifest["tools"]["evaluator_receipt"]["sha256"]},
        "backend": {"kind": "mcp-stdio", "path": str(tool_root / "product"),
                    "sha256": manifest["tools"]["product"]["sha256"]},
        "scorer": {"root": str(helper_root), "files":
                   {p: manifest["helpers"][p]["sha256"] for p in compat.SCORER_FILES}},
        "platform": compat.current_platform(), "suites": suite_entries,
        "comparison_policy": {"path": str(helper_root / "crates/cc-eval/benchmarks/manifests/comparison-policy.json"),
                              "sha256": manifest["helpers"]["crates/cc-eval/benchmarks/manifests/comparison-policy.json"]["sha256"]},
        "timeout_seconds": manifest["command_timeout_seconds"]}
    lock_path = inputs / (pin["name"] + "-lock.json")
    write_json(lock_path, lock)
    checked = compat.inspect_lock(lock_path, configuration_profile="local-text-hidden")
    write_json(public / "locks" / (pin["name"] + ".json"), lock)
    drift = []
    for profile in ("compat", "native"):
        original_query = suite_dir / ("queries." + profile + ".jsonl")
        original_suite = suite_dir / ("suite." + profile + ".json")
        original_bytes = original_query.read_bytes()
        control = owned_new(inputs / (pin["name"] + "-query-drift-" + profile))
        copied_query = control / original_query.name
        rows = read_rows(original_query)
        rows[0]["query"] += " [isolated query-lock drift control]"
        copied_query.write_bytes(serde_queries(rows))
        shutil.copyfile(original_suite, control / original_suite.name)
        need(copied_query.read_bytes() != original_bytes, "drift control did not change query")
        receipt = run_cli(compat, [tool_root / "evaluator", "validate", "--suite", control / original_suite.name],
                          logs, "query-drift-" + profile, commands, expected=2)
        need(b"source or query content lock drift" in (logs / ("query-drift-" + profile + ".log")).read_bytes(),
             "drift failed for a different reason")
        need(original_query.read_bytes() == original_bytes, "original query changed by control")
        drift.append({"profile": profile, "actual_exit": receipt["exit_code"],
                      "before_sha256": sha(original_bytes), "copy_after_sha256": sha(copied_query.read_bytes()),
                      "original_preserved": True, "rejection": "source or query content lock drift"})
    checkout_identity(root, pin)
    return lock_path, {"dataset": pin["name"], "actual_questions": 100,
        "tracked_files": len(inventory["tracked"]), "common_files": len(paths),
        "patterns_matched": sum(len(v) for v in matches.values()), "all_import_commands_succeeded": True,
        "native_gold_independent_review": "not_run", "commands": commands, "query_drift_controls": drift,
        "identity": checked["identity"]}

def query_material(inputs, datasets):
    snapshots, tokens = {}, {}
    for pin in datasets:
        for profile in ("compat", "native"):
            q = inputs / (pin["name"] + "-suites") / ("queries." + profile + ".jsonl")
            rows = read_rows(q)
            snapshots[pin["name"] + ":" + profile] = serde_queries(rows)
            if profile == "compat":
                for row in rows:
                    text = row["query"]
                    for encoding, raw in (
                        ("utf8", text.encode()),
                        ("json_utf8", json.dumps(text, ensure_ascii=False)[1:-1].encode()),
                        ("json_ascii", json.dumps(text, ensure_ascii=True)[1:-1].encode())):
                        need(raw, "empty question")
                        tokens[pin["name"] + ":" + row["id"] + ":" + encoding] = raw
    return snapshots, tokens

def package_raw(runs, destination, inputs, datasets, private_restore):
    """Lossless external-body references; verify restoration now, before claiming pass."""
    destination.mkdir()
    payload = owned_new(destination / "payload")
    snapshots, tokens = query_material(inputs, datasets)
    reverse = {}
    for key, raw in sorted(tokens.items()):
        reverse.setdefault(raw, key)
    expression = re.compile(b"|".join(re.escape(v) for v in sorted(reverse, key=lambda x: (-len(x), x))))
    files = []
    for p in sorted(runs.rglob("*")):
        if p.is_dir():
            continue
        need(p.is_file() and not p.is_symlink(), "unexpected run output")
        name = p.relative_to(runs).as_posix()
        relative(name)
        raw = p.read_bytes()
        record = {"path": name, **bytes_record(raw)}
        parts = name.split("/")
        if p.name == "queries.jsonl":
            need(len(parts) == 3 and parts[1] in ("compat", "native"), "unexpected query snapshot location")
            key = parts[0] + ":" + parts[1]
            need(raw == snapshots[key], "query snapshot serializer mismatch")
            record["external_query_snapshot"] = key
        else:
            segments, cursor = [], 0
            for m in expression.finditer(raw):
                if m.start() > cursor:
                    piece = raw[cursor:m.start()]
                    digest = sha(piece)
                    if not (payload / digest).exists():
                        (payload / digest).write_bytes(piece)
                    segments.append({"payload_sha256": digest, "bytes": len(piece)})
                segments.append({"question_reference": reverse[m.group()]})
                cursor = m.end()
            if cursor < len(raw) or not segments:
                piece = raw[cursor:]
                digest = sha(piece)
                if not (payload / digest).exists():
                    (payload / digest).write_bytes(piece)
                segments.append({"payload_sha256": digest, "bytes": len(piece)})
            record["segments"] = segments
        files.append(record)
    # Payload contains no exact full question representation used by this fixed corpus.
    for p in payload.iterdir():
        need(expression.search(p.read_bytes()) is None, "question body escaped factoring")
    index = {"schema_version": 1, "kind": "external-body-reference-package-not-original-raw-zip",
             "original_files": files, "source": SOURCE,
             "scope": "Original measured bytes recoverable only with the fixed externally supplied OCE inputs.",
             "query_snapshot_recipe": "fixed Query field order plus sorted BTreeMap annotations; verified against actual runner",
             "third_party_query_or_gold_corpus_included": False}
    write_json(destination / "index.json", index)
    restored = owned_new(private_restore)
    restore_raw(destination, restored, inputs, datasets)
    for entry in files:
        actual = (restored / entry["path"]).read_bytes()
        need(actual == (runs / entry["path"]).read_bytes(), "restored raw bytes differ")
    # Do not place restored private corpus inside the public upload directory.
    shutil.rmtree(restored)
    write_json(destination / "roundtrip.json",
               {"schema_version": 1, "all_files_restored_identically": True, "files": len(files),
                "total_bytes": sum(v["bytes"] for v in files), "index_sha256": sha((destination / "index.json").read_bytes()),
                "scope": "actual same-run byte equality, not quality acceptance"})

def restore_raw(package, output, inputs, datasets):
    index = read_json(package / "index.json")
    need(index["schema_version"] == 1 and index["source"] == SOURCE, "package identity")
    snapshots, tokens = query_material(inputs, datasets)
    seen = set()
    for entry in index["original_files"]:
        name = str(relative(entry["path"]))
        need(name not in seen, "duplicate package path")
        seen.add(name)
        if "external_query_snapshot" in entry:
            raw = snapshots[entry["external_query_snapshot"]]
        else:
            chunks = []
            for segment in entry["segments"]:
                if "question_reference" in segment:
                    chunks.append(tokens[segment["question_reference"]])
                else:
                    digest = segment["payload_sha256"]
                    need(re.fullmatch("[0-9a-f]{64}", digest) is not None, "invalid payload digest")
                    piece = safe_file(package / "payload", digest).read_bytes()
                    need(sha(piece) == digest and len(piece) == segment["bytes"], "payload drift")
                    chunks.append(piece)
            raw = b"".join(chunks)
        need(bytes_record(raw) == {"bytes": entry["bytes"], "sha256": entry["sha256"]}, "restored digest mismatch")
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as f:
            f.write(raw)

def main():
    global SOURCE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--expected-source", required=True)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--public-output", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--restore-package", type=Path)
    parser.add_argument("--restore-output", type=Path)
    args = parser.parse_args()
    need(re.fullmatch("[0-9a-f]{40}", args.expected_source) is not None, "full candidate source pin")
    need(re.fullmatch("[0-9a-f]{64}", args.manifest_sha256) is not None
         and sha(args.manifest.read_bytes()) == args.manifest_sha256, "explicit manifest pin")
    manifest = read_json(args.manifest)
    SOURCE = args.expected_source
    need(manifest["product_source"] == SOURCE, "source pin")
    need(sha(Path(__file__).read_bytes()) == manifest["recovery_script_sha256"], "recovery script drift")
    need(manifest["configuration_profile"] == "local-text-hidden", "explicit configuration profile")
    need(manifest["engine_config"] == {"auto_index": {"enabled": False},
         "indexing": {"include_text_files": True, "include_hidden_files": True}}, "exact configuration")
    work = args.work.absolute()
    if args.restore_package:
        need(args.restore_output is not None and args.public_output is None, "restore arguments")
        output = owned_new(args.restore_output.absolute())
        restore_raw(args.restore_package.absolute(), output, work / "inputs", manifest["datasets"])
        print(json.dumps({"restored": True, "output": str(output)}))
        return 0
    need(args.archive is not None and args.public_output is not None, "execution arguments")
    public = args.public_output.absolute()
    need(not (public == work or public.is_relative_to(work) or work.is_relative_to(public)),
         "public output and private work must be separate")
    owned_new(work)
    owned_new(public)
    result = {"schema_version": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
              "source": SOURCE, "phase": "not_started", "release_certified": False,
              "P8_003_done": False, "P8_002_dependency": "not_bypassed",
              "historical_local_raw": "unavailable; not reconstructed as new raw",
              "datasets": [], "new_drift_controls": "not_run", "ranking": "not_run",
              "script_sha256": sha(Path(__file__).read_bytes()),
              "manifest_sha256": sha(args.manifest.read_bytes())}
    code = 2
    try:
        need(platform.system() == "Linux" and platform.machine() == "x86_64", "fixed platform required")
        compat, tools, helpers, extracted = extract_tools(args.archive.absolute(), work, manifest)
        write_json(public / "tool-extraction.json", extracted)
        build_validation = read_json(tools / "candidate_build_validation")
        need(build_validation["source"]["source_commit"] == SOURCE
             and build_validation["status"] == "actual_same_source_release_builds_verified",
             "fresh candidate validation identity")
        for role, key in (("product", "product"), ("runner", "evaluator")):
            receipt = read_json(tools / (key + "_receipt"))
            need(receipt["source_before"] == build_validation["source"] == receipt["source_after"]
                 and receipt["build_exit_code"] == 0 and receipt["build_profile"] == "release"
                 and receipt["binary_sha256"] == sha((tools / key).read_bytes()),
                 "fresh candidate build receipt mismatch")
            need(sha((tools / (key + "_raw_build")).read_bytes())
                 == build_validation["builds"][role]["raw_build_sha256"], "retained raw build drift")
            need(sha((tools / (key + "_source_inputs")).read_bytes())
                 == build_validation["builds"][role]["source_manifest_sha256"], "source inventory drift")
        shutil.copyfile(tools / "candidate_build_validation", public / "candidate-build-validation.json")
        write_json(public / "environment.json",
                   {"platform": platform.platform(), "machine": platform.machine(),
                    "python": sys.version, "executable": sys.executable,
                    "git_version": subprocess.check_output(["git", "--version"], timeout=10).decode().strip(),
                    "source": SOURCE, "actual_build_profile": "release, with actual Cargo profile in both receipts",
                    "process_probe": "original p8_compat default disabled; not a process-tree resource claim"})
        for key in ("evaluator_receipt", "product_receipt"):
            shutil.copyfile(tools / key, public / (key + ".json"))
        inputs = owned_new(work / "inputs")
        upstream = clone_pinned(work, manifest["upstream"], compat, public / "clone" / "oce-benchmark")
        locks = []
        result["phase"] = "preparing"
        for pin in manifest["datasets"]:
            lock, record = make_inputs(pin, upstream, inputs, tools, helpers, manifest, compat, public)
            locks.append((pin, lock))
            result["datasets"].append(record)
        result["phase"] = "prepared"
        result["new_drift_controls"] = "actual copied-query controls rejected by the original Rust validator"
        if args.prepare_only:
            code = 0
        else:
            runs = owned_new(work / "runs")
            codes = []
            result["ranking"] = "executing"
            for pin, lock in locks:
                measured, measured_code = compat.run_locked(lock, runs / pin["name"],
                                                            configuration_profile="local-text-hidden")
                codes.append(measured_code)
                observations = {}
                for profile in ("compat", "native"):
                    run = runs / pin["name"] / profile
                    attempts = [entry for entry in measured["commands"]
                                if len(entry["argv"]) > 1 and entry["argv"][1] == "run"
                                and "--suite" in entry["argv"]
                                and Path(entry["argv"][entry["argv"].index("--suite") + 1]).name
                                == "suite." + profile + ".json"]
                    normalized = run / "normalized.jsonl"
                    observations[profile] = {
                        "attempted": bool(attempts),
                        "process_exit_codes": [entry["process_exit_code"] for entry in attempts],
                        "effective_exit_codes": [entry["exit_code"] for entry in attempts],
                        "verified_complete_by_original_wrapper": profile in measured["profiles"],
                        "observed_measured_rows": len(read_rows(normalized)) if normalized.is_file() else None,
                        "original_gate": read_json(run / "gate.json") if (run / "gate.json").is_file() else None,
                        "replay_log_present": (runs / pin["name"] / (profile + "-replay.log")).is_file(),
                        "status": ("verified_complete" if profile in measured["profiles"]
                                   else "attempted_not_verified_complete" if attempts else "not_run")}
                record = {"dataset": pin["name"], "exit_code": measured_code,
                          "status": measured["status"], "native_gold_review": "not_run",
                          "error_type": measured.get("error_type"),
                          "error": measured.get("error"),
                          "profile_observations": observations,
                          "profiles": {name: {k: value[k] for k in ("gate", "metrics", "adapter", "adapter_version",
                                                                  "replay_exit_code") if k in value}
                                       for name, value in measured["profiles"].items()}}
                result.setdefault("measurement_observations", {})[pin["name"]] = observations
                write_json(public / "measured" / (pin["name"] + ".json"), record)
                write_json(public / "inventories" / (pin["name"] + ".json"),
                           {p.relative_to(runs / pin["name"]).as_posix(): bytes_record(p.read_bytes())
                            for p in sorted((runs / pin["name"]).rglob("*")) if p.is_file() and not p.is_symlink()})
            observations = [value for profiles in result["measurement_observations"].values()
                            for value in profiles.values()]
            result["ranking"] = {
                "attempted_profiles": sum(v["attempted"] for v in observations),
                "verified_complete_profiles": sum(v["verified_complete_by_original_wrapper"] for v in observations),
                "observed_measured_rows": sum(v["observed_measured_rows"] or 0 for v in observations),
                "not_run_profiles": sum(v["status"] == "not_run" for v in observations),
                "scope": "Observed raw/command evidence only; planned requests are never counted."}
            result["run_exit_codes"] = codes
            package_raw(runs, public / "external-reference-package", inputs, manifest["datasets"],
                        work / "immediate-restoration-private")
            result["raw_package"] = "same-run lossless restoration verified; not original raw ZIP"
            code = max(codes)
            result["phase"] = ("measured_and_packaged" if code == 0 else
                               "measured_gate_failed" if code == 1 else
                               "invalid_measurement" if code == 2 else "cancelled_or_budget_terminated")
        for pin in manifest["datasets"]:
            checkout_identity(inputs / pin["name"], pin)
        result["exit_code"] = code
    except Exception as error:
        result.update(phase="failed", exit_code=2, error_type=type(error).__name__,
                      error_frames=[{"file": f.filename, "line": f.lineno, "function": f.name}
                                    for f in traceback.extract_tb(sys.exc_info()[2])],
                      error="Input, execution, or packaging failed; command receipts retain available details.")
        code = 2
    result["finished_at_utc"] = datetime.now(timezone.utc).isoformat()
    write_json(public / "result.json", result)
    print(json.dumps({"phase": result["phase"], "exit_code": code,
                      "P8_003_done": False, "public_output": str(public)}))
    return code

if __name__ == "__main__":
    raise SystemExit(main())

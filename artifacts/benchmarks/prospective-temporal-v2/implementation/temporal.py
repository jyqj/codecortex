#!/usr/bin/env python3
"""Artifact-domain prospective temporal controls. Never edits product/scorer/v1 data."""
import sys
sys.dont_write_bytecode = True
import argparse
import base64
import collections
import datetime
import hashlib
import http.server
import threading
import json
import math
import os
from pathlib import Path
import platform
import random
import re
import shutil
import signal
import stat
import subprocess
import time
import unicodedata
import urllib.error
import urllib.request
import zipfile

PROTOCOL = "p8-004-prospective-temporal-v2"
ARMS = ("candidate_local_default", "rg_baseline")
VARIANTS = ("en_original", "zh_translation", "en_paraphrase", "zh_paraphrase")
CONFIG = {"auto_index": {"enabled": False},
          "indexing": {"include_text_files": True, "include_hidden_files": True}}
BUDGET = {"top_k": 10, "repetitions": 3, "warmup": 0, "timeout_ms": 30000, "seed": 20261003}
FACETS = {"api_or_symbol_location": 3, "behavior_or_semantic_feature": 3,
          "configuration_or_error_handling": 2, "multi_step_call_chain": 2,
          "no_answer_under_explicit_scope": 2}
CATEGORIES = {"file_exact_match", "configuration_lookup", "component_location", "api_usage",
              "semantic_feature", "error_handling", "architecture_understanding",
              "cross_language", "symbol_location", "call_chain"}
DERIVED = ("metrics.json", "scores.jsonl", "query-slices.json", "costs.jsonl",
           "latency-summary.json", "latency-strata.json", "resource-ledger.json",
           "gate.json", "report.md")
REQUIRED_HELPERS = {"temporal.py", "README.md", "protocol.json", "PROTOCOL.md",
                    "locks.template.json", "schedule.row-indices.json"}
HEX = re.compile(r"[0-9a-f]{64}")
MAX_FILE = 512 * 1024 * 1024

def need(value, label):
    if not value:
        raise ValueError(label)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def jb(obj):
    return (json.dumps(obj, sort_keys=True, ensure_ascii=False, indent=2,
                       allow_nan=False) + "\n").encode()

def raw(path):
    path = Path(path).absolute()
    for p in (path, *path.parents):
        need(not p.is_symlink(), "symlink input: " + str(p))
    mode = path.stat()
    need(stat.S_ISREG(mode.st_mode) and mode.st_size <= MAX_FILE, "input type/size")
    result = path.read_bytes()
    need(len(result) == mode.st_size, "input changed while reading")
    return result

def read(path):
    return json.loads(raw(path))

def lines(path):
    return [json.loads(line) for line in raw(path).decode("utf-8").splitlines() if line.strip()]

def write(path, obj):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as f:
        f.write(jb(obj))
        f.flush()
        os.fsync(f.fileno())

def relative(name):
    need(isinstance(name, str) and name and not any(c in name for c in "\\:*?[")
         and all(ord(c) >= 32 for c in name)
         and all(p not in ("", ".", "..") for p in name.split("/")), "relative path")
    return name

def file_record(path):
    content = raw(path)
    return {"sha256": sha(content), "bytes": len(content),
            "mode": stat.S_IMODE(Path(path).stat().st_mode)}

def check_record(path, record, mode=False):
    observed = file_record(path)
    need(HEX.fullmatch(str(record.get("sha256", ""))) is not None, "invalid SHA256 pin")
    need(all(observed[k] == record[k] for k in ("sha256", "bytes")), "file digest/size drift: " + str(path))
    if mode:
        need(observed["mode"] == record["mode"], "file mode drift: " + str(path))
    return observed

def inventory(directory):
    directory = Path(directory).resolve(strict=True)
    return {p.relative_to(directory).as_posix(): file_record(p)
            for p in sorted(directory.rglob("*")) if not p.is_dir() or p.is_symlink()}

def disjoint(out, *inputs):
    out = Path(out).resolve()
    for p in inputs:
        p = Path(p).resolve()
        need(out != p and not out.is_relative_to(p) and not p.is_relative_to(out),
             "output and immutable inputs overlap")

def fresh(path):
    path = Path(path).absolute()
    need(not path.exists() and not path.is_symlink(), "new output required: " + str(path))
    path.mkdir(parents=True)
    return path

def git(root, *args):
    return subprocess.check_output(
        ["git", "-C", str(root), *args], stdin=subprocess.DEVNULL, stderr=subprocess.PIPE,
        env=dict(os.environ, GIT_NO_LAZY_FETCH="1", GIT_TERMINAL_PROMPT="0",
                 GIT_OPTIONAL_LOCKS="0"), timeout=120)

def candidate_snapshot(root, commit):
    root = Path(root).resolve(strict=True)
    need(re.fullmatch("[0-9a-f]{40}", commit) is not None, "full candidate SHA")
    need(git(root, "rev-parse", "--show-toplevel").decode().strip() == str(root), "checkout root")
    need(git(root, "rev-parse", "HEAD").decode().strip() == commit, "candidate HEAD drift")
    need(not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "dirty candidate")
    need(not git(root, "submodule", "status", "--recursive").strip(), "submodules")
    groups = {}
    for name, domains in (("source", ("crates", "Cargo.toml", "Cargo.lock")),
                          ("validation", ("scripts", ".github/workflows", "tests/source_integrity"))):
        entries = {}
        for e in git(root, "ls-tree", "-r", "-z", commit, "--", *domains).split(b"\0"):
            if not e:
                continue
            meta, path = e.split(b"\t", 1)
            mode, kind, oid = meta.decode().split()
            need(kind == "blob" and mode in ("100644", "100755"), "nonregular committed input")
            path = path.decode()
            b = raw(root / relative(path))
            need(hashlib.sha1(b"blob " + str(len(b)).encode() + b"\0" + b).hexdigest() == oid,
                 "working bytes differ from Git: " + path)
            need(stat.S_IMODE((root / path).stat().st_mode) == int(mode[-3:], 8), "working mode drift")
            entries[path] = {"bytes": len(b), "sha256": sha(b), "git_blob": oid, "git_mode": mode}
        need(entries, "empty candidate inventory")
        groups[name] = {"count": len(entries), "sha256": sha(jb(entries)), "entries": entries}
    return {"commit": commit, "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
            "dirty": False, **groups}

def helper_snapshot(directory):
    entries = inventory(directory)
    need(set(entries) == REQUIRED_HELPERS, "complete artifact helper map differs")
    need(Path(__file__).resolve() == Path(directory).resolve() / "temporal.py", "wrong helper entrypoint")
    return {"entries": entries, "sha256": sha(jb(entries)), "count": len(entries)}

def python_closure():
    result = {}
    for name, mod in sorted(sys.modules.items()):
        path = getattr(mod, "__file__", None)
        if path and Path(path).is_file():
            path = Path(path).resolve()
            if path.suffix == ".pyc" and path.with_suffix(".py").is_file():
                path = path.with_suffix(".py")
            result[name] = {"path": str(path), **file_record(path)}
        else:
            result[name] = {"builtin_or_frozen": True}
    return {"executable": file_record(Path(sys.executable).resolve(strict=True)), "version": sys.version,
            "unicodedata_version": unicodedata.unidata_version,
            "modules": result, "scope": "actual loaded Python dependency closure; no third-party modules"}

def environment(rg):
    need(sys.maxsize == 2**63 - 1, "64-bit Python/runner environment required")
    version = subprocess.check_output([str(rg), "--version"], timeout=30).decode()
    need(version.startswith("ripgrep "), "rg identity")
    return {"system": platform.system(), "kernel": platform.release(),
            "architecture": platform.machine(), "logical_cpus": os.cpu_count(),
            "python": platform.python_version(), "unicodedata": unicodedata.unidata_version,
            "rg": file_record(rg), "rg_version": version,
            "rustc": {"path": shutil.which("rustc"),
                      "version": subprocess.check_output(["rustc", "--version", "--verbose"], timeout=30).decode()},
            "cargo": {"path": shutil.which("cargo"),
                      "version": subprocess.check_output(["cargo", "--version", "--verbose"], timeout=30).decode()},
            "cpu_model": next((x.split(":", 1)[1].strip() for x in Path("/proc/cpuinfo").read_text().splitlines()
                               if x.startswith("model name") and ":" in x), None) if Path("/proc/cpuinfo").exists() else None,
            "memory_limit": Path("/sys/fs/cgroup/memory.max").read_text().strip() if Path("/sys/fs/cgroup/memory.max").exists() else None,
            "git": file_record(shutil.which("git")),
            "git_version": subprocess.check_output(["git", "--version"], timeout=30).decode(),
            "actions": {k: os.environ.get(k) for k in
                        ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_JOB", "GITHUB_SHA", "ImageOS", "ImageVersion")},
            "network_observation": "not_observed; no syscall-zero claim"}

def execution_env(rg, home):
    # An explicit allowlist prevents provider credentials/config or Python imports
    # inherited from the operator's environment from entering the benchmark.
    result = {k: os.environ[k] for k in
              ("PATH", "LD_LIBRARY_PATH", "RUSTUP_HOME", "CARGO_HOME", "RUSTUP_TOOLCHAIN")
              if k in os.environ}
    result.update(PATH=str(Path(rg).resolve().parent) + os.pathsep + result.get("PATH", ""),
                  HOME=str(home), XDG_CONFIG_HOME=str(home / "config"),
                  XDG_CACHE_HOME=str(home / "cache"), LANG="C.UTF-8", LC_ALL="C.UTF-8",
                  TZ="UTC", PYTHONNOUSERSITE="1", PYTHONDONTWRITEBYTECODE="1",
                  GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0", GIT_NO_LAZY_FETCH="1")
    need(Path(shutil.which("rg", path=result["PATH"])).resolve() == Path(rg).resolve(), "rg PATH drift")
    return result

def command(argv, destination, cwd, env, timeout):
    destination = fresh(destination)
    start = time.time()
    receipt = {"argv": [str(x) for x in argv], "cwd": str(cwd), "started_unix": start,
               "timeout_seconds": timeout, "executable": file_record(argv[0]),
               "process_exit_code": None, "timed_out": False}
    with (destination / "stdout.log").open("xb") as out, (destination / "stderr.log").open("xb") as err:
        p = subprocess.Popen(receipt["argv"], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                             stdout=out, stderr=err, start_new_session=True)
        receipt["pid"] = p.pid
        try:
            receipt["process_exit_code"] = p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            receipt["process_exit_code"] = p.wait()
            receipt["timed_out"] = True
    receipt["completed_unix"] = time.time()
    receipt["effective_exit_code"] = 2 if receipt["timed_out"] else receipt["process_exit_code"]
    receipt["streams"] = {name: file_record(destination / name) for name in ("stdout.log", "stderr.log")}
    write(destination / "receipt.json", receipt)
    return receipt

def zero(receipt):
    need(receipt["effective_exit_code"] == 0, "command failed; original streams retained")

def schedule(protocol_dir):
    planned = read(protocol_dir / "schedule.row-indices.json")
    expected = []
    mask = (1 << 64) - 1
    for arm in ARMS:
        for rep in range(3):
            order, state = list(range(48)), 20261003 + rep
            for i in range(47, 0, -1):
                if state == 0:
                    state = 0x9e3779b97f4a7c15
                state ^= (state << 13) & mask
                state ^= state >> 7
                state ^= (state << 17) & mask
                j = state % (i + 1)
                order[i], order[j] = order[j], order[i]
            for row in order:
                expected.append({"ordinal": len(expected), "arm": arm, "repetition": rep,
                                 "input_row_index": row, "family_slot": "f%04d" % (row // 4 + 1),
                                 "variant": VARIANTS[row % 4]})
    need(planned["protocol_id"] == PROTOCOL and planned["count"] == 288
         and planned["requests"] == expected, "planned schedule differs from original Rust algorithm")
    return expected

def unpack_package(package, package_sha, archive, output, root, expected_source):
    check_record(package, {"sha256": package_sha, "bytes": Path(package).stat().st_size})
    m = read(package)
    need(m["product_source"] == m["evidence_source"] == expected_source, "package source")
    need(m["configuration_profile"] == "local-text-hidden" and m["engine_config"] == CONFIG,
         "candidate package configuration")
    check_record(archive, m["archive"])
    declared = {item["member"]: item for item in [*m["tools"].values(), *m["helpers"].values()]}
    need(len(declared) == len(m["tools"]) + len(m["helpers"]), "duplicate package member")
    with zipfile.ZipFile(archive) as z:
        need(len(z.namelist()) == len(declared) and set(z.namelist()) == set(declared), "package ZIP inventory")
        for name, item in declared.items():
            relative(name)
            info = z.getinfo(name)
            need(info.file_size <= MAX_FILE and not info.is_dir(), "package member shape")
            b = z.read(name)
            need(len(b) == item["bytes"] and sha(b) == item["sha256"], "package member drift")
            dest = output / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            with dest.open("xb") as f:
                f.write(b)
            dest.chmod(0o755 if item.get("executable") else 0o644)
    source = candidate_snapshot(root, expected_source)
    source_map = {p: row["sha256"] for p, row in source["source"]["entries"].items()}
    binaries = {}
    toolchains = []
    for key, role, name in (("product", "product", "codecortex"), ("evaluator", "runner", "cc-eval")):
        binary = output / m["tools"][key]["member"]
        rec = read(output / m["tools"][key + "_receipt"]["member"])
        need(rec["build_exit_code"] == 0 and type(rec["build_exit_code"]) is int, "actual build exit")
        summary = rec["source_before"]
        need(summary == rec["source_after"] and summary["source_commit"] == expected_source
             and summary["source_tree"] == source["tree"]
             and summary["input_count"] == len(source_map)
             and summary["manifest_sha256"] == sha(jb(source_map)), "actual source build identity")
        need(read(output / m["tools"][key + "_source_inputs"]["member"]) == source_map, "build source inputs")
        binary_bytes = raw(binary)
        need(rec["binary_sha256"] == sha(binary_bytes), "actual binary hash")
        need(binary_bytes[:5] == b"\x7fELF\x02", "actual64-bit Linux ELF binary required")
        artifact = rec["cargo_artifact"]
        need(artifact["target"]["name"] == name and artifact["target"]["kind"] == ["bin"]
             and artifact["features"] == [] and artifact["profile"]["test"] is False,
             "default actual Cargo artifact")
        need(rec["build_profile"] == "release" and "--release" in rec["build_command"]
             and rec["actual_cargo_profile"] == artifact["profile"]
             and artifact["profile"]["opt_level"] in ("1", "2", "3", "s", "z")
             and artifact["profile"]["debug_assertions"] is False, "actual optimized release profile")
        pkg = "cc-server" if role == "product" else "cc-eval"
        need(Path(artifact["manifest_path"]) == root / "crates" / pkg / "Cargo.toml",
             "restore original compile-time candidate checkout path")
        cargo = lines(output / m["tools"][key + "_raw_build"]["member"])
        candidates = [x for x in cargo if x.get("reason") == "compiler-artifact"
                      and x.get("target", {}).get("name") == name]
        need(len(candidates) == 1 and candidates[0] == artifact
             and [x.get("success") for x in cargo if x.get("reason") == "build-finished"] == [True],
             "raw Cargo artifact identity")
        toolchains.append(rec["toolchain"])
        binaries[role] = binary
    need(toolchains[0] == toolchains[1], "different build toolchains")
    for name, item in m["helpers"].items():
        need(raw(root / relative(name)) == raw(output / item["member"]), "fixed package helper drift")
    validation = read(output / m["tools"]["candidate_build_validation"]["member"])
    need(validation["status"] == "actual_same_source_release_builds_verified"
         and validation["release_certified"] is False, "original package validation receipt")
    return m, source, binaries


PROTECTED_PARTS = {".git", ".hg", ".svn", ".codecortex", ".codecortex.json",
    ".DS_Store", "Thumbs.db", ".config", ".cache", ".local", ".ssh", ".gnupg",
    ".venv", "venv", "__pycache__", "node_modules", "vendor", ".mypy_cache",
    ".pytest_cache", ".tox", ".eggs", "target", "dist", "build", "coverage",
    ".next", ".idea", ".vscode"}

def source_policy_reason(name, content):
    parts = name.split("/")
    if any(p in PROTECTED_PARTS or p.startswith((".env", ".codecortex")) for p in parts):
        return "fixed_product_protected_path"
    if len(content) > 512000:
        return "fixed_default_product_size_cap_512000"
    if b"\0" in content:
        return "NUL_binary"
    try:
        content.decode("utf-8")
    except UnicodeDecodeError:
        return "non_UTF8"
    if content.startswith(b"version https://git-lfs.github.com/spec/"):
        return "unresolved_LFS_pointer"
    return None

def generic_source_admission(root, commit):
    need(re.fullmatch("[0-9a-f]{40}", commit) is not None, "full upstream commit")
    need(git(root, "rev-parse", "HEAD").decode().strip() == commit
         and not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "clean upstream source")
    need(not git(root, "submodule", "status", "--recursive").strip(), "expanded submodule source not supported")
    admitted, excluded = {}, {}
    for entry in git(root, "ls-tree", "-r", "-z", commit).split(b"\0"):
        if not entry:
            continue
        header, name = entry.split(b"\t", 1)
        mode, kind, blob = header.decode().split()
        name = relative(name.decode())
        need(kind == "blob" and mode in ("100644", "100755"), "upstream symlink/gitlink not admitted")
        content = raw(root / name)
        need(hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest() == blob,
             "upstream bytes differ from exact Git tree")
        record = {**file_record(root / name), "git_blob": blob, "git_mode": mode}
        need(record["mode"] == int(mode[-3:], 8), "upstream mode drift")
        reason = source_policy_reason(name, content)
        if reason is None:
            admitted[name] = record
        else:
            excluded[name] = {**record, "reason": reason}
    need(admitted, "empty admitted upstream source")
    return {"schema_version": 1, "protocol_id": PROTOCOL, "commit": commit,
            "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
            "admitted": admitted, "excluded": excluded,
            "policy": {"kind": "fixed_generic_local_text_hidden_admission",
                       "max_bytes": 512000, "UTF8": True, "NUL_free": True,
                       "protected_parts": sorted(PROTECTED_PARTS), "protected_prefixes": [".env", ".codecortex"],
                       "Git_tracked_only": True, "submodules_symlinks_LFS": "reject_or_exclude",
                       "license_files": "admitted if same generic text rules pass; no gold-dependent filter",
                       "gitignore": "materialized source has no .git; runner copies the explicit full admitted inventory",
                       "query_or_gold_based_filter": False},
            "helper_sha256": sha(raw(__file__)),
            "scope": "all tracked upstream files classified before authoring; metadata novelty/license review separate"}

def prepare_source(args):
    disjoint(args.output, args.upstream_root, args.source_root, Path(__file__).parent,
             args.package_manifest, args.archive)
    out = fresh(args.output)
    args.output_owned = True
    root, helper_dir, helper, env, package, candidate, binaries, child_env, binaries_before = setup(args, out)
    upstream = args.upstream_root.resolve(strict=True)
    admission = generic_source_admission(upstream, args.upstream_commit)
    write(out / "source-admission.json", admission)
    digest_only = fresh(out / "digest-only-control")
    # The original Rust freeze API requires one valid Query. This fixed invented
    # setup record is never searched, never supplied as new test data and never
    # used for gold selection. Its purpose is solely computing original BLAKE3
    # SourceLock over the complete prospective file list.
    control = {"id": "digest-only-invented-control", "category": "api_usage", "difficulty": 1,
               "language": "unspecified", "split": "dev", "query_family": "digest-only-invented-control",
               "query": "digest_only_never_dispatched", "path_prefix": None, "no_answer": True,
               "expected_files": [], "answers": [], "annotations": {"not_test_data": True}}
    (digest_only / "queries.jsonl").write_bytes(json.dumps(control).encode() + b"\n")
    write(digest_only / "suite.json", {"schema_version": 1, "name": "pre-author-source-digest-only",
          "source": {"root": str(upstream), "commit": args.upstream_commit,
                     "digest": "", "files": sorted(admission["admitted"])},
          "queries": "queries.jsonl", "queries_digest": "", "scoring": "codecortex-native-v1",
          **BUDGET, "engine_config": CONFIG})
    zero(command([binaries["runner"], "freeze", "--suite", digest_only / "suite.json"],
                 out / "commands/source-lock-freeze", root, child_env, 600))
    zero(command([binaries["runner"], "validate", "--suite", digest_only / "suite.json"],
                 out / "commands/source-lock-validate", root, child_env, 600))
    need(binary_snapshot(binaries) == binaries_before, "source-preparation evaluator/product bytes drifted")
    need(generic_source_admission(upstream, args.upstream_commit) == admission, "upstream drift")
    need(candidate_snapshot(root, args.expected_source) == candidate
         and helper_snapshot(helper_dir) == helper, "candidate/helper drift")
    locked = read(digest_only / "suite.json")["source"]
    write(out / "source-lock.json", locked)
    write(out / "result.json", {"status": "pre_author_source_inventory_and_original_source_lock_recorded",
         "source_admission": file_record(out / "source-admission.json"),
         "source_lock": file_record(out / "source-lock.json"), "source_digest": locked["digest"],
         "admitted": len(admission["admitted"]), "excluded": len(admission["excluded"]),
         "candidate": candidate, "helpers": helper, "environment": env,
         "python_dependency_closure": python_closure(), "new_test_bodies": 0,
         "scheduled_product_queries": 0, "operational_index_readiness": "not_run",
         "repository_license_novelty_independent_review": "separate_required",
         "authoring_authorized": False})
    print(json.dumps({"status": "source_inputs_prepared_not_authoring_authorization",
                      "admitted": len(admission["admitted"]), "excluded": len(admission["excluded"]),
                      "scheduled_product_queries": 0}))
    return 0

def template_shape(actual, template, prefix=""):
    if isinstance(template, dict):
        need(isinstance(actual, dict) and set(actual) == set(template), "lock template keys: " + prefix)
        for k in template:
            template_shape(actual[k], template[k], prefix + "." + k)

def dependency_fingerprint():
    closure = python_closure()
    return {"executable": closure["executable"], "version": closure["version"],
            "unicodedata_version": closure["unicodedata_version"],
            "modules": {name: {k: v for k, v in record.items() if k != "path"}
                        for name, record in closure["modules"].items()}}

def tokens(text):
    return set(re.findall(r"[^\W_]+", unicodedata.normalize("NFKC", text).casefold(), flags=re.UNICODE))

def span_bytes(root, evidence, admission):
    path = relative(evidence["path"])
    need(path in admission, "gold/negative outside presealed admitted source")
    b = raw(root / path)
    need(sha(b) == admission[path]["sha256"], "source span file drift")
    start, end = evidence["span"]["start"], evidence["span"]["end"]
    need(type(start) is int and type(end) is int and 0 <= start < end <= len(b), "byte span bounds")
    b[:start].decode("utf-8")
    b[:end].decode("utf-8")
    part = b[start:end]
    need(sha(part) == evidence["span_sha256"], "gold/negative span hash")
    return part.decode("utf-8")

def validate_data(data, synthetic=False):
    bundle = read(data / "data.json")
    need(bundle["protocol_id"] == PROTOCOL and bundle["synthetic_control_only"] is synthetic,
         "real/synthetic data domain confusion")
    suite_path = data / relative(bundle["suite"])
    suite = read(suite_path)
    need(suite["schema_version"] == 1 and suite["scoring"] == "codecortex-native-v1", "original native suite")
    need(all(type(suite[k]) is int and suite[k] == v for k, v in BUDGET.items()), "original complete budgets")
    need(suite["engine_config"] == CONFIG
         and type(suite["engine_config"]["auto_index"]["enabled"]) is bool
         and all(type(v) is bool for v in suite["engine_config"]["indexing"].values()), "exact named config")
    root = (suite_path.parent / suite["source"]["root"]).resolve(strict=True)
    query_path = (suite_path.parent / suite["queries"]).resolve(strict=True)
    need(not query_path.is_relative_to(root), "question data cannot enter product source")
    rows = lines(query_path)
    need(len(rows) == 48, "exact 48 native rows")
    admitted = read(data / relative(bundle["source_admission"]))["admitted"]
    need(suite["source"]["files"] == sorted(admitted), "presealed full source inclusion changed")
    for name, pin in admitted.items():
        check_record(root / relative(name), pin, mode=True)
    if synthetic:
        need(suite["source"]["commit"] is None and not (root / ".git").exists(), "synthetic snapshot")
    else:
        commit = suite["source"]["commit"]
        need(re.fullmatch("[0-9a-f]{40}", str(commit)) is not None, "real upstream fixed commit")
        need(git(root, "rev-parse", "HEAD").decode().strip() == commit
             and not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "upstream source drift")
        admission = read(data / relative(bundle["source_admission"]))
        need(admission == generic_source_admission(root, commit), "generic preauthor admission was filtered or drifted")
        excluded = admission["excluded"]
        need(not (set(admitted) & set(excluded)), "overlapping source admission")
        all_paths = {p.decode() for p in git(root, "ls-files", "-z").split(b"\0") if p}
        need(all_paths == set(admitted) | set(excluded), "incomplete generic source admission")
        for name, pin in excluded.items():
            need(pin["reason"], "excluded source lacks prospective reason")
            check_record(root / relative(name), pin, mode=True)
    gold = read(data / relative(bundle["gold_review"]))
    repo_id = bundle["reserved_repository_id"]
    need(re.fullmatch("[a-z0-9][a-z0-9_-]*", repo_id), "reserved repository id")
    families = ["ptv2." + repo_id + ".f%04d" % n for n in range(1, 13)]
    need(bundle["reserved_family_ids"] == families and set(gold["families"]) == set(families),
         "monotonic reserved family inventory")
    need(gold["author_id"] != gold["reviewer_id"] and gold["author_id"] and gold["reviewer_id"],
         "distinct source/gold reviewer")
    need(gold["status"] == ("synthetic_control_definition" if synthetic else "accepted_source_only"),
         "source/gold review not accepted")
    by_id, coverage, zero_lexical, span_rows = {}, collections.Counter(), [], []
    for idx, family in enumerate(families):
        four = rows[4 * idx:4 * idx + 4]
        review = gold["families"][family]
        facet = review["source_task_facet"]
        need(facet in FACETS, "source-task coverage tag")
        coverage[facet] += 1
        first = four[0]
        need(review["global_component"] == family, "merged/quarantined scope is below planned 12 components")
        shared = {k: first[k] for k in ("answers", "expected_files", "path_prefix", "no_answer",
                                       "category", "difficulty", "language", "query_family")}
        for q, variant in zip(four, VARIANTS):
            need(q["id"] == family + "." + variant and q["query_family"] == family, "family/variant order")
            need(q["id"] not in by_id, "duplicate query")
            by_id[q["id"]] = q
            need({k: q[k] for k in shared} == shared, "variant gold/scope/task metadata differs")
            need(q["category"] in CATEGORIES and q["split"] == "holdout"
                 and type(q["no_answer"]) is bool and type(q["difficulty"]) is int and q["difficulty"] in (1, 2, 3), "original query categories")
            a = q["annotations"]["prospective_temporal_v2"]
            need(a["protocol_id"] == PROTOCOL and a["split_scheme"] == "prospective_temporal_test_only"
                 and a["variant"] == variant and a["global_component"] == family
                 and a["query_language"] == ("zh" if variant.startswith("zh") else "en"),
                 "explicit temporal annotations")
            need(q["query"].strip() and len(q["query"].encode()) <= 4096, "query bounds")
        need(review["variants_semantically_equivalent"] is True
             and review["natural_language_accepted"] is True and review["scope"] == first["path_prefix"],
             "source-only semantic/scope review")
        if first["no_answer"]:
            need(facet == "no_answer_under_explicit_scope" and not first["answers"]
                 and not first["expected_files"] and first["path_prefix"]
                 and review["absence_evidence"] and review["no_answer_scope_reviewed"] is True,
                 "independent no-answer scope evidence")
            continue
        need(facet != "no_answer_under_explicit_scope" and first["answers"]
             and any(g["primary"] is True for g in first["answers"]) and not first["expected_files"],
             "native primary/supporting gold")
        evidence = review["gold_spans"]
        expected = [a for group in first["answers"] for a in group["alternatives"]]
        need(len(evidence) == len(expected), "every alternative needs source evidence")
        gold_tokens = set()
        for alt, ev in zip(expected, evidence):
            need(ev["path"] == alt["path"] and ev["span"] == alt["span"]
                 and ev["symbol"] == alt["symbol"], "gold evidence/answer disagreement")
            text = span_bytes(root, ev, admitted)
            symbols = " ".join(str(v) for v in (ev["symbol"] or {}).values() if v is not None)
            gold_tokens |= tokens(text + " " + ev["path"] + " " + symbols)
            span_rows.append({"family": family, **ev})
        negatives = review["hard_negatives"]
        need(negatives, "answerable family requires a real hard negative")
        negative_tokens = []
        for neg in negatives:
            need(neg["why_wrong"].strip() and neg["independent_wrongness_reviewed"] is True,
                 "hard negative wrongness review")
            text = span_bytes(root, neg, admitted)
            negative_tokens.append(tokens(text + " " + neg["path"] + " " + neg.get("symbol_name", "")))
        if review["zero_lexical_english_original"] is True:
            need(facet == "behavior_or_semantic_feature", "English semantic zero-overlap stratum")
            qt = tokens(first["query"])
            intersection = qt & gold_tokens
            ni = [qt & n for n in negative_tokens]
            need(qt and not intersection and any(len(x) > len(intersection) for x in ni),
                 "raw-token zero-overlap predicate failed")
            zero_lexical.append({"family": family, "query_tokens": sorted(qt),
                                 "gold_union_tokens": sorted(gold_tokens),
                                 "intersection": sorted(intersection),
                                 "negative_intersections": [sorted(x) for x in ni]})
    need(dict(coverage) == FACETS and len(zero_lexical) >= 2, "planned family coverage")
    check_record(query_path, bundle["query_record"])
    need(bundle["review_status"] == ("synthetic_control_only" if synthetic else "accepted_independent"),
         "real source-gold review required")
    return {"data_root": data, "bundle": bundle, "suite": suite, "suite_path": suite_path, "root": root,
            "query_path": query_path, "rows": rows, "by_id": by_id, "gold": gold,
            "source": admitted, "zero_lexical": zero_lexical, "spans": span_rows}

def data_commitments(checked):
    families = checked["gold"]["families"]
    return {"native_query_sha256": sha(raw(checked["query_path"])),
            "gold_sha256": sha(raw(checked["data_root"] / checked["bundle"]["gold_review"])),
            "source_span_manifest_sha256": sha(jb(checked["spans"])),
            "hard_negatives_sha256": sha(jb({f: x["hard_negatives"] for f, x in families.items()})),
            "components_sha256": sha(jb({f: x["global_component"] for f, x in families.items()})),
            "zero_lexical_receipt_sha256": sha(jb(checked["zero_lexical"]))}


def binary_snapshot(binaries):
    return {role: file_record(path) for role, path in sorted(binaries.items())}

def sealed_binary_binding(locked, permit, binaries):
    actual = binary_snapshot(binaries)
    for role, key in (("product", "product_binary_sha256"), ("runner", "eval_binary_sha256")):
        need(locked["candidate"][key] == permit[key] == actual[role]["sha256"],
             "sealed/authorized/actual binary identity differs: " + role)
    return actual

def chronology(stamps, exposure):
    need(stamps["candidate_seal"] < stamps["author_start"] < stamps["first_draft"]
         and stamps["candidate_seal"] < stamps["reviewer_start"] < stamps["data_seal"]
         and stamps["first_draft"] < stamps["data_seal"] < stamps["release"]
         and stamps["release"].timestamp() < time.time(),
         "prospective service chronology")
    if exposure["kind"] == "witnessed_exposure":
        when = datetime.datetime.fromisoformat(exposure["observed_utc"].replace("Z", "+00:00"))
        need(when.tzinfo is not None and stamps["candidate_seal"] < when
             and stamps["first_draft"] <= when and when.timestamp() <= time.time(),
             "witnessed exposure predates freeze/draft or is in the future")
    else:
        need(exposure["kind"] == "not_observed_before_execution",
             "unknown exposure cannot be replaced with not-observed evidence")
        cutoff = datetime.datetime.fromisoformat(exposure["observation_cutoff_utc"].replace("Z", "+00:00"))
        need(cutoff.tzinfo is not None and cutoff >= stamps["release"]
             and cutoff.timestamp() <= time.time()
             and exposure["independent_information_flow_review"] == "accepted_observation",
             "not-observed information-flow coverage must reach release")
    return True

def nonnull(obj, prefix="", exceptions=()):
    if prefix in exceptions:
        return
    need(obj is not None, "unfilled blocking lock: " + prefix)
    if isinstance(obj, dict):
        for k, v in obj.items():
            nonnull(v, prefix + "." + k if prefix else k, exceptions)
    elif isinstance(obj, list):
        for i, v in enumerate(obj):
            nonnull(v, prefix + "[" + str(i) + "]", exceptions)

def permit_check(path, expected_sha, data, candidate, helpers, env):
    need(sha(raw(path)) == expected_sha, "execution authorization pin")
    permit = read(path)
    need(permit["protocol_id"] == PROTOCOL and permit["kind"] == "independent_one_attempt_authorization"
         and permit["status"] == "released_for_one_execution", "explicit independent execution release")
    locked = read(data / "locks.json")
    # Future results cannot exist before execution. The original template's
    # other deliberate nulls remain blockers, including dependency receipts.
    template_shape(locked, read(Path(__file__).parent / "locks.template.json"))
    nonnull(locked, exceptions=("execution", "freeze_order.first_scheduled_query_record"))
    need(locked["status"] == "DATA_SEALED" and locked["data"]["status"] == "SEALED"
         and locked["execution"]["status"] == "NOT_RUN"
         and locked["execution"]["attempt_consumed"] is False, "two seals required before attempt")
    need(locked["preserved_old_v1"] == {
        "custody_decision_sha256": "a8c88f2479843566d74a8a47f05da81da0588029dffa681c313e64939f344f9c",
        "certified_clean": 0, "checker_modified": False, "old_dataset_relabelled": False},
         "old custody boundary changed")
    need(locked["roles"]["author_agent_id"] != locked["roles"]["reviewer_agent_id"]
         and locked["roles"]["acl_boundary_claimed"] is False, "role/custody boundary")
    need(locked["candidate"]["git_commit"] == candidate["commit"]
         and locked["candidate"]["git_tree"] == candidate["tree"]
         and locked["candidate"]["source_inventory_sha256"] == candidate["source"]["sha256"]
         and locked["candidate"]["source_input_count"] == candidate["source"]["count"]
         and locked["candidate"]["validation_inventory_sha256"] == candidate["validation"]["sha256"]
         and locked["candidate"]["validation_input_count"] == candidate["validation"]["count"],
         "candidate/source/validation lock drift")
    measurement = locked["measurement"]
    need(all(measurement[k] == v and type(measurement[k]) is int for k, v in BUDGET.items())
         and type(measurement["concurrency"]) is int and measurement["concurrency"] == 1
         and type(measurement["total_planned_requests"]) is int and measurement["total_planned_requests"] == 288
         and measurement["provider_keys_present"] is False
         and measurement["pointer_width_bits"] == 64, "measurement lock changed")
    need(measurement["additional_artifact_inputs_complete_map"] == helpers
         and measurement["rg_binary_sha256"] == env["rg"]["sha256"]
         and measurement["rg_version"] == env["rg_version"]
         and measurement["python_version"] == env["python"]
         and measurement["unicodedata_version"] == env["unicodedata"], "helper/runtime lock drift")
    checked = validate_data(data, False)
    need(locked["data"]["native_query_sha256"] == sha(raw(checked["query_path"]))
         and locked["data"]["gold_sha256"] == sha(raw(data / checked["bundle"]["gold_review"]))
         and locked["repository"]["source_admission_manifest_sha256"] ==
             sha(raw(data / checked["bundle"]["source_admission"]))
         and locked["repository"]["source_lock_digest"] == checked["suite"]["source"]["digest"]
         and locked["repository"]["fixed_commit"] == checked["suite"]["source"]["commit"]
         and measurement["config_bytes_sha256"] == sha(jb(CONFIG))
         and measurement["planned_case_schedule_sha256"] ==
             sha(raw(Path(__file__).parent / "schedule.row-indices.json")),
         "data/source/config/schedule seal binding")
    need(checked["gold"]["author_id"] == locked["roles"]["author_agent_id"]
         and checked["gold"]["reviewer_id"] == locked["roles"]["reviewer_agent_id"],
         "sealed fresh roles differ from actual source-gold review identities")
    need(checked["bundle"]["reserved_repository_id"] == locked["repository"]["canonical_repository_id"],
         "reserved repository identity differs from the pre-author source seal")
    need(locked["protocol"]["file_sha256"] == sha(raw(Path(__file__).parent / "protocol.json"))
         and locked["measurement"]["model_revision"] == "not_applicable_local_only"
         and locked["measurement"]["encoding_space"] == "not_applicable_local_only"
         and locked["data"]["reserved_family_ids"] == checked["bundle"]["reserved_family_ids"]
         and locked["data"]["variant_schedule"] == list(VARIANTS)
         and locked["data"]["native_suite_lock"] == checked["suite"],
         "protocol/local-only model/data schedule/suite commitment")
    commitments = data_commitments(checked)
    need(all(locked["data"][k] == v for k, v in commitments.items()), "reviewed data commitments")
    for key in ("public_reference_allowlist_sha256", "leakage_review_sha256",
                "independent_source_gold_review_sha256", "quarantine_history_sha256"):
        name = checked["bundle"]["review_evidence_files"][key]
        need(sha(raw(data / relative(name))) == locked["data"][key], "independent review evidence pin")
    need(permit["actual_python_dependency_fingerprint"] == dependency_fingerprint(),
         "actual Python dependency closure differs from synthetic-presealed closure")
    need(permit["locks_sha256"] == sha(raw(data / "locks.json"))
         and permit["data_inventory"] == inventory(data)
         and permit["candidate_commit"] == candidate["commit"]
         and permit["helper_inventory_sha256"] == helpers["sha256"], "authorized data bytes")
    seal = {"candidate": candidate["commit"], "candidate_source": candidate["source"]["sha256"],
            "candidate_validation": candidate["validation"]["sha256"],
            "helpers": helpers["sha256"], "locks": permit["locks_sha256"],
            "data": sha(jb(permit["data_inventory"]))}
    attempt = sha(jb(seal))
    need(permit["attempt_id"] == attempt, "deterministic attempt identity")
    job = env["actions"]
    need(job["GITHUB_RUN_ID"] and job["GITHUB_RUN_ATTEMPT"] == "1"
         and permit["authorized_actions"] == {
             "run_id": job["GITHUB_RUN_ID"], "run_attempt": "1",
             "job_name": job["GITHUB_JOB"], "utility_sha": job["GITHUB_SHA"]},
         "only the exact authorized Actions job may dispatch")
    # These are independent raw service witnesses, not Git author dates or a
    # claim that a local lock prevents another machine from running.
    witnesses = permit["service_witnesses"]
    expected = ("candidate_seal", "author_start", "reviewer_start", "first_draft", "first_engine_owner_exposure", "data_seal", "release")
    need(set(witnesses) == set(expected), "service event witness coverage")
    stamps = {}
    for name in expected:
        witness = witnesses[name]
        check_record(path.parent / relative(witness["raw_path"]), witness["record"])
        need(witness["service_event_id"] and witness["reviewer_id"] != locked["roles"]["author_agent_id"],
             "independent service event binding")
        if name != "first_engine_owner_exposure":
            stamps[name] = datetime.datetime.fromisoformat(witness["observed_utc"].replace("Z", "+00:00"))
            need(stamps[name].tzinfo is not None, "observed UTC witness timezone")
    chronology(stamps, witnesses["first_engine_owner_exposure"])
    history = permit["prior_attempts"]
    check_record(path.parent / relative(history["raw_path"]), history["record"])
    need(history["independently_reviewed"] is True and history["same_seal_dispatched_attempts"] == []
         and history["unknown_dispatch_attempts"] == [], "prior or unknown dispatch consumes attempt")
    need(permit["manual_service_and_dependency_review"] == "accepted",
         "no self-authored metadata can replace independent service/dependency review")
    return permit, attempt

def make_synthetic(data):
    source = fresh(data / "source")
    inputs = fresh(data / "inputs")
    prefix = "ptv2.invented-control"
    rows, families, admitted = [], {}, {}
    facets = [key for key, n in FACETS.items() for _ in range(n)]
    for index in range(1, 13):
        family = prefix + ".f%04d" % index
        no_answer = index > 10
        path = ("absence/" if no_answer else "units/") + "unit%02d.py" % index
        body = ("def unit%02d(value):\n    return value + %d\n" % (index, index)).encode()
        target = source / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(body)
        target.chmod(0o644)
        admitted[path] = file_record(target)
        query = ("cobalt lantern" if index == 4 else "amber compass" if index == 5
                 else "unimplementedtransport%02d" % index if no_answer else "unit%02d" % index)
        symbol = {"name": "unit%02d" % index, "qname": None, "kind": None}
        span = {"start": 0, "end": len(body)}
        ev = {"path": path, "span": span, "symbol": symbol, "span_sha256": sha(body)}
        negatives = []
        if not no_answer:
            negative_path = "decoys/wrong%02d.py" % index
            neg = ("# " + query + "\ndef irrelevant():\n    return None\n").encode()
            t = source / negative_path
            t.parent.mkdir(parents=True, exist_ok=True)
            t.write_bytes(neg)
            t.chmod(0o644)
            admitted[negative_path] = file_record(t)
            negatives = [{"path": negative_path, "span": {"start": 0, "end": len(neg)},
                          "span_sha256": sha(neg), "symbol_name": "irrelevant",
                          "why_wrong": "Invented comment mentions the phrase; this function returns None, not the target increment.",
                          "independent_wrongness_reviewed": True}]
        scope = "absence/" if no_answer else None
        gold = [] if no_answer else [{"id": "primary", "primary": True, "grade": 2,
                                     "alternatives": [{"path": path, "symbol": symbol, "span": span}]}]
        facet = facets[index - 1]
        category = {"api_or_symbol_location": "symbol_location",
                    "behavior_or_semantic_feature": "semantic_feature",
                    "configuration_or_error_handling": "configuration_lookup",
                    "multi_step_call_chain": "call_chain",
                    "no_answer_under_explicit_scope": "api_usage"}[facet]
        families[family] = {"global_component": family, "source_task_facet": facet,
                            "variants_semantically_equivalent": True, "natural_language_accepted": True,
                            "scope": scope, "absence_evidence": "Invented absence/ contains only integer addition." if no_answer else None,
                            "no_answer_scope_reviewed": no_answer,
                            "gold_spans": [] if no_answer else [ev], "hard_negatives": negatives,
                            "zero_lexical_english_original": index in (4, 5)}
        for variant in VARIANTS:
            # These are explicitly artificial test controls, not independently
            # authored English/Chinese benchmark evidence or naturalness proof.
            text = query if variant.startswith("en") else "控制 " + query
            rows.append({"id": family + "." + variant, "category": category, "difficulty": 1,
                         "language": "Python", "split": "holdout", "query_family": family,
                         "query": text, "path_prefix": scope, "no_answer": no_answer,
                         "expected_files": [], "answers": gold,
                         "annotations": {"prospective_temporal_v2": {
                             "protocol_id": PROTOCOL, "split_scheme": "prospective_temporal_test_only",
                             "variant": variant, "global_component": family,
                             "query_language": "zh" if variant.startswith("zh") else "en",
                             "synthetic_control_only": True}}})
    queries = inputs / "queries.jsonl"
    queries.write_bytes(b"".join(json.dumps(q, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
                               for q in rows))
    write(inputs / "source-admission.json", {"admitted": admitted, "excluded": {},
                                           "scope": "invented controls only, no new public repository"})
    write(inputs / "gold-review.json", {"author_id": "synthetic-generator",
                                       "reviewer_id": "synthetic-control-definition-not-human-gold-review",
                                       "status": "synthetic_control_definition", "families": families})
    write(inputs / "suite.json", {"schema_version": 1, "name": "temporal-v2-invented-control",
                                 "source": {"root": "../source", "commit": None,
                                            "digest": "", "files": sorted(admitted)},
                                 "queries": "queries.jsonl", "queries_digest": "",
                                 "scoring": "codecortex-native-v1", **BUDGET, "engine_config": CONFIG})
    write(inputs / "data.json", {"protocol_id": PROTOCOL, "synthetic_control_only": True,
                                 "reserved_repository_id": "invented-control",
                                 "reserved_family_ids": list(families), "suite": "suite.json",
                                 "source_admission": "source-admission.json", "gold_review": "gold-review.json",
                                 "query_record": file_record(queries), "review_status": "synthetic_control_only"})
    return inputs


def contract_controls(data, output):
    before = inventory(data)
    baseline = validate_data(data, True)
    cases = ("variant_gold", "merged_component", "invented_category", "zero_overlap",
             "missing_span", "no_answer_scope", "configuration", "repetitions")
    expected_messages = {"variant_gold": "variant gold/scope/task metadata differs",
        "merged_component": "merged/quarantined scope", "invented_category": "original query categories",
        "zero_overlap": "raw-token zero-overlap predicate failed",
        "missing_span": "every alternative needs source evidence",
        "no_answer_scope": "independent no-answer scope evidence",
        "configuration": "exact named config", "repetitions": "original complete budgets"}
    results = []
    for label in cases:
        copied = output / label
        shutil.copytree(data, copied)
        suite = read(copied / "suite.json")
        suite["source"]["root"] = str(baseline["root"])
        query = lines(copied / "queries.jsonl")
        gold = read(copied / "gold-review.json")
        bundle = read(copied / "data.json")
        first = query[0]["query_family"]
        if label == "variant_gold":
            query[1]["answers"] = []
        elif label == "merged_component":
            gold["families"][first]["global_component"] = query[4]["query_family"]
        elif label == "invented_category":
            for row in query[-4:]:
                row["category"] = "no_answer"
        elif label == "zero_overlap":
            query[12]["query"] = "return"
        elif label == "missing_span":
            gold["families"][first]["gold_spans"] = []
        elif label == "no_answer_scope":
            for row in query[-4:]:
                row["path_prefix"] = None
            gold["families"][query[-1]["query_family"]]["scope"] = None
        elif label == "configuration":
            suite["engine_config"]["indexing"]["include_hidden_files"] = False
        else:
            suite["repetitions"] = 2
        # Only disposable owned copies change. Re-pin the control's query SHA
        # so a generic stale-hash error cannot substitute for the intended gate.
        (copied / "queries.jsonl").write_bytes(b"".join(json.dumps(q).encode() + b"\n" for q in query))
        bundle["query_record"] = file_record(copied / "queries.jsonl")
        for name, obj in (("suite.json", suite), ("gold-review.json", gold), ("data.json", bundle)):
            (copied / name).write_bytes(jb(obj))
        try:
            validate_data(copied, True)
        except ValueError as error:
            need(expected_messages[label] in str(error), "wrong temporal negative-control rejection")
            results.append({"case": label, "rejected": True, "reason": str(error),
                            "new_product_queries": 0})
        else:
            raise ValueError("temporal invalid control accepted: " + label)
    need(inventory(data) == before, "contract controls changed original data")
    return results

def negative_controls(validated, runner, work, cwd, env):
    original = validated["suite_path"].parent
    before = inventory(original)
    results = []
    # Preserve the same sibling source reference while making new owned query/
    # suite copies; no original source, binary or input is ever changed.
    for label in ("query_drift", "source_digest_drift", "configuration", "schedule_budget"):
        copied = fresh(work / label)
        suite = json.loads(raw(validated["suite_path"]))
        suite["source"]["root"] = str(validated["root"])
        qpath = copied / "queries.jsonl"
        qpath.write_bytes(raw(validated["query_path"]))
        suite["queries"] = "queries.jsonl"
        if label == "query_drift":
            with qpath.open("ab") as f:
                f.write(b"\n")
        elif label == "source_digest_drift":
            suite["source"]["digest"] = "0" * 64
        elif label == "configuration":
            suite["engine_config"]["indexing"]["include_hidden_files"] = False
        else:
            suite["repetitions"] = 2
        write(copied / "suite.json", suite)
        if label in ("query_drift", "source_digest_drift"):
            rec = command([runner, "validate", "--suite", copied / "suite.json"],
                          copied / "command", cwd, env, 600)
            need(rec["effective_exit_code"] == 2, "original Rust drift control must reject")
            output = raw(copied / "command/stdout.log") + raw(copied / "command/stderr.log")
            need(b"source or query content lock drift" in output, "wrong Rust rejection reason")
            results.append({"control": label, "actual_rust_exit": 2})
        else:
            # The original Rust accepts many budgets/configs; the prospective
            # operator must independently refuse departures from this plan.
            rejected = suite["engine_config"] != CONFIG or any(suite[k] != v for k, v in BUDGET.items())
            need(rejected, "prospective config/budget negative control")
            results.append({"control": label, "prospective_contract_rejected": True,
                            "original_Rust_run": "not_dispatched"})
    need(inventory(original) == before, "controls changed original inputs")
    return results


def redirect_control(output):
    counts = {"redirect_source": 0, "sink": 0, "sink_authorization": []}
    class Sink(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            counts["sink"] += 1
            counts["sink_authorization"].append(self.headers.get("Authorization"))
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"synthetic redirect sink")
        def log_message(self, *args):
            pass
    sink = http.server.HTTPServer(("127.0.0.1", 0), Sink)
    class Redirect(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            counts["redirect_source"] += 1
            self.send_response(302)
            self.send_header("Location", "http://127.0.0.1:%d/sink" % sink.server_port)
            self.end_headers()
        def log_message(self, *args):
            pass
    redirect = http.server.HTTPServer(("127.0.0.1", 0), Redirect)
    threads = [threading.Thread(target=s.serve_forever, daemon=True) for s in (sink, redirect)]
    for t in threads:
        t.start()
    try:
        request = urllib.request.Request("http://127.0.0.1:%d/start" % redirect.server_port,
                                         headers={"Authorization": "Bearer invented-control-not-a-secret"})
        try:
            github_open(request)
        except urllib.error.HTTPError as error:
            need(error.code == 302, "unexpected real redirect control result")
        else:
            raise ValueError("real redirect was followed")
        need(counts == {"redirect_source": 1, "sink": 0, "sink_authorization": []},
             "authorization header crossed redirect boundary")
        # The sink is actually reachable; its zero prior calls were not a
        # disabled listener or a mock networking verdict.
        with github_open(urllib.request.Request("http://127.0.0.1:%d/direct" % sink.server_port)) as response:
            need(response.read() == b"synthetic redirect sink", "live sink control")
        need(counts == {"redirect_source": 1, "sink": 1, "sink_authorization": [None]},
             "direct real sink control failed")
    finally:
        for server in (redirect, sink):
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=5)
            need(not thread.is_alive(), "owned control HTTP thread did not exit")
    result = {"kind": "actual_loopback_HTTP_redirect_negative_and_reachable_sink_positive",
              "counts": counts, "redirect_status": 302, "new_product_queries": 0,
              "scope": "actual opener rejects before forwarding; no external syscall-zero claim"}
    write(output, result)
    return result


def seal_controls(binaries, output):
    actual = binary_snapshot(binaries)
    base = {"candidate": {"product_binary_sha256": actual["product"]["sha256"],
                          "eval_binary_sha256": actual["runner"]["sha256"]}}
    permit = dict(base["candidate"])
    sealed_binary_binding(base, permit, binaries)
    results = []
    for side in ("candidate_seal", "release_permit"):
        for key in ("product_binary_sha256", "eval_binary_sha256"):
            lock_copy = json.loads(json.dumps(base))
            permit_copy = dict(permit)
            target = lock_copy["candidate"] if side == "candidate_seal" else permit_copy
            target[key] = "0" * 64 if target[key] != "0" * 64 else "1" * 64
            try:
                sealed_binary_binding(lock_copy, permit_copy, binaries)
            except ValueError as error:
                need("sealed/authorized/actual binary identity" in str(error), "wrong binary-seal rejection")
                results.append({"case": side + ":" + key, "rejected": True, "reason": str(error)})
            else:
                raise ValueError("single-field binary identity drift passed")
    now = datetime.datetime.now(datetime.timezone.utc)
    stamps = {name: now - datetime.timedelta(seconds=offset) for name, offset in (
        ("candidate_seal", 60), ("author_start", 50), ("reviewer_start", 40),
        ("first_draft", 30), ("data_seal", 20), ("release", 10))}
    observed = {"kind": "witnessed_exposure",
                "observed_utc": (now - datetime.timedelta(seconds=25)).isoformat()}
    absent = {"kind": "not_observed_before_execution",
              "observation_cutoff_utc": (now - datetime.timedelta(seconds=5)).isoformat(),
              "independent_information_flow_review": "accepted_observation"}
    chronology(stamps, observed)
    chronology(stamps, absent)
    for case in ("reviewer_after_seal", "exposure_before_draft", "future_exposure",
                 "unknown_exposure", "flow_cutoff_before_release"):
        changed = dict(stamps)
        exposure = dict(observed if case in ("exposure_before_draft", "future_exposure") else absent)
        if case == "reviewer_after_seal":
            changed["reviewer_start"] = now - datetime.timedelta(seconds=15)
        elif case == "exposure_before_draft":
            exposure["observed_utc"] = (now - datetime.timedelta(seconds=35)).isoformat()
        elif case == "future_exposure":
            exposure["observed_utc"] = (now + datetime.timedelta(days=1)).isoformat()
        elif case == "unknown_exposure":
            exposure["kind"] = "unknown"
        else:
            exposure["observation_cutoff_utc"] = (now - datetime.timedelta(seconds=15)).isoformat()
        try:
            chronology(changed, exposure)
        except ValueError as error:
            results.append({"case": case, "rejected": True, "reason": str(error)})
        else:
            raise ValueError("single-field chronology drift passed")
    # Real file drift only touches an owned copied product, never the actual
    # executed product/runner or original package. The common snapshot guard
    # must detect the changed bytes even if the file name/mode is retained.
    copied = fresh(output / "binary-copy")
    copy = copied / "product"
    shutil.copy2(binaries["product"], copy)
    alternate = {**binaries, "product": copy}
    need(binary_snapshot(alternate) == actual, "binary-copy control initial bytes")
    with copy.open("ab") as f:
        f.write(b"owned-copy-drift-control")
    need(binary_snapshot(alternate) != actual, "real executable file drift not detected")
    need(binary_snapshot(binaries) == actual, "seal controls changed actual executable")
    results.append({"case": "owned_executable_copy_byte_drift", "rejected": True,
                    "actual_binary_changed": False, "new_product_queries": 0})
    write(output / "review.json", {"status": "actual_preseal_synthetic_seal_controls",
                                  "controls": results, "new_product_queries": 0,
                                  "real_service_chronology_verified": False})
    return results

def setup(args, out):
    root = args.source_root.resolve(strict=True)
    helper_dir = Path(__file__).resolve().parent
    helper = helper_snapshot(helper_dir)
    need(helper["sha256"] == args.helper_map_sha256, "independently pinned helper map")
    schedule(helper_dir)
    env = environment(args.rg)
    binaries_dir = fresh(out / "package")
    package, candidate, binaries = unpack_package(
        args.package_manifest, args.package_manifest_sha256, args.archive,
        binaries_dir, root, args.expected_source)
    runtime = fresh(out / "runtime-home")
    child_env = execution_env(args.rg, runtime)
    service_source = github_json("/repos/jyqj/codecortex/git/commits/" + args.expected_source,
                                 out / "service-candidate-source.json")
    need(service_source["sha"] == args.expected_source
         and service_source["tree"]["sha"] == candidate["tree"], "actual GitHub source metadata binding")
    # Exercise the same actual HTTPS opener before the pre-author closure is
    # captured, in every execution/replay mode. IDNA/TLS lazy imports may not be
    # silently discarded from a later formal dependency fingerprint.
    write(out / "before.json", {"candidate": candidate, "helpers": helper, "environment": env,
                                "python_dependency_closure": python_closure(),
                                "package_manifest": file_record(args.package_manifest),
                                "archive": file_record(args.archive)})
    return root, helper_dir, helper, env, package, candidate, binaries, child_env, binary_snapshot(binaries)

def unchanged(args, helper_dir, helper, candidate, data, data_before, env_before, binaries, binaries_before):
    need(binary_snapshot(binaries) == binaries_before, "actual unpacked execution binaries drifted")
    need(candidate_snapshot(args.source_root, args.expected_source) == candidate, "candidate drift during execution")
    need(helper_snapshot(helper_dir) == helper, "artifact helper drift")
    need(inventory(data) == data_before, "sealed data drift")
    current = environment(args.rg)
    need(current == env_before, "execution environment/rg drift")
    return current


def positive_target_rows(rows, case_id, target):
    witnesses = [r for r in rows if r["case_id"] == case_id]
    need(len(witnesses) == 3 and all(
        any(h["path"] == target and h.get("evidence_valid") is True for h in r["hits"])
        for r in witnesses), "synthetic real positive target was not returned in every repetition")
    return witnesses

def execute_arms(args):
    immutable = (args.source_root, Path(__file__).parent, args.package_manifest, args.archive, args.rg)
    if args.command == "execute":
        immutable += (args.data, args.authorization)
    disjoint(args.output, *immutable)
    out = fresh(args.output)
    args.output_owned = True
    root, helper_dir, helper, env, package, candidate, binaries, child_env, binaries_before = setup(args, out)
    synthetic = args.command == "synthetic-control"
    if synthetic:
        data = make_synthetic(fresh(out / "synthetic-inputs"))
        freeze = command([binaries["runner"], "freeze", "--suite", data / "suite.json"],
                         out / "commands/freeze-control", root, child_env, 600)
        zero(freeze)
    else:
        data = args.data.resolve(strict=True)
    checked = validate_data(data, synthetic)
    validate = command([binaries["runner"], "validate", "--suite", checked["suite_path"]],
                       out / "commands/validate-original", root, child_env, 600)
    zero(validate)
    permit = None
    if synthetic:
        attempt_id = "synthetic-control-" + sha(raw(checked["query_path"]))
        controls = negative_controls(checked, binaries["runner"], fresh(out / "negative-controls"),
                                     root, child_env)
        controls += contract_controls(data, fresh(out / "temporal-contract-controls"))
        controls.append(redirect_control(out / "redirect-control.json"))
        controls += seal_controls(binaries, fresh(out / "seal-controls"))
        controls += report_contract_controls(out / "report-contract-controls.json")
        write(out / "negative-controls.json", controls)
    else:
        binding = read(args.authorization_binding)
        need(binding["authorization_sha256"] == args.authorization_sha256
             and binding["files"]["authorization.json"] == file_record(args.authorization),
             "authorization service binding")
        write(out / "authorization-ref-before.json", authorization_ref_check(binding, out / "service-ref-before-raw.json"))
        permit, attempt_id = permit_check(args.authorization, args.authorization_sha256, data,
                                           candidate, helper, env)
        need(permit["candidate_package_manifest_sha256"] == args.package_manifest_sha256
             and permit["archive_sha256"] == sha(raw(args.archive)), "authorized actual candidate package")
        sealed_binary_binding(read(data / "locks.json"), permit, binaries)
    data_before = inventory(data)
    source_before = {p: file_record(checked["root"] / p) for p in checked["source"]}
    write(out / "data-validation.json", {"status": "synthetic_controls_validated" if synthetic else "sealed_data_mechanically_validated",
                                       "scope": "not task acceptance or restricted custody",
                                       "rows": 48, "components": 12, "zero_lexical": checked["zero_lexical"],
                                       "source": source_before, "data_inventory": data_before,
                                       "derived_data_commitments": data_commitments(checked)})
    # O_EXCL is only local protection. Formal permit binds a unique external
    # Actions job and preserved service history. Final review must enumerate
    # service executions again, including this one, to exclude duplicate runs.
    guard = out / "attempt-started.json"
    write(guard, {"attempt_id": attempt_id, "synthetic_control_only": synthetic,
                  "conservative_attempt_consumed": True,
                  "reason": "dispatch becomes possible after this durable record; unknown failures consume",
                  "first_query_observed": None, "actions": env["actions"],
                  "authorization_sha256": None if synthetic else args.authorization_sha256,
                  "observed_unix": time.time()})
    runs = {}
    for arm in ARMS:
        if not synthetic:
            write(out / ("authorization-ref-" + arm + ".json"), authorization_ref_check(binding, out / ("service-ref-" + arm + "-raw.json")))
        unchanged(args, helper_dir, helper, candidate, data, data_before, env, binaries, binaries_before)
        need(source_before == {p: file_record(checked["root"] / p) for p in checked["source"]},
             "presealed admitted source drift")
        path = out / "runs" / arm
        argv = [binaries["runner"], "run", "--backend", "mcp-stdio" if arm == ARMS[0] else "rg",
                "--suite", checked["suite_path"], "--output", path, "--profile", "smoke"]
        if arm == ARMS[0]:
            argv += ["--binary", binaries["product"]]
        # At most144 per-query30s plus preparation/close overhead; the original
        # benchmark deadline remains30s. Outer infrastructure timeout is no gate relaxation.
        rec = command(argv, out / "commands" / arm, root, child_env, 4800)
        runs[arm] = rec
        unchanged(args, helper_dir, helper, candidate, data, data_before, env, binaries, binaries_before)
        need(source_before == {p: file_record(checked["root"] / p) for p in checked["source"]}, "source drift")
        write(out / (arm + "-observed.json"), {
            "raw_run_inventory": inventory(path) if path.exists() else {},
            "exit": rec["effective_exit_code"],
            "actual_completed_rows": len(lines(path / "normalized.jsonl")) if (path / "normalized.jsonl").exists() else 0})
        # Continue the predeclared second arm even if the first is bad/invalid;
        # retain all failures and the same denominator. No arm is retried.
    write(out / "execution.json", {"schema_version": 1, "protocol_id": PROTOCOL,
                                   "synthetic_control_only": synthetic, "attempt_id": attempt_id,
                                   "actions": env["actions"], "runs": runs,
                                   "candidate": candidate, "helpers": helper,
                                   "data_inventory": data_before, "source": source_before,
                                   "completed_unix": time.time(), "task_acceptance": "not_decided",
                                   "same_seal_external_history_final_review": "required_after_run",
                                   "all_rows": "unverified_until_raw_replay",
                                   "raw_explicit_no_match": {"supported": False, "value": None,
                                       "reason": "frozen public payload has no documented explicit no-match field"}})
    replay = replay_results(out / "runs", checked, binaries["runner"], root, child_env,
                            helper_dir, fresh(out / "replay"))
    unchanged(args, helper_dir, helper, candidate, data, data_before, env, binaries, binaries_before)
    if not synthetic:
        write(out / "authorization-ref-after.json", authorization_ref_check(binding, out / "service-ref-after-raw.json"))
    write(out / "after.json", {"candidate": candidate_snapshot(root, args.expected_source),
                               "helpers": helper_snapshot(helper_dir), "environment": environment(args.rg),
                               "python_dependency_closure": python_closure(),
                               "dependency_fingerprint": dependency_fingerprint(),
                               "source": {p: file_record(checked["root"] / p) for p in checked["source"]}})
    positive_witnesses = []
    if synthetic:
        first = checked["rows"][0]
        target = first["answers"][0]["alternatives"][0]["path"]
        for arm in ARMS:
            rows = lines(out / "runs" / arm / "normalized.jsonl")
            witnesses = positive_target_rows(rows, first["id"], target)
            control_dir = fresh(out / "all-no-match-control" / arm)
            copied_rows = json.loads(json.dumps(rows))
            for control_row in copied_rows:
                control_row["hits"] = []
                control_row["status"] = "no_match"
            copied_file = control_dir / "normalized.jsonl"
            copied_file.write_bytes(b"".join(json.dumps(r).encode() + b"\n" for r in copied_rows))
            try:
                positive_target_rows(lines(copied_file), first["id"], target)
            except ValueError as error:
                need("synthetic real positive target" in str(error), "wrong all-no-match rejection")
                write(control_dir / "rejection.json", {"status": "rejected", "reason": str(error),
                      "original_raw_modified": False, "new_product_queries": 0,
                      "mutated_owned_observation": file_record(copied_file)})
            else:
                raise ValueError("all-no-match observations falsely passed synthetic capability")
            positive_witnesses.append({"arm": arm, "case_id": first["id"], "target_path": target,
                "repetitions": [{"repetition": r["repetition"], "raw_path": r["raw_path"],
                    "raw_sha256": sha(raw(out / "runs" / arm / r["raw_path"]))} for r in witnesses],
                "scope": "real admitted source hit; not a native quality threshold or rg symbol assertion"})
        write(out / "synthetic-positive-witnesses.json", positive_witnesses)
    exits = [runs[a]["effective_exit_code"] for a in ARMS]
    need(all(code in (0, 1, 2, 3) for code in exits), "abnormal actual process exit")
    code = 3 if 3 in exits else 2 if 2 in exits else 1 if 1 in exits else 0
    if synthetic:
        need(code == 0, "synthetic original runtime gates did not pass")
    decision = {"status": "synthetic_capability_verified" if synthetic else "attempt_recorded_pending_independent_review",
                "synthetic_control_only": synthetic, "actual_scheduled_results": 288, "synthetic_positive_witnesses": positive_witnesses,
                "replay": replay, "task_acceptance": "not_decided", "clean_v1_count": 0,
                "execution_inventory": inventory(out / "runs"),
                "scope": "full fixed schedule/core scorer replay; no quality or release acceptance"}
    write(out / "result.json", decision)
    # The original gates remain actual. Synthetic controls require both gate0.
    # Formal measurements preserve failure1/invalid2 instead of calling them pass.
    print(json.dumps({"status": decision["status"], "actual_rows": 288, "original_gate_exits": exits,
                      "synthetic_control_only": synthetic, "task_acceptance": "not_decided"}))
    return code

def raw_observation(payload, row):
    hits = None
    form = "unsupported_shape"
    if isinstance(payload, list):
        hits, form = payload, "symbol_array"
    elif isinstance(payload, dict):
        pack = payload.get("machine_pack")
        if isinstance(pack, dict) and isinstance(pack.get("hits"), list):
            hits, form = pack["hits"], "machine_pack.hits"
        elif isinstance(payload.get("nodes"), list):
            hits, form = [n for n in payload["nodes"] if isinstance(n, dict)
                          and n.get("node_type") == "SEARCH_HIT"], "nodes.SEARCH_HIT"
    raw_error = payload.get("error_kind") if isinstance(payload, dict) else None
    retrieval = payload.get("evidence_summary", {}).get("retrieval") if isinstance(payload, dict) else None
    flags = {}
    def collect(value, prefix=""):
        if isinstance(value, dict):
            for k, v in value.items():
                p = prefix + "/" + k
                if k in ("partial", "truncated", "_truncated", "status", "error_kind"):
                    flags[p] = v
                if k in ("evidence_summary", "retrieval", "packing", "source_freshness",
                         "graph_explain", "graph_enrichment", "grep", "lane_receipts", "lanes"):
                    collect(v, p)
        elif isinstance(value, list):
            for n, v in enumerate(value):
                collect(v, prefix + "/" + str(n))
    collect(payload)
    return {"raw_shape": form, "raw_search_hit_count": None if hits is None else len(hits),
            "raw_error_kind": raw_error, "observed_raw_flags": flags,
            "retrieval_receipt": retrieval,
            "normalized_status": row["status"], "normalized_hits": len(row["hits"]),
            "normalized_no_match_zero_hits": row["status"] == "no_match" and not row["hits"],
            "complete_empty_under_original_normalizer": (
                hits == [] and not raw_error and row["status"] == "no_match" and not row["hits"]),
            "raw_explicit_no_match": None,
            "raw_explicit_no_match_capability": "unsupported; no documented raw status field",
            "interpretation": "raw shape/flags plus retained original normalized status; not independent re-normalization"}

def paired_bootstrap(values):
    keys = sorted(values)
    if len(keys) < 2 or any(v is None or not math.isfinite(v) for v in values.values()):
        return {"status": "inconclusive", "independent_components": len(keys),
                "defined_paired_components": sum(v is not None and math.isfinite(v) for v in values.values()),
                "reason": "undefined complete-family metric or fewer than2 components; no valid-subset resampling"}
    rng = random.Random(20261003)
    draws = []
    for _ in range(10000):
        draws.append(sum(values[keys[rng.randrange(len(keys))]] for _ in keys) / len(keys))
    draws.sort()
    return {"status": "conditional_exploratory", "independent_components": len(keys),
            "replicates": 10000, "seed": 20261003,
            "mean_candidate_minus_rg": sum(values.values()) / len(keys),
            "low": draws[math.ceil(.025 * len(draws)) - 1],
            "high": draws[math.ceil(.975 * len(draws)) - 1],
            "sampling_unit": "paired complete family component, retaining all4variants/3repetitions",
            "scope": "one purposively selected repository; not population/general release inference"}

def original_gate_exit(gate):
    code = gate["exit_code"]
    need(type(code) is int and code in (0, 1),
         "original invalid/cancelled measurement cannot be complete temporal acceptance")
    return code

def admitted_result(row, query, source):
    need(row["status"] in ("success", "no_match", "partial", "timeout", "tool_error",
                          "protocol_error", "cancelled"), "unknown original result status")
    for hit in row["hits"]:
        need(hit["path"] in source and hit.get("evidence_valid") is True,
             "missing or invalid admitted source evidence")
        prefix = query["path_prefix"]
        need(prefix is None or hit["path"].startswith(prefix), "query hard scope leakage")

def complete_mean(samples, expected):
    need(len(samples) == expected, "complete metric denominator")
    need(all(v is None or isinstance(v, (int, float)) and math.isfinite(float(v))
             for v in samples), "nonfinite original metric")
    return None if any(v is None for v in samples) else sum(samples) / expected

def report_contract_controls(output):
    # Invented helper-unit observations only. These are not additional product
    # queries or a replacement for the actual two-arm288 scorer/raw replay.
    results = []
    def rejected(name, call, reason):
        try:
            call()
        except ValueError as error:
            need(reason in str(error), "wrong invented report-control rejection")
            results.append({"case": name, "rejected": True, "reason": str(error)})
        else:
            raise ValueError("invented report control passed: " + name)
    need(original_gate_exit({"exit_code": 0}) == 0
         and original_gate_exit({"exit_code": 1}) == 1, "preserve original gate0/gate1")
    query, source = {"path_prefix": "units/"}, {"units/ok.py": {}, "outside/ok.py": {}}
    partial = {"status": "partial", "hits": [{"path": "units/ok.py", "evidence_valid": True}]}
    admitted_result(partial, query, source)
    results.append({"case": "original_gate1_partial_with_valid_scope", "preserved": True,
                    "quality_gate_passed": False})
    for code in (2, 3, True):
        rejected("original_gate_" + str(code), lambda c=code: original_gate_exit({"exit_code": c}),
                 "original invalid/cancelled")
    rejected("unknown_status", lambda: admitted_result({"status": "invented_unknown", "hits": []},
             query, source), "unknown original result status")
    for name, path, valid, reason in (
            ("outside_source", "missing.py", True, "missing or invalid admitted source evidence"),
            ("invalid_evidence", "units/ok.py", False, "missing or invalid admitted source evidence"),
            ("outside_scope", "outside/ok.py", True, "query hard scope leakage")):
        rejected(name, lambda p=path, v=valid: admitted_result(
            {"status": "partial", "hits": [{"path": p, "evidence_valid": v}]}, query, source), reason)
    full = [0.25] * 12
    need(complete_mean(full, 12) == 0.25, "finite complete family mean")
    incomplete = list(full)
    incomplete[7] = None
    family = complete_mean(incomplete, 12)
    need(family is None, "one undefined row must leave whole family undefined")
    arm = complete_mean([0.25] * 9 + [family], 10)
    need(arm is None, "one undefined component must leave whole arm undefined")
    finite = {str(i): 0.25 for i in range(10)}
    complete = paired_bootstrap(finite)
    need(complete["status"] == "conditional_exploratory"
         and complete["independent_components"] == 10
         and complete["mean_candidate_minus_rg"] == complete["low"] == complete["high"] == 0.25,
         "finite complete-family paired bootstrap")
    uncertain = paired_bootstrap({**finite, "7": None})
    need(uncertain["status"] == "inconclusive"
         and uncertain["independent_components"] == 10
         and uncertain["defined_paired_components"] == 9,
         "undefined paired component must not shrink planned denominator")
    rejected("missing_family_row", lambda: complete_mean(full[:-1], 12), "complete metric denominator")
    results.append({"case": "one_null_row_propagates_without_valid_subset", "family": family,
                    "arm": arm, "paired": uncertain, "finite_paired_control": complete,
                    "planned_family_rows": 12, "planned_components": 10})
    write(output, {"status": "actual_invented_helper_controls", "controls": results,
                   "new_product_queries": 0, "actual_measurement_evidence": False})
    return results

def replay_results(runs, checked, runner, root, env, helper_dir, output):
    expected = schedule(helper_dir)
    source_inventory = inventory(runs)
    schedules, row_reports, means = [], [], {}
    controls = []
    for arm in ARMS:
        original = runs / arm
        snapshot = inventory(original)
        need(read(original / "manifest.json")["infrastructure_failure"] is None, "original invalid infrastructure")
        m = read(original / "manifest.json")
        need(m["suite"] == checked["suite"] and m["engine"]["engine_head_observed"] ==
             git(root, "rev-parse", "HEAD").decode().strip() and m["engine"]["dirty_observed"] == "",
             "actual original run manifest source/suite")
        need(m["engine"]["eval_debug_assertions"] is False and m["adapter_version"] == "cc-eval-public-v7",
             "actual release evaluator/protocol")
        need(m["adapter"] == ("mcp-stdio" if arm == ARMS[0] else "rg-literal"),
             "actual adapter identity")
        need(lines(original / "queries.jsonl") == checked["rows"], "canonical actual query snapshot")
        rows, scores = lines(original / "normalized.jsonl"), lines(original / "scores.jsonl")
        need(len(rows) == len(scores) == 144, "missing scheduled measurements")
        copy = output / (arm + "-original-scorer-replay")
        shutil.copytree(original, copy)
        invocation = command([runner, "replay", "--run", copy], output / (arm + "-replay-command"),
                             root, env, 600)
        gate = read(original / "gate.json")
        original_gate_exit(gate)
        need(not invocation["timed_out"] and invocation["process_exit_code"] == gate["exit_code"],
             "actual original replay exit must equal preserved gate")
        need(all(raw(copy / name) == raw(original / name) for name in DERIVED),
             "original scorer derived files differ byte-for-byte")
        need(inventory(original) == snapshot, "replay mutated original raw")
        replay_inputs = ("manifest.json", "queries.jsonl", "normalized.jsonl")
        need(all(raw(copy / name) == raw(original / name) for name in replay_inputs)
             and inventory(copy / "raw") == inventory(original / "raw"), "replay changed locked input bytes")
        # Negative replay only mutates a disposable copy. The real run remains
        # unchanged and no product query is issued.
        bad = output / (arm + "-raw-drift-control")
        shutil.copytree(original, bad)
        bad_raw = bad / relative(rows[0]["raw_path"])
        with bad_raw.open("ab") as f:
            f.write(b"\n")
        negative = command([runner, "replay", "--run", bad], output / (arm + "-raw-drift-command"),
                           root, env, 600)
        need(negative["effective_exit_code"] == 2, "raw digest drift must be refused by unchanged Rust replay")
        streams = raw(output / (arm + "-raw-drift-command") / "stdout.log") + raw(output / (arm + "-raw-drift-command") / "stderr.log")
        need(b"raw response drift" in streams, "wrong raw replay negative-control reason")
        controls.append({"arm": arm, "raw_drift_original_replay_exit": 2})
        family_samples = collections.defaultdict(lambda: collections.defaultdict(list))
        for position, (row, score) in enumerate(zip(rows, scores)):
            item = expected[len(schedules)]
            q = checked["rows"][item["input_row_index"]]
            need(item["arm"] == arm and row["case_id"] == q["id"]
                 and row["repetition"] == item["repetition"], "actual runner order differs from frozen288 schedule")
            need(row["raw_path"] == "raw/%06d.json" % position, "actual raw response ordinal")
            admitted_result(row, q, checked["source"])
            # Original gate1 remains a recorded failed gate, including Partial
            # and row-level failures. This verifier never upgrades it to green,
            # discards its rows, or invents a stronger all-success quality gate.
            payload = read(original / relative(row["raw_path"]))
            observation = raw_observation(payload, row)
            values = {k: score[k] for k in ("top1", "ndcg10", "recall5", "recall10", "mrr10",
                                           "span_precision", "span_recall")}
            values["no_answer_correct"] = score["no_answer_correct"]
            values["normalized_no_match_zero_hits"] = observation["normalized_no_match_zero_hits"]
            if q["no_answer"]:
                need(type(score["no_answer_correct"]) is bool, "native no-answer score missing")
            else:
                need(score["no_answer_correct"] is None, "answerable/no-answer metric strata")
            for key, value in values.items():
                if (q["no_answer"] and key in ("no_answer_correct", "normalized_no_match_zero_hits")) or (
                        not q["no_answer"] and key not in ("no_answer_correct", "normalized_no_match_zero_hits")):
                    need(value is None or isinstance(value, (int, float)) and math.isfinite(float(value)),
                         "nonfinite original metric")
                    family_samples[q["query_family"]][key].append(None if value is None else float(value))
            schedules.append({**item, "case_id": row["case_id"], "raw_sha256": sha(raw(original / row["raw_path"])),
                              "elapsed_us": row["elapsed_us"], "status": row["status"]})
            row_reports.append({"arm": arm, "case_id": row["case_id"], "repetition": row["repetition"],
                                "category": q["category"], "family": q["query_family"],
                                "variant": q["annotations"]["prospective_temporal_v2"]["variant"],
                                "native_score": score, "raw_observation": observation})
        means[arm] = {}
        for family, metrics in family_samples.items():
            means[arm][family] = {}
            for name, samples in metrics.items():
                need(len(samples) == 12, "family must retain4variants times3repetitions")
                means[arm][family][name] = complete_mean(samples, 12)
    need(len(schedules) == 288 and inventory(runs) == source_inventory, "full immutable raw inventory")
    strata = {}
    for kind, no_answer in (("answerable", False), ("no_answer", True)):
        families = sorted({q["query_family"] for q in checked["rows"] if q["no_answer"] is no_answer})
        need(len(families) == (2 if no_answer else 10), "component denominator")
        keys = sorted(means[ARMS[0]][families[0]])
        metrics = {}
        for name in keys:
            delta = {f: None if means[ARMS[0]][f][name] is None or means[ARMS[1]][f][name] is None
                        else means[ARMS[0]][f][name] - means[ARMS[1]][f][name] for f in families}
            metrics[name] = {"arms": {arm: complete_mean([means[arm][f][name] for f in families], len(families))
                                      for arm in ARMS},
                             "denominators": {arm: {
                                 "planned_components": len(families), "planned_rows": len(families) * 12,
                                 "defined_components": sum(means[arm][f][name] is not None for f in families),
                                 "aggregation": "all12 observations per component required; undefined stays null"}
                                 for arm in ARMS},
                             "paired_component_deltas": delta, "bootstrap": paired_bootstrap(delta)}
        strata[kind] = {"components": len(families), "rows_per_arm": len(families) * 12, "metrics": metrics}
    category_means = {}
    for arm in ARMS:
        for category in sorted({q["category"] for q in checked["rows"] if not q["no_answer"]}):
            subset = [r for r in row_reports if r["arm"] == arm and r["category"] == category
                      and not checked["by_id"][r["case_id"]]["no_answer"]]
            category_means[arm + ":" + category] = {
                "rows": len(subset), "queries": len({r["case_id"] for r in subset}),
                "components": len({r["family"] for r in subset}),
                "query_micro_top1": sum(r["native_score"]["top1"] for r in subset) / len(subset),
                "query_micro_ndcg10": sum(r["native_score"]["ndcg10"] for r in subset) / len(subset)}
    report = {"schema_version": 1, "protocol_id": PROTOCOL,
              "verification_kind": "original_raw_hash_binding_and_original_scorer_replay",
              "raw_re_normalization": "not_performed; original replay verifies raw/normalized locks then re-scores",
              "status": "complete_288_results_replayed_not_task_acceptance",
              "actual_result_schedule": schedules, "per_row": row_reports,
              "family_means": means, "strata": strata, "category_means": category_means,
              "source_scope_invalid_hits": 0, "negative_controls": controls,
              "original_gates": {arm: read(runs / arm / "gate.json") for arm in ARMS},
              "row_status_counts": {arm: dict(collections.Counter(r["raw_observation"]["normalized_status"]
                  for r in row_reports if r["arm"] == arm)) for arm in ARMS},
              "metric_policy": "all288 original rows/scores retained; gate1 stays failed; undefined stays null/inconclusive, no denominator shrink",
              "quality_review_triggers": {"ndcg10_regression": .01, "primary_top1_regression": .01,
                                         "automatic_quality_acceptance": False},
              "unsupported": ["Recall20", "graph/facet scoring", "live_semantic", "100k", "tail_latency",
                              "platform_release_coverage", "v1_restricted_custody"],
              "independent_final_service_history_review": "required; local O_EXCL is not cross-run exclusion",
              "raw_inventory": source_inventory}
    write(output / "review.json", report)
    return {"report": file_record(output / "review.json"), "rows": 288,
            "original_gates": report["original_gates"], "raw_re_normalization": False}

def standalone_replay(args):
    disjoint(args.output, args.runs, args.data, args.source_root, Path(__file__).parent,
             args.package_manifest, args.archive)
    out = fresh(args.output)
    args.output_owned = True
    root, helper_dir, helper, env, package, candidate, binaries, child_env, binaries_before = setup(args, out)
    checked = validate_data(args.data.resolve(strict=True), args.synthetic_control)
    need(sha(jb(inventory(args.runs))) == args.raw_inventory_sha256, "independently pinned original raw inventory")
    data_before = inventory(args.data)
    report = replay_results(args.runs, checked, binaries["runner"], root, child_env, helper_dir,
                            fresh(out / "replay"))
    unchanged(args, helper_dir, helper, candidate, args.data, data_before, env, binaries, binaries_before)
    write(out / "result.json", {"status": "raw_only_verification", "new_product_queries": 0,
                               "review": report, "candidate": candidate, "helpers": helper,
                               "python_dependency_closure": python_closure()})
    print(json.dumps({"status": "raw_only_verification", "new_product_queries": 0, "rows": 288}))
    return 0


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None

def github_open(request):
    # Refuse before forwarding an Authorization header to any redirect target.
    return urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect()).open(request, timeout=30)

def github_json(endpoint, saved, allow_missing=False):
    need(endpoint.startswith("/repos/jyqj/codecortex/"), "fixed authorization repository only")
    headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28",
               "User-Agent": "codecortex-prospective-temporal-v2"}
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request("https://api.github.com" + endpoint, headers=headers)
    observed = time.time()
    try:
        with github_open(request) as response:
            need(response.geturl() == request.full_url, "unexpected GitHub authorization redirect")
            content = response.read(32 * 1024 * 1024 + 1)
            need(len(content) <= 32 * 1024 * 1024, "GitHub authorization response too large")
            status = response.status
    except urllib.error.HTTPError as error:
        content = error.read(1024 * 1024)
        status = error.code
    saved.parent.mkdir(parents=True, exist_ok=True)
    saved.write_bytes(content)
    write(saved.with_suffix(".receipt.json"), {"endpoint": endpoint, "status": status,
                                               "observed_unix": observed, "response": file_record(saved)})
    if status == 404 and allow_missing:
        return None
    need(status == 200, "GitHub authorization read failed with HTTP " + str(status))
    return json.loads(content)

def authorization_ref_check(binding, evidence):
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    branch = "work/ptv2-permit-" + run_id
    need(re.fullmatch("[0-9]+", run_id)
         and binding["endpoint"] == "/repos/jyqj/codecortex/git/ref/heads/" + branch
         and binding["ref"] == "refs/heads/" + branch
         and re.fullmatch("[0-9a-f]{40}", binding["commit"]), "fixed authorization branch identity")
    ref = github_json(binding["endpoint"], evidence)
    need(ref["ref"] == binding["ref"] and ref["object"]["type"] == "commit"
         and ref["object"]["sha"] == binding["commit"], "authorization branch moved")
    return {"endpoint": binding["endpoint"], "raw_response": str(evidence),
            "response": file_record(evidence), "commit": binding["commit"],
            "observed_unix": time.time()}


def wait_authorization(args):
    out = fresh(args.output)
    args.output_owned = True
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    need(re.fullmatch("[0-9]+", run_id) and os.environ.get("GITHUB_RUN_ATTEMPT") == "1",
         "actual first Actions attempt required")
    branch = "work/ptv2-permit-" + run_id
    endpoint = "/repos/jyqj/codecortex/git/ref/heads/" + branch
    prefix = "artifacts/benchmarks/prospective-temporal-v2/authorizations/" + run_id + "/"
    write(out / "waiting.json", {"run_id": run_id, "job": os.environ.get("GITHUB_JOB"),
          "utility_sha": os.environ.get("GITHUB_SHA"), "permission_branch": "refs/heads/" + branch,
          "deadline_seconds": 600, "candidate_query_dispatch": 0,
          "instruction": "root independently creates exact permit+raw service commit, then first creates this branch"})
    print(json.dumps(read(out / "waiting.json")), flush=True)
    end = time.monotonic() + 600
    response = None
    for poll in range(61):
        response = github_json(endpoint, out / "service" / ("ref-poll-%02d.json" % poll), True)
        if response is not None:
            break
        need(time.monotonic() < end, "independent authorization was not supplied before deadline")
        time.sleep(min(10, max(0, end - time.monotonic())))
    need(response is not None and response["ref"] == "refs/heads/" + branch
         and response["object"]["type"] == "commit", "authorization must directly pin a commit")
    commit_sha = response["object"]["sha"]
    need(re.fullmatch("[0-9a-f]{40}", commit_sha), "authorization commit identity")
    commit = github_json("/repos/jyqj/codecortex/git/commits/" + commit_sha, out / "service/commit.json")
    need(commit["sha"] == commit_sha, "authorization commit response")
    tree = github_json("/repos/jyqj/codecortex/git/trees/" + commit["tree"]["sha"] + "?recursive=1",
                       out / "service/tree.json")
    need(tree["truncated"] is False and tree["sha"] == commit["tree"]["sha"], "incomplete or wrong authorization tree")
    entries = [e for e in tree["tree"] if e["path"].startswith(prefix) and e["type"] == "blob"]
    need(1 <= len(entries) <= 64 and sum(e["size"] for e in entries) <= 8 * 1024 * 1024,
         "authorization data bounded scope")
    files = fresh(out / "files")
    records = {}
    for index, entry in enumerate(entries):
        name = relative(entry["path"][len(prefix):])
        need(entry["mode"] == "100644" and Path(name).suffix in (".json", ".txt", ".log"),
             "authorization data-only files")
        payload = github_json("/repos/jyqj/codecortex/git/blobs/" + entry["sha"],
                              out / "service" / ("blob-%02d.json" % index))
        need(payload["encoding"] == "base64", "Git blob encoding")
        content = base64.b64decode(payload["content"], validate=False)
        need(len(content) == entry["size"]
             and hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest() == entry["sha"],
             "authorization fixed blob identity")
        target = files / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as f:
            f.write(content)
        records[name] = file_record(target)
    need("authorization.json" in records, "authorization payload missing")
    binding = {"endpoint": endpoint, "ref": "refs/heads/" + branch, "commit": commit_sha,
               "tree": commit["tree"]["sha"], "data_prefix": prefix,
               "authorization_sha256": records["authorization.json"]["sha256"],
               "files": records, "service_responses": inventory(out / "service"),
               "scope": "first resolved commit is immutable; mutable ref checked again before/after execution; no ACL claim"}
    binding["last_ref_check"] = authorization_ref_check(binding, out / "service/ref-final.json")
    write(out / "binding.json", binding)
    print(json.dumps({"status": "authorization_downloaded_not_yet_accepted",
                      "authorization": str(files / "authorization.json"),
                      "authorization_sha256": records["authorization.json"]["sha256"],
                      "binding": str(out / "binding.json"), "commit": commit_sha}), flush=True)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest="command", required=True)
    for name in ("synthetic-control", "execute", "replay", "prepare-source"):
        p = subs.add_parser(name)
        p.add_argument("--source-root", type=Path, required=True)
        p.add_argument("--expected-source", required=True)
        p.add_argument("--package-manifest", type=Path, required=True)
        p.add_argument("--package-manifest-sha256", required=True)
        p.add_argument("--archive", type=Path, required=True)
        p.add_argument("--helper-map-sha256", required=True)
        p.add_argument("--rg", type=Path, required=True)
        p.add_argument("--output", type=Path, required=True)
        if name in ("execute", "replay"):
            p.add_argument("--data", type=Path, required=True)
        if name == "prepare-source":
            p.add_argument("--upstream-root", type=Path, required=True)
            p.add_argument("--upstream-commit", required=True)
        if name == "execute":
            p.add_argument("--authorization", type=Path, required=True)
            p.add_argument("--authorization-sha256", required=True)
            p.add_argument("--authorization-binding", type=Path, required=True)
        if name == "replay":
            p.add_argument("--runs", type=Path, required=True)
            p.add_argument("--raw-inventory-sha256", required=True)
            p.add_argument("--synthetic-control", action="store_true")
    p = subs.add_parser("wait-authorization")
    p.add_argument("--output", type=Path, required=True)
    p = subs.add_parser("validate-data")
    p.add_argument("--data", type=Path, required=True)
    p.add_argument("--synthetic-control", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "wait-authorization":
            return wait_authorization(args)
        if args.command == "validate-data":
            checked = validate_data(args.data.resolve(strict=True), args.synthetic_control)
            print(json.dumps({"status": "data_contract_validated", "native_rows": len(checked["rows"]),
                              "zero_lexical_families": len(checked["zero_lexical"]),
                              "synthetic_control_only": args.synthetic_control,
                              "Rust_schema_and_content_locks": "separate_original_cc_eval_validate_required",
                              "task_acceptance": "not_decided"}))
            return 0
        if args.command == "prepare-source":
            return prepare_source(args)
        if args.command == "replay":
            return standalone_replay(args)
        return execute_arms(args)
    except (ValueError, KeyError, OSError, TypeError, subprocess.SubprocessError) as error:
        # Do not overwrite any prior file or turn a partial setup into success.
        out = getattr(args, "output", None)
        if getattr(args, "output_owned", False) and out is not None and out.is_dir() and not (out / "failure.json").exists():
            write(out / "failure.json", {"status": "invalid_or_incomplete", "error_type": type(error).__name__,
                                        "error": str(error), "task_acceptance": "not_decided",
                                        "attempt_consumed_if_dispatch_not_excluded": True,
                                        "original_raw": "retained; no automatic retry or lock repair"})
        print(json.dumps({"status": "invalid_or_incomplete", "error": str(error)}), file=sys.stderr)
        return 2

if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Inspect committed admission history; does NOT simulate or pass cross-build gates."""
import hashlib
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
BASE = "83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad"
OLD = "0a56a257f9a92c54d06ea5be0ce1d1763917a527"
PATHS = [
    "crates/cc-model/src/project_model.rs",
    "crates/cc-model/src/resolution.rs",
    "crates/cc-model/src/identity.rs",
    "crates/cc-model/src/chunk_policy.rs",
    "crates/cc-semantic/src/spec.rs",
    "crates/cc-parsers/src/exports/rust.rs",
    "crates/cc-parsers/src/exports/python.rs",
    "crates/cc-parsers/src/exports/jsts.rs",
    "crates/cc-parsers/src/exports/go.rs",
    "crates/cc-parsers/src/exports/conservative.rs",
]
VERSION = re.compile(r'pub const ((?:PROJECT_MODEL|RESOLUTION|DOCUMENT|CHUNK_POLICY|ENCODING_SPEC)_VERSION|TOKEN_ESTIMATOR):[^=]+ = ([^;]+);')
EXTRACTOR = re.compile(r'"((?:rust|python|jsts|go)-declared-v\d+|conservative-v\d+|import-bindings-v\d+)"')


def git(*args, optional=False):
    result = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, check=False)
    if result.returncode:
        if optional:
            return None
        raise RuntimeError(result.stderr.decode())
    return result.stdout


def snapshot(commit):
    files = {}
    for path in PATHS:
        raw = git("show", f"{commit}:{path}", optional=True)
        if raw is None:
            files[path] = {"exists": False}
            continue
        text = raw.decode()
        files[path] = {
            "exists": True,
            "sha256": hashlib.sha256(raw).hexdigest(),
            "version_constants": dict(VERSION.findall(text)),
            "extractor_labels": sorted(set(EXTRACTOR.findall(text))),
        }
        if path.endswith("/spec.rs"):
            metric = re.search(r"pub enum DistanceMetric\s*\{([^}]+)\}", text)
            assert metric
            files[path]["metric_variants"] = re.findall(r"\b(\w+)\s*,", metric.group(1))
    return {
        "source_sha": commit,
        "tree_sha": git("rev-parse", f"{commit}^{{tree}}").decode().strip(),
        "cargo_lock_sha256": hashlib.sha256(git("show", f"{commit}:Cargo.lock")).hexdigest(),
        "build_sha256": None,
        "build_status": "not_built_no_admitted_version_transition",
        "files": files,
    }


def inventory():
    commits = git("log", "--format=%H", BASE, "--", *PATHS).decode().splitlines()
    snapshots = [snapshot(c) for c in dict.fromkeys([BASE, OLD, *commits])]
    versions = {}
    labels = {}
    metrics = set()
    for snap in snapshots:
        for path, data in snap["files"].items():
            for name, value in data.get("version_constants", {}).items():
                versions.setdefault(name, set()).add(value)
            for value in data.get("extractor_labels", []):
                labels.setdefault(path, set()).add(value)
            metrics.update(data.get("metric_variants", []))
    expected = {
        "PROJECT_MODEL_VERSION": {"3"}, "RESOLUTION_VERSION": {"1"},
        "DOCUMENT_VERSION": {"1"}, "ENCODING_SPEC_VERSION": {"1"},
        "CHUNK_POLICY_VERSION": {'"source-chunks-v2"'},
        "TOKEN_ESTIMATOR": {'"utf8-bytes-div-ceil-4-v1"'},
    }
    assert versions == expected, versions
    assert metrics == {"Cosine"}, metrics
    assert all(len(values) == 1 for path, values in labels.items() if not path.endswith("jsts.rs"))
    assert labels["crates/cc-parsers/src/exports/jsts.rs"] == {"jsts-declared-v3", "import-bindings-v1"}
    admission = git("show", f"{BASE}:crates/cc-semantic/src/admission.rs").decode()
    assert "if tokenizer != TOKEN_ESTIMATOR" in admission
    schema = {}
    for commit in [OLD, BASE]:
        raw = git("show", f"{commit}:crates/cc-db/src/index_migrate.rs")
        schema[commit] = {
            "sha256": hashlib.sha256(raw).hexdigest(),
            "version": int(re.search(rb"CURRENT_SCHEMA_VERSION: u32 = (\d+)", raw)[1]),
        }
    assert [schema[c]["version"] for c in [OLD, BASE]] == [21, 22]
    return {
        "kind": "committed_source_admission_inventory_not_runtime_validation",
        "base_source_sha": BASE,
        "history_scope": "all ancestors of fixed PR58 base touching listed admission source paths",
        "history_commits": commits,
        "snapshots": snapshots,
        "admitted_versions": {k: sorted(v) for k, v in versions.items()},
        "extractor_labels": {k: sorted(v) for k, v in labels.items()},
        "metric_variants": sorted(metrics),
        "schema_pair": schema,
        "results": {"V11/derived": "not_run_cross_build", "V11/frozen-spec": "not_run_cross_build"},
        "runtime_tests_run": 0,
        "old_pinned_handles_tested": False,
        "migration_builds_produced": 0,
    }


if __name__ == "__main__":
    result = inventory()
    encoded = json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if len(sys.argv) == 2 and sys.argv[1] == "--check":
        assert pathlib.Path(__file__).with_name("admission-inventory.json").read_text() == encoded
        print("PASS: committed source hashes/history/admission inventory match; both cross-build gates remain NOT RUN")
    else:
        sys.stdout.write(encoded)

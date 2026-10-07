#!/usr/bin/env python3
"""Audit pinned public DEV Git blobs without importing a corpus or reading holdout.

The historical review receipt is evidence of its declared scope, never authority
to certify today's engine, a clean checkout, independent families, or a release.
This SHA256/source-span/projection audit deliberately does not reimplement the
cc-eval BLAKE3 validator or scorer. No fetch, checkout, model, or ranking runs.
"""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import unicodedata

from p8_release_evidence import Invalid, canonical, path as local_path, read_bytes, write_json


ROOT = Path(__file__).resolve().parents[1]
PREFIX = "crates/cc-eval/benchmarks/public-v19/"
ANCHOR = "5385f5a7a2a875c6d5cbd049bdde039bf71bbf32"
ADMISSION = PREFIX + "protocol/global-dev-review/typescript-extension/admission.json"
SOURCES = PREFIX + "protocol/source-locks.json"
CUSTODY = PREFIX + "protocol/custody/decision.json"
COMPONENTS = PREFIX + "protocol/global-dev-review/typescript-extension/global-components.json"
CONTROL_PINS = {
    ADMISSION: "b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425",
    SOURCES: "c17f10b6b289c427c1324a73b5251ed1985272882a073c1763e8fe774da174b4",
    CUSTODY: "a8c88f2479843566d74a8a47f05da81da0588029dffa681c313e64939f344f9c",
    COMPONENTS: "6324f6152aad34d3c8614b8db0991be800d9a91b32cb092950ea5a95229a94d7",
}
MAX_BLOB = 4 * 1024 * 1024
MAX_TOTAL = 64 * 1024 * 1024
CATEGORIES = {
    "file_exact_match", "configuration_lookup", "component_location", "api_usage",
    "semantic_feature", "error_handling", "architecture_understanding",
    "cross_language", "symbol_location", "call_chain",
}
LANGUAGES = {"rust", "javascript", "typescript", "python", "go"}
QUERY_FIELDS = {
    "id", "query_family", "category", "difficulty", "language", "split", "query",
    "path_prefix", "no_answer", "expected_files", "answers", "annotations",
}


def require(condition, message):
    if not condition:
        raise Invalid(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def normalize(value):
    return " ".join(unicodedata.normalize("NFKC", value).casefold().split())


def relative(value):
    require(isinstance(value, str) and 0 < len(value) <= 1024, "invalid relative path")
    require(not value.startswith("/") and "\\" not in value
            and not any(ord(c) < 32 or ord(c) == 127 for c in value)
            and all(p not in ("", ".", "..", ".git") for p in value.split("/")),
            "unsafe relative path")
    require(not re.search(r"hold[-_]?out|held[-_]?out", value, re.I), "protected body path")
    return value


def split_entry(entry):
    require(isinstance(entry, str) and ":" in entry, "invalid Git entry")
    ref, name = entry.split(":", 1)
    require(re.fullmatch(r"[0-9a-f]{40}", ref) is not None, "full fixed Git SHA required")
    relative(name)
    require(name.startswith(PREFIX), "outside public corpus allowlist")
    return ref, name


def parse_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    try:
        return json.loads(raw, object_pairs_hook=unique,
                          parse_constant=lambda _: (_ for _ in ()).throw(Invalid("nonfinite JSON")))
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise Invalid("invalid JSON input") from exc


def git(root, *args):
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1",
           "GIT_OPTIONAL_LOCKS": "0"}
    result = subprocess.run(["git", "--no-pager", "-C", str(root), *args],
                            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, env=env, timeout=30)
    require(result.returncode == 0, "fixed local Git object unavailable; no fetch attempted")
    return result.stdout


class PinnedBlobs:
    """Read only explicit SHA/path/hash pins; reject protected paths before Git."""

    def __init__(self, root):
        self.root = local_path(root)
        self.pins = {ANCHOR + ":" + p: digest for p, digest in CONTROL_PINS.items()}
        self.cache = {}
        self.total_bytes = 0

    def add_admission_pins(self, pins):
        require(isinstance(pins, dict) and 0 < len(pins) <= 500, "invalid admission pins")
        for entry, digest in pins.items():
            split_entry(entry)
            require(isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest),
                    "invalid SHA256 pin")
            require(entry not in self.pins or self.pins[entry] == digest, "conflicting pin")
        self.pins.update(pins)

    def read(self, entry):
        ref, name = split_entry(entry)
        require(entry in self.pins, "entry absent from frozen public DEV allowlist")
        if entry in self.cache:
            return self.cache[entry]
        tree = git(self.root, "ls-tree", "-z", ref, "--", ":(literal)" + name).split(b"\0")
        require(len(tree) == 2 and tree[-1] == b"", "missing Git file entry")
        meta, actual_name = tree[0].split(b"\t", 1)
        require(meta.split()[:2] in ([b"100644", b"blob"], [b"100755", b"blob"])
                and actual_name.decode() == name, "non-regular Git blob rejected")
        size = int(git(self.root, "cat-file", "-s", entry))
        require(0 <= size <= MAX_BLOB and self.total_bytes + size <= MAX_TOTAL,
                "public input byte budget exceeded")
        raw = git(self.root, "show", entry)
        require(len(raw) == size and sha(raw) == self.pins[entry], "public input hash drift")
        self.cache[entry] = raw
        self.total_bytes += size
        return raw

    def json(self, entry):
        value = parse_json(self.read(entry))
        require(isinstance(value, dict), "JSON object required")
        return value


def load_inputs(root=ROOT):
    blobs = PinnedBlobs(root)
    admission = blobs.json(ANCHOR + ":" + ADMISSION)
    require(admission["status"] == "development_admitted_snapshot_scope_only"
            and admission["errors"] == {}, "historical development admission not clean")
    blobs.add_admission_pins(admission["inputs_sha256"])
    # Hash every listed source/review/license/dev input, including evidence not
    # consumed as a current semantic decision. Never traverse a tree or fetch.
    for entry in sorted(blobs.pins):
        blobs.read(entry)
    return blobs, admission


def query_rows(raw):
    rows = []
    for line in raw.decode("utf-8-sig").splitlines():
        if not line.strip():
            continue
        row = parse_json(line)
        require(isinstance(row, dict) and set(row) == QUERY_FIELDS, "query schema fields")
        require(row["split"] == "dev", "non-DEV row rejected without echoing body")
        require(all(isinstance(row[k], str) and row[k] for k in
                    ("id", "query_family", "query", "category", "language")), "query identity")
        require(len(row["query"].encode()) <= 4096, "query bounds")
        require(type(row["no_answer"]) is bool and type(row["difficulty"]) is int
                and 1 <= row["difficulty"] <= 5, "query types or difficulty")
        require(isinstance(row["answers"], list) and isinstance(row["expected_files"], list)
                and isinstance(row["annotations"], dict), "query gold types")
        rows.append(row)
    require(0 < len(rows) <= 10000, "empty or oversized query file")
    return rows


def check_projection(native, compat):
    errors = Counter()
    native_by_id, compat_by_id = {}, {}
    for profile, rows, target in (("native", native, native_by_id), ("compat", compat, compat_by_id)):
        for row in rows:
            if row["id"] in target:
                errors[profile + "_duplicate_id"] += 1
            target[row["id"]] = row
    projected = 0
    for ident, row in compat_by_id.items():
        original = native_by_id.get(ident)
        if original is None:
            errors["compat_without_native"] += 1
            continue
        projected += 1
        if any(original[k] != row[k] for k in
               ("query", "query_family", "split", "category", "language", "difficulty", "path_prefix")):
            errors["projection_identity_drift"] += 1
        expected = list(dict.fromkeys(
            a["path"] for g in sorted(original["answers"], key=lambda g: not g["primary"])
            for a in g["alternatives"]))
        if row["no_answer"] or row["answers"] or not expected or row["expected_files"] != expected:
            errors["projection_gold_drift"] += 1
    expected_ids = {r["id"] for r in native if not r["no_answer"]}
    if set(compat_by_id) != expected_ids:
        errors["compat_denominator_drift"] += 1
    return dict(errors), projected


def source_check(rows, sources, upstream_sha):
    errors = Counter()
    spans = 0
    for row in rows:
        annotation = row["annotations"].get("v19", {})
        if annotation.get("source_sha") != upstream_sha:
            errors["row_upstream_drift"] += 1
        if row["expected_files"] or (row["no_answer"] and row["answers"]):
            errors["native_gold_shape"] += 1
        if not row["no_answer"] and not any(g["primary"] for g in row["answers"]):
            errors["native_primary_missing"] += 1
        for group in row["answers"]:
            for alt in group["alternatives"]:
                name = relative(alt["path"])
                raw = sources.get(name)
                span = alt.get("span")
                if raw is None:
                    errors["gold_outside_source"] += 1
                elif not isinstance(span, dict) or set(span) != {"start", "end"}:
                    errors["missing_source_span"] += 1
                elif (type(span["start"]) is not int or type(span["end"]) is not int
                      or not 0 <= span["start"] < span["end"] <= len(raw)):
                    errors["span_bounds"] += 1
                else:
                    try:
                        raw[:span["start"]].decode("utf-8")
                        raw[span["start"]:span["end"]].decode("utf-8")
                        spans += 1
                    except UnicodeError:
                        errors["span_utf8_boundary"] += 1
        evidence = annotation.get("gold_evidence", []) + annotation.get("author_provenance", {}).get("source_evidence", [])
        for item in evidence:
            raw = sources.get(relative(item["path"]))
            span = item.get("span", {})
            start = item.get("start_byte", item.get("byte_start", span.get("start")))
            end = item.get("end_byte", item.get("byte_end", span.get("end")))
            if (raw is None or type(start) is not int or type(end) is not int
                    or not 0 <= start < end <= len(raw)):
                errors["annotation_span_bounds"] += 1
                continue
            if (item.get("source_sha") != upstream_sha
                    or item.get("file_sha256", sha(raw)) != sha(raw)
                    or item.get("span_sha256", item.get("sha256")) != sha(raw[start:end])
                    or ("text" in item and item["text"].encode() != raw[start:end])):
                errors["annotation_source_evidence_drift"] += 1
    return dict(errors), spans


def suite_entries(record):
    return record.get("dev_suite_entries") or [record["native_dev_suite_entry"],
                                              record["compat_dev_suite_entry"]]


def collect_public_dev(blobs, admission):
    result, all_native, all_compat, errors = {}, [], [], Counter()
    locks = blobs.json(ANCHOR + ":" + SOURCES)
    locked = {r["repository"].rsplit("/", 1)[-1].lower(): r for r in locks["candidates"]}
    for name, previous in admission["repo_results"].items():
        native, compat, source, suites = [], [], {}, []
        base = previous["author_sha"] + ":" + PREFIX + name + "/"
        manifest = blobs.json(base + "source-manifest.json")
        source_records = manifest.get("files", manifest.get("admitted", manifest.get("admitted_files", [])))
        by_path = {relative(r["path"]): r for r in source_records}
        if len(by_path) != len(source_records):
            errors["duplicate_source_path"] += 1
        upstream = manifest.get("upstream_sha", manifest.get("source_sha"))
        if upstream != previous["upstream_sha"] or upstream != locked[name]["source_sha"]:
            errors["upstream_lock_drift"] += 1
        for entry in suite_entries(previous):
            ref, suite_path = split_entry(entry)
            suite = blobs.json(entry)
            profile = {"codecortex-native-v1": "native", "oce-compat-v1": "compat"}.get(suite["scoring"])
            require(profile is not None, "unrecognized scoring profile")
            require(suite["queries"] == "queries." + profile + ".dev.jsonl", "non-DEV query pointer")
            query_entry = ref + ":" + str(PurePosixPath(suite_path).parent / suite["queries"])
            rows = query_rows(blobs.read(query_entry))
            (native if profile == "native" else compat).extend(rows)
            files = suite["source"]["files"]
            require(len(files) == len(set(files)), "duplicate source path in suite")
            # Source roots in original manifests may contain ../. Bind only the
            # exact frozen namespace's source/<allowlisted file>, never resolve it.
            for source_path in files:
                relative(source_path)
                require(source_path in by_path, "source outside manifest")
                raw = blobs.read(base + "source/" + source_path)
                record = by_path[source_path]
                require(record.get("admitted", True) is not False
                        and record.get("included", True) is not False, "excluded source in suite")
                if sha(raw) != record["sha256"] or len(raw) != record["bytes"]:
                    errors["source_manifest_drift"] += 1
                require(b"\0" not in raw and len(raw) <= 1_000_000, "binary or oversized source")
                raw.decode("utf-8")
                require(not raw.startswith(b"version https://git-lfs.github.com/spec/"), "LFS pointer source")
                source[source_path] = raw
            suites.append({"entry": entry, "sha256": sha(blobs.read(entry)), "profile": profile,
                           "query_sha256": sha(blobs.read(query_entry)), "rows": len(rows),
                           "snapshot_commit": suite["source"]["commit"],
                           "cc_eval_blake3_validation": "not_run"})
        expected_source = {p for p in by_path if base + "source/" + p in blobs.pins}
        if set(source) != expected_source:
            errors["source_inventory_incomplete"] += 1
        projection_errors, projected = check_projection(native, compat)
        span_errors, spans = source_check(native, source, upstream)
        errors.update(projection_errors)
        errors.update(span_errors)
        if (len(native) != previous["native_dev_rows"] or len(compat) != previous["compat_dev_rows"]
                or spans != previous["spans_verified"]):
            errors["historical_count_drift"] += 1
        annotations = [r["annotations"].get("v19", {}) for r in native]
        result[name] = {
            "upstream_sha": upstream, "author_sha": previous["author_sha"],
            "review_sha": previous["review_sha"], "native_dev_rows": len(native),
            "compat_dev_rows": len(compat), "projection_rows_checked": projected,
            "no_answer_rows": sum(r["no_answer"] for r in native),
            "distinct_family_labels": len({r["query_family"] for r in native}),
            "categories": dict(sorted(Counter(r["category"] for r in native).items())),
            "languages": dict(sorted(Counter(normalize(r["language"]) for r in native).items())),
            "author_annotation_review_status": dict(sorted(Counter(a.get("review_status", "missing") for a in annotations).items())),
            "historical_recorded_content_accept": previous["current_content_hash_accept"],
            "new_independent_gold_review": 0,
            "source_files_checked": len(source), "source_spans_checked": spans,
            "manifest_records_not_indexed": len(set(by_path) - set(source)),
            "faceted_rows": sum(bool(a.get("facets")) for a in annotations),
            "graph_constraint_rows": sum(bool(a.get("graph_constraints")) for a in annotations),
            "snapshot_only_not_upstream_checkout_verification": True, "suites": suites,
        }
        all_native.extend(native)
        all_compat.extend(compat)
    global_errors, _ = check_projection(all_native, all_compat)
    errors.update(global_errors)
    return result, all_native, all_compat, dict(errors)


def run(root=ROOT):
    blobs, admission = load_inputs(root)
    repos, native, compat, errors = collect_public_dev(blobs, admission)
    categories = Counter(r["category"] for r in native)
    languages = Counter(normalize(r["language"]) for r in native)
    fingerprints = Counter(normalize(r["query"]) for r in native)
    components = blobs.json(ANCHOR + ":" + COMPONENTS)["components"]
    members = [m for c in components for m in c["members"]]
    if len(members) != len(set(members)) or set(members) != {r["query_family"] for r in native}:
        errors["global_component_coverage_drift"] = 1
    if len(components) != admission["global_review"]["conservative_global_correlation_components"]:
        errors["global_component_count_drift"] = 1
    custody = blobs.json(ANCHOR + ":" + CUSTODY)
    result = {
        "schema_version": 1, "scope": "fixed_public_dev_integrity_and_coverage_only",
        "status": "passed_local_audit" if not errors else "failed_local_audit", "errors": errors,
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "worktree_head": git(root, "rev-parse", "HEAD").decode().strip(),
        "audit_script_sha256": sha(read_bytes(Path(__file__), limit=MAX_BLOB)),
        "historical_public_protocol_sha": ANCHOR,
        "historical_admission_sha256": CONTROL_PINS[ADMISSION],
        "input_blobs_verified": len(blobs.cache), "input_bytes_verified": blobs.total_bytes,
        "verified_inputs_sha256": {k: sha(v) for k, v in sorted(blobs.cache.items())},
        "repositories": repos,
        "totals": {"native_dev_rows": len(native), "compat_dev_rows": len(compat),
                   "repositories": len(repos), "source_files": sum(r["source_files_checked"] for r in repos.values()),
                   "source_spans": sum(r["source_spans_checked"] for r in repos.values()),
                   "categories": dict(sorted(categories.items())), "languages": dict(sorted(languages.items())),
                   "distinct_family_labels": len({r["query_family"] for r in native}),
                   "historical_conservative_components_rechecked": len(components),
                   "normalized_duplicate_query_groups": sum(n > 1 for n in fingerprints.values()),
                   "new_independent_gold_reviews": 0, "formal_accepted_families": 0},
        "coverage_gaps": {"missing_categories": sorted(CATEGORIES - categories.keys()),
                          "missing_languages": sorted(LANGUAGES - languages.keys()),
                          "missing_candidate_repositories": ["serde", "vite"],
                          "mixed_monorepo_certified": False, "formal_family_target": 600,
                          "formal_family_shortfall": 600},
        "custody": {"decision_sha256": CONTROL_PINS[CUSTODY], "status": custody["status"],
                    "historically_reported_candidate_ids": custody["would_be_holdout_ids"],
                    "historically_reported_exposed_lower_bound": custody["known_exposed_ids_lower_bound"],
                    "clean_holdout_certified": 0, "body_reads_this_run": 0},
        "release_certified": False,
        "not_run": ["cc_eval_blake3_validate", "production_retrieval", "new_source_gold_semantic_review",
                    "upstream_checkout_cleanliness", "formal_family_independence", "heldout_evaluation",
                    "facet_graph_quality", "six_repository_release_certification"],
    }
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    output = local_path(args.output, exists=False)
    require(not output.exists(), "output exists; retain old audit receipts")
    try:
        result = run(local_path(args.repo_root))
        code = 1 if result["errors"] else 0
    except (Invalid, OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        result = {"schema_version": 1, "status": "invalid_audit_inputs", "release_certified": False,
                  "error_type": type(exc).__name__, "error": "public audit input or execution failed"}
        code = 2
    output.parent.mkdir(parents=True, exist_ok=True)
    write_json(output, result)
    print(json.dumps({"status": result["status"], "exit_code": code,
                      "receipt_sha256": sha(canonical(result))}))
    return code


if __name__ == "__main__":
    sys.exit(main())

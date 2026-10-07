#!/usr/bin/env python3
"""Public DEV metadata boundaries and bounded production literal/dependency scan.

No holdout bodies, custody changes, ranking, gold relabeling, or release claims.
Literal signatures are a review aid, not proof against semantic overfitting.
"""

import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import sys
import tomllib

import p8_corpus_audit as corpus
from p8_release_evidence import Invalid, canonical, path as local_path, read_bytes, write_json


# This intentionally scans inline test modules too. No source is assumed to be
# excluded from a production build merely because its filename contains tests.
SOURCE_EXTENSIONS = {".rs", ".sql", ".json"}
IMPORT_PATTERN = re.compile(r"\b(?:include(?:_str|_bytes)?!|mod|use)\s*(?:\([^;]*?)?"
                            r"(?:public-v19|benchmarks/|cc_eval\b)", re.S)
LITERALS = re.compile(r'r(?P<hashes>\#{0,16})"(?P<raw>.*?)"(?P=hashes)|"(?P<normal>(?:\\.|[^"\\])*)"', re.S)


def audit_family_metadata(rows, components):
    """No text/gold accepted: use public fingerprints and opaque family labels.

    Synthetic test metadata can contain a heldout split label without disclosing
    any heldout query. Real runs supply only the pinned public DEV projection.
    """
    errors = Counter()
    family_splits = defaultdict(set)
    query_splits = defaultdict(set)
    seen = set()
    for row in rows:
        corpus.require(set(row) == {"id_sha256", "family", "split", "query_sha256"}, "body-free family metadata required")
        corpus.require(isinstance(row["family"], str) and 0 < len(row["family"]) <= 256
                       and row["split"] in ("dev", "holdout", "quarantine"), "invalid family or split label")
        for key in ("id_sha256", "query_sha256"):
            corpus.require(re.fullmatch(r"[0-9a-f]{64}", row[key]) is not None, "invalid metadata fingerprint")
        if row["id_sha256"] in seen:
            errors["duplicate_metadata_id"] += 1
        seen.add(row["id_sha256"])
        family_splits[row["family"]].add(row["split"])
        query_splits[row["query_sha256"]].add(row["split"])
    errors["family_cross_split"] = sum(len(s) > 1 for s in family_splits.values())
    errors["normalized_query_cross_split"] = sum(len(s) > 1 for s in query_splits.values())
    membership = set()
    for component in components:
        members = component["members"]
        if not members or len(members) != len(set(members)):
            errors["invalid_component_members"] += 1
        if membership.intersection(members):
            errors["duplicate_component_membership"] += 1
        membership.update(members)
        if not set(members).issubset(family_splits):
            errors["unknown_component_family"] += 1
        splits = set().union(*(family_splits[m] for m in members)) if members else set()
        if len(splits) > 1:
            errors["component_cross_split"] += 1
    if membership != set(family_splits):
        errors["unregistered_family"] += 1
    return {k: v for k, v in sorted(errors.items()) if v}


def decode_literal(match):
    if match.group("raw") is not None:
        return match.group("raw")
    value = match.group("normal")
    # Decode common Rust escapes, preserving unknown forms conservatively. This
    # is not a Rust parser; macro-generated strings remain an explicit limit.
    value = re.sub(r"\\u\{([0-9a-fA-F_]+)\}",
                   lambda m: chr(int(m[1].replace("_", ""), 16)), value)
    value = re.sub(r"\\x([0-9a-fA-F]{2})", lambda m: chr(int(m[1], 16)), value)
    escapes = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", '"': '"', "0": "\0"}
    return re.sub(r'\\([nrt\\"0])', lambda m: escapes[m[1]], value)


def scan_text(name, raw, query_fingerprints, gold_paths):
    text = raw.decode("utf-8")
    findings = []
    for match in LITERALS.finditer(text):
        value = decode_literal(match)
        normalized = corpus.normalize(value)
        rule = None
        if len(normalized) >= 32 and corpus.sha(normalized.encode()) in query_fingerprints:
            rule = "public_dev_query_literal"
        elif value in gold_paths and "/" in value and len(value) >= 12:
            rule = "public_dev_gold_path_literal_review"
        elif re.search(r"public-v19|codecortex-public-v19|v19\.(?:express|requests|gin|typescript)\.", value):
            rule = "public_corpus_identifier_literal"
        if rule:
            findings.append({"path": name, "line": text.count("\n", 0, match.start()) + 1,
                             "rule": rule, "value_sha256": corpus.sha(value.encode())})
    for match in IMPORT_PATTERN.finditer(text):
        findings.append({"path": name, "line": text.count("\n", 0, match.start()) + 1,
                         "rule": "benchmark_import_review", "value_sha256": corpus.sha(match[0].encode())})
    return findings


def dependency_findings(name, raw, workspace_dependencies=None):
    data = tomllib.loads(raw.decode("utf-8"))
    findings = []
    def check(section, table):
        for alias, spec in table.items():
            if isinstance(spec, dict) and spec.get("workspace") is True:
                corpus.require(workspace_dependencies is not None and alias in workspace_dependencies,
                               "unresolved workspace dependency")
                spec = workspace_dependencies[alias]
            package = spec.get("package", alias) if isinstance(spec, dict) else alias
            dep_path = spec.get("path", "") if isinstance(spec, dict) else ""
            if package == "cc-eval" or re.search(r"(?:^|/)cc-eval(?:/|$)", dep_path):
                findings.append({"path": name, "rule": "production_eval_dependency", "section": section,
                                 "alias": alias})
    for section in ("dependencies", "build-dependencies"):
        check(section, data.get(section, {}))
    for target, tables in data.get("target", {}).items():
        for section in ("dependencies", "build-dependencies"):
            check("target." + target + "." + section, tables.get(section, {}))
    return findings


def production_scan(root, rows):
    root = local_path(root)
    raw_names = corpus.git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "crates")
    names = sorted(set(p.decode() for p in raw_names.split(b"\0") if p))
    selected = []
    for name in names:
        parts = Path(name).parts
        if len(parts) < 3 or parts[1] == "cc-eval":
            continue
        if (len(parts) == 3 and parts[2] in ("Cargo.toml", "build.rs")) or (
                len(parts) >= 4 and parts[2] == "src" and Path(name).suffix in SOURCE_EXTENSIONS):
            selected.append(name)
    corpus.require(0 < len(selected) <= 20000, "empty or oversized production scope")
    queries = {corpus.sha(corpus.normalize(r["query"]).encode()) for r in rows}
    gold = {a["path"] for r in rows for g in r["answers"] for a in g["alternatives"]}
    workspace_raw = read_bytes(local_path(root / "Cargo.toml"), limit=corpus.MAX_BLOB)
    workspace = tomllib.loads(workspace_raw.decode("utf-8")).get("workspace", {}).get("dependencies", {})
    findings, files = [], {"Cargo.toml": corpus.sha(workspace_raw)}
    total = len(workspace_raw)
    for name in selected:
        raw = read_bytes(local_path(root / name), limit=corpus.MAX_BLOB)
        total += len(raw)
        corpus.require(total <= corpus.MAX_TOTAL, "production scan byte budget")
        files[name] = corpus.sha(raw)
        findings.extend(dependency_findings(name, raw, workspace) if name.endswith("Cargo.toml")
                        else scan_text(name, raw, queries, gold))
    return {"files_scanned": len(files), "bytes_scanned": total,
            "source_manifest_sha256": corpus.sha(canonical(files)), "files_sha256": files,
            "findings": findings, "inline_tests_and_comments_in_scope": True,
            "ignored_generated_files_in_scope": False}


def run(root=corpus.ROOT):
    blobs, admission = corpus.load_inputs(root)
    _, native, _, input_errors = corpus.collect_public_dev(blobs, admission)
    corpus.require(not input_errors, "public DEV input audit failed")
    components = blobs.json(corpus.ANCHOR + ":" + corpus.COMPONENTS)["components"]
    metadata = [{"id_sha256": corpus.sha(r["id"].encode()), "family": r["query_family"], "split": r["split"],
                 "query_sha256": corpus.sha(corpus.normalize(r["query"]).encode())} for r in native]
    errors = audit_family_metadata(metadata, components)
    scan = production_scan(root, native)
    custody = blobs.json(corpus.ANCHOR + ":" + corpus.CUSTODY)
    return {
        "schema_version": 1, "scope": "public_dev_metadata_and_production_signatures_only",
        "status": "passed_local_boundary_scan" if not errors and not scan["findings"] else "boundary_review_required",
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "worktree_head": corpus.git(root, "rev-parse", "HEAD").decode().strip(),
        "audit_script_sha256": corpus.sha(read_bytes(Path(__file__), limit=corpus.MAX_BLOB)),
        "corpus_audit_script_sha256": corpus.sha(read_bytes(Path(corpus.__file__), limit=corpus.MAX_BLOB)),
        "historical_public_protocol_sha": corpus.ANCHOR,
        "historical_admission_sha256": corpus.CONTROL_PINS[corpus.ADMISSION],
        "public_rows": len(native), "public_components": len(components),
        "metadata_errors": errors, "production_scan": scan,
        "custody": {"status": custody["status"], "metadata_sha256": corpus.CONTROL_PINS[corpus.CUSTODY],
                    "historically_reported_candidates": custody["would_be_holdout_ids"],
                    "historically_reported_exposed_lower_bound": custody["known_exposed_ids_lower_bound"],
                    "clean_holdout_certified": 0, "holdout_body_reads_this_run": 0,
                    "permissions_or_visibility_changed": False},
        "release_certified": False, "overfitting_absence_proved": False,
        "not_run": ["sealed_holdout_access_verification", "frozen_config_heldout_evaluation",
                    "semantic_paraphrase_equivalence_review", "production_binary_package_contents",
                    "macro_generated_literal_expansion", "ignored_generated_sources"],
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=corpus.ROOT)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    output = local_path(args.output, exists=False)
    corpus.require(not output.exists(), "output exists; retain old audit receipts")
    try:
        result = run(local_path(args.repo_root))
        code = 0 if result["status"] == "passed_local_boundary_scan" else 1
    except (Invalid, OSError, ValueError, TypeError, KeyError, corpus.subprocess.SubprocessError) as exc:
        result = {"schema_version": 1, "status": "invalid_audit_inputs", "release_certified": False,
                  "error_type": type(exc).__name__, "error": "boundary audit input or execution failed"}
        code = 2
    output.parent.mkdir(parents=True, exist_ok=True)
    write_json(output, result)
    print(json.dumps({"status": result["status"], "exit_code": code,
                      "receipt_sha256": corpus.sha(canonical(result))}))
    return code


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Materialize and validate the frozen 600-question public DEV registry.

Historical inputs retain their original snapshot and review scope. New source
checkouts and independent receipts are checked separately. This command never
fetches Git objects, freezes gold, runs retrieval, or reads a protected corpus.
"""

import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import time

import p8_corpus_audit as audit
from p8_release_evidence import Invalid, canonical, disjoint, path as local_path, read_bytes


ROOT = Path(__file__).resolve().parents[1]
PREFIX = "crates/cc-eval/benchmarks/public-dev-20261008/"
INDEX = PREFIX + "index.json"
EXPECTED = {"native_rows": 600, "compat_rows": 554, "repositories": 6,
            "historical_native_rows": 301, "new_native_rows": 299}
PROFILE = {"codecortex-native-v1": "native", "oce-compat-v1": "compat"}
require = audit.require
sha = audit.sha


def locked_file(root, relative, expected):
    relative = audit.relative(relative)
    require(re.fullmatch(r"[0-9a-f]{64}", expected or "") is not None,
            "invalid current-input digest")
    raw = read_bytes(root / relative, limit=audit.MAX_BLOB)
    require(sha(raw) == expected, "current input hash drift: " + relative)
    return raw


def write_new(path, raw):
    local_path(path, exists=False)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(raw)


def write_json(path, value):
    write_new(path, canonical(value))


def review_acceptance(name, record, review, native_raw, manifest_raw):
    """Read the two existing receipt formats without rewriting author fields."""
    native = audit.query_rows(native_raw)
    require(isinstance(review.get("author"), str) and review["author"].strip()
            and isinstance(review.get("reviewer"), str) and review["reviewer"].strip()
            and review["author"] != review["reviewer"], "independent reviewer required")
    require(review["author"] == record["author"] and review["reviewer"] == record["reviewer"],
            "review identity drift")
    require(review.get("source_sha") == record["upstream_sha"], "review source commit drift")
    if name == "serde":
        require(review.get("verdict") == "accepted_scoped" and review.get("independent") is True
                and review.get("unresolved_blockers") == [], "Serde review not accepted")
        require(review.get("packet_question_sha256") == sha(native_raw)
                and review.get("packet_source_manifest_sha256") == sha(manifest_raw),
                "review does not bind exact query/source-manifest bytes")
        require(review.get("reviewed_rows") == len(native), "review count drift")
        decisions, id_key, decision_key = review["rows"], "query_id", "verdict"
        exact_rows = {r["id"]: sha(line) for r, line in zip(native, native_raw.splitlines())}
        for decision in decisions:
            require(decision.get("exact_row_sha256") == exact_rows.get(decision.get(id_key)),
                    "review row digest drift")
            require(decision.get("author") == record["author"]
                    and decision.get("reviewer") == record["reviewer"]
                    and decision.get("unresolved_blockers") == [], "row review identity or blockers")
    elif name == "vite":
        require(review.get("status") == "accepted_scoped", "Vite review not accepted")
        bindings = review["input_sha256"]
        require(bindings.get("queries.native.dev.jsonl") == sha(native_raw)
                and bindings.get("source-manifest.json") == sha(manifest_raw),
                "review does not bind exact query/source-manifest bytes")
        require(review.get("questions_reviewed") == len(native)
                and review.get("questions_accepted") == len(native), "review count drift")
        decisions, id_key, decision_key = review["question_records"], "id", "decision"
    else:
        raise Invalid("unknown independent receipt format")
    require(isinstance(decisions, list) and len(decisions) == len(native), "review rows incomplete")
    ids = [d.get(id_key) for d in decisions]
    require(len(ids) == len(set(ids)) and set(ids) == {r["id"] for r in native},
            "review ID coverage drift")
    require(all(d.get(decision_key) == "accepted_scoped" for d in decisions),
            "unaccepted row review")
    return {"scope": review["scope"], "author": review["author"], "reviewer": review["reviewer"],
            "rows": len(native), "status": "accepted_scoped", "receipt_sha256": sha(canonical(review))}


def global_checks(groups, expected=EXPECTED):
    """Check only public DEV IDs, projections and declared family membership."""
    native, compat, owners, family_splits = [], [], defaultdict(set), defaultdict(set)
    for name, n, c in groups:
        native.extend(n)
        compat.extend(c)
        for row in n + c:
            require(row["split"] == "dev", "non-DEV row in public registry")
            family_splits[row["query_family"]].add(row["split"])
            owners[row["query_family"]].add(name)
    errors, projected = audit.check_projection(native, compat)
    require(not errors, "global projection or duplicate ID failure: " + json.dumps(errors, sort_keys=True))
    require(all(s == {"dev"} for s in family_splits.values()), "family split drift")
    require(all(len(names) == 1 for names in owners.values()), "family ID shared across repositories")
    require(len(native) == expected["native_rows"] and len(compat) == expected["compat_rows"]
            and len(groups) == expected["repositories"], "registry total count drift")
    categories = Counter(r["category"] for r in native)
    languages = Counter(audit.normalize(r["language"]) for r in native)
    texts = Counter(audit.normalize(r["query"]) for r in native)
    return {"native_rows": len(native), "compat_rows": len(compat), "projection_rows_checked": projected,
            "no_answer_rows": sum(r["no_answer"] for r in native), "repositories": len(groups),
            "distinct_family_labels": len(owners), "families_are_independent_samples": False,
            "family_split_coverage": {"dev": len(family_splits)},
            "duplicate_id_errors": 0, "cross_repository_family_id_collisions": 0,
            "normalized_duplicate_query_groups": sum(n > 1 for n in texts.values()),
            "categories": dict(sorted(categories.items())), "languages": dict(sorted(languages.items())),
            "missing_categories": sorted(audit.CATEGORIES - categories.keys()),
            "missing_core_languages": sorted(audit.LANGUAGES - languages.keys())}


def relocate_suite(suite, source_root):
    require(suite.get("scoring") in PROFILE, "unknown scoring profile")
    require(re.fullmatch(r"[0-9a-f]{40}", suite["source"].get("commit") or "") is not None,
            "new source requires a full Git commit")
    result = json.loads(json.dumps(suite))
    result["source"]["root"] = str(local_path(source_root))
    unchanged = json.loads(json.dumps(result))
    unchanged["source"]["root"] = suite["source"]["root"]
    require(unchanged == suite, "suite relocation changed a scoring or lock field")
    return result


def export_legacy(output, blobs, admission, repo_results):
    exported = {}
    for entry in sorted(blobs.pins):
        commit, relative = audit.split_entry(entry)
        path = output / "legacy" / commit / relative
        raw = blobs.read(entry)
        write_new(path, raw)
        exported[entry] = path
    suites = []
    for name, previous in admission["repo_results"].items():
        for entry in audit.suite_entries(previous):
            suite = blobs.json(entry)
            require(suite["source"]["commit"] is None, "legacy snapshot mode drift")
            path = exported[entry]
            # The frozen ../ roots must resolve to this exact repository's
            # exported source namespace; no arbitrary legacy path is followed.
            root = Path(os.path.abspath(path.parent / suite["source"]["root"]))
            expected_root = output / "legacy" / previous["author_sha"] / audit.PREFIX / name / "source"
            require(root == expected_root, "legacy source root outside pinned snapshot namespace")
            local_path(root)
            require(not (root / ".git").exists(), "snapshot unexpectedly has Git metadata")
            detail = next(s for s in repo_results[name]["suites"] if s["entry"] == entry)
            suites.append({"repository": name, "profile": PROFILE[suite["scoring"]],
                "path": str(path), "original_entry": entry, "original_sha256": blobs.pins[entry],
                "materialized_sha256": sha(path.read_bytes()), "rows": detail["rows"],
                "source_mode": "historically_accepted_snapshot_commit_null",
                "source_root_only_changed": False, "suite_bytes_unchanged": True})
    return suites


def current_inventory(root, index):
    pins = index["current_inputs_sha256"]
    require(isinstance(pins, dict) and len(pins) == 40, "current corpus input list incomplete")
    observed = set()
    for name in ("serde", "vite"):
        directory = local_path(root / PREFIX / name)
        for item in directory.rglob("*"):
            local_path(item)
            if item.is_file():
                observed.add(item.relative_to(root).as_posix())
    require(observed == set(pins), "unregistered or missing current corpus file")
    return {p: locked_file(root, p, digest) for p, digest in sorted(pins.items())}


def new_corpus(name, record, cache, source_root, output):
    source_root = local_path(source_root)
    base = PREFIX + name + "/"
    manifest_raw = cache[base + "source-manifest.json"]
    manifest = audit.parse_json(manifest_raw)
    require(manifest["upstream_sha"] == record["upstream_sha"], "source manifest commit drift")
    require(audit.git(source_root, "rev-parse", "HEAD").decode().strip() == record["upstream_sha"],
            "source checkout commit drift")
    require(not audit.git(source_root, "status", "--porcelain", "--untracked-files=all"),
            "source checkout is not clean")
    source_tree = audit.git(source_root, "rev-parse", "HEAD^{tree}").decode().strip()
    require(source_tree == manifest["upstream_tree"], "source tree drift")
    require(not audit.git(source_root, "submodule", "status"), "submodules are outside admitted scope")
    source, inventory = {}, []
    for file in manifest["files"]:
        require(file.get("admitted") is True, "excluded manifest record in admitted file list")
        rel = audit.relative(file["path"])
        raw = locked_file(source_root, rel, file["sha256"])
        require(len(raw) == file["bytes"], "source byte count drift")
        tree = audit.git(source_root, "ls-tree", "-z", "HEAD", "--", ":(literal)" + rel).split(b"\0")
        require(len(tree) == 2 and not tree[-1], "source Git entry unavailable")
        meta, actual = tree[0].split(b"\t", 1)
        require(meta.split() == [file["git_mode"].encode(), b"blob", file["git_blob"].encode()]
                and actual.decode() == rel and file["git_mode"] in ("100644", "100755"),
                "source Git blob or mode drift")
        require(rel not in source, "duplicate source manifest entry")
        source[rel] = raw
        inventory.append({"path": rel, "bytes": len(raw), "sha256": sha(raw), "git_blob": file["git_blob"]})
    suite_data = {profile: audit.parse_json(cache[base + path])
                  for profile, path in record["suites"].items()}
    native_raw = cache[base + suite_data["native"]["queries"]]
    native = audit.query_rows(native_raw)
    compat = audit.query_rows(cache[base + suite_data["compat"]["queries"]])
    review_raw = cache[base + record["review"]]
    acceptance = review_acceptance(name, record, audit.parse_json(review_raw), native_raw, manifest_raw)
    # Report the exact file hash, not a reserialized review hash.
    acceptance["receipt_sha256"] = sha(review_raw)
    require(len(native) == record["native_rows"] and len(compat) == record["compat_rows"],
            "new repository count drift")
    projection, checked = audit.check_projection(native, compat)
    source_errors, spans = audit.source_check(native, source, record["upstream_sha"])
    require(not projection and not source_errors, "new source/span/projection audit failed")
    for path, raw in cache.items():
        if path.startswith(base) and path[len(base):] not in record["suites"].values():
            write_new(output / "current" / path, raw)
    suites = []
    for profile, original in suite_data.items():
        require(PROFILE.get(original["scoring"]) == profile, "suite profile drift")
        require(original["source"]["commit"] == record["upstream_sha"]
                and set(original["source"]["files"]) == set(source)
                and len(original["source"]["files"]) == len(source), "suite source inventory drift")
        original_path = base + record["suites"][profile]
        path = output / "current" / original_path
        write_json(path, relocate_suite(original, source_root))
        suites.append({"repository": name, "profile": profile, "path": str(path),
            "original_path": original_path, "original_sha256": sha(cache[original_path]),
            "materialized_sha256": sha(path.read_bytes()), "rows": len(native if profile == "native" else compat),
            "source_mode": "clean_pinned_upstream_git", "source_root": str(source_root),
            "source_root_only_changed": True, "source_digest_unchanged": original["source"]["digest"],
            "queries_digest_unchanged": original["queries_digest"], "original_source_root": original["source"]["root"]})
    return native, compat, suites, {"upstream_sha": record["upstream_sha"], "source_tree": source_tree,
        "source_root": str(source_root), "native_rows": len(native), "compat_rows": len(compat),
        "source_files": len(source), "source_bytes": sum(map(len, source.values())),
        "source_spans_checked": spans, "projection_rows_checked": checked,
        "source_manifest_sha256": sha(manifest_raw), "source_inventory": inventory,
        "independent_acceptance": acceptance, "distinct_family_labels": len({r["query_family"] for r in native})}


def validate_suites(binary, suites, output):
    calls = []
    for number, suite in enumerate(suites, 1):
        command = [str(binary), "validate", "--suite", suite["path"]]
        started = time.monotonic()
        try:
            result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, timeout=60)
            code, raw, status = result.returncode, result.stdout, "passed" if result.returncode == 0 else "failed"
        except subprocess.TimeoutExpired as exc:
            code, raw, status = None, exc.stdout or b"", "timeout"
        log = output / "logs" / f"{number:02d}-{suite['repository']}-{suite['profile']}.log"
        write_new(log, raw)
        calls.append({"repository": suite["repository"], "profile": suite["profile"], "command": command,
            "exit_code": code, "status": status, "elapsed_seconds": round(time.monotonic() - started, 6),
            "log": str(log), "log_sha256": sha(raw), "rows": suite["rows"]})
    return calls


def run(args):
    root, binary = local_path(args.repo_root), local_path(args.cc_eval)
    script_raw = read_bytes(Path(__file__), limit=audit.MAX_BLOB)
    output = local_path(args.output_dir, exists=False)
    source_roots = {"serde": local_path(args.serde_source), "vite": local_path(args.vite_source)}
    disjoint(output, [root, binary, *source_roots.values()])
    require(not output.exists(), "output exists; preserve previous registry receipts")
    index_raw = read_bytes(root / INDEX, limit=audit.MAX_BLOB)
    index = audit.parse_json(index_raw)
    require(index.get("schema_version") == 1 and index.get("kind") == "frozen_public_dev_registry",
            "unsupported registry schema")
    require(index["expected_counts"] == EXPECTED, "registry expected totals drift")
    require(sha(read_bytes(binary, limit=512 * 1024 * 1024)) == index["validator"]["sha256"],
            "validator binary identity drift")
    executed_helpers = {"scripts/p8_corpus_audit.py": Path(audit.__file__),
                        "scripts/p8_release_evidence.py": Path(sys.modules["p8_release_evidence"].__file__)}
    require(set(index["helper_inputs_sha256"]) == set(executed_helpers), "helper inventory drift")
    helper_raw = {}
    for path, digest in index["helper_inputs_sha256"].items():
        helper_raw[path] = locked_file(root, path, digest)
        require(read_bytes(executed_helpers[path], limit=audit.MAX_BLOB) == helper_raw[path],
                "executed helper differs from registered helper")
    blobs, admission = audit.load_inputs(root)
    require(index["historical_anchor"] == audit.ANCHOR
            and index["historical_inputs_sha256"] == blobs.pins, "historical registry pins drift")
    historical, old_native, old_compat, errors = audit.collect_public_dev(blobs, admission)
    require(not errors and len(old_native) == 301 and len(old_compat) == 256, "historical DEV audit failed")
    require(set(index["historical_repositories"]) == set(historical), "historical repository set drift")
    for name, record in index["historical_repositories"].items():
        for field in ("author_sha", "review_sha", "upstream_sha", "native_dev_rows", "compat_dev_rows"):
            require(record[field] == historical[name][field], "historical repository metadata drift")
        require(record["suites"] == audit.suite_entries(admission["repo_results"][name]),
                "historical suite list drift")
    cache = current_inventory(root, index)
    output.mkdir(parents=True)
    suites = export_legacy(output, blobs, admission, historical)
    groups = []
    for name in historical:
        native, compat = [], []
        for entry in audit.suite_entries(admission["repo_results"][name]):
            ref, path = audit.split_entry(entry)
            suite = blobs.json(entry)
            query = ref + ":" + str(PurePosixPath(path).parent / suite["queries"])
            (native if PROFILE[suite["scoring"]] == "native" else compat).extend(audit.query_rows(blobs.read(query)))
        groups.append((name, native, compat))
    added = {}
    require(set(index["new_repositories"]) == {"serde", "vite"}, "new repository set drift")
    for name in ("serde", "vite"):
        native, compat, new_suites, detail = new_corpus(name, index["new_repositories"][name], cache,
                                                       source_roots[name], output)
        groups.append((name, native, compat)); suites.extend(new_suites); added[name] = detail
    coverage = global_checks(groups)
    require(not coverage["missing_categories"] and not coverage["missing_core_languages"],
            "required public DEV category or language coverage missing")
    components = blobs.json(audit.ANCHOR + ":" + audit.COMPONENTS)["components"]
    members = [m for c in components for m in c["members"]]
    require(len(members) == len(set(members)) and set(members) == {r["query_family"] for r in old_native},
            "historical component membership drift")
    coverage.update({"historical_reviewed_rows": 301, "new_independently_reviewed_rows": 299,
        "historical_source_files": sum(r["source_files_checked"] for r in historical.values()),
        "new_source_files": sum(r["source_files"] for r in added.values()),
        "historical_correlation_components_rechecked": len(components),
        "source_modes": {"historical_repositories": "frozen Git blobs; snapshot commit=null",
                         "new_repositories": "clean fixed upstream Git checkout"},
        "compat_rows_are_projections_not_new_questions": True,
        "current_gold_quality_or_release_certified": False, "protected_body_reads": 0})
    require(len(suites) == 20 and Counter(s["profile"] for s in suites) == {"native": 10, "compat": 10},
            "suite coverage drift")
    materialized_before = {p.relative_to(output).as_posix(): sha(p.read_bytes())
                           for p in sorted(output.rglob("*")) if p.is_file()}
    calls = validate_suites(binary, suites, output)
    for path, expected in materialized_before.items():
        require(sha(read_bytes(output / path, limit=audit.MAX_BLOB)) == expected,
                "validator changed materialized inputs")
    require(current_inventory(root, index) == cache, "current inputs changed during validation")
    require(read_bytes(root / INDEX, limit=audit.MAX_BLOB) == index_raw, "registry changed during validation")
    require(read_bytes(Path(__file__), limit=audit.MAX_BLOB) == script_raw,
            "registry code changed during validation")
    for path, raw in helper_raw.items():
        require(read_bytes(root / path, limit=audit.MAX_BLOB) == raw
                and read_bytes(executed_helpers[path], limit=audit.MAX_BLOB) == raw,
                "validation helper changed during validation")
    require(sha(read_bytes(binary, limit=512 * 1024 * 1024)) == index["validator"]["sha256"],
            "validator binary changed during validation")
    for name, detail in added.items():
        source = source_roots[name]
        require(audit.git(source, "rev-parse", "HEAD").decode().strip() == detail["upstream_sha"]
                and not audit.git(source, "status", "--porcelain", "--untracked-files=all"),
                "source checkout changed during validation")
        for item in detail["source_inventory"]:
            locked_file(source, item["path"], item["sha256"])
    passed = all(c["status"] == "passed" for c in calls)
    result = {"schema_version": 1, "status": "passed_native_validation" if passed else "native_validation_failed",
        "scope": "frozen_public_dev_admission_integrity_projection_and_existing_native_validator",
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "worktree_head": audit.git(root, "rev-parse", "HEAD").decode().strip(),
        "registry_sha256": sha(index_raw), "script_sha256": sha(script_raw),
        "historical_inputs_sha256": blobs.pins, "current_inputs_sha256": index["current_inputs_sha256"],
        "helper_inputs_sha256": index["helper_inputs_sha256"], "validator": index["validator"],
        "historical_repositories": historical, "new_repositories": added, "coverage": coverage,
        "suites": suites, "native_validator_calls": calls, "all_20_suites_validated": passed,
        "successful_validator_calls": sum(c["status"] == "passed" for c in calls),
        "inputs_unchanged": True, "materialized_inputs_sha256": materialized_before,
        "retrieval_calls": 0, "freeze_calls": 0, "protected_body_reads": 0,
        "current_semantic_quality_or_release_certified": False,
        "limitations": ["Historical acceptance retains its fixed snapshot/source/review scope.",
                        "The 600 count is reviewed questions, not 600 independent families.",
                        "DEV admission and BLAKE3 validation do not complete a quality or release gate.",
                        "Prose facets, graph constraints and hard negatives retain the existing scorer's enforcement scope."]}
    write_json(output / "coverage.json", coverage)
    write_json(output / "suites.json", suites)
    write_json(output / "registry-validation.json", result)
    return result, 0 if passed else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=ROOT)
    parser.add_argument("--cc-eval", type=Path, required=True)
    parser.add_argument("--serde-source", type=Path, required=True)
    parser.add_argument("--vite-source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    output_existed = os.path.lexists(args.output_dir)
    try:
        result, code = run(args)
    except (Invalid, OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        result, code = {"schema_version": 1, "status": "invalid_registry_inputs_or_execution",
                        "error_type": type(exc).__name__, "error": str(exc),
                        "all_20_suites_validated": False, "current_semantic_quality_or_release_certified": False}, 2
        # A failed attempt gets an additive diagnostic where safe; an existing
        # completed output is never overwritten or silently reused.
        try:
            output = local_path(args.output_dir, exists=False)
            disjoint(output, [local_path(args.repo_root), local_path(args.cc_eval),
                              local_path(args.serde_source), local_path(args.vite_source)])
            if not output_existed:
                write_json(output / "failed-registry-validation.json", result)
        except (Invalid, OSError):
            pass
    print(json.dumps({"status": result["status"], "exit_code": code,
                      "output_dir": str(args.output_dir), "all_20_suites_validated": result["all_20_suites_validated"]}))
    return code


if __name__ == "__main__":
    sys.exit(main())

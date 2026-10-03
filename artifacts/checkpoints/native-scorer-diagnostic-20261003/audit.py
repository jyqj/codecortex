#!/usr/bin/env python3
"""Read-only aggregate audit. Never emits query, gold, source or hit bodies.

Inputs: extracted group archives and admission-hash-verified source snapshots.
This diagnoses predicates; it does not calculate alternate scores or repair raw.
"""
import argparse
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rows(path):
    return [json.loads(line) for line in path.read_text().splitlines()]


def audit_group(folder, manifest):
    inventory = json.loads(manifest.read_text())["files"]
    checked = 0
    for name, receipt in inventory.items():
        path = folder / name
        assert path.stat().st_size == receipt["bytes"]
        assert sha(path) == receipt["sha256"]
        checked += 1
    return {"manifest_sha256": sha(manifest), "verified_file_count": checked}


def audit_suite(folder, source_root):
    queries = rows(folder / "queries.jsonl")
    assert all(q["split"] == "dev" for q in queries)
    by_id = {q["id"]: q for q in queries}
    normalized = rows(folder / "normalized.jsonl")
    seen = set()
    counts, failures, kinds = Counter(), Counter(), Counter()
    row_stages = Counter()
    for row in normalized:
        key = (row["case_id"], row["repetition"])
        assert key not in seen
        seen.add(key)
        query = by_id[row["case_id"]]
        raw = json.loads((folder / row["raw_path"]).read_text())
        wire = raw["machine_pack"]["hits"]
        assert len(wire) == len(row["hits"])
        counts["rows"] += 1
        counts["status:" + row["status"]] += 1
        counts["no_answer_rows"] += query["no_answer"]
        stages = set()
        for hit, public in zip(row["hits"], wire):
            meta = public.get("metadata") or {}
            assert hit["path"] == public["file_path"]
            for hk, pk, mk in [("symbol_name", "symbol_name", "symbol_name"),
                               ("qname", "qname", "qname"), ("kind", "symbol_kind", "symbol_kind")]:
                expected = public.get("kind") if hk == "kind" else None
                expected = expected or public.get(pk) or meta.get(mk)
                assert hit.get(hk) == expected
                counts[hk + "_populated"] += hit.get(hk) is not None
            counts["hits"] += 1
            counts["valid_evidence_hits"] += hit.get("evidence_valid") is True
            counts["span_populated"] += hit.get("span") is not None
            for alternative in (a for g in query["answers"] for a in g["alternatives"]):
                if hit["path"] != alternative["path"]:
                    continue
                counts["same_path_pairs"] += 1
                failed = []
                symbol = alternative.get("symbol")
                if hit.get("evidence_valid") is False:
                    failed.append("evidence")
                if symbol:
                    for sk, hk in [("name", "symbol_name"), ("qname", "qname"), ("kind", "kind")]:
                        if symbol.get(sk) is not None and symbol[sk] != hit.get(hk):
                            failed.append(sk)
                    if "kind" in failed:
                        kinds[str(symbol.get("kind")) + "->" + str(hit.get("kind"))] += 1
                span, returned = alternative.get("span"), hit.get("span")
                if span and (not returned or max(span["start"], returned["start"]) >= min(span["end"], returned["end"])):
                    failed.append("span")
                failures[",".join(failed) or "match"] += 1
                stages.add("same_path")
                if not any(x in failed for x in ["evidence", "name", "span"]):
                    stages.add("same_path_name_valid_overlap")
                if not any(x in failed for x in ["evidence", "name", "span", "kind"]):
                    stages.add("same_path_name_kind_valid_overlap")
                if not failed:
                    stages.add("all_native_predicates")
        row_stages.update(stages)
    expected = {(q["id"], rep) for q in queries for rep in range(3)}
    assert seen == expected
    gold_counts = Counter()
    if source_root is not None:
        for q in queries:
            for a in (a for g in q["answers"] for a in g["alternatives"]):
                body = (source_root / a["path"]).read_bytes()
                symbol, span = a.get("symbol"), a.get("span")
                gold_counts["alternatives"] += 1
                if span:
                    assert 0 <= span["start"] < span["end"] <= len(body)
                    body[:span["start"]].decode("utf8")
                    fragment = body[span["start"]:span["end"]].decode("utf8")
                    gold_counts["valid_source_byte_spans"] += 1
                    if symbol:
                        assert symbol["name"] in fragment
                        gold_counts["name_present_in_span"] += 1
                if symbol:
                    gold_counts["qname_required"] += bool(symbol.get("qname"))
                    if a["path"].endswith(".py"):
                        kinds_in_source = set()
                        def visit(node):
                            for child in ast.iter_child_nodes(node):
                                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)) and child.name == symbol["name"]:
                                    kinds_in_source.add("class" if isinstance(child, ast.ClassDef) else "method" if isinstance(node, ast.ClassDef) else "function")
                                visit(child)
                        visit(ast.parse(body))
                        assert kinds_in_source
                        if len(kinds_in_source) == 1:
                            gold_counts["gold_to_ast:" + str(symbol.get("kind")) + "->" + next(iter(kinds_in_source))] += 1
                    elif a["path"].endswith(".go") and span:
                        name = re.escape(symbol["name"])
                        if re.search(r"func\s*\([^\n]*\)\s*" + name + r"\s*\(", fragment):
                            gold_counts["gold_function_receiver_method"] += symbol.get("kind") == "function"
    return {"query_file_sha256": sha(folder / "queries.jsonl"), "query_count": len(queries),
            "schedule_complete": True, "missing_rows": 0, "counts": dict(counts),
            "same_path_pair_failures": dict(failures), "kind_mismatch_pairs": dict(kinds),
            "rows_with_candidate_predicates": dict(row_stages), "source_gold_checks": dict(gold_counts)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repository", required=True, type=Path)
    args = parser.parse_args()
    base = args.inputs
    result = {"scope": "public_DEV_read_only_predicate_diagnosis_not_rescoring", "groups": {}, "suites": {}, "all_profiles_schedule": {}}
    admission_path = base / "admission/crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/typescript-extension/admission.json"
    admission = json.loads(admission_path.read_text())
    frozen_queries = []
    for repo, receipt in admission["repo_results"].items():
        entries = receipt.get("dev_suite_entries") or [receipt["native_dev_suite_entry"], receipt["compat_dev_suite_entry"]]
        for entry in entries:
            raw_suite = subprocess.check_output(["git", "show", entry], cwd=args.repository)
            assert hashlib.sha256(raw_suite).hexdigest() == admission["inputs_sha256"][entry]
            suite = json.loads(raw_suite)
            query_entry = entry.rsplit("/", 1)[0] + "/" + suite["queries"]
            raw_queries = subprocess.check_output(["git", "show", query_entry], cwd=args.repository)
            assert hashlib.sha256(raw_queries).hexdigest() == admission["inputs_sha256"][query_entry]
            parsed = [json.loads(line) for line in raw_queries.splitlines()]
            assert all(q["split"] == "dev" for q in parsed)
            frozen_queries.append((parsed, hashlib.sha256(raw_queries).hexdigest()))
    checked_sources = 0
    for entry, expected_sha in admission["inputs_sha256"].items():
        if "/source/" not in entry:
            continue
        repo = entry.split("/public-v19/", 1)[1].split("/", 1)[0]
        if repo not in ["requests", "gin"]:
            continue
        local = base / "sources" / repo / entry.split("/source/", 1)[1]
        assert sha(local) == expected_sha
        checked_sources += 1
    result["source_integrity"] = {"admission_sha256": sha(admission_path), "source_files_verified": checked_sources}
    for group in ["js", "pygo"]:
        owner = base / group / "artifacts/checkpoints" / ("public-dev-current-group-" + group + "-20261003")
        result["groups"][group] = audit_group(base / group / "raw", owner / "raw-artifact-manifest.json")
        rawroot = base / group / "raw"
        if group == "js":
            rawroot /= "full-current01"  # pilot is verified but never double counted.
        for folder in sorted(rawroot.iterdir()):
            if not folder.is_dir() or not (folder / "normalized.jsonl").exists():
                continue
            profile = folder.name.rsplit("-", 1)[1]
            persisted = rows(folder / "queries.jsonl")
            matches = [digest for parsed, digest in frozen_queries if parsed == persisted]
            assert len(matches) == 1
            observed = rows(folder / "normalized.jsonl")
            keys = [(r["case_id"], r["repetition"]) for r in observed]
            expected = {(q["id"], rep) for q in persisted for rep in range(3)}
            assert len(keys) == len(set(keys)) and set(keys) == expected
            result["all_profiles_schedule"][folder.name] = {
                "queries": len(persisted), "scheduled": len(expected), "executed": len(keys),
                "missing": 0, "status_counts": dict(Counter(r["status"] for r in observed)),
                "frozen_query_sha256": matches[0], "persisted_query_sha256": sha(folder / "queries.jsonl"),
                "persisted_query_semantics_equal_frozen": True}
            if profile != "native":
                continue
            source = base / "sources" / folder.name.split("-", 1)[0] if group == "pygo" else None
            suite = audit_suite(folder, source)
            suite["frozen_query_sha256"] = matches[0]
            suite["persisted_query_semantics_equal_frozen"] = True
            result["suites"][folder.name] = suite
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"suite_count": len(result["suites"]), "output_sha256": sha(args.output),
                      "native_rows": sum(v["counts"]["rows"] for v in result["suites"].values())}))


if __name__ == "__main__":
    main()

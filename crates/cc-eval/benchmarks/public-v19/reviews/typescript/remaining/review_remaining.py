#!/usr/bin/env python3
"""Replay separately frozen manual decisions on remaining current public dev only."""
import argparse
import collections
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[6]
AUTHOR = "4c01f627bd6df73e70bfe1dbdc42b2a9234e797a"
PREFIX = "crates/cc-eval/benchmarks/public-v19/typescript/"
SOURCE = "ed4807212c28c90777c1d7ef2bf8e47af5d08519"
PROTOCOL = "03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6"
REVIEWER = "independent-source-review/cloud-typescript-dev-review"
FIRST20 = "398e58dd12fac85ce62c43ba93815b26af74fef41714e261f85d2430e0f24e98"
RANGES = {1: (20, 40), 2: (40, 60), 3: (60, 73)}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path):
    allowed = {"source-manifest.json"}
    allowed.update(f"blocks/{b}/{f}" for b in ["01-migration", "02", "03", "04", "05"] for f in ["queries.native.dev.jsonl", "queries.compat.dev.jsonl"])
    assert path in allowed or path.startswith("source/")
    assert ".." not in path and "holdout" not in path and "questions/" not in path
    return subprocess.check_output(["git", "show", f"{AUTHOR}:{PREFIX}{path}"], cwd=ROOT)


def audit(block):
    first_raw = (HERE.parent / "dev-020-review.json").read_bytes()
    assert sha(first_raw) == FIRST20
    first = json.loads(first_raw)
    assert first["author_current_sha"] == AUTHOR and first["reviewer_id"] == REVIEWER
    manifest_raw = read("source-manifest.json")
    assert sha(manifest_raw) == first["source_manifest_sha256"]
    manifest = json.loads(manifest_raw)
    admitted = {f["path"]: f for f in manifest["admitted_files"]}
    # Admission is inherited; no source/license network requests or re-admission.
    sources = {}

    def source(path):
        assert path in admitted
        if path not in sources:
            sources[path] = read("source/" + path)
        return sources[path]

    rows, compat, file_hashes = [], {}, []
    for b in ["01-migration", "02", "03", "04", "05"]:
        n = read(f"blocks/{b}/queries.native.dev.jsonl")
        c = read(f"blocks/{b}/queries.compat.dev.jsonl")
        rows.extend((json.loads(line), line) for line in n.splitlines(keepends=True))
        compat.update({r["id"]: r for r in map(json.loads, c.splitlines())})
        file_hashes.append({"block": b, "native_dev_sha256": sha(n), "compat_dev_sha256": sha(c)})
    assert len(rows) == 73 and len({r["query_family"] for r, _ in rows}) == 73
    start, end = RANGES[block]
    directory = HERE / f"block-{start + 1:03}-{end:03}"
    manual_raw = (directory / "manual-decisions.json").read_bytes()
    manual = json.loads(manual_raw)
    assert manual["author_current_sha"] == AUTHOR and manual["reviewer_id"] == REVIEWER
    assert manual["manual_source_review_complete"] is True
    signed = {d["row_sha256"]: d for d in manual["rows"]}
    assert len(signed) == end - start

    def verify(e):
        assert e["source_sha"] == SOURCE
        raw = source(e["path"])
        begin, finish = e["start_byte"], e["end_byte"]
        assert 0 <= begin < finish <= len(raw)
        fragment = raw[begin:finish]
        assert sha(fragment) == e["span_sha256"]
        assert e["file_sha256"] == admitted[e["path"]]["sha256"]
        assert b"".join(raw.splitlines(keepends=True)[e["start_line"] - 1:e["end_line"]]) == fragment
        fragment.decode("utf-8")

    decisions = []
    for r, line in rows[start:end]:
        a, groups = r["annotations"]["v19"], r["answers"]
        assert a["author_id"] != REVIEWER and not a["ranking_inspected"]
        assert a["source_sha"] == SOURCE and a["protocol_commit"] == PROTOCOL
        assert r["split"] == "dev" and a["custody_status"] == "public_dev_candidate"
        key = a["global_family"]
        assert key == r["query_family"]
        digest = hashlib.sha256(("codecortex-public-v19-split-v1\n" + key).encode()).digest()
        assert a["split_hash_sha256"] == digest.hex() and int.from_bytes(digest[:8], "big") >= 2**62
        assert not r["expected_files"]
        for e in a["gold_evidence"]:
            verify(e)
        if r["no_answer"]:
            assert not groups and not a["gold_evidence"] and r["id"] not in compat
            absence = a["absence_evidence"]
            assert len(absence["scope_files"]) == 1 and absence["source_sha"] == SOURCE
            path = absence["scope_files"][0]
            raw = source(path)
            assert path == r["path_prefix"] == a["hard_scope"]["path_prefix"]
            assert absence["file_sha256"] == admitted[path]["sha256"]
            assert absence["checked_bytes"] == [0, len(raw)] and absence["checked_lines"] == [1, len(raw.splitlines())]
            assert all(t.lower().encode() not in raw.lower() for t in absence["absent_tokens"])
        else:
            assert sum(g["primary"] for g in groups) == 1
            assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in groups}
            for g in groups:
                assert g["grade"] == (3 if g["primary"] else 2)
                for alt in g["alternatives"]:
                    assert any(alt["path"] == e["path"] and alt["symbol"]["name"] == e["symbol"] and alt["span"] == {"start": e["start_byte"], "end": e["end_byte"]} for e in a["gold_evidence"])
            c = compat[r["id"]]
            assert c["query"] == r["query"] and c["answers"] == [] and c["split"] == "dev"
            assert c["expected_files"] == list(dict.fromkeys(alt["path"] for g in sorted(groups, key=lambda g: not g["primary"]) for alt in g["alternatives"]))
        by_group = {g["id"]: g for g in groups}
        for edge in a["graph_constraints"]:
            assert edge["source_sha"] == SOURCE
            assert edge["from_group"] in by_group and edge["to_group"] in by_group
            begin, finish = edge["start_byte"], edge["end_byte"]
            assert source(edge["path"])[begin:finish].decode() == edge["token"]
            assert any(alt["path"] == edge["path"] and alt["span"]["start"] <= begin < finish <= alt["span"]["end"] for alt in by_group[edge["from_group"]]["alternatives"])
        signature = signed[sha(line)]
        assert signature["decision"] in {"accept", "needschange", "reject"}
        assert bool(signature["error_codes"]) == (signature["decision"] != "accept")
        decisions.append({"row_sha256": sha(line), "family_id_sha256": sha(r["query_family"].encode()), "component_id_sha256": sha(key.encode()), "no_answer": r["no_answer"], "source_evidence_sha256": [e["span_sha256"] for e in a["gold_evidence"]], **signature})
    counts = collections.Counter(d["decision"] for d in decisions)
    return directory, {"author_current_sha": AUTHOR, "source_sha": SOURCE, "protocol_sha": PROTOCOL, "reviewer_id": REVIEWER, "first20_review_sha": "d6a41bbcf76b5dc15e4924e54c7375f673d714f7", "first20_receipt_sha256": FIRST20, "first20_mutated": False, "inherited_official_source_admission_receipt_sha256": first["official_upstream_admission_receipt_sha256"], "source_license_admission_repeated": False, "review_sequence": [start + 1, end], "input_dev_files": file_hashes, "manual_decisions_sha256": sha(manual_raw), "counts": {"reviewed_public_dev_this_block": len(decisions), "accept": counts["accept"], "reject": counts["reject"], "needschange": counts["needschange"], "bounded_noanswer_reviewed": sum(d["no_answer"] for d in decisions), "proposed_local_components_reviewed": len(decisions), "blocked_holdout": 27, "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "ranking_runs": 0, "formal_complete20_blocks": 0, "accepted_confirmatory_holdout": 0}, "rows": decisions, "component_disputes": manual.get("component_disputes", []), "error_code_counts": dict(collections.Counter(c for d in decisions for c in d["error_codes"])), "scope_error_codes": ["G_CROSS_REPOSITORY_GLOBAL_TEMPLATE_EQUIVALENCE_AND_BLOCKED_COMPONENTS_UNREVIEWED", "H_CUSTODY_BLOCKED_27"], "checker_scope": "immutable_current_dev_bytes_spans_structure_projection_and_manual_receipt_replay; does_not_infer_semantic_truth_from_schema_or_token_checks", "source_gold_review": "manual_against_real_admitted_source; full_declared_absence_files; actual_bindings_branches_and_call_construction_dataflow_types", "global_association_frozen": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--block", type=int, choices=[1, 2, 3], required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    directory, report = audit(args.block)
    raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
    path = directory / "review-receipt.json"
    if args.check:
        assert path.read_text() == raw
    else:
        path.write_text(raw)
    print(json.dumps({"counts": report["counts"], "error_code_counts": report["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

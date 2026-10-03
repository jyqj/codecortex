#!/usr/bin/env python3
"""Frozen Gin public-dev review. No gold/evidence.json, old bodies, rank or provider."""
import argparse
import collections
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[5]
AUTHOR = "77d8707110afcb9935d29117ddf6162df4cef277"
SOURCE = "43fe48e8a0f44af783116cdb010725e6bb50255f"
PROTOCOL = "03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6"
PREFIX = "crates/cc-eval/benchmarks/public-v19/gin/"
REVIEWER = "independent-source-review/cloud-gin-dev-review"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path):
    assert path in {"queries.native.dev.jsonl", "queries.compat.dev.jsonl", "source-manifest.json", "provenance/source-lock.json", "license/LICENSE", "corpus-receipt.json"} or path.startswith("source/")
    assert ".." not in path and "holdout" not in path
    return subprocess.check_output(["git", "show", f"{AUTHOR}:{PREFIX}{path}"], cwd=ROOT)


def audit(limit):
    manifest = json.loads(read("source-manifest.json"))
    assert manifest["source_sha"] == SOURCE
    assert sha(read("license/LICENSE")) == manifest["license_sha256"]
    sources = {}
    inventory = []
    for item in manifest["files"]:
        if not item["admitted"]:
            continue
        raw = read("source/" + item["path"])
        assert sha(raw) == item["sha256"] and len(raw) == item["bytes"]
        assert b"MIT style" in raw[:250]
        raw.decode("utf-8")
        sources[item["path"]] = raw
        inventory.append({"path_sha256": sha(item["path"].encode()), "sha256": sha(raw), "bytes": len(raw)})
    assert len(sources) == 53
    native_raw = read("queries.native.dev.jsonl")
    compat_raw = read("queries.compat.dev.jsonl")
    lines = native_raw.splitlines(keepends=True)
    rows = [json.loads(line) for line in lines]
    compat = {r["id"]: r for r in map(json.loads, compat_raw.splitlines())}
    assert len(rows) == 67 and len(compat) == 55
    assert sum(r["no_answer"] for r in rows) == 12
    assert len({r["query_family"] for r in rows}) == len(rows)
    assert len({" ".join(r["query"].lower().split()) for r in rows}) == len(rows)
    manual = {14: ["R_PRIMARY_NOT_DISTINGUISHING_FACET"]}
    decisions = []
    for ordinal, r in enumerate(rows[:limit], 1):
        a = r["annotations"]["v19"]
        assert r["split"] == "dev" and a["source_sha"] == SOURCE
        assert a["author_id"] == "gin-author-cloud" and a["author_id"] != REVIEWER
        assert a["global_family"] == r["query_family"]
        assert int.from_bytes(hashlib.sha256(("codecortex-public-v19-split-v1\n" + a["global_family"]).encode()).digest()[:8], "big") >= 2**62
        evidence = a["gold_evidence"]
        evidence_map = {}

        def verify_evidence(e):
            raw = sources[e["path"]]
            start, end = e["span"]["start"], e["span"]["end"]
            assert 0 <= start < end <= len(raw) and e["source_sha"] == SOURCE
            assert sha(raw[start:end]) == e["sha256"]
            raw[start:end].decode("utf-8")
            assert raw[:start].count(b"\n") + 1 == e["start_line"]
            assert raw[:end - 1].count(b"\n") + 1 == e["end_line"]

        for e in evidence:
            verify_evidence(e)
            evidence_map[(e["path"], e["span"]["start"], e["span"]["end"])] = e
        groups = r["answers"]
        assert not r["expected_files"]
        if r["no_answer"]:
            assert not groups and r["id"] not in compat
            scope = a["absence_review"]
            assert scope["scope"] and "Within" in r["query"]
            for path in scope["scope"]:
                assert sha(sources[path]) == scope["scope_sha256"][path]
        else:
            assert groups and any(g["primary"] for g in groups)
            assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in groups}
            for group in groups:
                assert group["grade"] == (3 if group["primary"] else 2)
                for alt in group["alternatives"]:
                    e = evidence_map[(alt["path"], alt["span"]["start"], alt["span"]["end"])]
                    assert alt["symbol"]["name"] == e["symbol"].split(".")[-1]
            expected = list(dict.fromkeys(alt["path"] for g in sorted(groups, key=lambda g: not g["primary"]) for alt in g["alternatives"]))
            c = compat[r["id"]]
            assert c["expected_files"] == expected and c["answers"] == []
            assert c["query"] == r["query"] and c["split"] == "dev"
        for edge in a["graph_constraints"]:
            assert edge["from_group"] in {g["id"] for g in groups}
            assert edge["to_group"] in {g["id"] for g in groups}
            assert edge["direction"] == "forward" and edge["required"]
            for e in edge["evidence"]:
                verify_evidence(e)
        codes = manual.get(ordinal, [])
        decisions.append({"row_sha256": sha(lines[ordinal - 1]), "family_id_sha256": sha(r["query_family"].encode()), "component_id_sha256": sha(a["global_family"].encode()), "evidence_sha256": [e["sha256"] for e in evidence], "decision": "needschange" if codes else "accept", "error_codes": codes, "source_facts": "accept", "span_facets_projection_structure": "accept", "bounded_noanswer_scope": "accept" if r["no_answer"] else "not_applicable", "global_cross_repository_and_blocked_holdout_review": "not_run_scope_not_supplied"})
    return {"author_current_sha": AUTHOR, "source_sha": SOURCE, "protocol_sha": PROTOCOL, "reviewer_id": REVIEWER, "author_id": "gin-author-cloud", "native_file_sha256": sha(native_raw), "compat_file_sha256": sha(compat_raw), "license_sha256": manifest["license_sha256"], "source_manifest_sha256": sha(read("source-manifest.json")), "source_inventory": inventory, "source_scope": "existing_authorized_immutable_repository_snapshot; upstream_commit_not_independently_fetched", "counts": {"public_dev_reviewed": len(decisions), "accept": sum(d["decision"] == "accept" for d in decisions), "reject": 0, "needschange": sum(d["decision"] == "needschange" for d in decisions), "visible_singleton_components": len(decisions), "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "ranking_runs": 0, "blocked_holdout": 33, "accepted_confirmatory_holdout": 0, "formal_complete20_blocks": 0}, "rows": decisions, "components": [{"component_id_sha256": d["component_id_sha256"], "member_family_sha256": [d["family_id_sha256"]], "decision": d["decision"], "error_codes": d["error_codes"], "scope": "visible_dev_local_only"} for d in decisions], "error_code_counts": dict(collections.Counter(c for d in decisions for c in d["error_codes"])), "scope_error_codes": ["G_CROSS_REPOSITORY_AND_BLOCKED_COMPONENTS_UNREVIEWED", "H_CUSTODY_BLOCKED_33", "U_UPSTREAM_GIT_ORIGIN_NOT_INDEPENDENTLY_REPLAYED"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--limit", type=int, choices=[20, 67], default=67)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    r = audit(args.limit)
    raw = json.dumps(r, indent=2, sort_keys=True) + "\n"
    output = HERE / f"dev-{args.limit:03}-review.json"
    if args.check:
        assert output.read_text() == raw
    else:
        output.write_text(raw)
    print(json.dumps({"counts": r["counts"], "error_code_counts": r["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

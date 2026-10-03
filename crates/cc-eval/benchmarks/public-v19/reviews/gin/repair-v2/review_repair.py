#!/usr/bin/env python3
"""Frozen Gin four-row repair recheck; no aggregate gold or holdout access."""
import argparse
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[6]
AUTHOR = "949a9471f552d496e85b27c460c7b76536f13803"
PREVIOUS = "0ecfc52a14e423a1d37b81240667d4c1511076cb"
PROVENANCE = "ba79bcfc3f2bcc622297ef53b5de37ce205a5c89"
SOURCE = "43fe48e8a0f44af783116cdb010725e6bb50255f"
PREFIX = "crates/cc-eval/benchmarks/public-v19/gin/"
REVIEWER = "independent-source-review/cloud-gin-dev-review"
PRIOR_HASH = "ed9b3afeca246ec7754a46ebce014bf0d83d8fb89e14c6223c4df37440e9f6a2"
PROVENANCE_HASH = "9c64ebcfdf6879a5f209e1ec48e7c7292a48c744d8e8c2200d4366b060823427"
NATIVE_HASH = "c7524598e5fa511face9f31d04f49295187f71e22902cb9b2b85c0dbebb738b7"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def audit():
    def read(path):
        assert path in {"queries.native.dev.jsonl", "queries.compat.dev.jsonl", "source-manifest.json", "license/LICENSE", "provenance/dev-revision-v2.json"} or path.startswith("source/")
        assert ".." not in path
        return subprocess.check_output(["git", "show", f"{AUTHOR}:{PREFIX}{path}"], cwd=ROOT)

    own = "crates/cc-eval/benchmarks/public-v19/reviews/gin/"
    old_raw = subprocess.check_output(["git", "show", f"{PREVIOUS}:{own}dev-067-review.json"], cwd=ROOT)
    proof_raw = subprocess.check_output(["git", "show", f"{PROVENANCE}:{own}provenance/upstream-admission.json"], cwd=ROOT)
    assert sha(old_raw) == PRIOR_HASH and sha(proof_raw) == PROVENANCE_HASH
    old, proof = json.loads(old_raw), json.loads(proof_raw)
    assert old["reviewer_id"] == REVIEWER and proof["source_sha"] == SOURCE
    old_by_family = {d["family_id_sha256"]: d for d in old["rows"]}
    mapping_raw = read("provenance/dev-revision-v2.json")
    mapping = json.loads(mapping_raw)
    assert mapping["independent_review_commit"] == PREVIOUS
    assert mapping["independent_review_receipt_sha256"] == PRIOR_HASH
    assert mapping["independent_reviewer_id"] == REVIEWER
    changes = {r["family"]: r for r in mapping["rows"]}
    assert len(changes) == 4
    claimed = {r["family_sha256"]: r for r in mapping["unchanged_accepted_byte_proof"]}
    manifest_raw = read("source-manifest.json")
    assert sha(manifest_raw) == old["source_manifest_sha256"]
    manifest = json.loads(manifest_raw)
    proof_files = {f["path_sha256"]: f for f in proof["inventory"]}
    sources = {}
    for f in manifest["files"]:
        if not f["admitted"]:
            continue
        raw = read("source/" + f["path"])
        assert sha(raw) == f["sha256"] == proof_files[sha(f["path"].encode())]["sha256"]
        assert len(raw) == f["bytes"]
        sources[f["path"]] = raw
    assert len(sources) == 53
    assert sha(read("license/LICENSE")) == proof["license"]["sha256"]
    native, compat = read("queries.native.dev.jsonl"), read("queries.compat.dev.jsonl")
    assert sha(native) == NATIVE_HASH == mapping["after_file_sha256"]["native_dev"]
    assert sha(compat) == mapping["after_file_sha256"]["compat_dev"]
    rows = [(json.loads(line), line) for line in native.splitlines(keepends=True)]
    cs = {r["id"]: (r, line) for line in compat.splitlines(keepends=True) for r in [json.loads(line)]}
    assert len(rows) == 67 and len(cs) == 55

    def verify(e):
        assert e["source_sha"] == SOURCE
        raw = sources[e["path"]]
        start, end = e["span"]["start"], e["span"]["end"]
        assert 0 <= start < end <= len(raw) and sha(raw[start:end]) == e["sha256"]
        assert raw[:start].count(b"\n") + 1 == e["start_line"]
        assert raw[:end - 1].count(b"\n") + 1 == e["end_line"]

    decisions, retained, components = [], [], {}
    for r, line in rows:
        family = sha(r["query_family"].encode())
        prior = old_by_family[family]
        a, groups = r["annotations"]["v19"], r["answers"]
        key = a["global_family"]
        assert a["source_sha"] == SOURCE and r["split"] == "dev"
        assert int.from_bytes(hashlib.sha256(("codecortex-public-v19-split-v1\n" + key).encode()).digest()[:8], "big") >= 2**62
        components.setdefault(key, []).append(family)
        if not r["no_answer"]:
            c, cline = cs[r["id"]]
            expected = list(dict.fromkeys(alt["path"] for g in sorted(groups, key=lambda g: not g["primary"]) for alt in g["alternatives"]))
            assert c["expected_files"] == expected and c["query"] == r["query"] and c["answers"] == []
        else:
            assert not groups and r["id"] not in cs
        if r["query_family"] not in changes:
            assert prior["decision"] == "accept" and sha(line) == prior["row_sha256"]
            assert claimed[family]["native_row_sha256"] == sha(line)
            if not r["no_answer"]:
                assert sha(cline) == claimed[family]["compat_row_sha256"]
            retained.append({"family_id_sha256": family, "native_row_sha256": sha(line), "decision_basis": "prior_exact_native_row_acceptance; current_compat_projection_verified"})
            continue
        m = changes[r["query_family"]]
        assert prior["decision"] == "needschange"
        assert m["before_native_row_sha256"] == prior["row_sha256"]
        assert sha(line) == m["after_native_row_sha256"]
        assert set(m["error_codes"]) == set(prior["error_codes"])
        assert m["previous_split"] == m["current_split"] == "dev"
        assert key == m["current_global_family"]
        assert a["author_id"] != REVIEWER and a["review_status"] == "pending"
        assert a["revision"]["previous_row_sha256"] == prior["row_sha256"]
        assert sum(g["primary"] for g in groups) == 1
        assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in groups}
        evidence = a["gold_evidence"]
        for e in evidence:
            verify(e)
        for g in groups:
            assert g["grade"] == (3 if g["primary"] else 2)
            for alt in g["alternatives"]:
                assert any(alt["path"] == e["path"] and alt["span"] == e["span"] and alt["symbol"]["name"] == e["symbol"].split(".")[-1] for e in evidence)
        for edge in a["graph_constraints"]:
            assert edge["from_group"] in {g["id"] for g in groups} and edge["to_group"] in {g["id"] for g in groups}
            assert edge["direction"] == "forward" and edge["required"]
            for e in edge["evidence"]:
                verify(e)
        if "status_precondition" in a:
            cond = a["status_precondition"]
            assert cond["status_code"] == 200 and cond["body_allowed"] is True
            verify(cond["source_evidence"])
            assert all(edge["precondition"] == "bodyAllowedForStatus(200) == true" for edge in a["graph_constraints"][1:])
        decisions.append({"family_id_sha256": family, "component_id_sha256": sha(key.encode()), "old_native_row_sha256": prior["row_sha256"], "new_native_row_sha256": sha(line), "new_compat_row_sha256": sha(cline), "resolved_previous_error_codes": prior["error_codes"], "decision": "accept", "error_codes": [], "source_facts_primary_facets_spans_chain_projection": "manually_accepted", "source_evidence_sha256": [e["sha256"] for e in evidence]})
    assert len(decisions) == 4 and len(retained) == 63 and len(components) == 66
    pair = ["v19.gin.f0045", "v19.gin.f0053"]
    assert set(components[min(pair)]) == {sha(f.encode()) for f in pair}
    return {"author_current_sha": AUTHOR, "previous_review_sha": PREVIOUS, "previous_review_receipt_sha256": PRIOR_HASH, "source_admission_review_sha": PROVENANCE, "source_admission_receipt_sha256": PROVENANCE_HASH, "source_sha": SOURCE, "reviewer_id": REVIEWER, "native_dev_file_sha256": sha(native), "compat_dev_file_sha256": sha(compat), "revision_mapping_sha256": sha(mapping_raw), "counts": {"repaired_dev_reviewed": 4, "repaired_accept": 4, "repaired_needschange": 0, "repaired_reject": 0, "unchanged_prior_accepted_native_rows": 63, "combined_content_accepted_native_rows": 67, "compat_projections_verified": 55, "bounded_noanswer_unchanged": 12, "visible_dev_local_components": 66, "local_equivalence_pairs_adjudicated": 1, "author_proposed_total_candidate_components": 99, "blocked_holdout": 33, "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "ranking_runs": 0, "formal_complete20_blocks": 0, "accepted_confirmatory_holdout": 0}, "rows": decisions, "retained_prior_acceptance": retained, "local_equivalence": {"decision": "accept_conservative_local_association", "member_family_sha256": [sha(f.encode()) for f in pair], "canonical_component_sha256": sha(min(pair).encode()), "reason_code": "C_SHARED_CORE_ANSWER_OBLIGATION_EXTRA_CHAIN_NOT_INDEPENDENCE", "canonical_min_id_and_fixed_dev_split": "verified", "global_association_frozen": False}, "error_code_counts": {}, "scope_error_codes": ["G_CROSS_REPOSITORY_AND_BLOCKED_COMPONENTS_UNREVIEWED", "H_CUSTODY_BLOCKED_33", "G_TOTAL_99_IS_AUTHOR_CANDIDATE_COUNT_NOT_GLOBAL_CERTIFICATION"], "gold_fragment_byte_identity": "author_assertion_only; aggregate_gold_not_read; independent_native_evidence_fact_review_and_prior_native_row_hash_replay_completed", "compat_old_row_byte_identity": "current_unchanged_rows_match_author_mapping_hashes_and_projection; prior_independent_per_row_compat_hashes_not_available", "prior_review_and_provenance_receipts_mutated": False, "checker_scope": "hash_structure_projection_frozen_receipt_replay_only; semantic_decisions_manual"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = audit()
    raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
    path = HERE / "dev-004-repair-review.json"
    if args.check:
        assert path.read_text() == raw
    else:
        path.write_text(raw)
    print(json.dumps({"counts": report["counts"], "error_code_counts": report["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

#!/usr/bin/env python3
"""First20 current public TypeScript dev review; no legacy candidates/holdout reads."""
import argparse
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[5]
AUTHOR = "4c01f627bd6df73e70bfe1dbdc42b2a9234e797a"
SOURCE = "ed4807212c28c90777c1d7ef2bf8e47af5d08519"
PROTOCOL = "03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6"
PREFIX = "crates/cc-eval/benchmarks/public-v19/typescript/"
REVIEWER = "independent-source-review/cloud-typescript-dev-review"
UPSTREAM_HASH = "e7d952edcaba0e3d04779d4fa2342f0a478551becf7ed94a387530a082fc0679"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read(path):
    allowed = {"source-manifest.json", "provenance.json", "protocol-reference/lock.json", "protocol-reference/source-locks.json", "blocks/validation-through-05.json"}
    allowed.update("blocks/" + b + "/" + f for b in ["01-migration", "02"] for f in ["queries.native.dev.jsonl", "queries.compat.dev.jsonl", "corpus-receipt.json"])
    assert path in allowed or path.startswith("source/") or path in {"license/LICENSE.txt", "license/NOTICE.txt", "license/vscode-uri-LICENSE.md"}
    assert ".." not in path and "holdout" not in path and "questions/" not in path
    return subprocess.check_output(["git", "show", f"{AUTHOR}:{PREFIX}{path}"], cwd=ROOT)


def audit():
    proof_raw = (HERE / "upstream-source-admission.json").read_bytes()
    assert sha(proof_raw) == UPSTREAM_HASH
    proof = json.loads(proof_raw)
    assert proof["repository"] == "microsoft/TypeScript" and proof["source_sha"] == SOURCE
    assert proof["official_commit_tree"] == "82e7df2ae99d151725fc2331a37887109b11c8b7"
    proof_by_path = {r["path_sha256"]: r for r in proof["records"]}
    manifest_raw, provenance_raw = read("source-manifest.json"), read("provenance.json")
    manifest, provenance = json.loads(manifest_raw), json.loads(provenance_raw)
    assert provenance["repository"] == "microsoft/TypeScript"
    assert provenance["upstream_sha"] == manifest["upstream_sha"] == SOURCE
    locks = json.loads(read("protocol-reference/source-locks.json"))
    lock = next(r for r in locks["candidates"] if r["repository"] == "microsoft/TypeScript")
    assert lock["source_sha"] == SOURCE and lock["language_role"] == "TypeScript" and lock["license_declared"] == "Apache-2.0"
    assert json.loads(read("protocol-reference/lock.json"))["commit"] == PROTOCOL
    sources = {}
    inventory = []
    for f in manifest["admitted_files"]:
        assert f["path"].startswith("packages/typescript/src/") and f["path"].endswith(".ts")
        assert not f["path"].endswith(".generated.ts") and "/vendor/" not in f["path"]
        raw = read("source/" + f["path"])
        assert sha(raw) == f["sha256"] and len(raw) == f["bytes"]
        upstream = proof_by_path[sha(("source/" + f["path"]).encode())]
        assert sha(raw) == upstream["sha256"] and len(raw) == upstream["bytes"] and upstream["snapshot_matches_upstream"]
        raw.decode("utf-8")
        sources[f["path"]] = raw
        inventory.append({"path_sha256": sha(f["path"].encode()), "sha256": sha(raw), "bytes": len(raw)})
    assert len(sources) == manifest["admitted_file_count"] == 20
    assert sum(len(raw) for raw in sources.values()) == manifest["admitted_bytes"]
    for f in manifest["license_artifacts"]:
        raw = read(f["path"])
        assert sha(raw) == f["sha256"] == proof_by_path[sha(f["path"].encode())]["sha256"]
    assert sha(read("license/LICENSE.txt")) == lock["license_files"][0]["sha256"]
    assert len(read("license/LICENSE.txt")) == lock["license_files"][0]["bytes"]
    status = json.loads(read("blocks/validation-through-05.json"))
    assert status["public_dev_families"] == sum(b["public_dev"] for b in status["blocks"]) == 73
    assert sum(b["holdout_custody_blocked"] for b in status["blocks"]) == 27
    combined, compat, input_hashes = [], {}, []
    for block in ["01-migration", "02"]:
        n = read(f"blocks/{block}/queries.native.dev.jsonl")
        c = read(f"blocks/{block}/queries.compat.dev.jsonl")
        receipt = json.loads(read(f"blocks/{block}/corpus-receipt.json"))
        native_partition = next(p for p in receipt["native_partitions"] if p["partition"] == "dev")
        assert sha(n) == native_partition["file_sha256"] and sha(c) == receipt["compat_file_sha256"]
        assert receipt["protocol_commit"] == PROTOCOL and receipt["source_sha"] == SOURCE
        combined.extend((json.loads(line), line) for line in n.splitlines(keepends=True))
        compat.update({r["id"]: r for r in map(json.loads, c.splitlines())})
        input_hashes.append({"block": block, "native_dev_sha256": sha(n), "compat_dev_sha256": sha(c)})
    reviewed = combined[:20]
    assert len(reviewed) == 20 and len({r["query_family"] for r, _ in reviewed}) == 20
    assert len({" ".join(r["query"].lower().split()) for r, _ in reviewed}) == 20

    def verify(e):
        assert e["source_sha"] == SOURCE
        raw = sources[e["path"]]
        start, end = e["start_byte"], e["end_byte"]
        assert 0 <= start < end <= len(raw)
        fragment = raw[start:end]
        assert sha(fragment) == e["span_sha256"] and sha(raw) == e["file_sha256"]
        assert b"".join(raw.splitlines(keepends=True)[e["start_line"] - 1:e["end_line"]]) == fragment
        fragment.decode("utf-8")

    decisions = []
    for r, line in reviewed:
        a = r["annotations"]["v19"]
        assert r["split"] == "dev" and a["custody_status"] == "public_dev_candidate"
        assert a["source_sha"] == SOURCE and a["protocol_commit"] == PROTOCOL
        assert a["author_id"] == "typescript-independent-author-cloud" and a["author_id"] != REVIEWER
        assert not a["ranking_inspected"] and not a["independent_review_signed"]
        family, key = r["query_family"], a["global_family"]
        assert family == key
        assert a["family_hash_sha256"] == sha(family.encode())
        digest = hashlib.sha256(("codecortex-public-v19-split-v1\n" + key).encode()).digest()
        assert digest.hex() == a["split_hash_sha256"] and int.from_bytes(digest[:8], "big") >= 2**62
        evidence, groups = a["gold_evidence"], r["answers"]
        assert not r["expected_files"]
        for e in evidence:
            verify(e)
        if r["no_answer"]:
            assert not groups and not evidence and r["id"] not in compat
            absence = a["absence_evidence"]
            assert absence["source_sha"] == SOURCE and len(absence["scope_files"]) == 1
            path = absence["scope_files"][0]
            raw = sources[path]
            assert path == r["path_prefix"] == a["hard_scope"]["path_prefix"]
            assert sha(raw) == absence["file_sha256"] and absence["checked_bytes"] == [0, len(raw)]
            assert absence["checked_lines"] == [1, len(raw.splitlines())]
            assert all(token.encode().lower() not in raw.lower() for token in absence["absent_tokens"])
        else:
            assert sum(g["primary"] for g in groups) == 1
            assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in groups}
            for group in groups:
                assert group["grade"] == (3 if group["primary"] else 2)
                for alt in group["alternatives"]:
                    assert any(alt["path"] == e["path"] and alt["symbol"]["name"] == e["symbol"] and alt["span"] == {"start": e["start_byte"], "end": e["end_byte"]} for e in evidence)
            c = compat[r["id"]]
            assert c["query"] == r["query"] and c["answers"] == [] and c["split"] == "dev"
            assert c["expected_files"] == list(dict.fromkeys(alt["path"] for g in sorted(groups, key=lambda g: not g["primary"]) for alt in g["alternatives"]))
        by_group = {g["id"]: g for g in groups}
        for edge in a["graph_constraints"]:
            assert edge["source_sha"] == SOURCE and edge["relation"] == "calls"
            assert edge["from_group"] in by_group and edge["to_group"] in by_group
            start, end = edge["start_byte"], edge["end_byte"]
            assert sources[edge["path"]][start:end].decode() == edge["token"]
            assert any(alt["path"] == edge["path"] and alt["span"]["start"] <= start < end <= alt["span"]["end"] for alt in by_group[edge["from_group"]]["alternatives"])
            assert any(edge["token"] == alt["symbol"]["name"] + "(" for alt in by_group[edge["to_group"]]["alternatives"])
        decisions.append({"row_sha256": sha(line), "family_id_sha256": sha(family.encode()), "component_id_sha256": sha(key.encode()), "source_evidence_sha256": [e["span_sha256"] for e in evidence], "no_answer": r["no_answer"], "decision": "accept", "error_codes": [], "source_facts_spans_primary_facets_chain_projection": "manual_accept", "bounded_full_file_absence": "manual_accept" if r["no_answer"] else "not_applicable", "local_first20_component_independence": "manual_accept_distinct_core_obligations", "remaining_dev_global_and_blocked_independence": "not_reviewed"})
    return {"author_current_sha": AUTHOR, "source_sha": SOURCE, "protocol_sha": PROTOCOL, "reviewer_id": REVIEWER, "author_id": "typescript-independent-author-cloud", "input_dev_files": input_hashes, "source_manifest_sha256": sha(manifest_raw), "provenance_sha256": sha(provenance_raw), "official_upstream_admission_receipt_sha256": UPSTREAM_HASH, "declared_and_actual_repository": "microsoft/TypeScript", "language_admission": "20 actual TypeScript .ts API/AST sources under locked packages/typescript/src; whole_repository_primary_language_not_claimed", "license_admission": "accept_for_20_admitted_files; exact_root_Apache-2.0_and_full_NOTICE_retained; referenced_vscode-uri_MIT_license_retained; excluded_generated_vendor_dependency_bodies_not_admitted", "source_inventory": inventory, "counts": {"declared_current_public_dev": 73, "first_dev_reviewed": 20, "accept": 20, "reject": 0, "needschange": 0, "positive_reviewed": 17, "bounded_noanswer_reviewed": 3, "local_first20_components_accepted": 20, "remaining_public_dev_unreviewed": 53, "official_source_files_byte_equal": 20, "official_license_files_byte_equal": 3, "blocked_holdout": 27, "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "ranking_runs": 0, "accepted_confirmatory_holdout": 0, "formal_complete20_blocks": 0}, "rows": decisions, "components": [{"component_id_sha256": d["component_id_sha256"], "member_family_sha256": [d["family_id_sha256"]], "decision": "accept_first20_local_only", "error_codes": []} for d in decisions], "error_code_counts": {}, "scope_error_codes": ["G_REMAINING_53_DEV_AND_CROSS_REPOSITORY_COMPONENTS_UNREVIEWED", "H_CUSTODY_BLOCKED_27"], "manual_review_basis": "source_facts_named_symbol_identity_branch_semantics_primary_required_secondary_facets_import_bound_calls_and_full_declared_absence_files; shared_files_or_topics_not_equivalence", "checker_scope": "SHA256_hash_span_structure_projection_receipt_replay_only; semantic_decisions_manual; independent_native_BLAKE3_or_evaluator_run_not_claimed", "read_boundary": "current_dev_01-migration_and_02_only; metadata_source_licenses; no_questions/candidates.jsonl_no_author_scripts_no_legacy_or_blocked_bodies", "author_pr_creation": "not_attempted_or_substituted; reviewer_PR_only", "global_association_frozen": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = audit()
    raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
    path = HERE / "dev-020-review.json"
    if args.check:
        assert path.read_text() == raw
    else:
        path.write_text(raw)
    print(json.dumps({"counts": report["counts"], "error_code_counts": report["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

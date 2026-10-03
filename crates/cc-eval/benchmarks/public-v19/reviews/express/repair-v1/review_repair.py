#!/usr/bin/env python3
"""Same-reviewer PR78 delta receipt. Read only hash/blob-gated current dev inputs."""
import argparse
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[6]
AUTHOR = "465e9e0bd435e2e30c08de8702f78a0d10c49c8e"
PREVIOUS_REVIEW = "24affebcaab1caa9acd08841233867c89931dbf3"
SOURCE = "7ef98448f8b38099ab1ded55e458538ad47a51e7"
PROTOCOL = "03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6"
REVIEWER = "independent-source-review/cloud-express-dev-review"
PREFIX = "crates/cc-eval/benchmarks/public-v19/express/"
ENTRY = PREFIX + "review/public-dev-review-entry-repair-v1.json"
ENTRY_HASH = "d59b67a56a529b93e9d52e065d085b36af91129916b85935dbfdba45dbac8250"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def audit():
    entry_raw = git("show", f"{AUTHOR}:{ENTRY}")
    assert sha(entry_raw) == ENTRY_HASH
    entry = json.loads(entry_raw)
    assert entry["fixed_review_commit"] == PREVIOUS_REVIEW
    assert entry["source_sha"] == SOURCE and entry["protocol_commit"] == PROTOCOL
    assert entry["holdout_bodies_in_allowlist"] == 0
    allowed = {f["repo_path"]: f for f in entry["files"]}
    admitted = {}
    for path, f in allowed.items():
        assert path.startswith(PREFIX) and "holdout" not in path and ".." not in path
        raw = git("show", f"{AUTHOR}:{path}")
        assert sha(raw) == f["sha256"] and len(raw) == f["bytes"]
        assert hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest() == f["git_blob"]
        admitted[path[len(PREFIX):]] = raw
    assert len(admitted) == 23

    def read(relative):
        return admitted[relative]

    prior_names = ["dev-068-review.json", "dev-002-delta-review.json", "component-dispute.json"]
    prior = {name: json.loads(read("review/independent-pr71/" + name)) for name in prior_names}
    prior_path = "crates/cc-eval/benchmarks/public-v19/reviews/express/"
    for name in prior_names:
        # Compare immutable Git identities only, without reopening historical gold.
        assert git("rev-parse", f"{PREVIOUS_REVIEW}:{prior_path}{name}").strip() == allowed[PREFIX + "review/independent-pr71/" + name]["git_blob"].encode()
    previous = [*prior[prior_names[0]]["rows"], *prior[prior_names[1]]["rows"]]
    assert len(previous) == 70 and all(p["reviewer_id"] == REVIEWER for p in prior.values())
    old_by_family = {d["family_id_sha256"]: d for d in previous}
    mapping_raw = read("review/dev-repair-v1-row-mapping.json")
    mapping = json.loads(mapping_raw)
    assert mapping["independent_review_commit"] == PREVIOUS_REVIEW
    mappings = {m["id"]: m for m in mapping["mappings"]}
    assert len(mappings) == 8
    native = read("intake/dev-repair-v1/queries.native.dev.jsonl")
    compat = read("intake/dev-repair-v1/queries.compat.dev.jsonl")
    lines = native.splitlines(keepends=True)
    rows = [json.loads(line) for line in lines]
    compat_lines = compat.splitlines(keepends=True)
    compat_by_id = {r["id"]: (r, line) for line in compat_lines for r in [json.loads(line)]}
    assert len(rows) == 70 and len(compat_by_id) == 59
    sources = {p[len("source/"):]: b for p, b in admitted.items() if p.startswith("source/")}
    assert len(sources) == 7
    license_hash = sha(read("license/LICENSE"))
    assert license_hash == "95a5762890e5c1c9808921cef095661fc482c5e1f0bba31446ac85595df6237c"
    for path in sources:
        assert git("rev-parse", f"0850acc4fdacde2609c56cf733b7776e5027ceff:{PREFIX}source/{path}").strip() == allowed[PREFIX + "source/" + path]["git_blob"].encode()

    def verify_evidence(e):
        assert e["source_sha"] == SOURCE
        raw = sources[e["path"]]
        start, end = e["byte_start"], e["byte_end"]
        assert 0 <= start < end <= len(raw)
        fragment = raw[start:end]
        assert sha(fragment) == e["sha256"] and fragment.decode() == e["text"]
        assert b"".join(raw.splitlines(keepends=True)[e["line_start"] - 1:e["line_end"]]) == fragment

    decisions = []
    unchanged = []
    components = {}
    for r, line in zip(rows, lines):
        family = sha(r["query_family"].encode())
        a = r["annotations"]["v19"]
        key = a["global_family"]
        assert r["split"] == "dev" and a["source_sha"] == SOURCE
        assert int.from_bytes(hashlib.sha256(("codecortex-public-v19-split-v1\n" + key).encode()).digest()[:8], "big") >= 2**62
        components.setdefault(key, []).append(family)
        old = old_by_family[family]
        if r["id"] not in mappings:
            assert old["decision"] == "accept" and sha(line) == old["row_sha256"]
            unchanged.append({"family_id_sha256": family, "row_sha256": sha(line), "decision_basis": "unchanged_prior_exact_row_acceptance"})
            continue
        m = mappings[r["id"]]
        assert m["reviewer_id"] == REVIEWER and m["review_commit"] == PREVIOUS_REVIEW
        assert m["old_native_row_sha256"] == old["row_sha256"]
        assert m["new_native_row_sha256"] == sha(line) and old["decision"] == "needschange"
        assert key == m["new_proposed_component"]
        assert a["author_id"] == "B/express" and a["author_id"] != REVIEWER
        assert a["reviewer_id"] is None and not r["no_answer"] and not r["expected_files"]
        p = a["author_provenance"]
        assert not p["retrieval_inspected"]
        assert p["author_revision"]["supersedes_native_row_sha256"] == old["row_sha256"]
        assert p["chain_edges"] == a["graph_constraints"]
        evidence = p["source_evidence"]
        for e in evidence:
            verify_evidence(e)
        groups = r["answers"]
        assert sum(g["primary"] for g in groups) == 1
        assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in groups}
        for g in groups:
            assert g["grade"] == (3 if g["primary"] else 2)
            for alt in g["alternatives"]:
                assert any(alt["path"] == e["path"] and alt["symbol"]["name"] == e["symbol"] and alt["span"] == {"start": e["byte_start"], "end": e["byte_end"]} for e in evidence)
        c, c_line = compat_by_id[r["id"]]
        assert sha(c_line) == m["new_compat_row_sha256"]
        assert c["query"] == r["query"] and c["split"] == "dev" and c["answers"] == []
        assert c["expected_files"] == list(dict.fromkeys(alt["path"] for g in sorted(groups, key=lambda g: not g["primary"]) for alt in g["alternatives"]))
        for edge in a["graph_constraints"]:
            for side in ["from", "to"]:
                target = edge[side]
                if "evidence_index" in target:
                    e = evidence[target["evidence_index"]]
                    assert target["path"] == e["path"]
                    assert target.get("declared_source_symbol", target["symbol"]) == e["symbol"]
                else:
                    assert side == "to" and target["kind"] in {"external_boundary", "dynamic_callback"}
                    if target["kind"] == "external_boundary":
                        assert target["implementation_admitted"] is False
                    else:
                        assert target["implementation_admitted"] == "not_proven_for_all_configurations"
                    for e in target["binding_evidence"]:
                        verify_evidence(e)
            if "callsite_evidence" in edge:
                verify_evidence(edge["callsite_evidence"])
            assert all(0 <= index < len(evidence) for index in edge["supporting_evidence"])
        decisions.append({"family_id_sha256": family, "component_id_sha256": sha(key.encode()), "old_native_row_sha256": old["row_sha256"], "new_native_row_sha256": sha(line), "new_compat_row_sha256": sha(c_line), "resolved_previous_error_codes": m["original_error_codes"], "decision": "accept", "error_codes": [], "facts_spans_facets_primary_chain_projection": "manually_accepted_against_admitted_source", "source_evidence_sha256": [e["sha256"] for e in evidence]})
    assert len(unchanged) == 62 and len(decisions) == 8 and len(components) == 67
    proposal = json.loads(read("review/dev-repair-v1-component-proposal.json"))
    pair = proposal["pair"]
    assert proposal["proposed_canonical"] == min(pair)
    assert set(components[min(pair)]) == {sha(f.encode()) for f in pair}
    assert proposal["old_split"] == ["dev", "dev"] and proposal["new_candidate_split"] == "dev"
    assert proposal["cross_split_move"] is False
    relations = json.loads(read("relations.dev-repair-v1.json"))
    for c in relations["components"]:
        assert c["global_family"] == min(c["members"])
        assert set(components[c["global_family"]]) == {sha(f.encode()) for f in c["members"]}
    return {"author_current_sha": AUTHOR, "previous_review_sha": PREVIOUS_REVIEW, "protocol_sha": PROTOCOL, "source_sha": SOURCE, "allowlist_entry_sha256": ENTRY_HASH, "reviewer_id": REVIEWER, "author_id": "B/express", "native_dev_file_sha256": sha(native), "compat_dev_file_sha256": sha(compat), "license_sha256": license_hash, "row_mapping_sha256": sha(mapping_raw), "counts": {"allowlisted_files_verified": 23, "source_files_verified": 7, "repaired_public_dev_reviewed": 8, "repaired_accept": 8, "repaired_needschange": 0, "repaired_reject": 0, "unchanged_prior_accepted_native_rows": 62, "combined_content_accepted_native_rows": 70, "compat_dev_rows": 59, "visible_dev_local_components": 67, "local_component_pairs_adjudicated": 1, "author_proposed_total_candidate_components": 99, "blocked_holdout": 32, "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "rank_runs": 0, "formal_complete20_blocks": 0, "accepted_confirmatory_holdout": 0}, "repaired_rows": decisions, "unchanged_prior_acceptance": unchanged, "local_equivalence": {"decision": "accept_conservative_local_association", "member_family_sha256": [sha(f.encode()) for f in pair], "canonical_component_sha256": sha(min(pair).encode()), "reason_code": "C_SHARED_CORE_ANSWER_OBLIGATION_EXTRA_CHAIN_NOT_INDEPENDENCE", "source_task_review": "manual; shared_core_answer_obligation_not_merely_common_file_or_topic", "canonical_min_id_and_fixed_dev_split": "verified", "global_association_frozen": False, "cross_repository_or_blocked_members_reviewed": False}, "local_components": [{"component_id_sha256": sha(k.encode()), "member_family_sha256": v, "decision": "accept_local_only", "basis": "unchanged_prior_local_acceptance_or_repaired_rows_and_local_pair_review"} for k, v in sorted(components.items())], "source_origin": "same_source_git_blobs_as_previous_author; previous_review_recorded_official_raw_source_verification; no_new_upstream_fetch", "checker_scope": "hash_blob_structure_projection_and_frozen_receipt_replay; semantic_decisions_manual", "previous_decisions_mutated": False, "error_code_counts": {}, "scope_error_codes": ["G_CROSS_REPOSITORY_AND_BLOCKED_COMPONENTS_UNREVIEWED", "H_CUSTODY_BLOCKED_32", "G_TOTAL_99_IS_AUTHOR_CANDIDATE_COUNT_NOT_GLOBAL_CERTIFICATION"], "compat_unchanged_byte_replay": "not_claimed; repaired_8_row_hashes_and_projections_verified; prior_62_native_rows_byte_identity_verified_from_prior_review_receipts"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = audit()
    raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
    path = HERE / "dev-008-repair-review.json"
    if args.check:
        assert path.read_text() == raw
    else:
        path.write_text(raw)
    print(json.dumps({"counts": report["counts"], "error_code_counts": report["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

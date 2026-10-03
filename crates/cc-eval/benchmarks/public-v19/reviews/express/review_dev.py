#!/usr/bin/env python3
"""Read only fixed current public dev/source/protocol objects. Never read history gold."""
import argparse
import collections
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[5]
AUTHOR = "a739431b6595bb44e79130843e27e2cde2d34eb1"
PROTOCOL = "03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6"
PREFIX = "crates/cc-eval/benchmarks/public-v19/express/"
REVIEWER = "independent-source-review/cloud-express-dev-review"
SOURCE = "7ef98448f8b38099ab1ded55e458538ad47a51e7"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read(relative):
    # Strict allowlist: neither historical candidate bodies nor migration scripts.
    allowed = relative in {
        "provenance/source-lock.json", "source-manifest.json", "relations.json",
        "license/LICENSE", "provenance/protocol-v1/receipt.json",
        "intake/full-100/queries.native.dev.jsonl",
        "intake/full-100/queries.compat.dev.jsonl",
    } or relative.startswith("source/") or relative.startswith("provenance/protocol-v1/")
    assert allowed and ".." not in relative and "holdout" not in relative
    return subprocess.check_output(["git", "show", f"{AUTHOR}:{PREFIX}{relative}"], cwd=ROOT)


def load(relative):
    return json.loads(read(relative))


def audit(limit):
    lock = load("provenance/source-lock.json")
    assert lock["upstream_lock"]["source_sha"] == SOURCE
    assert sha(read("license/LICENSE")) == lock["license_sha256"]
    sources = {}
    source_checks = []
    for item in lock["files"]:
        raw = read("source/" + item["path"])
        assert len(raw) == item["bytes"] and sha(raw) == item["sha256"]
        assert hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest() == item["git_blob"]
        raw.decode("utf-8")
        sources[item["path"]] = raw
        source_checks.append({"path_sha256": sha(item["path"].encode()), "sha256": sha(raw), "bytes": len(raw)})
    protocol_checks = []
    for item in load("provenance/protocol-v1/receipt.json")["files"]:
        path = pathlib.PurePosixPath(item["upstream_path"]).name
        copied = read("provenance/protocol-v1/" + path)
        # Only protocol files are read at the older protocol commit.
        upstream = subprocess.check_output(["git", "show", f'{PROTOCOL}:{item["upstream_path"]}'], cwd=ROOT)
        assert sha(copied) == item["sha256"] and copied == upstream
        protocol_checks.append({"sha256": sha(copied), "bytes": len(copied)})
    native_raw = read("intake/full-100/queries.native.dev.jsonl")
    compat_raw = read("intake/full-100/queries.compat.dev.jsonl")
    native_lines = native_raw.splitlines(keepends=True)
    native = [json.loads(line) for line in native_lines]
    compat = {r["id"]: r for r in map(json.loads, compat_raw.splitlines())}
    assert len(native) == 68 and len(compat) == 57
    assert all(r["split"] == "dev" for r in [*native, *compat.values()])
    assert len({r["query_family"] for r in native}) == 68
    assert sum(r["no_answer"] for r in native) == 11
    selected = native[:limit]
    decisions = []
    # Manual source-task/edge review, frozen to exact row bytes at AUTHOR.
    manual = {
        41: ["R_CITATION_SYMBOL_WRONG", "C_TASK_EQUIVALENCE_REVIEW_NEEDED"],
        43: ["R_CHAIN_EXTERNAL_TARGET_INTERNAL"],
        44: ["R_CHAIN_DYNAMIC_TARGET_INTERNAL"],
        45: ["R_CHAIN_TARGET_WRONG"],
        51: ["R_CHAIN_EXTERNAL_TARGET_INTERNAL", "C_TASK_EQUIVALENCE_REVIEW_NEEDED"],
        52: ["R_CHAIN_EXTERNAL_TARGET_INTERNAL"],
        53: ["R_CHAIN_TARGET_WRONG"],
    }
    for ordinal, r in enumerate(selected, 1):
        a = r["annotations"]["v19"]
        p = a["author_provenance"]
        assert a["source_sha"] == p["source_sha"] == SOURCE
        assert a["author_id"] == "B/express" and a["reviewer_id"] is None
        assert REVIEWER != a["author_id"] and not p["retrieval_inspected"]
        assert a["hard_scope"]["path_prefix"] == r["path_prefix"]
        key = a["global_family"]
        split = int.from_bytes(hashlib.sha256(("codecortex-public-v19-split-v1\n" + key).encode()).digest()[:8], "big")
        assert split >= 2**62
        evidence = p["source_evidence"]
        evidence_hashes = []
        for e in evidence:
            raw = sources[e["path"]]
            segment = raw[e["byte_start"]:e["byte_end"]]
            assert e["source_sha"] == SOURCE and sha(segment) == e["sha256"]
            assert segment.decode() == e["text"]
            lines = raw.splitlines(keepends=True)
            assert b"".join(lines[e["line_start"] - 1:e["line_end"]]) == segment
            evidence_hashes.append(sha(segment))
        groups = r["answers"]
        assert not r["expected_files"]
        if r["no_answer"]:
            assert not groups and r["id"] not in compat
            absence = p["absence_proof"]
            assert set(absence["scope_files"]) == set(sources)
            assert "Within" in r["query"] and "admitted" in r["query"]
            for token in absence["tokens"]:
                assert all(token.encode() not in raw for raw in sources.values())
            assert all(check["hits"] == [] for check in absence["checks"])
        else:
            assert groups and any(g["primary"] for g in groups)
            assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in groups}
            for g in groups:
                assert g["grade"] == (3 if g["primary"] else 2)
                for alt in g["alternatives"]:
                    assert any(alt["path"] == e["path"] and alt["symbol"]["name"] == e["symbol"] and alt["span"] == {"start": e["byte_start"], "end": e["byte_end"]} for e in evidence)
            expected_paths = list(dict.fromkeys(alt["path"] for g in sorted(groups, key=lambda g: not g["primary"]) for alt in g["alternatives"]))
            assert compat[r["id"]]["expected_files"] == expected_paths
            assert compat[r["id"]]["answers"] == []
            assert compat[r["id"]]["query"] == r["query"]
            assert compat[r["id"]]["query_family"] == r["query_family"]
        assert a["graph_constraints"] == p["chain_edges"]
        for edge in p["chain_edges"]:
            for endpoint in ["from", "to"]:
                e = evidence[edge[endpoint]["evidence_index"]]
                assert (e["path"], e["symbol"]) == (edge[endpoint]["path"], edge[endpoint]["symbol"])
        codes = manual.get(ordinal, [])
        decisions.append({
            "row_sha256": sha(native_lines[ordinal - 1]),
            "family_id_sha256": sha(r["query_family"].encode()),
            "component_id_sha256": sha(key.encode()),
            "source_evidence_sha256": evidence_hashes,
            "decision": "needschange" if codes else "accept",
            "error_codes": codes,
            "no_answer": r["no_answer"],
            "compat_projection": "not_applicable" if r["no_answer"] else "accept",
            "source_span_and_hash": "accept",
            "source_fact_review": "needschange" if "R_CITATION_SYMBOL_WRONG" in codes else "accept",
            "chain_review": "needschange" if any(c.startswith("R_CHAIN") for c in codes) else "accept",
            "current_local_component_review": "needschange" if codes else "accept",
            "global_cross_repository_review": "not_run_scope_not_supplied",
        })
    components = []
    for component in sorted({d["component_id_sha256"] for d in decisions}):
        members = [d for d in decisions if d["component_id_sha256"] == component]
        codes = sorted({c for d in members for c in d["error_codes"]})
        components.append({"component_id_sha256": component, "member_family_sha256": sorted(d["family_id_sha256"] for d in members), "decision": "needschange" if codes else "accept", "error_codes": codes, "scope": "visible_dev_local_only", "global_cross_repository_review": "not_run_scope_not_supplied"})
    return {
        "author_current_sha": AUTHOR, "protocol_sha": PROTOCOL, "source_sha": SOURCE,
        "reviewer_id": REVIEWER, "author_id": "B/express",
        "counts": {"reviewed_public_dev": len(decisions), "row_accept": sum(d["decision"] == "accept" for d in decisions), "row_needschange": sum(d["decision"] == "needschange" for d in decisions), "row_reject": 0, "visible_components": len(components), "component_accept_local": sum(d["decision"] == "accept" for d in components), "component_needschange_local": sum(d["decision"] == "needschange" for d in components), "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "rank_runs": 0, "accepted_confirmatory_holdout": 0, "formal_complete_20_blocks": 0},
        "native_dev_file_sha256": sha(native_raw), "compat_dev_file_sha256": sha(compat_raw),
        "source_checks": source_checks, "protocol_checks": protocol_checks,
        "rows": decisions, "components": components,
        "error_code_counts": dict(sorted(collections.Counter(c for d in decisions for c in d["error_codes"]).items())),
        "global_scope_error_codes": ["G_CROSS_REPOSITORY_DEV_NOT_SUPPLIED", "H_CUSTODY_BLOCKED_UNREVIEWED_32"],
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--limit", type=int, choices=[20, 68], default=68)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = audit(args.limit)
    raw = json.dumps(result, indent=2, sort_keys=True) + "\n"
    output = HERE / f"dev-{args.limit:03}-review.json"
    if args.check:
        assert output.read_text() == raw
    else:
        output.write_text(raw)
    print(json.dumps({"counts": result["counts"], "error_code_counts": result["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

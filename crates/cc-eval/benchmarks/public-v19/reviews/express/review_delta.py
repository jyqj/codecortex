#!/usr/bin/env python3
"""PR69 allowlist-gated two-public-dev delta; no historical/holdout queries."""
import hashlib
import json
import pathlib
import subprocess
import sys

sys.dont_write_bytecode = True

from review_dev import AUTHOR, PREFIX, REVIEWER, ROOT, SOURCE, read, sha

HERE = pathlib.Path(__file__).resolve().parent
DELTA = "0850acc4fdacde2609c56cf733b7776e5027ceff"
ENTRY_HASH = "b8b7fe5a22222fc91cd8e09cdccbe24e3d2fadf8e1a60df02e27a9d2ac4f6b15"
entry_raw = subprocess.check_output(["git", "show", f"{DELTA}:{PREFIX}review/public-dev-review-entry.json"], cwd=ROOT)
assert sha(entry_raw) == ENTRY_HASH
entry = json.loads(entry_raw)
assert entry["base_commit"] == AUTHOR and entry["no_holdout_bodies_in_allowlist"]
allowed = {item["repo_path"]: item for item in entry["files"]}


def admitted(relative):
    path = PREFIX + relative
    assert path in allowed and "holdout" not in path and ".." not in path
    item = allowed[path]
    raw = subprocess.check_output(["git", "show", f"{DELTA}:{path}"], cwd=ROOT)
    assert sha(raw) == item["sha256"] and len(raw) == item["bytes"]
    assert hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest() == item["git_blob"]
    return raw


native = admitted("intake/public-dev-102/queries.native.dev.jsonl")
compat = admitted("intake/public-dev-102/queries.compat.dev.jsonl")
old_native = read("intake/full-100/queries.native.dev.jsonl")
old_compat = read("intake/full-100/queries.compat.dev.jsonl")
assert native.startswith(old_native) and compat.startswith(old_compat)
assert admitted("relations.json") == read("relations.json")
new_lines = native[len(old_native):].splitlines(keepends=True)
new_rows = [json.loads(line) for line in new_lines]
new_compat = {r["id"]: r for r in map(json.loads, compat[len(old_compat):].splitlines())}
assert len(new_rows) == len(new_compat) == 2
source_lock = json.loads(admitted("provenance/source-lock.json"))
sources = {item["path"]: admitted("source/" + item["path"]) for item in source_lock["files"]}
for path, raw in sources.items():
    assert raw == read("source/" + path)
decisions = []
for ordinal, (row, line) in enumerate(zip(new_rows, new_lines), 1):
    a = row["annotations"]["v19"]
    p = a["author_provenance"]
    assert row["split"] == "dev" and not row["no_answer"]
    assert a["source_sha"] == p["source_sha"] == SOURCE
    assert a["author_id"] != REVIEWER and a["reviewer_id"] is None
    assert not p["retrieval_inspected"]
    key = a["global_family"]
    assert key == row["query_family"]
    assert int.from_bytes(hashlib.sha256(("codecortex-public-v19-split-v1\n" + key).encode()).digest()[:8], "big") >= 2**62
    evidence = p["source_evidence"]
    for e in evidence:
        raw = sources[e["path"]]
        segment = raw[e["byte_start"]:e["byte_end"]]
        assert segment.decode() == e["text"] and sha(segment) == e["sha256"]
        assert b"".join(raw.splitlines(keepends=True)[e["line_start"] - 1:e["line_end"]]) == segment
    assert {f["group_id"] for f in a["facets"] if f["required"]} == {g["id"] for g in row["answers"]}
    for group in row["answers"]:
        for alt in group["alternatives"]:
            assert any(alt["path"] == e["path"] and alt["symbol"]["name"] == e["symbol"] and alt["span"] == {"start": e["byte_start"], "end": e["byte_end"]} for e in evidence)
    assert new_compat[row["id"]]["expected_files"] == list(dict.fromkeys(alt["path"] for g in row["answers"] for alt in g["alternatives"]))
    assert new_compat[row["id"]]["query"] == row["query"]
    codes = [] if ordinal == 1 else ["R_PRIMARY_GROUP_NOT_ANSWER"]
    decisions.append({"row_sha256": sha(line), "family_id_sha256": sha(row["query_family"].encode()), "component_id_sha256": sha(key.encode()), "source_evidence_sha256": [e["sha256"] for e in evidence], "decision": "needschange" if codes else "accept", "error_codes": codes, "source_fact_review": "accept", "source_span_and_hash": "accept", "compat_projection": "accept", "primary_designation": "needschange" if codes else "accept", "current_visible_dev_independence": "accept", "global_cross_repository_and_blocked_holdout_independence": "not_run_scope_not_supplied"})
report = {"author_current_sha": DELTA, "previous_author_sha": AUTHOR, "allowlist_entry_sha256": ENTRY_HASH, "reviewer_id": REVIEWER, "author_id": "B/express", "source_sha": SOURCE, "native_dev_file_sha256": sha(native), "compat_dev_file_sha256": sha(compat), "old_native_prefix_sha256": sha(old_native), "old_compat_prefix_sha256": sha(old_compat), "old_gold_and_relations_byte_identical": True, "counts": {"new_public_dev_reviewed": 2, "accept": 1, "needschange": 1, "reject": 0, "local_components_reviewed": 2, "holdout_body_reads": 0, "historical_candidate_body_reads": 0, "rank_runs": 0}, "rows": decisions, "components": [{"component_id_sha256": d["component_id_sha256"], "member_family_sha256": [d["family_id_sha256"]], "decision": d["decision"], "error_codes": d["error_codes"], "scope": "visible_dev_local_only"} for d in decisions], "error_code_counts": {"R_PRIMARY_GROUP_NOT_ANSWER": 1}}
raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
path = HERE / "dev-002-delta-review.json"
if "--check" in sys.argv:
    assert path.read_text() == raw
else:
    path.write_text(raw)
print(json.dumps({"counts": report["counts"], "error_code_counts": report["error_code_counts"], "receipt_sha256": sha(raw.encode())}, sort_keys=True))

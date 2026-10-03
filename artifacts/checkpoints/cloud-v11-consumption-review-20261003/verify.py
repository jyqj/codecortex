#!/usr/bin/env python3
"""Verify frozen review provenance and independent literal/numeric receipts."""
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
receipt = json.loads((HERE / "receipt.json").read_text())
for relative, expected in receipt["file_sha256"].items():
    assert hashlib.sha256((ROOT / relative).read_bytes()).hexdigest() == expected, relative
for path, expected in receipt["subject_file_sha256"].items():
    raw = subprocess.check_output(["git", "show", f'{receipt["subject_sha"]}:{path}'], cwd=ROOT)
    assert hashlib.sha256(raw).hexdigest() == expected, path
rows = []
for p in (HERE / "raw").glob("*-immutable-domains.json"):
    data = json.loads(p.read_text())["rows"]
    fields = [r for r in data if "field" in r]
    assert len(fields) == 50 and len({r["field"] for r in fields}) == 50
    assert sum(r["score_or_output_changed"] for r in fields) == 13
    assert all(r["fresh_equal"] and r["old_domain_reused"] for r in fields)
    assert [r["actual_preselected_count"] for r in data if "tier" in r] == [120, 120, 120, 150, 200]
    rows.append(p.name)
assert len(rows) == 2, rows
analytic = json.loads((HERE / "raw/analytic-graph.json").read_text())
assert {h["file_path"] for h in analytic["warm"]} == {"a.py", "b.py", "z.py"}
for hit in analytic["warm"]:
    bill = dict(hit["score_trace"])
    assert abs(bill["boost:symbol-exact"] - 2.18) < 1e-12
    assert abs(sum(v for _, v in hit["score_trace"]) - hit["rerank_score"]) < 1e-12
final = json.loads((HERE / "raw/final-envelope.json").read_text())
before, after = final["generation_before"], final["generation_after"]
for key in ["incarnation", "index_epoch", "evidence_epoch"]:
    assert before[key] == after[key]
assert before["semantic_epoch"] != after["semantic_epoch"]
for warm, cold in [("warm", "fresh"), ("dense", "dense_fresh")]:
    a, b = final[warm], final[cold]
    assert a["machine_pack"]["hits"] == b["machine_pack"]["hits"]
    assert a["rendered_prompt"] == b["rendered_prompt"]
    assert a["evidence_summary"]["selection"] == b["evidence_summary"]["selection"]
    assert {h["file_path"] for h in a["machine_pack"]["hits"]} == {"a.py", "b.py", "z.py"}
assert final["selected"]["machine_pack"]["hits"][0]["file_path"] == "z.py"
assert final["budget_large_bytes"] == 13510 and final["budget_small_bytes"] == 8038
small = final["budget_small"]
assert small["evidence_summary"]["packing"]["limit_bytes"] == 8192
assert small["evidence_summary"]["packing"]["used_bytes"] == 8038
assert small["token_estimate"] == (8038 + 3) // 4
assert small["evidence_summary"]["packing"]["omitted_hits"] == 3
print("PASS: subject/file hashes, two original profile receipts, literal/analytic oracles, final warm/reopen comparison and real budget receipts")

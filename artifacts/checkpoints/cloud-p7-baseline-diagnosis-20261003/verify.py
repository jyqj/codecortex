#!/usr/bin/env python3
"""Read-only frozen diagnosis/current-run evidence consistency, not quality acceptance."""
import pathlib,json,hashlib
HERE=pathlib.Path(__file__).resolve().parent
ROOT=HERE.parents[2]
for name,digest in json.loads((HERE/'artifact-manifest.json').read_text()).items():
 assert hashlib.sha256((HERE/name).read_bytes()).hexdigest()==digest,name
r=json.loads((HERE/'receipt.json').read_text());s=json.loads((HERE/'diagnosis/summary.json').read_text())
assert r['current_V11']['total_passed']==44 and r['current_V11']['total_failed']==0
assert sum(t['passed'] for t in r['current_V11']['targets'])==31 and len(r['current_V11']['targets'])==10
rows=json.loads((HERE/'diagnosis/rows.json').read_text());assert len(rows)==164
assert s['permitted_partial_partition']=={'packing_and_lane':33,'packing_only':21,'lane_only':9}
assert s['actual_source_hit_byte_and_line_audits']==279 and s['invalid_source_bytes_or_spans']==0
assert s['complete_positive_recall_miss_question_ids']==['I04','I08','I12','I16','S02','S04','S06']
assert len(json.loads((HERE/'diagnosis/R09-invalid-material.json').read_text()))==3
s11=json.loads((HERE/'diagnosis/S11-complete-absence.json').read_text());assert len(s11)==3 and all(x['same_fixture_positive_S01_rows']==3 for x in s11)
for row in rows:
 assert hashlib.sha256((ROOT/row['raw_path']).read_bytes()).hexdigest()==row['raw_sha256']
budget=[json.loads(line) for line in (HERE/'p7-raw-budget-check-exact.jsonl').read_text().splitlines()]
assert len(budget)==164
for b in budget:
 assert b['actual_compact_bytes']==b['used_bytes'] and b['used_bytes']<=b['limit_bytes']
 assert b['token_estimate']==(b['used_bytes']+3)//4
print('PASS: frozen164 row classifications,279source audits,164budget checks,44current V11 passes; not a V19 quality certificate')

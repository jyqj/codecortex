"""Independent delta checks against immutable original reviewer evidence."""
import hashlib
import json
import subprocess
from pathlib import Path

D = Path(__file__).resolve().parent
P = Path('artifacts/controls/query-target-independent-20261004').resolve()
F = P / 'delta-v2'
for line in (F / 'frozen.sha256').read_text().splitlines():
    digest, name = line.split('  ', 1)
    assert hashlib.sha256((F / name).read_bytes()).hexdigest() == digest
old = json.loads((P / 'after/results.json').read_text()) + json.loads((P / 'supplement/after.json').read_text())
base = json.loads((P / 'before/results.json').read_text()) + json.loads((P / 'supplement/before.json').read_text())
new = json.loads((D / 'results.json').read_text())
assert len(old) == len(base) == len(new) == 70
counts = dict(controls=70, verified_sources=0, traces=0, common_hits_nonbonus_preserved=0, bonus_only_changes=0, exact_identity_preserved=0, hint_eligibility=0, baseline_restored_rows=0, unchanged_rows=0, max_trace_error=0.0)
packing, metadata, summaries, historical_packing = [], [], [], []
def hits(row, api):
    return row[api]['raw']['machine_pack']['hits']
def nonbonus(h):
    return [v for v in h['score_trace'] if v[0] != 'boost:symbol-exact']
def hasbonus(h):
    return any(k == 'boost:symbol-exact' for k, v in h['score_trace'])
for a, b, n in zip(old, base, new):
    assert a['id'] == b['id'] == n['id'] and a['query'] == n['query']
    e, m = hits(n, 'engine'), hits(n, 'mcp')
    length = min(len(e), len(m))
    assert n['engine']['normalized'][:length] == n['mcp']['normalized'][:length]
    for x, y in zip(e[:length], m[:length]):
        assert {k:v for k,v in x.items() if k != 'metadata'} == {k:v for k,v in y.items() if k != 'metadata'}
        if x.get('metadata') != y.get('metadata'): metadata.append(n['id'])
    if len(e) != len(m): packing.append(dict(id=n['id'], engine=len(e), mcp=len(m)))
    for api in ['engine', 'mcp']:
        for h in n[api]['normalized']:
            assert h['evidence_valid'] is True
            counts['verified_sources'] += 1
        for h in hits(n, api):
            err = abs(sum(v for k,v in h['score_trace']) - h['rerank_score'])
            assert err <= 1e-9 * max(1, abs(h['rerank_score']))
            counts['max_trace_error'] = max(counts['max_trace_error'], err)
            counts['traces'] += 1
            assert hasbonus(h) == ('symbol-exact' in h['reasons'])
            if n['expected'] != 'fallback' and 'exact-target' not in h['reasons']:
                desired = False
                if n['expected'].startswith('named:'):
                    _, name, kind = n['expected'].split(':')
                    desired = (h.get('symbol_name') or '').lower() == name and (kind == 'any' or h.get('symbol_kind') == kind)
                assert hasbonus(h) == desired, (n['id'], h.get('symbol_name'))
                counts['hint_eligibility'] += 1
        baseline_hits = {h['chunk_id']:h for h in hits(b, api)}
        x = {h['chunk_id']:h for h in hits(a, api)}
        y = {h['chunk_id']:h for h in hits(n, api)}
        for cid in x.keys() & y.keys():
            p, q = x[cid], y[cid]
            assert nonbonus(p) == nonbonus(q), (n['id'], cid)
            for field in ['symbol_name','symbol_kind','text','file_path','language','start_line','end_line','fused_score','lexical_score','grep_score','graph_score','source']:
                assert p.get(field) == q.get(field), (n['id'], field)
            assert ('exact-target' in p['reasons']) == ('exact-target' in q['reasons'])
            if 'exact-target' in p['reasons']:
                baseline_hit = baseline_hits.get(cid)
                assert baseline_hit is None or not hasbonus(baseline_hit) or hasbonus(q)
                counts['exact_identity_preserved'] += 1
            counts['common_hits_nonbonus_preserved'] += 1
            if p['score_trace'] != q['score_trace']: counts['bonus_only_changes'] += 1
        reference = b if n['expected'] == 'fallback' else a
        u, v = hits(n, api), hits(reference, api)
        prefix = min(len(u), len(v))
        assert u[:prefix] == v[:prefix], (n['id'], 'historical retained prefix')
        if len(u) != len(v): historical_packing.append(dict(id=n['id'],api=api,fixed=len(u),reference=len(v),comparison='base' if reference is b else 'v1'))
        if n['id'] in ['filename-multi-dot','filename-unindexed-extension']:
            assert u == v, (n['id'], 'counterexample exact restoration')
    if n['expected'] == 'fallback': counts['baseline_restored_rows'] += 1
    else: counts['unchanged_rows'] += 1
    if n['id'] in ['filename-multi-dot','filename-unindexed-extension']:
        summaries.append(dict(id=n['id'], query=n['query'], old=[(h.get('symbol_name'),h['rerank_score']) for h in hits(a,'engine')], fixed=[(h.get('symbol_name'),h['rerank_score']) for h in hits(n,'engine')]))
by_id = {r['id']:r for r in new}
assert hits(by_id['filename-multi-dot'],'engine')[0]['symbol_name'] == 'Beacon'
assert hits(by_id['filename-unindexed-extension'],'engine')[0]['rerank_score'] == 0.599123464984373
# Original controls remain byte-for-byte; their known failures stay recorded.
assert subprocess.check_output(['git','diff','56fd54e','--','artifacts/controls/query-target-20261004','artifacts/controls/query-target-independent-20261004/after','artifacts/controls/query-target-independent-20261004/before']) == b''
original = P.parent / 'query-target-20261004'
orig = {r['id']:r for r in json.loads((original/'after/results.json').read_text())}
assert hits(orig['ambiguous-prose'],'engine')[0]['symbol_name'] == 'Lantern'
assert not any(h.get('symbol_name')=='Ready' for h in hits(orig['qualified-field'],'engine'))
report = dict(fixed_source='e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696', verdict='PASS bounded P2 repair; no additional defect found', counts=counts, packing_differences=packing, historical_packing_differences=historical_packing, metadata_compaction_differences=metadata, counterexamples=summaries, public_metrics='FAIL; broad-prose unresolved; no rerun')
(D/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))

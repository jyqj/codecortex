"""Bounded review assertions; no gold, scorer, public query, or production edits."""
import hashlib
import json
from pathlib import Path

P = Path(__file__).resolve().parent
for directory in [P, P / 'supplement']:
    for line in (directory / 'frozen.sha256').read_text().splitlines():
        digest, name = line.split('  ', 1)
        assert hashlib.sha256((directory / name).read_bytes()).hexdigest() == digest
# Retain the author's old failed prose and absent property chunk, without rerun.
original = P.parent / 'query-target-20261004'
for line in (original / 'frozen.sha256').read_text().splitlines():
    digest, name = line.split('  ', 1)
    assert hashlib.sha256((original / name).read_bytes()).hexdigest() == digest
original_before = {r['id']: r for r in json.loads((original / 'before/results.json').read_text())}
original_after = {r['id']: r for r in json.loads((original / 'after/results.json').read_text())}
for api in ['engine', 'mcp']:
    old_hits = original_before['ambiguous-prose'][api]['raw']['machine_pack']['hits']
    new_hits = original_after['ambiguous-prose'][api]['raw']['machine_pack']['hits']
    assert old_hits == new_hits and new_hits[0]['symbol_name'] == 'Lantern'
    assert not any(h.get('symbol_name') == 'Ready' for h in original_after['qualified-field'][api]['raw']['machine_pack']['hits'])
pairs = [
    (json.loads((P / 'before/results.json').read_text()), json.loads((P / 'after/results.json').read_text())),
    (json.loads((P / 'supplement/before.json').read_text()), json.loads((P / 'supplement/after.json').read_text())),
]
checks = dict(controls=0, source_verified=0, score_traces=0, common_hits=0,
              changed_bonus_only=0, fallback_rows_identical=0, eligibility_checks=0,
              exact_tier_checks=0, max_trace_error=0.0)
packing = []
summary = []
def hits(row, api='engine'):
    return row[api]['raw']['machine_pack']['hits']
def bonus(hit):
    return any(k == 'boost:symbol-exact' for k, v in hit['score_trace'])
def nonbonus(hit):
    return [(k, v) for k, v in hit['score_trace'] if k != 'boost:symbol-exact']
for before, after in pairs:
    assert len(before) == len(after)
    for old, new in zip(before, after):
        assert old['id'] == new['id'] and old['query'] == new['query']
        checks['controls'] += 1
        for arm in [old, new]:
            e, m = hits(arm), hits(arm, 'mcp')
            n = min(len(e), len(m))
            assert e[:n] == m[:n], arm['id']
            assert arm['engine']['normalized'][:n] == arm['mcp']['normalized'][:n], arm['id']
            if len(e) != len(m):
                packing.append(dict(id=arm['id'], arm='before' if arm is old else 'after', engine=len(e), mcp=len(m)))
            for api in ['engine', 'mcp']:
                for h in arm[api]['normalized']:
                    assert h['evidence_valid'] is True
                    checks['source_verified'] += 1
                for h in hits(arm, api):
                    error = abs(sum(v for k, v in h['score_trace']) - h['rerank_score'])
                    checks['max_trace_error'] = max(checks['max_trace_error'], error)
                    assert error <= 1e-9 * max(1, abs(h['rerank_score']))
                    assert ('symbol-exact' in h['reasons']) == bonus(h)
                    assert all(v == 0.18 for k, v in h['score_trace'] if k == 'boost:symbol-exact')
                    checks['score_traces'] += 1
        for api in ['engine', 'mcp']:
            a = {h['chunk_id']: h for h in hits(old, api)}
            b = {h['chunk_id']: h for h in hits(new, api)}
            for cid in a.keys() & b.keys():
                x, y = a[cid], b[cid]
                assert nonbonus(x) == nonbonus(y), (old['id'], api, cid)
                for field in ['symbol_name', 'symbol_kind', 'text', 'file_path', 'language', 'start_line', 'end_line', 'fused_score', 'lexical_score', 'grep_score', 'graph_score', 'source']:
                    assert x.get(field) == y.get(field), (old['id'], field)
                assert ('exact-target' in x['reasons']) == ('exact-target' in y['reasons'])
                checks['common_hits'] += 1
                if x['score_trace'] != y['score_trace']:
                    checks['changed_bonus_only'] += 1
                if 'exact-target' in x['reasons']:
                    # Exemption preserves old eligibility; Named may grant a new bonus.
                    assert not bonus(x) or bonus(y), (old['id'], cid)
                    checks['exact_tier_checks'] += 1
            if old['expected'] == 'fallback':
                assert hits(old, api) == hits(new, api), old['id']
            for h in hits(new, api):
                model = new['expected']
                if model == 'fallback' or model.endswith('-ambiguous') or 'exact-target' in h['reasons']:
                    continue
                desired = False
                if model.startswith('named:'):
                    _, name, kind = model.split(':')
                    desired = (h.get('symbol_name') or '').lower() == name and (kind == 'any' or h.get('symbol_kind') == kind)
                assert desired == bonus(h), (new['id'], h.get('symbol_name'))
                checks['eligibility_checks'] += 1
        if old['expected'] == 'fallback':
            checks['fallback_rows_identical'] += 1
        summary.append(dict(id=new['id'], query=new['query'], before=[(h.get('symbol_name'), h.get('symbol_kind'), h['rerank_score'], bonus(h)) for h in hits(old)], after=[(h.get('symbol_name'), h.get('symbol_kind'), h['rerank_score'], bonus(h)) for h in hits(new)]))
all_after = {r['id']: r for pair in pairs for r in pair[1]}
for id, name, kind in [('direct-type','Beacon','class'), ('direct-callable','pulse','method'), ('direct-function','pulse','function'), ('same-name-type','pulse','class')]:
    assert hits(all_after[id])[0]['symbol_name'] == name and hits(all_after[id])[0]['symbol_kind'] == kind
assert len([h for h in hits(all_after['rust-class']) if h.get('symbol_name') == 'Vessel' and bonus(h)]) > 1
assert any(h['file_path'] == 'beacon.py' and h.get('symbol_kind') == 'method' for h in hits(all_after['qualified-unrelated']))
assert all(not bonus(h) for h in hits(all_after['context-explicit']) if h.get('symbol_name') == 'Beacon')
assert any(bonus(h) for h in hits(all_after['context-ambiguous']) if h.get('symbol_name') == 'Beacon')
# A retained counterexample, not an expectation changed to bless the implementation.
assert hits(all_after['filename-multi-dot'])[0]['symbol_name'] == 'py'
assert next(r for r in summary if r['id'] == 'filename-multi-dot')['before'][0][0] == 'Beacon'
report = dict(checks=checks, packing_differences=packing, original_public_metrics='FAIL; not rerun', rows=summary)
(P / 'comparison.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
print(json.dumps(dict(checks=checks, packing_differences=packing), indent=2))

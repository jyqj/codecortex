import hashlib, json, pathlib
p = pathlib.Path(__file__).resolve().parent
for line in (p / 'frozen.sha256').read_text().splitlines():
    digest, name = line.split('  ', 1)
    assert hashlib.sha256((p / name).read_bytes()).hexdigest() == digest, name
before = json.loads((p / 'before/results.json').read_text())
after = json.loads((p / 'after/results.json').read_text())
trace_count = common_count = changed_count = model_count = 0
max_trace_error = 0.0
transport_differences = []
summary = []
def hits(row, api):
    return row[api]['raw']['machine_pack']['hits']
def without_bonus(hit):
    return [x for x in hit['score_trace'] if x[0] != 'boost:symbol-exact']
def projection(hit):
    return [hit.get('symbol_name'), hit.get('symbol_kind'), hit['rerank_score']]
for old, new in zip(before, after):
    assert old['id'] == new['id']
    for arm in [old, new]:
        e, m = arm['engine']['normalized'], arm['mcp']['normalized']
        n = min(len(e), len(m))
        assert e[:n] == m[:n], arm['id']
        assert hits(arm, 'engine')[:n] == hits(arm, 'mcp')[:n], arm['id']
        if len(e) != len(m):
            transport_differences.append(dict(arm='before' if arm is old else 'after', id=arm['id'], engine=len(e), mcp=len(m), reason='existing packing omission; retained prefix identical'))
        for api in ['engine', 'mcp']:
            for h in hits(arm, api):
                trace_count += 1
                error = abs(sum(x[1] for x in h['score_trace']) - h['rerank_score'])
                max_trace_error = max(max_trace_error, error)
                assert error <= 1e-9 * max(abs(h['rerank_score']), 1), (old['id'], api)
            assert all(h['evidence_valid'] for h in arm[api]['normalized'])
    for api in ['engine', 'mcp']:
        for h in hits(new, api):
            model = new['expected_model']
            if model == 'fallback' or 'exact-target' in h['reasons']:
                continue
            expected = False
            if model.startswith('named:'):
                _, name, kind = model.split(':')
                expected = (h.get('symbol_name') or '').lower() == name.lower() and (kind == 'any' or h.get('symbol_kind') == kind)
            actual = any(key == 'boost:symbol-exact' for key, _ in h['score_trace'])
            assert actual == expected, (new['id'], h['chunk_id'], model)
            model_count += 1
    changes = []
    for api in ['engine', 'mcp']:
        a = {h['chunk_id']: h for h in hits(old, api)}
        b = {h['chunk_id']: h for h in hits(new, api)}
        for cid in sorted(a.keys() & b.keys()):
            common_count += 1
            h, k = a[cid], b[cid]
            assert without_bonus(h) == without_bonus(k), (old['id'], cid)
            for key in ['symbol_name','symbol_kind','text','file_path','start_line','end_line','fused_score']:
                assert h.get(key) == k.get(key), (old['id'],cid,key)
            if h['score_trace'] != k['score_trace']:
                changed_count += 1
                if api == 'engine':
                    changes.append(dict(chunk_id=cid, name=k.get('symbol_name'), kind=k.get('symbol_kind'), before=h['rerank_score'], after=k['rerank_score']))
        if old['expected_model'] == 'fallback':
            assert hits(old, api) == hits(new, api), old['id']
    summary.append(dict(id=old['id'], query=old['query'], before=projection(hits(old,'engine')[0]) if hits(old,'engine') else None,
                        after=projection(hits(new,'engine')[0]) if hits(new,'engine') else None,
                        retained_hits_changed_only_name_bonus=changes))
# Predeclared explicit same-name kind distinction and exact DSL tier controls.
by_id = {r['id']: r for r in after}
assert hits(by_id['direct-callable'],'engine')[0]['symbol_kind'] == 'method'
assert hits(by_id['same-name-type'],'engine')[0]['symbol_kind'] == 'class'
assert all('exact-target' in h['reasons'] for h in hits(by_id['dsl-kind'],'engine'))
assert len([h for h in hits(by_id['split-type'],'engine') if h.get('symbol_name') == 'Reservoir']) > 1
report = dict(controls=len(before), normalized_engine_mcp_retained_prefix_equal=True, transport_packing_differences=transport_differences, all_source_verified=True,
              trace_totals_checked=trace_count, max_trace_error=max_trace_error, common_hits_nonbonus_and_identity_unchanged=common_count,
              changed_hit_traces=changed_count, frozen_model_eligibility_checks=model_count, public_metrics='FAIL; no public rerun or independent verification', rows=summary)
(p/'comparison.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k!='rows'}, indent=2))

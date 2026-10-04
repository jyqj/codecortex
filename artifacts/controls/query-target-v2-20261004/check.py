import hashlib, json, pathlib, subprocess
p = pathlib.Path(__file__).resolve().parent
for root in [p, p.parent/'query-target-20261004']:
    for line in (root/'frozen.sha256').read_text().splitlines():
        digest, name = line.split('  ', 1)
        assert hashlib.sha256((root/name).read_bytes()).hexdigest() == digest
# Historical author's entire evidence directory is unchanged, not just the freeze.
assert not subprocess.check_output(['git','diff','c1aa660','--','artifacts/controls/query-target-20261004'])
counts = dict(controls=0, trace_checks=0, common_hits=0, changed_name_traces=0, explicit_unchanged_controls=0)
max_error = 0.0
packing = []
metadata_compactions = []
summary = []
def hits(row, api): return row[api]['raw']['machine_pack']['hits']
def no_bonus(hit): return [x for x in hit['score_trace'] if x[0] != 'boost:symbol-exact']
def top(row, api):
    h = hits(row, api)
    return [h[0].get('symbol_name'),h[0].get('symbol_kind'),h[0]['rerank_score']] if h else None
for group in ['', 'reviewer']:
    root = p/group
    before = json.loads((root/'before/results.json').read_text())
    after = json.loads((root/'after/results.json').read_text())
    for old, new in zip(before, after, strict=True):
        assert old['id'] == new['id']
        counts['controls'] += 1
        changes = []
        for label, arm in [('before',old),('after',new)]:
            e, m = arm['engine']['normalized'], arm['mcp']['normalized']
            n = min(len(e),len(m))
            assert e[:n] == m[:n]
            for eh, mh in zip(hits(arm,'engine')[:n], hits(arm,'mcp')[:n]):
                assert {k:v for k,v in eh.items() if k != 'metadata'} == {k:v for k,v in mh.items() if k != 'metadata'}
                if eh.get('metadata') != mh.get('metadata'):
                    metadata_compactions.append(dict(group=group,arm=label,id=old['id'],chunk_id=eh['chunk_id'],engine=eh.get('metadata'),mcp=mh.get('metadata')))
            if len(e) != len(m): packing.append(dict(group=group,arm=label,id=old['id'],engine=len(e),mcp=len(m)))
            for api in ['engine','mcp']:
                assert all(h['evidence_valid'] for h in arm[api]['normalized'])
                for h in hits(arm,api):
                    counts['trace_checks'] += 1
                    error = abs(sum(x[1] for x in h['score_trace'])-h['rerank_score'])
                    max_error = max(max_error,error)
                    assert error <= 1e-9 * max(1,abs(h['rerank_score']))
        for api in ['engine','mcp']:
            a = {h['chunk_id']:h for h in hits(old,api)}
            b = {h['chunk_id']:h for h in hits(new,api)}
            for cid in a.keys() & b.keys():
                h,k = a[cid],b[cid]
                counts['common_hits'] += 1
                assert no_bonus(h) == no_bonus(k), (old['id'],cid)
                for key in ['symbol_name','symbol_kind','file_path','text','start_line','end_line','fused_score']:
                    assert h.get(key) == k.get(key), (old['id'],cid,key)
                if h['score_trace'] != k['score_trace']:
                    counts['changed_name_traces'] += 1
                    if api == 'engine':changes.append(dict(name=k.get('symbol_name'),kind=k.get('symbol_kind'),before=h['rerank_score'],after=k['rerank_score']))
            model = new['expected_model']
            if model != 'fallback':
                assert hits(old,api) == hits(new,api), (old['id'],api)
                for h in hits(new,api):
                    if 'exact-target' in h['reasons']:continue
                    expected = False
                    if model.startswith('named:'):
                        _,name,kind = model.split(':')
                        expected = (h.get('symbol_name') or '').lower() == name.lower() and (kind == 'any' or h.get('symbol_kind') == kind)
                    assert any(key=='boost:symbol-exact' for key,_ in h['score_trace']) == expected
            if old['id'] in ['ambiguous-prose','ambiguous-pair','fallback-sentence','ambiguous-qualified-prose']:
                assert hits(old,api) == hits(new,api)
        if new['expected_model'] != 'fallback':counts['explicit_unchanged_controls'] += 1
        summary.append(dict(group=group,id=old['id'],query=old['query'],expected_model=new['expected_model'],before=top(old,'engine'),after=top(new,'engine'),name_bonus_changes=changes))
    if group == 'reviewer':
        by_id = {x['id']:x for x in after}
        assert top(by_id['review-beacon-filename'],'engine')[:2] == ['Beacon','class']
        archived = json.loads(subprocess.check_output(['git','show','56fd54e:artifacts/controls/query-target-independent-20261004/supplement/before.json']))
        # Reviewer baseline (37dd042) is independently archived; both repaired
        # filename controls must restore its retained raw hits and score traces.
        for id in ['review-beacon-filename','review-vessel-filename']:
            row = by_id[id]
            original = next(x for x in archived if x['query'] == row['query'])
            for api in ['engine','mcp']:
                assert hits(row,api) == hits(original,api), (id,api,'reviewer baseline restoration')
report = dict(**counts,max_trace_error=max_error,packing_differences=packing,metadata_compactions=metadata_compactions,
              original_v1_artifacts_unchanged=True,reviewer_two_filename_baselines_restored=True,
              public_metrics='FAIL; broad prose unresolved; no formal rerun',rows=summary)
(p/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k!='rows'},indent=2))

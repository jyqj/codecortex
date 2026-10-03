"""Read-only verification of the bounded Go identity repair evidence."""
import hashlib
import json
from pathlib import Path
root = Path(__file__).resolve().parent
repo = root.parents[2]
r = json.loads((root/'receipt.json').read_text())
for item in r['files']:
    f = repo/item['path']
    assert hashlib.sha256(f.read_bytes()).hexdigest() == item['sha256'], str(f)
for lock in json.loads((root/'public-source-lock.json').read_text()):
    assert hashlib.sha256((root/'public-source'/lock['path']).read_bytes()).hexdigest() == lock['sha256']
for mode, source in [('fixed', 'fixed-probe-source.rs'), ('old', 'probe.rs')]:
    build = json.loads((root/mode/'build-receipt.json').read_text())
    assert build['probe_source_sha256'] == hashlib.sha256((root/source).read_bytes()).hexdigest()
old = {v['label']:v for v in json.loads((root/'old/results.json').read_text())}
fixed = {v['label']:v for v in json.loads((root/'fixed/results.json').read_text())}
assert len(old)==3 and len(fixed)==6
for label, previous in old.items():
    current = fixed[label]
    assert not previous['index_ok'] and 'conflicting duplicate' in previous['index_error']
    assert current['source_sha256']==previous['source_sha256']
    assert (current['calls'],current['refs'])==(previous['calls'],previous['refs'])
for current in fixed.values():
    assert current['index_ok'] and current['duplicate_id_groups']==0
    assert current['all_spans_exact_callee_tokens'] and current['parse_deterministic']
assert old['locked_gin_context']['duplicate_id_groups']==13
assert fixed['locked_gin_context']['calls']==fixed['locked_gin_context']['refs']==256
assert r['go_tests']['passed']==15 and r['go_tests']['failed']==0
assert r['cache_version_invalidation']=='required_main_integration_not_changed_here'
print(json.dumps({'status':'passed','old_negative_index_cases':3,'fixed_index_cases':6,'Gin_calls_preserved':256,'Gin_conflict_groups_before':13,'Gin_conflict_groups_after':0,'cache_migration_certified':False}))

#!/usr/bin/env python3
"""Inspect retained results and source bytes only. Never import or execute probe.py."""
import hashlib
import json
from pathlib import Path
import statistics

B = Path(__file__).resolve().parent
P = B / 'extracted/sql-cost'
result = json.loads((P / 'probe-result.json').read_bytes())
published = json.loads((P / 'scoped-performance-risk-review.json').read_bytes())
assert result['source_before'] == result['source_after']
assert len(result['cases']) == 4
G = Path('/dev/shm/todo-audit-H-dependency-exclusion-v3/candidate/crates/cc-db/src/resolution_dependency_store.rs')
gbytes = G.read_bytes()
assert hashlib.sha256(gbytes).hexdigest() == 'e2dc1d0a90190ddc7ea942fa5310956ffe8b2f0636d7b644c21b6b0a6043003e'
assert hashlib.sha1(b'blob ' + str(len(gbytes)).encode() + b'\0' + gbytes).hexdigest() == '6a875ce6ef7b82aa4bdc3183a59e7887c227825f'
M = B / 'extracted/candidate-M7/crates/cc-db/src/resolution_dependency_store.rs'
before = b'AND file_path NOT IN (SELECT value FROM json_each(?{}))'
after = b'AND (file_path IS NULL OR file_path COLLATE BINARY NOT IN (SELECT value FROM json_each(?{})))'
assert gbytes.count(before) == 1 and gbytes.replace(before, after) == M.read_bytes()
schema = Path('/dev/shm/todo-audit-H-dependency-exclusion/original/crates/cc-db/src/sql/index_v1.sql').read_bytes()
assert hashlib.sha256(schema).hexdigest() == '6d7ee2193f388281aeef3dc98b914f0fa6f9478dda90833bf41f6bc12350cd06'
for statement in result['ddl']: assert statement.encode() in schema
cases = []
for case, summary in zip(result['cases'], published['probe_summary']):
    assert case['name'] == summary['name']
    variants = {}
    for name, data in case['variants'].items():
        observations = data['observations']
        assert len(observations) == 20
        assert all(x['output'] == case['output'] for x in observations)
        query = statistics.median(x['query_fetch_ns'] for x in observations)
        boundary = statistics.median(x['python_boundary_ns'] for x in observations)
        assert query == data['query_fetch_median_ns'] and boundary == data['boundary_median_ns']
        for key in ('statements', 'returned_rows', 'vm_progress_callbacks', 'query_fetch_median_ns', 'boundary_median_ns'):
            assert data[key] == summary['variants'][name][key]
        variants[name] = {key: data[key] for key in ('statements', 'returned_rows', 'vm_progress_callbacks', 'query_fetch_median_ns', 'boundary_median_ns')}
        variants[name]['observations'] = len(observations)
        variants[name]['EXPLAIN_selected_ops'] = [x for x in data['explain'] if x[1] in ('Once','OpenEphemeral','VFilter','IdxInsert','VNext','VOpen')]
    ratio = variants['candidate']['query_fetch_median_ns'] / variants['old']['query_fetch_median_ns']
    assert ratio == case['candidate_to_old_query_wall_ratio'] == summary['candidate_to_old_query_wall_ratio']
    cases.append({key: case[key] for key in ('name','family','keys','excluded_count','excluded_json_bytes','cap','output','output_parity')} | {'variants': variants, 'query_ratio': ratio})
out = {'source_checkpoint': '80d7979da10ae242ed4706cd04be74c62194261a', 'actual_G4': 'cc177ddc9e6b8180b8d847539787672e7b689a1a',
    'G4_production_byte_comparison': 'M7 resolution file is exactly G4 plus the one NULL/BINARY predicate replacement; all other bytes equal.',
    'python': result['python'], 'sqlite': result['sqlite'], 'cases': cases,
    'warm_samples': {'per_variant_per_case': 20, 'paired_iterations_per_case': 20, 'timed_execute_calls_total': 160, 'explanation': 'Four cases × two variants × 20 observations, plus one warmup and a separate VM-count pass per variant. This is not a registered N30 study.'},
    'probe_executed_here': False, 'sqlite_executed_here': False, 'native_executed_here': False,
    'DDL_statements_equal_retained_canonical_schema': True}
target = B / 'retained-probe-readback-facts.json'
with target.open('x') as f: json.dump(out, f, indent=2); f.write('\n')
print(json.dumps({'bytes': target.stat().st_size, 'sha256': hashlib.sha256(target.read_bytes()).hexdigest(), 'cases_recomputed_from_retained_result': 4, 'new_query_measurements': 0}))

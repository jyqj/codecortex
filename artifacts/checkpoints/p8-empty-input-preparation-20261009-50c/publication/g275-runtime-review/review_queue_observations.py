#!/usr/bin/env python3
"""Read-only queue-counter corroboration; does not measure or launch a workload."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--evidence-root', type=Path, default=Path(__file__).resolve().parent)
parser.add_argument('--output', type=Path)
args = parser.parse_args()
root = args.evidence_root.resolve()
output = args.output or root / 'queue-observations.json'
source = '275e8799d4947d297329073eaa3ca675d3fd0777'
results = []
for key in ['c1','c4','c8','c16','soak']:
    directory = root / key / 'extracted/p8-runtime'
    plan = json.loads((directory / 'plan.json').read_text())
    assert plan['source']['source_commit'] == source and plan['queue_capacity'] == 128
    states, offered = [], []
    for line in (directory / 'raw.jsonl').open():
        row = json.loads(line)
        if row['kind'] == 'resources':
            states.append(row['query_execution'])
        elif row['kind'] == 'endpoint_status':
            endpoint = row['response']['query_execution']
            states.append(endpoint)
        elif row['kind'] == 'operation':
            offered.append((row['id'],row['status']))
            if 'cache_probe' in row:
                states.extend(call['response']['diagnostics']['query_execution']
                              for call in row['cache_probe']['requests'] if call['name'] == 'status')
    assert sorted(n for n,status in offered) == list(range(plan['operations']))
    assert all(status == 'success' for n,status in offered)
    maxima = {field:max(row[field] for row in states) for field in states[0]}
    assert all(type(value) is int and value >= 0 for row in states for value in row.values())
    for field, expected in [('cpu_limit',4),('async_limit',8),('queue_limit',32)]:
        assert all(row[field] == expected for row in states)
    assert maxima['rejected'] == 0
    assert maxima['cpu_in_flight'] <= 4 and maxima['async_in_flight'] <= 8
    assert maxima['cpu_admitted'] <= 4 + 32 and maxima['async_admitted'] <= 8 + 32
    assert all(endpoint[field] == 0 for field in ['cpu_in_flight','async_in_flight','cpu_admitted','async_admitted','rejected'])
    with (directory / 'raw.jsonl').open('rb') as stream:
        raw_digest = hashlib.file_digest(stream,'sha256').hexdigest()
    results.append(dict(key=key, status='passed', raw_sha256=raw_digest,
                        original_status_observations=len(states), maximum_observed_query_counters=maxima,
                        original_endpoint_query_counters=endpoint,
                        all_original_offered_operations_successful=len(offered),
                        client_admission_queue_capacity=plan['queue_capacity']))
value = dict(schema_version=1, status='passed', source_commit=source,
             finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
             results=results, scope='all retained status observations and original terminal workload rows; '
                 'not continuous occupancy or transient-peak measurement; no boundedness claim beyond the original observed profile',
             full_task_complete=False, release_certified=False)
with output.open('x') as stream:
    json.dump(value, stream, sort_keys=True, indent=2)
    stream.write('\n')
print(json.dumps(dict(status='passed', cells=len(results), status_observations=sum(r['original_status_observations'] for r in results))))

"""Read-only validation of this review's bounded execution evidence."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parent
repo = root.parents[2]
receipt = json.loads((root / 'receipt.json').read_text())
for item in receipt['files']:
    path = repo / item['path']
    assert hashlib.sha256(path.read_bytes()).hexdigest() == item['sha256'], str(path)
rows = json.loads((root / 'results.json').read_text())
assert {(r['seed'], r['point']) for r in rows} == {
    (s, p) for s in [101, 211, 307]
    for p in ['uncommitted', 'claimed', 'response', 'artifact', 'published']
}
assert len(rows) == len({r['pid'] for r in rows}) == 15
assert len({r['input_digest'] for r in rows}) == 3
for r in rows:
    assert r['signal'] == 9 and r['integrity'] == 'ok'
    if r['point'] == 'uncommitted':
        assert r['transaction_rollback'] and r['prekill_writer_lock_observed'] and r['prekill_reader_invisible']
        continue
    assert r['prekill_reader_verified']
    assert r['unexpired_lease_noop'] == (r['point'] != 'published')
    assert r['total_fake_provider_calls'] == (2 if r['point'] == 'response' else 1)
    assert r['reclaimed'] == int(r['point'] != 'published')
    assert r['replayed'] == int(r['point'] == 'artifact')
    assert r['requeued'] == int(r['point'] in ['claimed', 'response'])
    assert r['reopen_provider_calls'] == int(r['point'] in ['claimed', 'response'])
    assert r['attempt_count'] == {'claimed': 3, 'response': 3, 'artifact': 2, 'published': 1}[r['point']]
    assert r['no_op_replay_rounds'] == 3
assert receipt['status'] == 'bounded_review_prepared_not_P7_016_done'
print(json.dumps({'verified_sigkill_children': 15, 'noop_scans': 36, 'response_before_cache_total_calls_per_case': 2, 'status': 'passed', 'P7_016_done': False}))

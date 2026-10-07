"""Read-only validation of this review's execution and preserved original evidence."""
import hashlib
import json
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent
repo = root.parents[2]
receipt = json.loads((root / 'receipt.json').read_text())
for item in receipt['files']:
    p = repo / item['path']
    assert hashlib.sha256(p.read_bytes()).hexdigest() == item['sha256'], str(p)
for item in receipt['preserved_files']:
    p = repo / item['path']
    assert subprocess.check_output(['git', 'show', receipt['review_target'] + ':' + item['path']], cwd=repo) == p.read_bytes()
    assert hashlib.sha256(p.read_bytes()).hexdigest() == item['sha256']
rows = json.loads((root / 'results.json').read_text())
assert len(rows) == 15
assert {(r['seed'], r['point']) for r in rows} == {
    (s, p) for s in [113, 223, 337]
    for p in ['staging-writing', 'staging-built', 'staging-swapped', 'gc-held', 'gc-committed']
}
pids = [p for r in rows for p in r['killed_pids']]
assert len(pids) == len(set(pids)) == 21
for r in rows:
    assert r['signal'] == 9 and r['integrity'] == 'ok'
    assert r['parent_prekill_observed'] and r['foreign_keys_ok']
    assert r['reuse_provider_calls'] == 0
    if r['point'].startswith('staging'):
        assert r['total_fake_calls'] == 1 and r['final_spaces'] == 1
        assert (r['old_incarnation'] != r['new_incarnation']) == (r['point'] == 'staging-swapped')
    else:
        assert r['total_fake_calls'] == 2 and r['final_manifest'] == 2 and r['active_spaces'] == 1
        assert r['orphan_control_deleted'] and r['postrestart_gc_deleted'] == 0
        assert r['gc']['deleted_objects'] == 1 and r['gc']['kept_fresh'] == 0
        assert r['gc']['kept_live_task'] == int(r['point'] == 'gc-held')
        assert r['gc']['kept_referenced'] == (1 if r['point'] == 'gc-held' else 2)
assert receipt['status'] == 'bounded_review_prepared_not_P7_016_done'
print(json.dumps({'status': 'passed', 'scenes': 15, 'owned_SIGKILL': 21, 'internal_swap_or_mark_unlink_coverage': False, 'generic_pin_coverage': False, 'P7_016_done': False}))

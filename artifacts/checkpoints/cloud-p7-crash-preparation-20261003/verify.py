"""Read-only verifier of frozen preparation evidence; does not rerun or mutate it."""
import hashlib
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent
repo = root.parents[2]
sha256 = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
manifest = json.loads((root / 'artifact-manifest.json').read_text())
for row in manifest:
    assert sha256(root / row['path']) == row['sha256'], row['path']
r = json.loads((root / 'receipt.json').read_text())
source = 'crates/cc-semantic/tests/p7_crash_preparation.rs'
frozen = subprocess.check_output(['git', 'show', r['source_sha'] + ':' + source], cwd=repo)
assert hashlib.sha256(frozen).hexdigest() == sha256(repo / source)
rows = json.loads((root / 'results.json').read_text())
assert len(rows) == 12
assert len({row['pid'] for row in rows}) == 12
assert {(row['seed'], row['point']) for row in rows} == {
    (seed, point) for seed in (17, 29, 43)
    for point in ('uncommitted', 'claimed', 'artifact', 'published')
}
assert len({row['input_digest'] for row in rows}) == 3
for row in rows:
    assert row['signal'] == 9 and row['integrity'] == 'ok'
    if row['point'] == 'uncommitted':
        assert row['transaction_rollback'] is True
        continue
    assert row['total_fake_provider_calls'] == 1
    assert row['no_op_replay_rounds'] == 3
    assert row['attempt_count'] == {'claimed': 3, 'artifact': 2, 'published': 1}[row['point']]
    assert row['replayed'] == int(row['point'] == 'artifact')
    assert row['reclaimed'] == int(row['point'] != 'published')
    assert row['requeued'] == int(row['point'] == 'claimed')
    assert row['reopen_provider_calls'] == int(row['point'] == 'claimed')
assert r['regression']['passed'] == 19 and r['regression']['failed'] == 0
assert r['status'] == 'prepared_independent_not_task_acceptance'
print(json.dumps({'status': 'passed', 'verified_artifact_hashes': len(manifest),
                  'actual_sigkill': 12, 'independent_acceptance': False}))

#!/usr/bin/env python3
"""Receive new complete shards sequentially with the frozen original helper."""
from datetime import datetime, timezone
from pathlib import Path
import json
import os
import subprocess
import sys
import time
import run_original_build_init as original

ROOT = Path(__file__).resolve().parent

def write(path, value):
    with path.open('x') as f:
        f.write(json.dumps(value, sort_keys=True, indent=2) + '\n')

def main():
    plan_path = Path(sys.argv[1]).resolve()
    plan = json.loads(plan_path.read_bytes())
    assert plan['source'] == '4fe927488d5cb7f26bd27c7624745fb1b6ca202b'
    assert plan['run_id'] == 37962416564 and plan['attempt'] == 1
    ids = plan['artifact_ids']
    assert ids and len(ids) == len(set(ids))
    out = ROOT / plan['output']
    assert out.parent == ROOT
    assert all(not (ROOT / 'validated/shards' / str(i)).exists() for i in ids)
    prior_paths = sorted((ROOT / 'validated/shards').glob('*/review.json'))
    prior = [json.loads(p.read_bytes()) for p in prior_paths]
    assert all(r['status'] == 'passed_original_validate_shard' for r in prior)
    def snapshot():
        s = original.snapshot()
        s['validated_state_sha256'] = original.sha(ROOT / 'validated/state.json')
        s['batch_plan_sha256'] = original.sha(plan_path)
        s['selected_metadata'] = {str(i): original.sha(ROOT / f'official-artifact-{i}.json') for i in ids}
        s['prior_review_hashes'] = {str(p.relative_to(ROOT)): original.sha(p) for p in prior_paths}
        return s
    out.mkdir(exist_ok=False)
    before = snapshot()
    assert before['head'] == plan['source']
    assert before['helper_sha256'] == 'b10728501358eb2a55a043d9a67e18cb6105a19398b84baae3b0689a9b0d0fee'
    write(out / 'inputs-before.json', before)
    started = datetime.now(timezone.utc).isoformat()
    begin = time.monotonic()
    calls = []
    code = 0
    for ident in ids:
        argv = ['python3', '-B', str(original.HELPER), 'shard', '--state', str(ROOT / 'validated'), '--archive', str(ROOT / 'original-zips' / f'artifact-{ident}.zip'), '--metadata', str(ROOT / f'official-artifact-{ident}.json')]
        stamp = datetime.now(timezone.utc).isoformat()
        tick = time.monotonic()
        r = subprocess.run(argv, cwd=ROOT, env={**os.environ, 'GIT_OPTIONAL_LOCKS': '0'}, capture_output=True)
        (out / f'{ident}.stdout').write_bytes(r.stdout)
        (out / f'{ident}.stderr').write_bytes(r.stderr)
        review_path = ROOT / 'validated/shards' / str(ident) / 'review.json'
        review = json.loads(review_path.read_bytes()) if review_path.exists() else {}
        call = {'artifact_id': ident, 'argv': argv, 'started_utc': stamp, 'finished_utc': datetime.now(timezone.utc).isoformat(), 'wall_seconds': time.monotonic() - tick, 'original_exit_code': r.returncode, 'status': review.get('status'), 'samples': review.get('sample_count', 0), 'stdout_bytes': len(r.stdout), 'stderr_bytes': len(r.stderr)}
        calls.append(call)
        if r.returncode != 0 or review.get('status') != 'passed_original_validate_shard':
            code = r.returncode or 1
            break
    after = snapshot()
    write(out / 'inputs-after.json', after)
    if before != after:
        code = code or 1
    accepted = len(calls) if code == 0 and len(calls) == len(ids) else 0
    samples = sum(c['samples'] for c in calls) if accepted else 0
    result = {'schema': 'actual-original-full-scale-shard-batch-intake-v1', 'source': before['head'], 'run_id': plan['run_id'], 'attempt': 1, 'started_utc': started, 'finished_utc': datetime.now(timezone.utc).isoformat(), 'wall_seconds': time.monotonic() - begin, 'calls': calls, 'wrapper_exit_code': code, 'inputs_unchanged': before == after, 'newly_accepted_shards': accepted, 'newly_accepted_samples': samples, 'total_accepted_shards': len(prior) + accepted, 'total_accepted_samples': sum(r['sample_count'] for r in prior) + samples, 'registered_shards': 150, 'registered_samples': 1500, 'scope': 'Unchanged original b107 helper once for each new complete original shard; no native replay, failure-prefix admission, partial aggregate or source pooling.', 'newly_completed_todos': 0, 'remaining_todos': 29}
    write(out / 'execution.json', result)
    print(json.dumps(result))
    return code

if __name__ == '__main__':
    raise SystemExit(main())

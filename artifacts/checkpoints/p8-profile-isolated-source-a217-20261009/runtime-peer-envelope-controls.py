"""Finite synthetic review of new profile outer/inner receipt wiring; no binary or workload."""
from pathlib import Path
import copy
import hashlib
import json
import sys
from unittest import mock

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/tests'))
import p8_profile_matrix as profile
import p8_scale_matrix as full
from test_p8_cold_matrix import ColdEnvelopeControls
from test_p8_profile_matrix import fixture, STUDY


def run(profile_name, fanout, mutation=None):
    c = ColdEnvelopeControls()
    c.setUp()
    try:
        (c.build_dir / 'cold-build.json').unlink()
        full.write_new(c.build_dir / 'profile-build.json', {'synthetic': True})
        (c.bundle / 'cold-shard.json').unlink()
        plan, events = fixture(profile_name, fanout)
        environment = c.outer_record['measurement_environment']
        worker_environment = {k: environment[k] for k in profile.ENVIRONMENT_KEYS[1:]}
        worker_environment.update({k: str(c.bundle / '.synthetic-worker') for k in ('TMPDIR','TMP','TEMP')})
        events[0]['profile_environment']['runtime_environment'] = worker_environment
        raw = ''.join(json.dumps(event)+'\n' for event in events).encode()
        (c.native / 'raw.jsonl').write_bytes(raw)
        for p in (c.inner / 'registered-plan.json', c.native / 'plan.json'):
            p.write_bytes(full.json_bytes(plan))
        parsed = profile.inspect_raw(c.native / 'raw.jsonl', plan)
        c.summary.update(stage_scope=profile.SCOPE, sample_count=len(parsed['measurements']), raw_bytes=len(raw),
                         groups=[dict(group=s['group'],samples=1,passed=1,failed_or_not_compared=0) for s in parsed['measurements']])
        (c.native / 'worker-summary.json').write_bytes(full.json_bytes(c.summary))
        c.report['profile_temporary_environment'] = c.report.pop('cold_temporary_environment')
        c.inner_record['plan'] = plan
        c.outer_record.update(schema=profile.SCHEMA, kind='profile-shard', stage_scope=profile.SCOPE,
                              study=copy.deepcopy(STUDY), profile_build_receipt_sha256=full.file_sha256(c.build_dir/'profile-build.json'))
        del c.outer_record['cold_build_receipt_sha256']
        c.outer_build['study'] = copy.deepcopy(STUDY)
        if mutation:
            mutation(c)
        (c.native / 'report.json').write_bytes(full.json_bytes(c.report))
        c.inner_record['files'] = full.inventory(c.inner, ('shard.json',))
        (c.inner / 'shard.json').write_bytes(full.json_bytes(c.inner_record))
        c.outer_record['files'] = full.inventory(c.bundle, ('profile-shard.json',))
        (c.bundle / 'profile-shard.json').write_bytes(full.json_bytes(c.outer_record))
        with mock.patch.object(profile,'driver_snapshot',return_value=c.driver), mock.patch.object(full,'native_digest',return_value='f'*64):
            return profile.validate_shard(c.bundle,c.outer_build,c.built,Path('synthetic-binary'),c.build_dir,c.root)
    finally:
        c.doCleanups()


cases=[]
for name,n in [('batch_1',None),('fanout',16)]:
    accepted=run(name,n)
    assert len(accepted['measurements'])==(1 if n else 2)
    cases.append({'case':name+'-actual-validator-envelope','outcome':'accepted_synthetic'})
for name,mutation in [
    ('failed-terminal',lambda c:c.report.update(status='deadline_exceeded',exit_code=3)),
    ('wrong-run',lambda c:c.outer_record['study'].update(run_id='98765')),
    ('changed-seed-cap',lambda c:c.outer_record['measurement_environment'].update(CODECORTEX_SEED_CACHE_MAX_SYMBOLS='0')),
    ('worker-is-parent',lambda c:c.report['profile_temporary_environment'].update(worker_root=str(c.bundle))),
    ('wrong-build',lambda c:c.outer_record.update(profile_build_receipt_sha256='d'*64)),
]:
    try:
        run('batch_1',None,mutation)
    except ValueError as error:
        cases.append({'case':name,'outcome':'rejected_synthetic','reason':str(error)})
    else:
        raise AssertionError(name+' unexpectedly accepted')
print(json.dumps({'schema':'profile-peer-envelope-controls-v1','scope':'Only synthetic receipts; driver snapshot and native digest mocked, all actual profile/full validators run. No native/binary/old artifact.','cases':cases},indent=2,sort_keys=True))

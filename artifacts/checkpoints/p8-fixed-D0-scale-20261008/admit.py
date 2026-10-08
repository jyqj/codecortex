#!/usr/bin/env python3
"""Admit an independently registered, new, exact-D0 scale study.

The original D0 scale driver owns build/shard/aggregate validation. This
controller only binds the upstream completed study, fixed artifacts and new
150-cell plan. It cannot grant a TODO or release approval.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import urllib.request

SOURCE = 'd0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
SOURCE_RUN = 37846370300
REPOSITORY = 'jyqj/codecortex'
SCALES = [100000, 50000, 10000, 5000, 1000]
HERE = Path(__file__).resolve().parent

def require(condition, message):
    if not condition:
        raise ValueError(message)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def write(path, value):
    raw = (json.dumps(value, indent=2, sort_keys=True) + '\n').encode()
    with path.open('xb') as stream:
        stream.write(raw)

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def load_registration():
    raw = (HERE / 'registration.json').read_bytes()
    require(sha(raw) == os.environ['P8_REGISTRATION_SHA256'], 'registration bytes changed')
    registered = json.loads(raw)
    require(registered['source'] == SOURCE and registered['upstream_run'] == SOURCE_RUN,
            'fixed original source/run differs')
    require(registered['repository'] == REPOSITORY and registered['upstream_attempt'] == 1,
            'fixed repository/upstream attempt differs')
    require(registered['controller_script_sha256'] == sha(Path(__file__).read_bytes()),
            'controller script differs from registered bytes')
    require(registered['new_execution']['scales'] == SCALES
            and registered['new_execution']['indices'] == list(range(30))
            and registered['new_execution']['repetitions'] == 30
            and registered['new_execution']['shard_count'] == 30
            and registered['new_execution']['maximum_concurrent_cells'] == 20,
            'new full matrix definition differs')
    return registered

def metadata(registration, out):
    require(os.environ.get('GITHUB_REPOSITORY') == REPOSITORY, 'different controller repository')
    require(os.environ.get('GITHUB_RUN_ATTEMPT') == '1', 'reruns cannot replace this registered study')
    require(os.environ.get('GITHUB_EVENT_NAME') == 'push', 'unexpected controller event')
    require(re.fullmatch(r'[0-9a-f]{40}', os.environ.get('GITHUB_SHA', '')) is not None,
            'controller commit is not immutable')
    require(os.environ['GITHUB_SHA'] != SOURCE, 'controller and measured source identities conflated')
    token = os.environ['GH_TOKEN']
    require(bool(token), 'missing read-only Actions credential')
    api_root = 'https://api.github.com/repos/' + REPOSITORY
    def api(suffix, name):
        request = urllib.request.Request(api_root + suffix, headers={
            'Accept': 'application/vnd.github+json', 'Authorization': 'Bearer ' + token,
            'X-GitHub-Api-Version': '2022-11-28'})
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read(2 * 1024 * 1024 + 1)
        require(len(raw) <= 2 * 1024 * 1024, 'API response exceeds metadata bound')
        (out / name).open('xb').write(raw)
        return json.loads(raw)
    run = api(f'/actions/runs/{SOURCE_RUN}', 'upstream-run.json')
    require(run['id'] == SOURCE_RUN and run['run_attempt'] == 1
            and run['head_sha'] == SOURCE and run['status'] == 'completed'
            and run['conclusion'] == 'success' and run['event'] == 'push'
            and run['head_branch'] == 'task/p8-prefix-window-engineering-20261009'
            and run['path'] == '.github/workflows/p8-prefix-window-engineering.yml',
            'upstream controls/build/diagnostics are not the registered completed run')
    jobs = api(f'/actions/runs/{SOURCE_RUN}/jobs?per_page=100', 'upstream-jobs.json')
    require(jobs['total_count'] == len(jobs['jobs']) == 3, 'upstream jobs are missing, duplicated or truncated')
    observed = {job['id']: job for job in jobs['jobs']}
    expected = registration['upstream_jobs']
    require(len(observed) == 3 and set(observed) == {job['id'] for job in expected}, 'upstream job identity differs')
    for job in expected:
        actual = observed[job['id']]
        require(actual['name'] == job['name'] and actual['head_sha'] == SOURCE
                and actual['status'] == 'completed' and actual['conclusion'] == 'success',
                'registered upstream job did not complete successfully')
    artifacts = api(f'/actions/runs/{SOURCE_RUN}/artifacts?per_page=100', 'upstream-artifacts.json')
    require(artifacts['total_count'] == len(artifacts['artifacts']), 'artifact metadata is truncated')
    actual_artifacts = {item['id']: item for item in artifacts['artifacts']}
    require(len(actual_artifacts) == len(artifacts['artifacts']), 'duplicate upstream artifact IDs')
    for item in registration['upstream_artifacts']:
        actual = actual_artifacts.get(item['id'])
        require(actual is not None and actual['name'] == item['name']
                and actual['expired'] is False and actual['size_in_bytes'] == item['bytes']
                and actual['digest'] == 'sha256:' + item['sha256'], 'upstream artifact identity differs')
    build = registration['build']
    extras = [item['id'] for item in registration['upstream_artifacts'] if item['id'] != build['artifact_id']]
    require(len(extras) == 5 and len(set(extras)) == 5, 'upstream artifact set is not the original complete six')
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as stream:
        stream.write('build_artifact=' + str(build['artifact_id']) + '\n')
        stream.write('extra_artifacts=' + ','.join(str(item) for item in extras) + '\n')
    return {'status': 'metadata_passed_original_successful_upstream',
            'controller_source': os.environ['GITHUB_SHA'], 'controller_run_id': int(os.environ['GITHUB_RUN_ID']),
            'measured_source': SOURCE, 'upstream_run': SOURCE_RUN,
            'new_measurements_performed': False}

def verify(registration, out):
    root = Path.cwd().resolve(strict=True)
    sys.path.insert(0, str(root / 'scripts'))
    import p8_scale_matrix as original
    require(json.loads((out / 'metadata-receipt.json').read_bytes())['result']['status']
            == 'metadata_passed_original_successful_upstream', 'upstream metadata was not admitted')
    snapshot = original.source_snapshot(root)
    require(snapshot['source_commit'] == SOURCE
            and snapshot['inputs'] == registration['complete_source_inputs'], 'D0 complete source inputs differ')
    require(original.driver_snapshot(root)['inputs'] == registration['scale_observer_inputs'],
            'D0 original scale observer inputs differ')
    expected_plans = []
    for scale in SCALES:
        for index in range(30):
            expected_plans.append({'scale': scale, 'shard_index': index,
                'plan': original.registered_plan(scale, index, 30, 30, 12648430, 18000000, original.CAPACITY_PROFILE)})
    require(expected_plans == registration['primary_cells'], 'registered original 150-cell plan differs')
    require(len({(row['scale'], row['shard_index']) for row in expected_plans}) == 150,
            'registered scale cells are not unique')
    temporary = Path(os.environ['RUNNER_TEMP'])
    build_dir = temporary / 'p8-scale-build'
    build_receipt = build_dir / 'build.json'
    require(sha(build_receipt.read_bytes()) == registration['build']['receipt_sha256'], 'upstream build receipt changed')
    require(sha((build_dir / 'p8-scale').read_bytes()) == registration['build']['binary_sha256'], 'upstream binary changed')
    (build_dir / 'p8-scale').chmod(0o755)
    built, binary = original.validate_build(build_dir, root)
    require(built['source_commit'] == SOURCE, 'original build source differs')
    extra_root = temporary / 'p8-d0-upstream'
    control_dir = extra_root / registration['controls']['artifact_name']
    control_raw = (control_dir / 'receipt.json').read_bytes()
    require(sha(control_raw) == registration['controls']['receipt_sha256'], 'original controls receipt changed')
    controls = json.loads(control_raw)
    require(controls['files'] == original.inventory(control_dir, ('receipt.json',)), 'original controls inventory changed')
    commands = registration['controls']['commands']
    require(len(commands) == 12 and controls['commands'] == commands
            and [row['argv'] for row in controls['results']] == commands
            and all(row['exit_code'] == 0 for row in controls['results'])
            and controls['not_run_command_indices'] == [] and controls['exit_code'] == 0
            and controls['status'] == 'commands_completed' and controls['source_unchanged'] is True,
            'original twelve controls did not complete')
    require(json.loads((control_dir / 'source-before.json').read_bytes()) == snapshot
            and json.loads((control_dir / 'source-after.json').read_bytes()) == snapshot,
            'upstream control source snapshots differ')
    diagnostics = []
    for item in registration['diagnostics']:
        directory = extra_root / item['artifact_name']
        require(sha((directory / 'shard.json').read_bytes()) == item['receipt_sha256'], 'original diagnostic receipt changed')
        validated = original.validate_shard(directory, built, binary, registration['build']['receipt_sha256'])
        require(validated['plan'] == original.registered_plan(item['scale'], 0, 30, 30, 12648430, 18000000, original.CAPACITY_PROFILE),
                'diagnostic plan differs')
        diagnostics.append({'scale': item['scale'], 'receipt_sha256': item['receipt_sha256'],
                            'original_full_raw_validation_passed': True})
    require([item['scale'] for item in diagnostics] == [1000, 10000], 'required original diagnostics missing')
    require(original.source_snapshot(root) == snapshot, 'source changed during original prerequisite replay')
    return {'status': 'original_prerequisites_passed_for_new_registered_study', 'source': SOURCE,
            'original_validate_build_passed': True, 'original_controls': 12, 'diagnostics': diagnostics,
            'new_primary_cells': 150, 'new_measurements_performed': False,
            'upstream_diagnostics_are_extra_not_new_primary_cells': True,
            'original_github_G_and_failed_local_G_studies_unchanged': True,
            'future_P5_full_CI_source_gate_and_release_lock': 'separate_not_granted'}

def main():
    if sys.flags.optimize:
        raise SystemExit('controller verification requires Python without -O')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=['metadata', 'verify'])
    args = parser.parse_args()
    registration = load_registration()
    out = Path(os.environ['RUNNER_TEMP']) / 'p8-d0-admission'
    if args.phase == 'metadata':
        out.mkdir(exist_ok=False)
        for name in ['admit.py', 'registration.json', 'controller-inputs.json']:
            (out / name).open('xb').write((HERE / name).read_bytes())
    else:
        require(out.is_dir() and not out.is_symlink(), 'missing original admission directory')
    result = {'phase': args.phase, 'started_utc': now(), 'status': 'failed', 'source': SOURCE,
              'registration_sha256': sha((HERE / 'registration.json').read_bytes()),
              'controller_sha256': sha(Path(__file__).read_bytes())}
    try:
        result['result'] = (metadata if args.phase == 'metadata' else verify)(registration, out)
        result['status'] = 'passed'
    except Exception as error:
        result['error'] = type(error).__name__ + ': ' + str(error)
    result['finished_utc'] = now()
    write(out / (args.phase + '-receipt.json'), result)
    print(json.dumps({key: result.get(key) for key in ('phase', 'status', 'error')}, sort_keys=True))
    raise SystemExit(0 if result['status'] == 'passed' else 1)

if __name__ == '__main__':
    main()

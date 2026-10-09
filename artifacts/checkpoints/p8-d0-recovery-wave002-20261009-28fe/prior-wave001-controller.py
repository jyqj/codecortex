#!/usr/bin/env python3
"""One registered D0 supplemental wave; never certifies the original study or TODOs."""
import argparse
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

REPO = 'jyqj/codecortex'
SOURCE = 'd0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
ORIGINAL_CONTROLLER = 'ca72a2dde363e35203f7b3cb30ed00729d460fd7'
ORIGINAL_RUN = 37854240827
BUILD_RUN = 37846370300
BUILD_ID = 11582571291
BRANCH = 'task/p8-d0-recovery-28fe-20261009'
WORKFLOW = '.github/workflows/p8-d0-recovery.yml'
HERE = Path(__file__).resolve().parent
CELLS = {(scale, rep) for scale in (1000, 5000, 10000, 50000, 100000) for rep in range(30)}
MEASURE = 'Execute every original phase for this new registered repetition'
UPLOAD = 'Preserve original shard including every unsuccessful measurement'
MAX_JSON = 2 * 1024 * 1024
MAX_LOG = 16 * 1024 * 1024
INPUT_NAMES = {'recovery.py', 'initial-identities.json', 'original-registration.json',
               'predecessor-original-job.log', 'predecessor-original-job.json',
               '37854240827-run.json', '37854240827-jobs1.json', '37854240827-jobs2.json',
               'README.md', 'test_recovery.py'}

def require(condition, message):
    if not condition:
        raise ValueError(message)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def strict_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'duplicate JSON field: ' + key)
            result[key] = value
        return result
    def finite(value):
        number = float(value)
        require(math.isfinite(number), 'nonfinite JSON number')
        return number
    def constant(value):
        raise ValueError('nonfinite JSON constant')
    return json.loads(raw, object_pairs_hook=pairs, parse_float=finite, parse_constant=constant)

def write(path, value):
    with path.open('xb') as stream:
        stream.write((json.dumps(value, sort_keys=True, indent=2) + '\n').encode())

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def key(row):
    require(type(row.get('scale')) is int and type(row.get('repetition')) is int, 'cell coordinates must be integers')
    value = row['scale'], row['repetition']
    require(value in CELLS, 'unknown original cell')
    return value

def initial_map(rows):
    result = {}
    ids = set()
    for row in rows:
        cell = key(row)
        require(cell not in result and type(row.get('job_id')) is int and row['job_id'] > 0 and row['job_id'] not in ids,
                'duplicate initial cell/job')
        require(row['run_id'] == ORIGINAL_RUN and row['run_attempt'] == 1
                and row['controller_source'] == ORIGINAL_CONTROLLER, 'initial lineage changed')
        result[cell] = row
        ids.add(row['job_id'])
    require(set(result) == CELLS, 'all 150 original identities are required including pending cells')
    return result

def check_ledger(initial, attempts, wave):
    """Validate scheduling from a complete immutable lineage; not a raw coverage certificate."""
    original = initial_map(initial)
    groups, seen_jobs = {}, set()
    for item in attempts:
        cell = key(item)
        ordinal = item.get('ordinal')
        require(type(ordinal) is int and 0 <= ordinal <= 2, 'supplemental ordinal exceeds registered bound')
        identity = item.get('run_id'), item.get('run_attempt'), item.get('job_id')
        require(all(type(v) is int and v > 0 for v in identity), 'attempt identity is incomplete')
        require(item['job_id'] not in seen_jobs, 'duplicate globally unique job identity')
        seen_jobs.add(item['job_id'])
        require(ordinal not in groups.setdefault(cell, {}), 'duplicate ordinal / concurrent replacement')
        groups[cell][ordinal] = item
        require(item.get('state') in ('not_started', 'running', 'terminal_unclassified',
                                     'runner_interrupted_unobserved', 'valid_complete', 'invalid'), 'unknown attempt state')
        require(isinstance(item.get('known_errors'), list), 'known error inventory required')
        if ordinal == 0:
            expected = original[cell]
            require(identity == (ORIGINAL_RUN, 1, expected['job_id']) and
                    item['controller_source'] == ORIGINAL_CONTROLLER, 'original attempt replaced')
        else:
            require(item['run_attempt'] == 1 and re.fullmatch('[0-9a-f]{40}', item['controller_source']) is not None
                    and item['controller_source'] != ORIGINAL_CONTROLLER and item['controller_source'] != SOURCE,
                    'supplement controller/source identity conflated')
    require(set(groups) == CELLS and all(0 in rows for rows in groups.values()), 'initial attempt ledger incomplete')
    supplemental_count = sum(len(rows) - 1 for rows in groups.values())
    require(supplemental_count <= 300, 'global cumulative quota exceeded')
    for rows in groups.values():
        require(sorted(rows) == list(range(max(rows) + 1)), 'attempt ordinal gap / hidden prior attempt')
        require(sum(row['state'] in ('not_started', 'running') for row in rows.values()) <= 1,
                'more than one active attempt for a cell')
        for ordinal in range(1, len(rows)):
            predecessor = rows[ordinal - 1]
            require(predecessor['state'] == 'runner_interrupted_unobserved'
                    and not predecessor['known_errors'], 'known failed or completed predecessor cannot be replaced')
    require(len(wave) == 1, 'this controller executes one explicitly registered serial cell only')
    proposed = wave[0]
    cell = key(proposed)
    ordinal = proposed.get('ordinal')
    require(type(ordinal) is int and ordinal in (1, 2), 'unknown supplemental ordinal')
    rows = groups[cell]
    require(max(rows) + 1 == ordinal, 'wave repeats or skips an already allocated attempt')
    require(all(row['state'] == 'runner_interrupted_unobserved' and not row['known_errors'] for row in rows.values()),
            'late error, active, successful or unclassified predecessor blocks supplementation')
    predecessor = rows[ordinal - 1]
    require(proposed['predecessor_run_id'] == predecessor['run_id'] and
            proposed['predecessor_run_attempt'] == predecessor['run_attempt'] and
            proposed['predecessor_job_id'] == predecessor['job_id'], 'wave predecessor identity mismatch')
    return {'initial_cells': 150, 'coverage_required_samples': 1500,
            'observed_attempt_records': len(attempts), 'supplemental_records': supplemental_count,
            'next_ordinal': ordinal, 'coverage_accepted': False,
            'coverage_scope': 'Scheduling only; complete original raw replay and all-attempt readback remain required.'}

def original_job_map(jobs, initial):
    result, seen_jobs, auxiliary = {}, set(), set()
    for job in jobs:
        require(type(job.get('id')) is int and job['id'] > 0 and job['id'] not in seen_jobs, 'duplicate/invalid original job id')
        seen_jobs.add(job['id'])
        require(job['run_id'] == ORIGINAL_RUN and job['run_attempt'] == 1 and job['head_sha'] == ORIGINAL_CONTROLLER, 'original job source/run drift')
        match = re.fullmatch(r'measure \((1000|5000|10000|50000|100000), ([0-9]+)\)', job['name'])
        if not match:
            require(job['name'] not in auxiliary, 'duplicate auxiliary job')
            if job['name'] == 'Receive and replay the fixed original D0 prerequisites':
                require(job['id'] == 113574473097 and job['status'] == 'completed' and job['conclusion'] == 'success', 'original admission job changed')
            else:
                require(job['name'] == 'aggregate', 'unknown original workflow job')
            auxiliary.add(job['name'])
            continue
        cell = int(match[1]), int(match[2])
        require(cell in initial and cell not in result, 'unexpected/duplicate original measurement job')
        require(job['id'] == initial[cell]['job_id'] and job['run_id'] == ORIGINAL_RUN
                and job['run_attempt'] == 1 and job['head_sha'] == ORIGINAL_CONTROLLER, 'original job identity drift')
        result[cell] = job
    require(set(result) == CELLS and 'Receive and replay the fixed original D0 prerequisites' in auxiliary, 'original jobs truncated or missing')
    return result

def validate_artifacts(artifacts):
    names, ids = set(), set()
    fixed = {f'p8-scale-build-{ORIGINAL_RUN}', f'p8-d0-study-admission-{ORIGINAL_RUN}', f'p8-scale-matrix-{ORIGINAL_RUN}'}
    for item in artifacts:
        name = item['name']
        require(type(item.get('id')) is int and item['id'] > 0 and item['id'] not in ids and name not in names,
                'duplicate artifact identity/name')
        ids.add(item['id']); names.add(name)
        if name not in fixed:
            match = re.fullmatch(r'p8-scale-(capacity|shard)-(1000|5000|10000|50000|100000)-([0-9]+)-' + str(ORIGINAL_RUN), name)
            require(match is not None and (int(match[2]), int(match[3])) in CELLS, 'unknown original artifact')
        require(item['workflow_run']['id'] == ORIGINAL_RUN and item['workflow_run']['head_sha'] == ORIGINAL_CONTROLLER,
                'original artifact source/run differs')
        require(item['expired'] is False and type(item.get('size_in_bytes')) is int and item['size_in_bytes'] >= 0 and
                re.fullmatch(r'sha256:[0-9a-f]{64}', item.get('digest', '')) is not None, 'unavailable or malformed original artifact')
    require(f'p8-scale-build-{ORIGINAL_RUN}' in names and f'p8-d0-study-admission-{ORIGINAL_RUN}' in names,
            'original successful admission/build artifacts missing')
    return names

def interruption(job, raw_log, artifacts, cell):
    """An observed shutdown, NOT proof of a platform-only cause or absent native errors."""
    require(job['status'] == 'completed' and job['conclusion'] == 'failure', 'predecessor is not a completed failed job')
    steps = {step['name']: step for step in job['steps']}
    require(len(steps) == len(job['steps']), 'duplicate step name')
    require(steps[MEASURE]['conclusion'] == 'failure' and steps[UPLOAD]['conclusion'] == 'skipped',
            'not the registered unobserved measurement interruption')
    require(all(step['conclusion'] not in ('failure', 'cancelled', 'timed_out') for step in job['steps']
                if step['name'] != MEASURE), 'another known step failure blocks recovery')
    text = raw_log.decode('utf-8', errors='strict')
    require('Process completed with exit code 143.' in text and
            'The runner has received a shutdown signal.' in text, 'explicit runner shutdown witness absent')
    errors = [line for line in text.splitlines() if '##[error]' in line]
    require(len(errors) == 2 and all('Process completed with exit code 143.' in line or
            'The runner has received a shutdown signal.' in line for line in errors), 'additional original error blocks recovery')
    require(not re.search(r'"(?:status|passed)"\s*:\s*(?:"failed"|false)|deadline_exceeded|parity[_ ](?:mismatch|failed)|source[_ ](?:changed|mismatch)|evidence[_ ](?:exceeded|budget)', text, re.I),
            'native/budget/parity/source/evidence failure cannot be replaced')
    name = f'p8-scale-shard-{cell[0]}-{cell[1]}-{ORIGINAL_RUN}'
    require(not any(item['name'] == name for item in artifacts),
            'late or existing original raw requires independent classification; supplementation revoked')
    return {'state': 'runner_interrupted_unobserved', 'known_errors': [],
            'native_outcome': None, 'root_cause': 'unknown', 'original_raw': 'not_uploaded',
            'observed_job_exit': 143, 'log_sha256': sha(raw_log)}

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

class API:
    def __init__(self, token, output):
        require(bool(token), 'missing read-only token')
        self.token, self.output = token, output
        self.opener = urllib.request.build_opener(NoRedirect)
        self.sequence = 0
    def _get(self, url, authenticated, maximum):
        parsed = urllib.parse.urlsplit(url)
        require(parsed.scheme == 'https' and not parsed.username and not parsed.password
                and parsed.port in (None, 443) and not parsed.fragment and '\\' not in url
                and not any(ord(c) < 33 or ord(c) == 127 for c in url), 'unsafe URL')
        if authenticated:
            require(parsed.hostname == 'api.github.com' and parsed.path.startswith('/repos/' + REPO + '/'),
                    'token may only go to fixed GitHub repository API')
            headers = {'Authorization': 'Bearer ' + self.token, 'Accept': 'application/vnd.github+json',
                       'X-GitHub-Api-Version': '2022-11-28'}
        else:
            require(parsed.hostname is not None and parsed.hostname.endswith('.blob.core.windows.net'),
                    'unapproved log redirect host')
            headers = {}
        with self.opener.open(urllib.request.Request(url, headers=headers), timeout=30) as response:
            raw = response.read(maximum + 1)
        require(len(raw) <= maximum, 'remote evidence exceeds fixed read bound')
        return raw
    def get(self, suffix, maximum=MAX_JSON, log=False):
        require(suffix.startswith('/') and not suffix.startswith('//'), 'invalid API suffix')
        url = 'https://api.github.com/repos/' + REPO + suffix
        try:
            raw = self._get(url, True, maximum)
        except urllib.error.HTTPError as error:
            require(log and error.code in (301, 302, 303, 307, 308), 'API request failed or unexpected redirect')
            location = error.headers.get('Location', '')
            decoded = location
            for _ in range(8):
                require(self.token not in decoded, 'credential in redirect URL')
                next_value = urllib.parse.unquote(decoded)
                if next_value == decoded:
                    break
                decoded = next_value
            else:
                raise ValueError('excessive redirect encoding')
            raw = self._get(location, False, maximum)
        self.sequence += 1
        filename = f'api-{self.sequence:03d}.log' if log else f'api-{self.sequence:03d}.json'
        with (self.output / filename).open('xb') as stream:
            stream.write(raw)
        write(self.output / (filename + '.identity.json'), {'api_path': suffix, 'bytes': len(raw), 'sha256': sha(raw)})
        return raw if log else strict_json(raw)
    def pages(self, suffix, field):
        result, total = [], None
        for page in range(1, 21):
            value = self.get(suffix + ('&' if '?' in suffix else '?') + f'per_page=100&page={page}')
            require(type(value.get('total_count')) is int and 0 <= value['total_count'] <= 2000, 'unbounded or invalid API total')
            if total is None:
                total = value['total_count']
            require(value['total_count'] == total, 'API inventory changed while paginating; no partial admission')
            rows = value[field]
            require(isinstance(rows, list) and len(rows) <= 100, 'invalid API page')
            result.extend(rows)
            if len(result) >= total:
                break
            require(len(rows) == 100, 'truncated API page')
        require(len(result) == total and len({row['id'] for row in result}) == total, 'incomplete/duplicate API inventory')
        return result

def load_package(controller_root):
    raw = (HERE / 'registration.json').read_bytes()
    require(sha(raw) == os.environ['P8_RECOVERY_REGISTRATION_SHA256'], 'recovery registration drift')
    reg = strict_json(raw)
    require(reg['schema'] == 'p8-D0-continuation-wave-v1' and reg['source'] == SOURCE and
            reg['original_run'] == ORIGINAL_RUN and reg['original_controller'] == ORIGINAL_CONTROLLER,
            'fixed lineage differs')
    require(reg['supplements_per_cell'] == 2 and reg['global_supplement_limit'] == 300 and
            reg['new_max_parallel'] == 1 and reg['original_max_parallel'] == 20,
            'registered prospective quotas/scheduling changed')
    require(reg['prior_supplemental_attempts'] == [] and reg['prior_supplemental_attempt_count'] == 0,
            'first-wave controller cannot omit or replace a prior supplemental lineage')
    require(reg['wave_id'] == 'wave-001' and reg['wave'] == [{'scale': 100000, 'repetition': 8, 'ordinal': 1,
            'predecessor_run_id': ORIGINAL_RUN, 'predecessor_run_attempt': 1, 'predecessor_job_id': 113580044384}],
            'only explicitly reviewed first wave is implemented')
    require(set(reg['controller_inputs']) == INPUT_NAMES, 'controller input set changed')
    for name, expected in reg['controller_inputs'].items():
        require(re.fullmatch(r'[A-Za-z0-9_.-]+', name) is not None and name not in ('.', '..'), 'unsafe controller input path')
        path = HERE / name
        require(path.is_file() and not path.is_symlink() and sha(path.read_bytes()) == expected, 'controller input drift: ' + name)
    workflow = (controller_root / WORKFLOW).read_bytes()
    normalized = workflow.replace(sha(raw).encode(), b'__REGISTRATION_SHA256__')
    require(workflow.count(sha(raw).encode()) == 1 and sha(normalized) == reg['workflow_template_sha256'], 'workflow drift')
    initial = strict_json((HERE / 'initial-identities.json').read_bytes())['cells']
    initial_map(initial)
    old = strict_json((HERE / 'original-registration.json').read_bytes())
    require(old['source'] == SOURCE and old['build']['artifact_id'] == BUILD_ID and old['upstream_run'] == BUILD_RUN,
            'original source/build registration differs')
    return reg, initial, old

def validate_original_run(original_run):
    require(original_run['id'] == ORIGINAL_RUN and original_run['head_sha'] == ORIGINAL_CONTROLLER and original_run['run_attempt'] == 1
            and original_run['event'] == 'push' and original_run['head_branch'] == 'task/p8-fixed-d0-scale-20261008'
            and original_run['path'] == '.github/workflows/p8-fixed-d0-scale.yml'
            and original_run['repository']['full_name'] == REPO and original_run['head_repository']['full_name'] == REPO,
            'original study was retried or changed; prior history requires review')


def observe(reg, initial, old, api, check_controller_runs=True):
    require(os.environ.get('GITHUB_REPOSITORY') == REPO and os.environ.get('GITHUB_REF_NAME') == BRANCH and
            os.environ.get('GITHUB_EVENT_NAME') == 'push' and os.environ.get('GITHUB_RUN_ATTEMPT') == '1',
            'wrong repository/branch/event or unregistered workflow retry')
    controller = os.environ['GITHUB_SHA']
    require(re.fullmatch('[0-9a-f]{40}', controller) is not None and controller not in (SOURCE, ORIGINAL_CONTROLLER), 'invalid controller identity')
    run_id = int(os.environ['GITHUB_RUN_ID'])
    if check_controller_runs:
        runs = api.pages('/actions/workflows/p8-d0-recovery.yml/runs?branch=' + urllib.parse.quote(BRANCH, safe=''), 'workflow_runs')
        require(len(runs) == 1 and runs[0]['id'] == run_id and runs[0]['head_sha'] == controller
                and runs[0]['run_attempt'] == 1 and runs[0]['event'] == 'push',
                'unregistered extra controller run/attempt; wave cannot be replayed')
    original_run = api.get(f'/actions/runs/{ORIGINAL_RUN}')
    validate_original_run(original_run)
    jobs = api.pages(f'/actions/runs/{ORIGINAL_RUN}/attempts/1/jobs', 'jobs')
    mapped = original_job_map(jobs, initial_map(initial))
    artifacts = api.pages(f'/actions/runs/{ORIGINAL_RUN}/artifacts', 'artifacts')
    validate_artifacts(artifacts)
    target = reg['wave'][0]
    cell = key(target)
    job = mapped[cell]
    raw = api.get(f'/actions/jobs/{job["id"]}/logs', maximum=MAX_LOG, log=True)
    require(sha(raw) == reg['predecessor_log_sha256'], 'late original log changed; reclassification required')
    observed = interruption(job, raw, artifacts, cell)
    attempts = []
    for coords, current in sorted(mapped.items()):
        state = {'queued': 'not_started', 'in_progress': 'running', 'completed': 'terminal_unclassified'}.get(current['status'])
        require(state is not None, 'unknown original job status')
        row = {'scale': coords[0], 'repetition': coords[1], 'ordinal': 0, 'run_id': ORIGINAL_RUN,
               'run_attempt': 1, 'job_id': current['id'], 'controller_source': ORIGINAL_CONTROLLER,
               'state': state, 'known_errors': [], 'official_status': current['status'],
               'official_conclusion': current['conclusion'], 'native_validation': 'not_observed', 'log_observed': False}
        if current['status'] == 'completed' and current['conclusion'] != 'success':
            row['known_errors'] = ['unclassified_official_failure']
            row['native_outcome'] = None
        if coords == cell:
            row.update(observed)
            row['log_observed'] = True
        attempts.append(row)
    ledger_result = check_ledger(initial, attempts, reg['wave'])
    upstream = api.get(f'/actions/runs/{BUILD_RUN}')
    require(upstream['id'] == BUILD_RUN and upstream['head_sha'] == SOURCE and upstream['run_attempt'] == 1
            and upstream['status'] == 'completed' and upstream['conclusion'] == 'success',
            'original build study no longer matches successful fixed upstream')
    build = api.get(f'/actions/artifacts/{BUILD_ID}')
    expected = next(item for item in old['upstream_artifacts'] if item['id'] == BUILD_ID)
    require(build['id'] == BUILD_ID and build['name'] == expected['name'] and build['expired'] is False and
            build['size_in_bytes'] == expected['bytes'] and build['digest'] == 'sha256:' + expected['sha256'] and
            build['workflow_run']['id'] == BUILD_RUN and build['workflow_run']['head_sha'] == SOURCE,
            'fixed original build artifact identity changed')
    return {'controller_source': controller, 'controller_run_id': run_id, 'measured_source': SOURCE,
            'wave': reg['wave'], 'all_initial_attempts': attempts, 'ledger': ledger_result,
            'predecessor': observed, 'old_study_verdict': 'failed_or_incomplete_under_original_registration',
            'new_raw_measurements_executed_here': False, 'runtime_or_TODO_acceptance': False}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=('admit', 'recheck', 'execute'))
    parser.add_argument('--controller-root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--admission', type=Path)
    parser.add_argument('--root', type=Path)
    parser.add_argument('--build', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=False, exist_ok=False)
    result = {'schema_version': 1, 'phase': args.phase, 'started_at_utc': now(), 'status': 'failed',
              'coverage_accepted': False, 'original_TODO_completed': 0, 'remaining_TODO': 29}
    code = 2
    try:
        reg, initial, old = load_package(args.controller_root)
        result.update(registration_sha256=sha((HERE / 'registration.json').read_bytes()),
                      controller_script_sha256=sha(Path(__file__).read_bytes()),
                      wave_id=reg['wave_id'], wave=reg['wave'], measured_source=SOURCE)
        observed_head = subprocess.check_output(['git', '-C', str(args.controller_root), 'rev-parse', 'HEAD']).decode().strip()
        require(observed_head == os.environ['GITHUB_SHA'], 'actual controller checkout differs')
        if args.phase in ('admit', 'recheck'):
            result['observation'] = observe(reg, initial, old, API(os.environ.get('GH_TOKEN'), args.output))
            result['status'] = 'admitted_wave_only' if args.phase == 'admit' else 'predecessor_still_eligible'
            code = 0
        else:
            require(args.root is not None and args.build is not None and args.admission is not None, 'missing execution bindings')
            admitted = strict_json((args.admission / 'receipt.json').read_bytes())
            require(admitted['status'] == 'admitted_wave_only' and admitted['observation']['controller_source'] == os.environ['GITHUB_SHA']
                    and admitted['observation']['controller_run_id'] == int(os.environ['GITHUB_RUN_ID'])
                    and admitted['observation']['wave'] == reg['wave'], 'execution does not match admitted wave')
            # Revalidate immediately before starting the only new measurement; retain this second API view.
            result['pre_execution_observation'] = observe(reg, initial, old, API(os.environ.get('GH_TOKEN'), args.output))
            for name, expected in old['scale_observer_inputs'].items():
                require(sha((args.root / name).read_bytes()) == expected, 'original scale observer drift')
            sys.path.insert(0, str(args.root / 'scripts'))
            import p8_scale_matrix as original
            require(Path(original.__file__).resolve() == (args.root / 'scripts/p8_scale_matrix.py').resolve(), 'wrong original validator import')
            snapshot = original.source_snapshot(args.root)
            require(snapshot['source_commit'] == SOURCE and snapshot['inputs'] == old['complete_source_inputs'], 'D0 native source drift')
            require(sha((args.build / 'build.json').read_bytes()) == old['build']['receipt_sha256'] and
                    sha((args.build / 'p8-scale').read_bytes()) == old['build']['binary_sha256'], 'original build/binary drift')
            (args.build / 'p8-scale').chmod(0o755)
            built, binary = original.validate_build(args.build, args.root)
            plan = original.registered_plan(100000, 8, 30, 30, 12648430, 18000000, original.CAPACITY_PROFILE)
            require(plan == next(row['plan'] for row in old['primary_cells'] if row['scale'] == 100000 and row['shard_index'] == 8), 'original cell plan changed')
            shard = args.output / 'shard'
            argv = [sys.executable, '-B', str(args.root / 'scripts/p8_scale_matrix.py'), 'run', '--root', str(args.root),
                    '--build', str(args.build), '--scale', '100000', '--shard-index', '8', '--shard-count', '30',
                    '--repetitions', '30', '--seed', '12648430', '--deadline-ms', '18000000',
                    '--capacity-profile', 'scale_capacity_v1', '--output', str(shard)]
            result.update(argv=argv, original_driver_launch_requested_at_utc=now(), native_measurement_started=None)
            write(args.output / 'started.json', result)
            with (args.output / 'driver.stdout').open('xb') as stdout, (args.output / 'driver.stderr').open('xb') as stderr:
                process = subprocess.run(argv, cwd=args.root, stdout=stdout, stderr=stderr, check=False)
            result['original_driver_returned'] = True
            result['original_driver_exit_code'] = process.returncode
            require(process.returncode == 0, 'original measurement failed; retained and never replaced')
            validated = original.validate_shard(shard, built, binary, old['build']['receipt_sha256'])
            require(validated['plan'] == plan and original.source_snapshot(args.root) == snapshot, 'raw replay/source after mismatch')
            result.update(status='supplemental_raw_validated_pending_external_attempt_readback',
                          original_raw_validation=True, native_measurement_started=True, shard_receipt_sha256=sha((shard / 'shard.json').read_bytes()))
            code = 0
    except (Exception, KeyboardInterrupt) as error:
        # Do not print credential-bearing transport exceptions or signed redirect URLs.
        result['error_type'] = type(error).__name__
        result['error'] = str(error) if isinstance(error, (ValueError, KeyError)) else 'Execution or evidence transport failed; preserved original local evidence.'
    result.update(finished_at_utc=now(), exit_code=code,
                  previous_admission_revoked=(args.phase == 'recheck' and code != 0))
    write(args.output / 'receipt.json', result)
    print(json.dumps({k: result[k] for k in ('phase', 'status', 'exit_code', 'coverage_accepted')}))
    return code

if __name__ == '__main__':
    raise SystemExit(main())

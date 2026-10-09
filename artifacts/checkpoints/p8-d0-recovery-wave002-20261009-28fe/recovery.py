#!/usr/bin/env python3
"""One registered D0 supplemental wave; never certifies the original study or TODOs."""
import argparse
import base64
import io
import stat
import zipfile
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
BRANCH = 'task/p8-d0-recovery-wave002-28fe-20261009'
WORKFLOW = '.github/workflows/p8-d0-recovery.yml'
HERE = Path(__file__).resolve().parent
CELLS = {(scale, rep) for scale in (1000, 5000, 10000, 50000, 100000) for rep in range(30)}
MEASURE = 'Execute every original phase for this new registered repetition'
UPLOAD = 'Preserve original shard including every unsuccessful measurement'
MAX_JSON = 2 * 1024 * 1024
MAX_LOG = 16 * 1024 * 1024
INPUT_NAMES = set(['37854240827-jobs1.json', '37854240827-jobs2.json', '37854240827-run.json', 'README.md', 'current-original-artifacts.json', 'current-original-jobs1.json', 'current-original-jobs2.json', 'fixed-build-artifacts.json', 'initial-identities.json', 'original-capacity-observer.py', 'original-capture001-receipt.json', 'original-registration.json', 'predecessor-admission.json', 'predecessor-original-job.json', 'predecessor-original-job.log', 'prior-receiver-receipt.json', 'prior-wave001-artifacts.json', 'prior-wave001-controller.py', 'prior-wave001-job.log', 'prior-wave001-jobs.json', 'prior-wave001-registration.json', 'prior-wave001-run.json', 'prior-wave001-workflow.yml', 'prior-wave001.zip.base64', 'recovery.py', 'test_recovery.py'])

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

def check_ledger(initial, attempts, wave, evidence):
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
    require(wave == FIXED_WAVE and len(rows) == 2 and
            rows[0]['state'] == 'runner_interrupted_unobserved' and not rows[0]['known_errors'] and
            not rows[0].get('late_evidence_revoked') and capacity_prior_eligible(rows[1], evidence),
            'late error, active, successful, unclassified or unproved capacity predecessor blocks supplementation')
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

POLICY_SHA256 = '20d94bc4798c2a6277fb7782307c868283b581b545460ac12d82ced2ce4163e8'
PRIOR_RECEIVER_SHA256 = 'eccffe7a89abdf3686306ddf0e54bab190fe429abb57861cd5a35b1ea8311d4a'
WAVE001_COMMIT = 'ad840759ba337608d3e33f2c1558042a4d5283c8'
WAVE001_RUN = 37865643378
WAVE001_JOB = 113611597284
WAVE001_BRANCH = 'task/p8-d0-recovery-28fe-20261009'
PREFIX = 'artifacts/checkpoints/p8-d0-recovery-wave002-20261009-28fe'
IDENTITY = ('scale', 'repetition', 'ordinal', 'run_id', 'run_attempt', 'job_id', 'controller_source')
FIXED_WAVE = [{'scale': 100000, 'repetition': 8, 'ordinal': 2,
               'predecessor_run_id': WAVE001_RUN, 'predecessor_run_attempt': 1,
               'predecessor_job_id': WAVE001_JOB}]

def canonical_sha(value):
    return sha(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8'))

def require_policy(evidence):
    raw = (json.dumps(evidence, sort_keys=True, indent=2) + '\n').encode()
    require(sha(raw) == POLICY_SHA256, 'only the independently classified fixed capacity failure is eligible')

def capacity_prior_eligible(row, evidence):
    require_policy(evidence)
    return (all(row.get(k) == evidence['attempt'][k] for k in IDENTITY)
            and row.get('state') == evidence['preserved_state'] == 'invalid'
            and row.get('known_errors') == evidence['observed_errors']
            and row.get('official_status') == 'completed' and row.get('official_conclusion') == 'failure'
            and row.get('artifact_ids') == [evidence['artifact']['id']]
            and row.get('log_sha256') == evidence['official_job_log_sha256']
            and row.get('native_outcome') is None and row.get('selected_for_coverage') is False
            and not row.get('late_evidence_revoked')
            and row.get('control_failure') == evidence
            and row.get('control_failure_currently_verified') is True)

def read_prior_receiver(reg, initial):
    raw = (HERE / 'prior-receiver-receipt.json').read_bytes()
    require(sha(raw) == PRIOR_RECEIVER_SHA256 == reg['prior_receiver_receipt_sha256'], 'prior receiver history changed')
    prior = strict_json(raw)
    require(prior['schema'] == 'p8-D0-continuation-receiver-v1' and prior['source'] == SOURCE and
            prior['coverage_accepted'] is False and prior['runtime_or_TODO_acceptance'] is False,
            'prior failed/incomplete receiver was relabeled')
    require(prior['unexpected_attempts'] == [] and prior['retained_policy_violations'] == [],
            'unregistered or policy-violating prior history requires separate review')
    require(len(prior['attempts']) == 151 and len({row['job_id'] for row in prior['attempts']}) == 151,
            'complete prior 150 initial plus one supplemental attempt required')
    fixed = initial_map(initial)
    originals = [row for row in prior['attempts'] if row['ordinal'] == 0]
    require(len(originals) == 150 and {key(row) for row in originals} == CELLS, 'prior initial denominator changed')
    for row in originals:
        require((row['run_id'], row['run_attempt'], row['job_id'], row['controller_source']) ==
                (ORIGINAL_RUN, 1, fixed[key(row)]['job_id'], ORIGINAL_CONTROLLER), 'prior initial identity drift')
    supplements = [row for row in prior['attempts'] if row['ordinal'] != 0]
    require(supplements == reg['prior_supplemental_attempts'] and reg['prior_supplemental_attempt_count'] == 1,
            'prior supplemental failure removed, replaced or omitted')
    require(all(supplements[0][k] == reg['predecessor_admission']['attempt'][k] for k in IDENTITY), 'wrong prior supplemental identity')
    require(supplements[0]['known_errors'] == reg['predecessor_admission']['observed_errors'] and
            supplements[0]['state'] == 'invalid' and not supplements[0].get('late_evidence_revoked'),
            'preserved predecessor errors/state differ')
    require(prior['registry']['prior_capture_receipts'] == reg['prior_capture_receipts'] ==
            ['993496adfbb7c1237f550a7e439ffc16ca5cd43d40ffdd5834e38d310cb5744d'],
            'original partial capture was removed or silently reconciled')
    require(prior['partial_capture_history']['unreconciled_capture_count'] == 1,
            'this scheduling controller cannot clear the unresolved partial capture')
    return prior

def verify_capacity_failure(evidence, prior_row, job, raw_log, artifacts):
    """A fixed pre-native control failure, retaining invalid and every prior error."""
    require_policy(evidence)
    require(canonical_sha(job) == evidence['official_job_canonical_sha256'],
            'late or different wave001 official job evidence; qualification revoked')
    require(type(raw_log) is bytes and len(raw_log) == evidence['official_job_log_bytes'] and
            sha(raw_log) == evidence['official_job_log_sha256'], 'late wave001 log differs; qualification revoked')
    require(job['id'] == WAVE001_JOB and job['run_id'] == WAVE001_RUN and job['run_attempt'] == 1 and
            job['head_sha'] == WAVE001_COMMIT and job['status'] == 'completed' and job['conclusion'] == 'failure',
            'capacity predecessor identity/status differs')
    steps = {step['number']: step for step in job['steps']}
    require(len(steps) == len(job['steps']) and steps[8]['name'] == 'Prepare the same registered capacity on this disposable VM'
            and steps[8]['conclusion'] == 'failure' and
            steps[9]['name'] == 'Execute exactly the unchanged D0 cell once and replay its raw' and
            steps[9]['status'] == 'completed' and steps[9]['conclusion'] == 'skipped',
            'native execution was not proven skipped at the exact capacity guard')
    require(all(step['conclusion'] == 'success' for number, step in steps.items() if number not in (8, 9)),
            'another known step failure is not covered by this exception')
    require(len(artifacts) == 1, 'late or missing supplemental raw artifact; qualification revoked')
    item = artifacts[0]; expected = evidence['artifact']
    require(item['id'] == expected['id'] and item['name'] == expected['name'] and
            item['size_in_bytes'] == expected['size_in_bytes'] and item['digest'] == expected['digest'] and
            item['workflow_run']['id'] == expected['run_id'] and item['workflow_run']['head_sha'] == expected['controller_source'] and
            item['expired'] is False, 'original capacity ZIP identity changed')
    encoded = (HERE / 'prior-wave001.zip.base64').read_bytes()
    require(len(encoded) <= 90000, 'unbounded original capacity archive encoding')
    raw_zip = base64.b64decode(b''.join(encoded.split()), validate=True)
    require(len(raw_zip) == expected['size_in_bytes'] == 60953 and 'sha256:' + sha(raw_zip) == expected['digest'],
            'original capacity ZIP bytes changed')
    receipts = {}
    with zipfile.ZipFile(io.BytesIO(raw_zip)) as archive:
        members = archive.infolist(); names = [member.filename for member in members]
        require(len(members) == len(set(names)) == 35 and sum(member.file_size for member in members) == 630214,
                'fixed original capacity ZIP inventory changed')
        require(all(name.startswith(('p8-d0-recovery-admission/', 'p8-d0-recovery-capacity/', 'p8-d0-recovery-recheck/'))
                    and '\\' not in name and not name.startswith('/') and
                    all(part not in ('', '.', '..') for part in name.rstrip('/').split('/')) for name in names),
                'unsafe or execution-bearing predecessor ZIP path')
        for member in members:
            require(not stat.S_ISLNK(member.external_attr >> 16) and 0 <= member.file_size <= 2 * 1024**2,
                    'unsafe predecessor ZIP member')
            # Read-only and bounded; consuming every member also checks its real CRC.
            with archive.open(member) as stream: data = stream.read(2 * 1024**2 + 1)
            require(len(data) == member.file_size and len(data) <= 2 * 1024**2, 'predecessor member size differs')
            for label, bound in evidence['receipts'].items():
                if member.filename == bound['archive_member']:
                    require(len(data) == bound['bytes'] and sha(data) == bound['sha256'], 'original predecessor receipt changed')
                    receipts[label] = strict_json(data)
    require(set(receipts) == {'admission', 'capacity', 'recheck'}, 'predecessor receipt set incomplete')
    capacity = receipts['capacity']
    require(capacity['error'] == evidence['capacity_error'] and capacity['exit_code'] == evidence['capacity_exit_code'] == 2
            and capacity['status'] == 'not_run_capacity_unavailable' and capacity['steps'] == [] and capacity['files'] == {}
            and capacity['artifact_state'] == 'sealed_after_commands_finished' and capacity['scale'] == 100000
            and capacity['required_free_bytes'] == evidence['required_free_bytes'] == 30509367296
            and capacity['initial']['free_bytes'] == evidence['observed_initial_free_bytes'] == 88661061632
            and capacity['initial']['free_bytes'] >= capacity['required_free_bytes']
            and capacity['context']['P8_EXPECTED_SOURCE'] == SOURCE
            and capacity['context']['GITHUB_WORKSPACE'] == '/home/runner/work/codecortex/codecortex'
            and 'source_commit' not in capacity, 'failure was not the fixed pre-native checkout identity guard')
    for label, wanted in (('admission', 'admitted_wave_only'), ('recheck', 'predecessor_still_eligible')):
        value = receipts[label]
        require(value['status'] == wanted and value['exit_code'] == 0 and value['coverage_accepted'] is False and
                value['observation']['controller_run_id'] == WAVE001_RUN and
                value['observation']['controller_source'] == WAVE001_COMMIT and
                value['registration_sha256'] == evidence['original_wave_registration_sha256'],
                'original admission or late-evidence recheck failed')
    row = strict_json(json.dumps(prior_row))
    row['control_failure'] = evidence
    row['control_failure_currently_verified'] = True
    row['native_measurement_started'] = False
    require(capacity_prior_eligible(row, evidence), 'prior invalid/error record does not match fixed control failure')
    return row

def fixed_run(run, run_id, controller, branch):
    require(run['id'] == run_id and run['head_sha'] == controller and run['run_attempt'] == 1 and
            run['event'] == 'push' and run['head_branch'] == branch and run['path'] == WORKFLOW and
            run['repository']['full_name'] == REPO and run['head_repository']['full_name'] == REPO,
            'registered supplemental run identity/rerun differs')

def branch_inventory(api, branch, run_id, controller):
    all_runs = api.pages('/actions/runs?branch=' + urllib.parse.quote(branch, safe=''), 'workflow_runs')
    matching = [row for row in all_runs if row.get('path') == WORKFLOW]
    require(len(matching) == 1, 'extra or missing registered workflow run; no duplicate wave is permitted')
    fixed_run(matching[0], run_id, controller, branch)
    return {'branch': branch, 'workflow': WORKFLOW, 'all_runs': all_runs}

def load_package(controller_root):
    require(controller_root.resolve(strict=True) / PREFIX == HERE, 'controller package is outside the fixed checkout path')
    raw = (HERE / 'registration.json').read_bytes()
    require(sha(raw) == os.environ['P8_RECOVERY_REGISTRATION_SHA256'], 'recovery registration drift')
    reg = strict_json(raw)
    require(reg['schema'] == 'p8-D0-continuation-wave-v2' and reg['repository'] == REPO and
            reg['source'] == SOURCE and reg['original_run'] == ORIGINAL_RUN and
            reg['original_controller'] == ORIGINAL_CONTROLLER and reg['branch'] == BRANCH,
            'fixed prospective lineage differs')
    require(reg['supplements_per_cell'] == 2 and reg['global_supplement_limit'] == 300 and
            reg['new_max_parallel'] == 1 and reg['original_max_parallel'] == 20 and
            reg['combined_declared_scheduling_upper_bound'] == 21 and reg['all_initial_cells'] == 150 and
            reg['complete_coverage_required_samples'] == 1500, 'registered prospective quota/coverage changed')
    require(reg['wave_id'] == 'wave-002' and reg['wave'] == FIXED_WAVE and
            reg['coverage_accepted'] is False and reg['original_TODO_closed'] == 0,
            'only this independently registered final ordinal is implemented')
    require(set(reg['controller_inputs']) == INPUT_NAMES, 'controller input set changed')
    for name, expected in reg['controller_inputs'].items():
        require(re.fullmatch(r'[A-Za-z0-9_.-]+', name) is not None and name not in ('.', '..'), 'unsafe controller input path')
        path = HERE / name
        require(path.is_file() and not path.is_symlink() and sha(path.read_bytes()) == expected, 'controller input drift: ' + name)
    require_policy(reg['predecessor_admission'])
    require(sha((HERE / 'predecessor-admission.json').read_bytes()) == POLICY_SHA256 and
            strict_json((HERE / 'predecessor-admission.json').read_bytes()) == reg['predecessor_admission'],
            'new pre-native classification is not the independently reviewed fixed evidence')
    workflow_path = controller_root / WORKFLOW
    require(workflow_path.is_file() and not workflow_path.is_symlink(), 'workflow is not a regular file')
    workflow = workflow_path.read_bytes()
    normalized = workflow.replace(sha(raw).encode(), b'__REGISTRATION_SHA256__')
    require(workflow.count(sha(raw).encode()) == 1 and sha(normalized) == reg['workflow_template_sha256'], 'workflow drift')
    initial = strict_json((HERE / 'initial-identities.json').read_bytes())['cells']
    initial_map(initial)
    old = strict_json((HERE / 'original-registration.json').read_bytes())
    require(old['source'] == SOURCE and old['build']['artifact_id'] == BUILD_ID and old['upstream_run'] == BUILD_RUN,
            'original source/build registration differs')
    require(sha((HERE / 'initial-identities.json').read_bytes()) == 'd52cf254e0941ba6b1a029629dc1792ad2673cf0d1eb35e8b267f347e8f079fe'
            and sha((HERE / 'original-registration.json').read_bytes()) == '4a56e31e9838f0db1d83a3efd4a10d0a51f8b443dbce4413e68253881740eebc',
            'original complete initial lineage or registration changed')
    read_prior_receiver(reg, initial)
    return reg, initial, old

def validate_original_run(original_run):
    require(original_run['id'] == ORIGINAL_RUN and original_run['head_sha'] == ORIGINAL_CONTROLLER and original_run['run_attempt'] == 1
            and original_run['event'] == 'push' and original_run['head_branch'] == 'task/p8-fixed-d0-scale-20261008'
            and original_run['path'] == '.github/workflows/p8-fixed-d0-scale.yml'
            and original_run['repository']['full_name'] == REPO and original_run['head_repository']['full_name'] == REPO,
            'original study was retried or changed; prior history requires review')


def observe(reg, initial, old, api):
    require(os.environ.get('GITHUB_REPOSITORY') == REPO and os.environ.get('GITHUB_REF_NAME') == BRANCH and
            os.environ.get('GITHUB_EVENT_NAME') == 'push' and os.environ.get('GITHUB_RUN_ATTEMPT') == '1',
            'wrong repository/branch/event or unregistered workflow retry')
    controller = os.environ['GITHUB_SHA']
    require(re.fullmatch('[0-9a-f]{40}', controller) is not None and controller not in
            (SOURCE, ORIGINAL_CONTROLLER, WAVE001_COMMIT), 'invalid or reused controller identity')
    run_id = int(os.environ['GITHUB_RUN_ID'])
    prior = read_prior_receiver(reg, initial)
    prior_by_job = {row['job_id']: row for row in prior['attempts']}
    inventory = [branch_inventory(api, WAVE001_BRANCH, WAVE001_RUN, WAVE001_COMMIT),
                 branch_inventory(api, BRANCH, run_id, controller)]
    current_run = api.get(f'/actions/runs/{run_id}')
    fixed_run(current_run, run_id, controller, BRANCH)
    own_jobs = api.pages(f'/actions/runs/{run_id}/attempts/1/jobs', 'jobs')
    require(len(own_jobs) == 1, 'only one explicitly registered supplemental job is permitted')
    own_job = own_jobs[0]
    require(own_job['run_id'] == run_id and own_job['run_attempt'] == 1 and own_job['head_sha'] == controller and
            own_job['name'] == 'D0 100000 repetition 8 supplemental ordinal 2' and
            type(own_job['id']) is int and own_job['id'] > 0 and own_job['id'] not in prior_by_job,
            'current wave job identity repeats or differs')
    current_attempt = {'scale': 100000, 'repetition': 8, 'ordinal': 2, 'run_id': run_id,
                       'run_attempt': 1, 'job_id': own_job['id'], 'controller_source': controller,
                       'official_status': own_job['status'], 'official_conclusion': own_job['conclusion'],
                       'native_measurement_started': None, 'native_outcome': None}
    original_run = api.get(f'/actions/runs/{ORIGINAL_RUN}')
    validate_original_run(original_run)
    jobs = api.pages(f'/actions/runs/{ORIGINAL_RUN}/attempts/1/jobs', 'jobs')
    mapped = original_job_map(jobs, initial_map(initial))
    artifacts = api.pages(f'/actions/runs/{ORIGINAL_RUN}/artifacts', 'artifacts')
    validate_artifacts(artifacts)
    current_artifacts = {item['id']: item for item in artifacts}
    for previous_artifact in prior['artifact_identities']:
        if previous_artifact['run_id'] != ORIGINAL_RUN:
            continue
        item = current_artifacts.get(previous_artifact['id'])
        require(item is not None and item['name'] == previous_artifact['name'] and
                item['digest'] == previous_artifact['digest'] and item['size_in_bytes'] == previous_artifact['size_in_bytes'] and
                item['workflow_run']['head_sha'] == previous_artifact['controller_source'],
                'previously observed original artifact disappeared or changed')
    raw = api.get('/actions/jobs/113580044384/logs', maximum=MAX_LOG, log=True)
    original_log_sha = reg['original_predecessor_log_sha256']
    require(sha(raw) == original_log_sha == 'c33de85feff3143d794ed4d337affdbfa6010979fe32fbb9883579acc80941cf',
            'late original rep8 log changed; supplementation revoked')
    interrupted = interruption(mapped[100000, 8], raw, artifacts, (100000, 8))
    initial_attempts = []
    for coords, current in sorted(mapped.items()):
        previous = prior_by_job[current['id']]
        state = {'queued': 'not_started', 'in_progress': 'running', 'completed': 'terminal_unclassified'}.get(current['status'])
        require(state is not None, 'unknown original job status')
        row = {'scale': coords[0], 'repetition': coords[1], 'ordinal': 0, 'run_id': ORIGINAL_RUN,
               'run_attempt': 1, 'job_id': current['id'], 'controller_source': ORIGINAL_CONTROLLER,
               'state': state, 'known_errors': [], 'official_status': current['status'],
               'official_conclusion': current['conclusion'], 'native_validation': 'not_observed',
               'native_outcome': None, 'native_duration_ms': None, 'log_observed': False,
               'selected_for_coverage': False, 'started_at': current.get('started_at'),
               'completed_at': current.get('completed_at'), 'artifact_ids': list(previous['artifact_ids']),
               'log_sha256': previous.get('log_sha256')}
        if current['status'] == 'completed' and current['conclusion'] != 'success':
            row['known_errors'] = ['unclassified_official_failure']
        if coords == (100000, 8): row.update(interrupted, log_observed=True)
        row['known_errors'] = sorted(set(row['known_errors']) | set(previous['known_errors']))
        if previous.get('late_evidence_revoked'): row['late_evidence_revoked'] = True
        require(not (previous['official_status'] == 'in_progress' and current['status'] == 'queued'),
                'previously running original job regressed to queued')
        if previous['official_status'] == 'completed':
            require(current['status'] == 'completed' and current['conclusion'] == previous['official_conclusion'],
                    'prior terminal official result was rewritten')
        if previous['state'] in ('invalid', 'valid_complete'):
            row['state'] = previous['state']
            row['prior_original_validation'] = previous
        initial_attempts.append(row)
    old_wave = api.get(f'/actions/runs/{WAVE001_RUN}')
    fixed_run(old_wave, WAVE001_RUN, WAVE001_COMMIT, WAVE001_BRANCH)
    require(old_wave['status'] == 'completed' and old_wave['conclusion'] == 'failure', 'old wave is pending or changed')
    predecessor_jobs = api.pages(f'/actions/runs/{WAVE001_RUN}/attempts/1/jobs', 'jobs')
    require(len(predecessor_jobs) == 1, 'extra or missing prior supplemental job')
    predecessor_artifacts = api.pages(f'/actions/runs/{WAVE001_RUN}/artifacts', 'artifacts')
    predecessor_log = api.get(f'/actions/jobs/{WAVE001_JOB}/logs', maximum=MAX_LOG, log=True)
    predecessor = verify_capacity_failure(reg['predecessor_admission'], prior_by_job[WAVE001_JOB],
                                          predecessor_jobs[0], predecessor_log, predecessor_artifacts)
    attempts = initial_attempts + [predecessor]
    ledger_result = check_ledger(initial, attempts, reg['wave'], reg['predecessor_admission'])
    upstream = api.get(f'/actions/runs/{BUILD_RUN}')
    require(upstream['id'] == BUILD_RUN and upstream['head_sha'] == SOURCE and upstream['run_attempt'] == 1 and
            upstream['status'] == 'completed' and upstream['conclusion'] == 'success',
            'original build study no longer matches successful fixed upstream')
    builds = api.pages(f'/actions/runs/{BUILD_RUN}/artifacts', 'artifacts')
    matching = [item for item in builds if item['id'] == BUILD_ID]
    require(len(matching) == 1, 'fixed original build missing or duplicated')
    build = matching[0]
    expected = next(item for item in old['upstream_artifacts'] if item['id'] == BUILD_ID)
    require(build['name'] == expected['name'] and build['expired'] is False and
            build['size_in_bytes'] == expected['bytes'] and build['digest'] == 'sha256:' + expected['sha256'] and
            build['workflow_run']['id'] == BUILD_RUN and build['workflow_run']['head_sha'] == SOURCE,
            'fixed original build artifact identity changed')
    return {'controller_source': controller, 'controller_run_id': run_id, 'measured_source': SOURCE,
            'wave': reg['wave'], 'all_initial_attempts': initial_attempts, 'all_prior_attempts': attempts,
            'prior_supplemental_attempts': [predecessor], 'current_wave_attempt': current_attempt,
            'observed_attempt_records_before_this_wave': len(attempts),
            'observed_attempt_records_including_current_wave': len(attempts) + 1,
            'cumulative_supplemental_attempts_including_current_wave': 2,
            'ledger': ledger_result, 'predecessor': predecessor,
            'original_predecessor': next(row for row in initial_attempts if key(row) == (100000, 8)),
            'prior_receiver_receipt_sha256': PRIOR_RECEIVER_SHA256,
            'prior_partial_capture_receipts': reg['prior_capture_receipts'],
            'partial_reconciliation_performed': False, 'registered_branch_run_inventories': inventory,
            'original_artifact_inventory': artifacts,
            'old_study_verdict': 'failed_or_incomplete_under_original_registration',
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

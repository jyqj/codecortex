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

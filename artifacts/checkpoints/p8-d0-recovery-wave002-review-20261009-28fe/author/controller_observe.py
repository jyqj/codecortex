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

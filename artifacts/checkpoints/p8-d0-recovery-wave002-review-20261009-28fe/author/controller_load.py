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

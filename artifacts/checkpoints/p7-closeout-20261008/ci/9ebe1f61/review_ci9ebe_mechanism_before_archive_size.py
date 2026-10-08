from pathlib import Path, PurePosixPath
from collections import Counter, defaultdict
from datetime import datetime
import copy
import hashlib
import json
import stat
import subprocess
import zipfile

ROOT = Path('/dev/shm/codecortex-closeout-996727/codecortex-root')
OUT = Path('/dev/shm/codecortex-closeout-996727/validation')
ZIP = Path('/workspace/scratch/9967275fe9a7/ci-import/ci9ebe-mechanism-artifact.zip')
PR = '9ebe1f619a298b9955d576d9d8250368259924d2'
BASE = '/home/runner/work/_temp/'
FACTORS = {'none': [], 'local': ['local_retrieval'], 'dense_only': ['semantic_dense'],
           'hybrid': ['local_retrieval', 'semantic_dense']}
LOCAL = {'exact_symbol', 'path', 'lexical', 'grep', 'graph'}

def sha(data):
    return hashlib.sha256(data).hexdigest()

def stream_sha(handle):
    result = hashlib.sha256()
    for block in iter(lambda: handle.read(1024 * 1024), b''):
        result.update(block)
    return result.hexdigest()

def canonical(value):
    return (json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2) + '\n').encode()

def compact(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()

def bootstrap(values, seed=19):
    mask = (1 << 64) - 1
    draws = []
    for _ in range(2000):
        total = 0.0
        for _ in values:
            if seed == 0:
                seed = 0x9e3779b97f4a7c15
            seed ^= (seed << 13) & mask
            seed ^= seed >> 7
            seed ^= (seed << 17) & mask
            total += values[seed % len(values)]
        draws.append(total / len(values))
    draws.sort()
    return {'mean': sum(values) / len(values), 'low': draws[49], 'high': draws[1949],
            'independent_units': len(values)}

with ZIP.open('rb') as handle:
    zip_hash = stream_sha(handle)
assert ZIP.stat().st_size == 91097221
assert zip_hash == '245959ade64c43884d50a5b90a40c2f6e4a37f1d6da83cb78f1ee7cc3d169d60'
with zipfile.ZipFile('/workspace/scratch/9967275fe9a7/ci-import/ci9ebe-engineering-artifact.zip') as eng:
    prior_source = json.loads(eng.read('source-before.json'))
expected_source = prior_source['inputs']
prior_review = json.loads((OUT / 'ci9ebe-engineering-independent-receipt.json').read_bytes())
assert prior_review['source_binding']['associated_pr_head'] == PR
assert prior_review['source_binding']['full_794_crate_cargo_inputs_equal_fixed_pr_git_blobs']

with zipfile.ZipFile(ZIP) as archive:
    read = lambda name: json.loads(archive.read(name))
    lines = lambda name: [json.loads(line) for line in archive.read(name).splitlines() if line]
    names = archive.namelist()
    assert len(names) == len(set(names)) == 4158
    inventory = {}
    for item in archive.infolist():
        name = PurePosixPath(item.filename)
        assert not name.is_absolute() and '..' not in name.parts and not item.is_dir()
        assert stat.S_IFMT(item.external_attr >> 16) != stat.S_IFLNK
        with archive.open(item) as handle:
            inventory[item.filename] = {'bytes': item.file_size, 'sha256': stream_sha(handle)}
    assert sum(row['bytes'] for row in inventory.values()) == 368706824
    controls = json.loads(subprocess.check_output(['git', 'show',
        PR + ':crates/cc-eval/src/benchmark/ablation/mechanism_controls.json'], cwd=ROOT))
    build = read('p7-019-build/mechanism-build.json')
    assert build == read('p7-019-evidence/run/mechanism-build.json')
    assert build['source_commit'] == PR and build['controls'] == controls
    assert {v['id']: v['enabled'] for v in build['variants']} == FACTORS
    reference = read('p7-019-build/reference-inventory.json')
    assert reference == expected_source and len(reference) == 794
    assert {n.removeprefix('p7-019-build/reference/') for n in names if n.startswith('p7-019-build/reference/')} == set(reference)
    assert all(inventory['p7-019-build/reference/' + n]['sha256'] == h for n, h in reference.items())
    source_binding = read('p7-019-build/source-binding.json')
    assert source_binding['source_commit'] == PR and source_binding['build_input_worktree_clean'] is True
    assert source_binding['reference_inventory_sha256'] == sha(json.dumps(reference, sort_keys=True, separators=(',', ':')).encode())

    binaries, sources, targets, receipt_paths, build_options, built = set(), set(), set(), set(), [], []
    for variant in build['variants']:
        cell = variant['id']
        assert variant['source_root'] == BASE + f'p7-019-build/sources/{cell}'
        assert variant['binary'] == BASE + f'p7-019-build/products/{cell}'
        assert variant['build_receipt'] == BASE + f'p7-019-build/builds/{cell}/receipt.json'
        prefix = f'p7-019-build/sources/{cell}/'
        files = {n.removeprefix(prefix) for n in names if n.startswith(prefix)}
        assert files == set(reference)
        changed = []
        predicted = dict(reference)
        for control in controls:
            ref = archive.read('p7-019-build/reference/' + control['path'])
            on, off = control['on_text'].encode(), control['off_text'].encode()
            assert ref.count(on) == 1 and ref.count(off) == 0
            if control['id'] not in variant['enabled']:
                changed.append(control['path'])
                predicted[control['path']] = sha(ref.replace(on, off))
        actual = {name: inventory[prefix + name]['sha256'] for name in sorted(files)}
        receipt = read(f'p7-019-build/builds/{cell}/receipt.json')
        assert actual == predicted == receipt['source_files'] == read(f'p7-019-build/builds/{cell}/source-inventory.json')
        assert receipt['exit_code'] == 0
        binary = inventory[f'p7-019-build/products/{cell}']['sha256']
        assert binary == receipt['binary_sha256']
        command = read(f'p7-019-build/builds/{cell}/command.json')
        options = receipt['build_options']
        assert command['exit_code'] == 0 and command['source_unchanged'] is True
        assert command['cwd'] == variant['source_root']
        assert command['argv'] == options['command']
        assert command['stdout_sha256'] == inventory[f'p7-019-build/builds/{cell}/cargo.stdout.log']['sha256']
        assert command['stderr_sha256'] == inventory[f'p7-019-build/builds/{cell}/cargo.stderr.log']['sha256']
        target = BASE + f'p7-019-build/targets/{cell}'
        environment = command['compiler_environment']
        assert command['target'] == options['CARGO_TARGET_DIR'] == target
        assert all(environment[key] == target for key in ['CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR'])
        assert environment['RUSTC_WRAPPER'] == environment['RUSTC_WORKSPACE_WRAPPER'] == ''
        assert environment['CARGO_INCREMENTAL'] == environment['CARGO_PROFILE_DEV_DEBUG'] == environment['CARGO_PROFILE_TEST_DEBUG'] == '0'
        assert environment['RUSTFLAGS'] == '-D warnings'
        assert options['profile'] == 'dev' and options['features'] == ['semantic-http'] and options['jobs'] == 2
        assert options['binding']['source_commit'] == PR
        assert options['binding']['rustc_executable_sha256'] == 'bff349e72704ff70bc08a234a3847338e797065bbedde5e556808bc87b7bf7c6'
        assert options['binding']['cargo_executable_sha256'] == '841072d1d92f9e841d9ba5b0814182a0adf064acf4527cd120967b7bc49dcb66'
        assert datetime.fromisoformat(command['started_utc']) < datetime.fromisoformat(command['finished_utc'])
        projected = copy.deepcopy(options)
        del projected['CARGO_TARGET_DIR']
        build_options.append(projected)
        binaries.add(binary); sources.add(variant['source_root']); targets.add(target); receipt_paths.add(variant['build_receipt'])
        built.append({'cell': cell, 'source_file_count': len(files), 'source_inventory_sha256': inventory[f'p7-019-build/builds/{cell}/source-inventory.json']['sha256'],
                      'only_controlled_files_changed': sorted(changed), 'binary_sha256': binary,
                      'binary_bytes': inventory[f'p7-019-build/products/{cell}']['bytes'], 'target': target,
                      'compiler_command_exit': command['exit_code'], 'source_commit_binding': options['binding']['source_commit']})
    assert len(binaries) == len(sources) == len(targets) == len(receipt_paths) == 4
    assert all(options == build_options[0] for options in build_options)

    suite = read('p7-019-evidence/suite.json')
    queries = lines('p7-019-evidence/queries.jsonl')
    assert len(queries) == 3 and {q['id'] for q in queries} == {'exact-a', 'exact-b', 'dense-control'}
    assert suite['warmup'] == 1 and suite['repetitions'] == 2 and suite['seed'] == 19 and suite['top_k'] == 1
    assert suite['engine_config']['semantic']['endpoint'].startswith('http://127.0.0.1:')
    comparison = read('p7-019-evidence/run/comparison.json')
    acceptance = read('p7-019-evidence/acceptance.json')
    assert acceptance['source_commit'] == PR and acceptance['passed'] is True
    assert comparison['exit_code'] == 0 and comparison['comparison_error'] is None
    assert comparison['profile'] == acceptance['profile'] == 'fake'
    assert comparison['held_out_semantic_quality_gate'] == acceptance['held_out_semantic_quality_gate'] == 'not_evaluated'
    assert comparison['equal_effective_budgets'] is True
    for control in ['actual_hybrid_mislabel_rejected', 'aliased_binary_rejected', 'aliased_source_rejected',
                    'misattributed_source_commit_rejected', 'paired_bootstrap_replayed',
                    'provider_query_positive_controls_observed', 'same_locked_inputs_and_effective_budgets',
                    'all_36_requests_preserved', 'original_p7_019_fake_engineering_leg_complete']:
        assert acceptance[control] is True
    acceptance_lines = [line.split('P7_019_MECHANISM ', 1)[1] for line in (OUT / 'ci9ebe-mechanism-job.log').read_text().splitlines()
                        if 'P7_019_MECHANISM {' in line]
    assert len(acceptance_lines) == 1 and json.loads(acceptance_lines[0]) == acceptance
    raw_log = archive.read('p7-019-runner/actual-mechanism.log').decode()
    assert '1 passed; 0 failed; 0 ignored' in raw_log
    assert json.loads(next(line.split('P7_019_MECHANISM ', 1)[1] for line in raw_log.splitlines()
                           if 'P7_019_MECHANISM {' in line)) == acceptance

    expected_space = read('p7-019-evidence/plan.json')['readiness']['expected_space']
    all_inputs, all_budgets, input_locks, observed_cells, metric_cases, raw_count = [], [], [], [], {}, 0
    for cell in FACTORS:
        prefix = f'p7-019-evidence/run/{cell}/'
        run_manifest = read(prefix + 'manifest.json')
        assert run_manifest['suite'] == suite and run_manifest['measurement_profile'] == 'fake'
        assert run_manifest['infrastructure_failure'] is None
        assert archive.read(prefix + 'queries.jsonl') == archive.read('p7-019-evidence/queries.jsonl')
        engine = run_manifest['engine']
        binding = engine['mechanism_ablation']
        assert engine['engine_head_observed'] == PR and engine['dirty_observed'] == '' and engine['eval_debug_assertions'] is True
        assert binding['source_commit'] == PR and binding['cell'] == cell and binding['requested_strategy'] == 'semantic'
        assert binding['enabled'] == FACTORS[cell] and binding['controls'] == controls
        assert binding['build_receipt'] == read(f'p7-019-build/builds/{cell}/receipt.json')
        input_locks.append(run_manifest['input'])
        observations, witnesses, costs = lines(prefix + 'profile-queries.jsonl'), lines(prefix + 'lane-execution.jsonl'), lines(prefix + 'strategy-costs.jsonl')
        assert len(observations) == len(witnesses) == len(costs) == 9
        assert [r['sequence'] for r in observations] == list(range(9))
        assert Counter(r['input']['query'] for r in observations) == {'needle': 3, 'copper': 3, 'zqxvplmnb': 3}
        all_inputs.append([r['input'] for r in observations])
        all_budgets.append({r['input_digest']: r['budget'] for r in observations})
        local, dense = 'local_retrieval' in FACTORS[cell], 'semantic_dense' in FACTORS[cell]
        expected_lanes = (LOCAL if local else set()) | ({'semantic'} if dense else set())
        result_counts, lane_sets, ready_phases = [], [], []
        for observation, witness, cost in zip(observations, witnesses, costs):
            assert observation['requested'] == 'semantic' and observation['effective'] == ('semantic' if dense else 'local')
            assert observation['error'] is None
            assert observation['input_digest'] == sha(compact(observation['input']))
            path = prefix + observation['raw_path']
            assert inventory[path]['sha256'] == observation['raw_sha256'] == witness['raw_sha256']
            assert witness['sequence'] == cost['sequence'] == observation['sequence']
            assert witness['input_digest'] == cost['input_digest'] == observation['input_digest']
            raw = read(path); raw_count += 1
            assert raw['query'] == observation['input']['query'] and raw['machine_pack']['top_k'] == 1
            retrieval, packing = raw['evidence_summary']['retrieval'], raw['evidence_summary']['packing']
            policy, scope, budget = retrieval['policy'], retrieval['scope'], observation['budget']
            assert policy['requested'] == observation['requested'] and policy['effective'] == observation['effective']
            assert {k: policy[k] for k in ['deadline_ms', 'lane_timeout_ms', 'semantic_timeout_ms', 'semantic_top_k']} == {
                'deadline_ms': 30000, 'lane_timeout_ms': 20000, 'semantic_timeout_ms': 5000, 'semantic_top_k': 24}
            assert budget['candidate_budget'] == {k: v for k, v in scope['budget'].items() if k != 'units'}
            assert budget['hard_scope'] == {k: v for k, v in scope['hard'].items() if k != 'semantics'}
            assert budget['policy_version'] == policy['version'] and budget['scope_policy'] == scope['policy']
            assert budget['packing_spec'] == packing['spec'] and budget['token_budget'] == raw['token_budget'] == 4000
            assert budget['configured_max_bytes'] == packing['configured_max_bytes'] == 18000
            assert budget['limit_bytes'] == packing['limit_bytes'] == 16000
            assert packing['used_bytes'] == inventory[path]['bytes'] <= 16000
            assert raw['token_estimate'] == (packing['used_bytes'] + 3) // 4
            lanes = retrieval['lanes']
            assert {lane['lane_id'] for lane in lanes} == expected_lanes and len(lanes) == len(expected_lanes)
            projected = [{k: lane[k] for k in ['lane_id', 'status', 'candidate_count', 'coverage', 'truncation_reason', 'elapsed_us']} for lane in lanes]
            assert witness['witness']['lane_receipts'] == projected
            assert witness['witness']['local_factor'] == local and witness['witness']['semantic_dense_factor'] == dense
            assert witness['witness']['cell'] == cell and witness['witness']['profile'] == 'fake'
            hits = raw['machine_pack']['hits']
            for hit in hits:
                source = archive.read('p7-019-evidence/input/' + hit['file_path'])
                assert source.decode() == hit['text']
                freshness = hit['metadata']['source_freshness']
                assert freshness['disk_checked'] is True and freshness['status'] == 'current_verified'
            if dense:
                lane = next(lane for lane in lanes if lane['lane_id'] == 'semantic')
                assert lane['status'] == 'complete' and lane['candidate_count'] == 2 and hits
            if cell == 'none':
                assert not lanes and not hits
            if local and observation['input']['query'] != 'zqxvplmnb':
                expected_path = 'a.rs' if observation['input']['query'] == 'needle' else 'b.rs'
                assert hits and hits[0]['file_path'] == expected_path
                assert next(lane for lane in lanes if lane['lane_id'] == 'exact_symbol')['candidate_count'] > 0
            if cell == 'local' and observation['input']['query'] == 'zqxvplmnb':
                assert not hits
            assert cost['profile'] == 'fake' and cost['provider_cost_units'] is None and cost['provider_cost_unknown'] is True
            assert cost['cache_reuse'] is None and cost['cache_reuse_unknown'] is True
            assert cost['originating_work'] == observation['originating_cost'] == retrieval['cost']
            assert cost['validation_work'] == observation['validation_work']
            result_counts.append({'sequence': observation['sequence'], 'query': raw['query'], 'hits': len(hits)})
            lane_sets.append(sorted(expected_lanes))
        readiness = lines(prefix + 'profile-readiness.jsonl')
        for row in readiness:
            ready_phases.append({'phase': row['phase'], 'ready': row['ready']})
            if row['ready']:
                status = row['raw']['retrieval']
                assert status['identity_validation'] == 'checked_at_observation_boundary'
                assert status['semantic_active_space'] == expected_space
                assert status['dense_state'] == 'ready' and status['dense_desired'] == status['dense_published'] == 2
        assert readiness[-1]['ready'] and readiness[-1]['phase'] == 'after_queries'
        assert archive.read(prefix + 'failures.jsonl') == b''
        normalized, scores, metrics = lines(prefix + 'normalized.jsonl'), lines(prefix + 'scores.jsonl'), read(prefix + 'metrics.json')
        assert len(normalized) == len(scores) == 6
        assert {(r['case_id'], r['repetition']) for r in normalized} == {(q['id'], rep) for q in queries for rep in [0, 1]}
        case_scores = defaultdict(list)
        for row, score in zip(normalized, scores):
            query = next(q for q in queries if q['id'] == row['case_id'])
            expected_empty = cell == 'none' or (cell == 'local' and query['id'] == 'dense-control')
            assert row['status'] == ('no_match' if expected_empty else 'success') and row['diagnostic'] is None
            raw = read(prefix + row['raw_path'])
            assert raw['query'] == query['query']
            raw_hits = raw['machine_pack']['hits']
            assert [(h['path'], h['text']) for h in row['hits']] == [(h['file_path'], h['text']) for h in raw_hits]
            assert all(h['evidence_valid'] is True for h in row['hits'])
            # This fixed authored fixture has one grade-3 answer and top_k=1.
            target = query['answers'][0]['alternatives'][0]['path']
            actual_score = float(bool(row['hits']) and row['hits'][0]['path'] == target)
            assert score['top1'] == score['ndcg10'] == actual_score
            case_scores[query['id']].append(actual_score)
        assert metrics['queries'] == 3 and metrics['measured_rows'] == 6
        assert metrics['unverified_hits'] == metrics['invalid_hits'] == 0
        current = {}
        for case in metrics['cases']:
            query = next(q for q in queries if q['id'] == case['id'])
            mean = sum(case_scores[case['id']]) / 2
            assert case['family'] == query['query_family'] and case['category'] == query['category'] and case['repetitions'] == 2
            assert case['top1'] == case['ndcg10'] == mean
            current[case['id']] = mean
        assert metrics['mean_top1'] == metrics['mean_ndcg10'] == sum(current.values()) / 3
        assert metrics['family_ndcg_ci'] == bootstrap([current[key] for key in sorted(current)])
        metric_cases[cell] = current
        observed_cells.append({'cell': cell, 'requests': 9, 'lane_set': sorted(expected_lanes),
                               'positive_negative_observations': result_counts, 'ready_phases': ready_phases,
                               'normalized_status_counts': dict(Counter(row['status'] for row in normalized)),
                               'case_scores_recomputed': current, 'provider_cost_units': None, 'cache_reuse': None})
    assert raw_count == 36
    assert all(value == all_inputs[0] for value in all_inputs)
    assert all(value == input_locks[0] for value in input_locks)
    assert all(value == all_budgets[0] == comparison['budgets_by_input'] for value in all_budgets)
    paired = []
    for pair in comparison['paired_against_local']:
        assert pair['baseline'] == 'local' and pair['candidate'] in ['dense_only', 'hybrid']
        observed = pair['observations']
        assert observed['seed'] == 19 and observed['bootstrap_draws'] == 2000
        assert observed['percentile_indices_zero_based'] == [49, 1949]
        delta = {q['id']: metric_cases[pair['candidate']][q['id']] - metric_cases['local'][q['id']] for q in queries}
        assert observed['family_means'] == delta
        assert observed['family_ndcg_delta_ci'] == bootstrap([delta[key] for key in sorted(delta)])
        for case in observed['cases']:
            assert case['delta_top1'] == case['delta_ndcg10'] == delta[case['id']]
            assert case['repetitions_averaged'] == 2
        for stratum in observed['strata']:
            selected = {q['id']: delta[q['id']] for q in queries if q['split'] == stratum['split'] and q['category'] == stratum['category']}
            assert stratum['family_means'] == selected
            assert stratum['family_ndcg_delta_ci'] == bootstrap([selected[key] for key in sorted(selected)])
        paired.append({'candidate': pair['candidate'], 'recomputed_family_means': delta,
                       'recomputed_bootstrap': observed['family_ndcg_delta_ci'],
                       'quality_gate': observed['quality_gate']})
    assert len(paired) == 2
    provider = read('p7-019-evidence/fake-provider-observations.json')
    observed_hashes = Counter(h for req in provider['requests'] for h in req['input_sha256'])
    query_provider_receipts = {q['id']: observed_hashes[sha(q['query'].encode())] for q in queries}
    assert all(count == 2 for count in query_provider_receipts.values())

inventory_path = OUT / 'ci9ebe-mechanism-archive-inventory.json'
inventory_path.write_bytes(canonical(inventory))
receipt = {'schema_version': 1, 'review_status': 'actual_ci_artifacts_streamed_and_recomputed',
           'workflow_run_id': 37724421493, 'job_id': 113139223006, 'artifact_id': 11527337724,
           'source_commit': PR, 'zip': str(ZIP), 'zip_sha256': zip_hash, 'zip_member_count': len(inventory),
           'zip_expanded_bytes_streamed': sum(item['bytes'] for item in inventory.values()),
           'complete_archive_inventory': str(inventory_path), 'complete_archive_inventory_sha256': sha(inventory_path.read_bytes()),
           'raw_job_log_sha256': sha((OUT / 'ci9ebe-mechanism-job.log').read_bytes()),
           'reference_full_794_inputs_match_previously_independently_checked_fixed_pr_git_blobs': True,
           'fixed_controls_match_git_source': True, 'builds': built,
           'all_four_complete_source_inventories_and_product_binary_sha256_verified': True,
           'all_source_binary_target_and_receipt_paths_distinct': True,
           'same_nonpositional_build_options': True, 'nested_build_outputs_fixed_to_each_target': True,
           'same_real_inputs_configuration_and_effective_budgets': True,
           'actual_requests_including_warmup': raw_count, 'cells': observed_cells,
           'recorded_scores_independently_recomputed_from_actual_result_paths': True,
           'paired_bootstrap_2000_draws_recomputed': paired,
           'actual_query_input_provider_witness_counts': query_provider_receipts,
           'actual_negative_controls': {key: acceptance[key] for key in ['actual_hybrid_mislabel_rejected', 'aliased_binary_rejected', 'aliased_source_rejected', 'misattributed_source_commit_rejected']},
           'negative_control_evidence': 'exact compiled CI test completed with unchanged runtime assert calls; archived and raw job acceptance agree',
           'original_p7_019_fake_engineering_leg_complete_at_this_source': True,
           'held_out_semantic_quality_gate': 'not_evaluated',
           'provider_billing_and_cache_attribution': 'unknown; never zero-filled',
           'limitations': ['fake authored mechanism input partitions do not certify held-out semantic benefit',
                           'source/build receipt and CI execution provenance, not remote source-to-binary attestation',
                           'compiler executable hashes are recorded by the build; compiler binaries are not part of this artifact',
                           'only ZIP streams read; downloaded products not locally executed; no Cargo run'],
           'parent_scope': 'all closeout jobs have succeeded; full task acceptance separately requires original dependencies, main CI and integrated independent review'}
target = OUT / 'ci9ebe-mechanism-independent-receipt.json'
target.write_bytes(canonical(receipt))
print(json.dumps({'receipt': str(target), 'sha256': sha(target.read_bytes()), 'source_commit': PR,
                  'members': len(inventory), 'builds': built, 'actual_requests': raw_count,
                  'cells': observed_cells, 'paired': paired, 'provider_query_witnesses': query_provider_receipts,
                  'negative_controls': receipt['actual_negative_controls'],
                  'original_p7_019_fake_engineering_leg_complete_at_this_source': True}, indent=2))

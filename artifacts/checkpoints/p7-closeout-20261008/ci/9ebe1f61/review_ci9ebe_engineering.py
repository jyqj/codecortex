from pathlib import Path
import hashlib
import json
import math
import subprocess
import zipfile

ROOT = Path('/dev/shm/codecortex-closeout-996727/codecortex-root')
OUTPUT = Path('/dev/shm/codecortex-closeout-996727/validation')
ZIP = Path('/workspace/scratch/9967275fe9a7/ci-import/ci9ebe-engineering-artifact.zip')
PR = '9ebe1f619a298b9955d576d9d8250368259924d2'
EXECUTED = 'c41bf2717aa745863520daedb89d8c4b3353d8c4'

def digest(data):
    return hashlib.sha256(data).hexdigest()

def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=ROOT, input=data)

def canonical(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()

def fixed_inputs():
    blobs = {}
    for entry in git('ls-tree', '-r', '-z', PR, '--', 'crates', 'Cargo.toml', 'Cargo.lock').split(b'\0'):
        if not entry:
            continue
        metadata, path = entry.split(b'\t', 1)
        mode, kind, oid = metadata.decode().split()
        assert kind == 'blob' and mode in ('100644', '100755')
        blobs[path.decode()] = oid
    objects = list(dict.fromkeys(blobs.values()))
    raw = git('cat-file', '--batch', data=('\n'.join(objects) + '\n').encode())
    hashes, offset = {}, 0
    for expected in objects:
        end = raw.index(b'\n', offset)
        oid, kind, size = raw[offset:end].decode().split()
        assert oid == expected and kind == 'blob'
        offset = end + 1
        hashes[oid] = digest(raw[offset:offset + int(size)])
        offset += int(size)
        assert raw[offset:offset + 1] == b'\n'
        offset += 1
    assert offset == len(raw)
    return {path: hashes[oid] for path, oid in sorted(blobs.items())}

assert ZIP.stat().st_size == 221887
zip_hash = digest(ZIP.read_bytes())
assert zip_hash == '7066eda6e181f47b12d980e9989ce00a2d3566fc6e293e1bdee0abac832d01dd'
with zipfile.ZipFile(ZIP) as archive:
    names = archive.namelist()
    assert len(names) == len(set(names))
    assert not any('request-failures' in name for name in names)
    inventory = {name: {'bytes': archive.getinfo(name).file_size, 'sha256': digest(archive.read(name))}
                 for name in sorted(names)}
    before_bytes, after_bytes = archive.read('source-before.json'), archive.read('source-after.json')
    assert before_bytes == after_bytes
    source = json.loads(before_bytes)
    expected = fixed_inputs()
    assert source['source_commit'] == EXECUTED
    assert source['source_tree'] == git('rev-parse', PR + '^{tree}').decode().strip()
    assert source['input_count'] == len(expected) == 794
    assert source['inputs'] == expected
    assert source['manifest_sha256'] == digest(canonical(expected))
    log = (OUTPUT / 'ci9ebe-engineering-job.log').read_bytes()
    assert EXECUTED.encode() in log and ('Merge ' + PR).encode() in log

    cells, stages, seeds, requests = [], [], [], []
    for seed in [7, 19, 43]:
        prefix = f'worker-contention/seed-{seed}/'
        read = lambda name: json.loads(archive.read(prefix + name + '.json'))
        protocol, held_before, summary = read('protocol'), read('held-before'), read('summary')
        assert protocol['seed'] == summary['seed'] == seed
        assert protocol['samples_per_cell'] == 32 and protocol['concurrency_cells'] == [1, 4]
        assert protocol['phases'] == ['quiet', 'held']
        assert protocol['query_watchdog_ms'] == 2000 and protocol['progress_watchdog_ms'] == 5000
        assert held_before['provider']['active'] == held_before['provider']['waiting'] == 4
        assert held_before['queue']['claimed'] == 4
        assert summary['old_provider']['active'] == summary['old_provider']['waiting'] == 0
        assert summary['old_provider']['maximum_active'] == 4
        assert summary['old_held_input_publications'] == 0
        assert summary['queue_final']['claimed'] == summary['queue_final']['pending'] == summary['queue_final']['uncovered'] == 0
        assert summary['queue_final']['published'] == summary['new_provider_calls'] == 25
        assert summary['db_availability']['generation_unchanged'] is True
        assert summary['db_availability']['generation_before'] == summary['db_availability']['generation_after']
        assert summary['db_availability']['writer_busy_timeout_ms'] == 100
        assert summary['db_availability']['writer_acquire_and_rollback_us'] < 100_000
        assert summary['fixed_watchdogs'] == {'local_query_ms': 2000, 'progress_ms': 5000}
        assert summary['switch_and_drain_us'] < 5_000_000
        for phase in ['quiet', 'held']:
            rows = read(phase + '-requests')
            assert len(rows) == 64
            assert {(r['concurrency'], r['ordinal']) for r in rows} == {(c, i) for c in [1, 4] for i in range(32)}
            for row in rows:
                assert 0 <= row['offered_to_api_return_us'] < 2_000_000
                hits = [h for h in row['hits'] if h['file_path'] == 'stable.rs']
                assert hits and any(h['text'] == f'pub fn stable_signal() -> u64 {{ {seed} }}\n' for h in hits)
                assert all(h['metadata']['source_freshness']['disk_checked'] is True
                           and h['metadata']['source_freshness']['status'] == 'current_verified' for h in hits)
                for key in ['caller_schedule_us', 'capture_admission_us', 'retrieval_us']:
                    assert 0 <= row[key] <= row['offered_to_api_return_us']
                requests.append({'seed': seed, 'phase': phase, **row})
            for concurrency in [1, 4]:
                times = sorted(r['offered_to_api_return_us'] for r in rows if r['concurrency'] == concurrency)
                actual = {'concurrency': concurrency, 'n': len(times),
                          **{f'p{p}_us': times[math.ceil(p * len(times) / 100) - 1] for p in [50, 95, 99]},
                          'max_us': times[-1]}
                saved = next(r for r in summary[phase] if r['concurrency'] == concurrency)
                assert all(saved[key] == value for key, value in actual.items())
                cells.append({'seed': seed, 'phase': phase, **actual})

        source_stages = [summary['quiet_resources'], held_before['resources'],
                         summary['held_after_resources'], summary['final_resources']]
        for stage in source_stages:
            usage = stage['shared_process_owner']['usage']
            assert stage['shared_process_owner']['components'] == ['test_runner', 'CodeIndex', 'fake_provider']
            assert stage['resource_gate'] == 'attributed_combined_process'
            assert usage['pid'] == stage['raw_process_snapshot']['pid'] == stage['raw_sampler']['runner_pid'] == 13025
            assert usage['peak_resident_bytes'] > 0
            assert stage['server']['separate_pid'] is None and stage['server_tree']['child_process_usage'] is None
            assert stage['current_executable'] == stage['proc_executable']
            pool = stage['cpu_pool_slots']
            assert pool['cpu_limit'] == 4 and pool['async_limit'] == 8 and pool['queue_limit'] == 32
            assert all(pool[k] == 0 for k in ['cpu_in_flight', 'cpu_admitted', 'async_in_flight', 'async_admitted', 'rejected'])
            stages.append({'seed': seed, 'stage': stage['stage'], **usage,
                           'cpu_pool_slots': pool,
                           'proc_resident_bytes_point_sample': stage['raw_process_snapshot']['resident_bytes'],
                           'ps_runner_rss_bytes_point_sample': stage['raw_sampler']['runner_rss_bytes']})
        usages = [s['shared_process_owner']['usage'] for s in source_stages]
        assert all(a['user_cpu_ns'] <= b['user_cpu_ns'] and a['system_cpu_ns'] <= b['system_cpu_ns']
                   for a, b in zip(usages, usages[1:]))
        seeds.append({'seed': seed, 'held_provider': held_before['provider'],
                      'final_provider': summary['old_provider'], 'final_queue': summary['queue_final'],
                      'old_held_input_publications': summary['old_held_input_publications'],
                      'new_provider_calls': summary['new_provider_calls'],
                      'initial_build_us': summary['initial_build_us'], 'held_build_us': summary['held_build_us'],
                      'write_delete_us': summary['write_delete_us'], 'switch_and_drain_us': summary['switch_and_drain_us'],
                      'db_availability': summary['db_availability'],
                      'held_requests_plus_db_control_cpu_delta_ns': {key: usages[2][key] - usages[1][key]
                          for key in ['user_cpu_ns', 'system_cpu_ns']}})

    lifecycle = {}
    for name in ['local-cancel', 'model-switch']:
        rows = json.loads(archive.read('lifecycle/' + name + '.json'))
        assert {r['seed'] for r in rows} == {7, 19, 43}
        for row in rows:
            assert row['local_query_ms'] < 2000
            if name == 'local-cancel':
                assert row['pins_after_exit'] == row['published_after_close'] == 0
                assert row['charged_attempts_after_close'] == row['provider_calls'] == 1
            else:
                assert row['old_space_publications'] == 0 and row['new_provider_calls'] == 1
        lifecycle[name] = [{k: v for k, v in row.items() if not k.endswith('_snapshot')} for row in rows]

assert len(requests) == 384
receipt = {
    'schema_version': 1, 'review_status': 'verified_actual_archived_evidence',
    'workflow_run_id': 37724421463, 'job_id': 113139222544, 'artifact_id': 11527696269,
    'artifact_zip': str(ZIP), 'artifact_zip_sha256': zip_hash, 'archive_file_inventory': inventory,
    'raw_job_log_sha256': digest(log),
    'source_binding': {'executed_checkout': EXECUTED, 'associated_pr_head': PR,
                       'archived_execution_tree': source['source_tree'], 'complete_tree_equal': True,
                       'archived_before_after_bytes_equal': True,
                       'full_794_crate_cargo_inputs_equal_fixed_pr_git_blobs': True,
                       'input_manifest_sha256': source['manifest_sha256']},
    'worker_requests': {'expected': 384, 'actual': len(requests), 'all_ordinals_present_once_per_cell': True,
                        'all_have_current_verified_expected_source': True, 'request_failures_artifact_absent': True,
                        'maximum_offered_to_api_return_us': max(r['offered_to_api_return_us'] for r in requests),
                        'maximum_capture_admission_us': max(r['capture_admission_us'] for r in requests),
                        'all_existing_2000ms_query_bounds_passed': True,
                        'raw_nearest_rank_statistics_recomputed_equal': True, 'cells': cells},
    'worker_seeds': seeds, 'resource_stage_observations': stages,
    'resource_scope': {'ownership': 'one PID 13025; runner + CodeIndex + fake provider counted once',
                       'cpu': 'RUSAGE_SELF cumulative user/system CPU and non-overlapping recorded stage deltas',
                       'peak_resident_bytes': max(s['peak_resident_bytes'] for s in stages),
                       'peak_scope': 'process lifetime high-water; not per-component or per-cell peak',
                       'transient_child_process_usage': None,
                       'no_resource_values_zero_filled': True,
                       'no_latency_sla_or_held_out_quality_claim': True},
    'original_lifecycle_controls': lifecycle,
    'task_acceptance_is_separate': True,
    'acceptance_dependencies': ['P7-014 actual HTTP lifecycle and original requirements', 'full current-source regression and independent integrated acceptance'],
}
target = OUTPUT / 'ci9ebe-engineering-independent-receipt.json'
target.write_bytes(canonical(receipt))
print(json.dumps({'receipt': str(target), 'sha256': digest(target.read_bytes()),
                  'source_binding': receipt['source_binding'], 'requests': receipt['worker_requests'],
                  'resource_scope': receipt['resource_scope'],
                  'seeds': [{k: v for k, v in row.items() if k not in ['held_provider', 'final_provider', 'db_availability']}
                            for row in seeds],
                  'lifecycle': lifecycle}, indent=2))

#!/usr/bin/env python3
"""Read preserved GitHub evidence and fixed Git blobs; never run products/tests."""
import collections
import hashlib
import json
import re
import stat
import subprocess
import zipfile
from pathlib import Path, PurePosixPath

import argparse

PARSER = argparse.ArgumentParser(description=__doc__)
PARSER.add_argument('--evidence-root', required=True, type=Path)
PARSER.add_argument('--repo', required=True, type=Path)
PARSER.add_argument('--head', required=True)
PARSER.add_argument('--run', required=True, type=int)
PARSER.add_argument('--job', required=True, type=int)
PARSER.add_argument('--artifact', required=True, type=int)
PARSER.add_argument('--event', choices=['pull_request', 'push'], required=True)
ARGS = PARSER.parse_args()
ROOT = ARGS.evidence_root.resolve(strict=True)
REPO = ARGS.repo.resolve(strict=True)
HEAD = ARGS.head
RUN_ID = ARGS.run
JOB_ID = ARGS.job
ARTIFACT_ID = ARGS.artifact
EVENT = ARGS.event
OLD = '5af7ac0089ee7522e78ff2ce2468f881c8cf70f2'
TREE = '25558c46010e085d13f134f6ff623e7ce285cbd3'
ARTIFACT = ROOT / f'p7-engineering-{ARTIFACT_ID}.zip'
TEMPLATE_SHA256 = 'd1baa9f2e25ade079f1524db7b47e9cf090075526be7c98b88f45ba33153f594'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def write_json(name, obj):
    data = (json.dumps(obj, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
    (ROOT / name).write_bytes(data)
    return sha(data)


def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=REPO, input=data)


def blob(commit, path):
    return git('show', commit + ':' + path)


def fixed_source(commit):
    paths = {}
    for entry in git('ls-tree', '-r', '-z', commit, '--', 'crates', 'Cargo.toml', 'Cargo.lock').split(b'\0'):
        if not entry:
            continue
        metadata, path = entry.split(b'\t', 1)
        mode, kind, oid = metadata.decode().split()
        assert mode in ('100644', '100755') and kind == 'blob'
        paths[path.decode()] = oid
    objects = list(dict.fromkeys(paths.values()))
    raw = git('cat-file', '--batch', data=('\n'.join(objects) + '\n').encode())
    hashes, offset = {}, 0
    for expected in objects:
        end = raw.index(b'\n', offset)
        oid, kind, size = raw[offset:end].decode().split()
        assert oid == expected and kind == 'blob'
        size, start = int(size), end + 1
        hashes[oid] = sha(raw[start:start + size])
        offset = start + size
        assert raw[offset:offset + 1] == b'\n'
        offset += 1
    assert offset == len(raw)
    inputs = {p: hashes[o] for p, o in sorted(paths.items())}
    manifest = (json.dumps(inputs, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
    return {'source_commit': commit, 'source_tree': git('rev-parse', commit + '^{tree}').decode().strip(),
            'input_count': len(inputs), 'manifest_sha256': sha(manifest), 'inputs': inputs}


def unpack_checked():
    api = json.loads((ROOT / 'artifacts-api.json').read_bytes())['structuredContent']['artifacts']
    assert len(api) == 1 and api[0]['id'] == ARTIFACT_ID
    meta = api[0]
    assert meta['workflow_run']['id'] == RUN_ID and meta['workflow_run']['head_sha'] == HEAD
    raw = ARTIFACT.read_bytes()
    assert len(raw) == meta['size_in_bytes'] and 'sha256:' + sha(raw) == meta['digest']
    extracted = ROOT / 'artifact'
    extracted.mkdir(exist_ok=True)
    members, payload = {}, 0
    with zipfile.ZipFile(ARTIFACT) as z:
        for entry in z.infolist():
            name = entry.filename
            path = PurePosixPath(name)
            assert name == path.as_posix() and not path.is_absolute()
            assert not any(c in ('', '.', '..') for c in name.split('/')) and '\\' not in name
            assert name not in members and not entry.is_dir()
            assert stat.S_ISREG(entry.external_attr >> 16) and not entry.flag_bits & 1
            assert 0 <= entry.file_size <= 4 * 1024 * 1024
            payload += entry.file_size
            assert payload <= 16 * 1024 * 1024
            data = z.read(entry)
            assert len(data) == entry.file_size
            target = extracted.joinpath(*path.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            for parent in [target.parent, *target.parent.parents]:
                if parent == ROOT.parent:
                    break
                assert not parent.is_symlink()
            assert not target.is_symlink()
            target.write_bytes(data)
            members[name] = {'size_bytes': len(data), 'sha256': sha(data)}
    write_json('artifact-members.json', members)
    return meta, members, extracted


def parse_rust(extracted, filename, package):
    target = None
    groups = []
    for lineno, line in enumerate((extracted / filename).read_text().splitlines(), 1):
        m = re.match(r'^\s*Running (.+?) \(([^)]+)\)$', line)
        if m:
            target = {'target': package + '/' + m[1], 'executable_path': m[2],
                      'raw_log': filename, 'start_line': lineno, 'test_cases': []}
            groups.append(target)
            continue
        m = re.match(r'^test (\S+) \.\.\. (ok|FAILED|ignored)(?:, (.*))?$', line)
        if m:
            assert target is not None
            target['test_cases'].append({'identity': target['target'] + '::' + m[1],
                                        'result': m[2], 'ignored_reason': m[3], 'log_line': lineno})
            continue
        m = re.match(r'^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in (.*)$', line)
        if m:
            assert target is not None and 'summary' not in target
            target['summary'] = dict(zip(['passed', 'failed', 'ignored', 'measured', 'filtered_out'], map(int, m.groups()[1:6])))
            target['summary'].update(result=m[1], duration=m[7], log_line=lineno)
    for group in groups:
        assert 'summary' in group
        actual = collections.Counter(t['result'] for t in group['test_cases'])
        assert (actual['ok'], actual['FAILED'], actual['ignored']) == tuple(group['summary'][k] for k in ['passed', 'failed', 'ignored'])
    return groups


def main():
    meta, members, extracted = unpack_checked()
    run_api = json.loads((ROOT / 'run-metadata-api.json').read_bytes())
    assert run_api.get('isError') is False
    run_metadata = json.loads(run_api['structuredContent']['content'])
    assert run_metadata == json.loads((ROOT / 'run-metadata.json').read_bytes())
    assert run_metadata['id'] == RUN_ID and run_metadata['head_sha'] == HEAD
    assert run_metadata['event'] == EVENT and run_metadata['run_attempt'] == 1
    assert run_metadata['status'] == 'completed' and run_metadata['conclusion'] == 'success'
    log_api = json.loads((ROOT / 'job-log-api.json').read_bytes())
    assert log_api.get('isError') is False
    assert log_api['structuredContent']['content'].encode('utf-8') == (ROOT / 'engineering-job.log').read_bytes()
    expected = fixed_source(HEAD)
    assert expected['input_count'] == 776 and expected['source_tree'] == TREE
    write_json('expected-source-head.json', expected)
    before_raw = (extracted / 'source-before.json').read_bytes()
    after_raw = (extracted / 'source-after.json').read_bytes()
    before, after = json.loads(before_raw), json.loads(after_raw)
    assert before_raw == after_raw and before == after
    actual_checkout = before['source_commit']
    assert re.fullmatch('[0-9a-f]{40}', actual_checkout)
    assert git('rev-parse', actual_checkout + '^{tree}').decode().strip() == TREE
    assert not git('diff', '--name-only', HEAD, actual_checkout, '--')
    assert meta['name'] == 'p7-engineering-' + actual_checkout
    if EVENT == 'push':
        assert actual_checkout == HEAD
    else:
        assert actual_checkout != HEAD
        assert HEAD in git('rev-list', '--parents', '-n', '1', actual_checkout).decode().split()[1:]
    write_json('actual-checkout-identity.json', {'audited_head': HEAD, 'actual_checkout': actual_checkout,
        'event': EVENT, 'tree': TREE, 'head_and_checkout_full_trees_equal': True,
        'parents': git('rev-list', '--parents', '-n', '1', actual_checkout).decode().strip().split()[1:]})
    for field in ['input_count', 'manifest_sha256', 'inputs', 'source_tree']:
        assert before[field] == expected[field], field

    receipt_path = 'artifacts/checkpoints/p7-deadline-wiring-20261007/p7-013-receipt.json'
    receipt = json.loads(blob(HEAD, receipt_path))
    expected_cases = [t for g in receipt['primary_matrix'] for t in g['test_cases']]
    expected_ids = {t['identity'] for t in expected_cases}
    assert len(expected_cases) == len(expected_ids) == 58
    write_json('expected-original58.json', {'source_receipt_path': receipt_path, 'source_receipt_sha256': sha(blob(HEAD, receipt_path)),
                                          'original_executed_source': OLD, 'test_cases': expected_cases})
    selections = [('coverage-config.log', 'cc-server'), ('deadline-cache.log', 'cc-server'),
                  ('query-encoding.log', 'cc-server'), ('execution-deadline.log', 'cc-search'),
                  ('gc-unlink.log', 'cc-semantic'), ('worker-strategy.log', 'cc-eval')]
    suites = {name: parse_rust(extracted, name, package) for name, package in selections}
    original_groups = [g for name in ['deadline-cache.log', 'query-encoding.log', 'execution-deadline.log'] for g in suites[name]]
    original_cases = [t for g in original_groups for t in g['test_cases']]
    assert len(original_cases) == 58 and {t['identity'] for t in original_cases} == expected_ids
    assert all(t['result'] == 'ok' for t in original_cases)
    assert all(len([t for t in original_cases if t['identity'] == identity]) == 1 for identity in expected_ids)
    write_json('actual-rust-test-index.json', suites)

    old_paths = set()
    for identity in expected_ids:
        target = identity.split('::')[0]
        if 'unittests' in target:
            old_paths.add('crates/cc-server/src/semantic_query_encoding.rs' if target.startswith('cc-server/') else 'crates/cc-search/src/execution.rs')
        else:
            old_paths.add('crates/' + target)
    unchanged = {}
    for path in sorted(old_paths):
        old, new = blob(OLD, path), blob(HEAD, path)
        unchanged[path] = {'old_sha256': sha(old), 'new_sha256': sha(new), 'byte_identical': old == new}
    assert len(unchanged) == 13 and all(v['byte_identical'] for v in unchanged.values())
    write_json('original58-source-comparison.json', unchanged)

    python_cases = []
    for line_no, line in enumerate((extracted / 'input-lock.log').read_text().splitlines(), 1):
        m = re.match(r'^test_\S+ \(([^)]+)\) \.\.\. (ok|FAIL|ERROR|skipped.*)$', line)
        if m:
            python_cases.append({'identity': m[1], 'result': m[2], 'log_line': line_no})
    assert len(python_cases) == 15 and all(t['result'] == 'ok' for t in python_cases)
    assert re.search(r'\nRan 15 tests in [0-9.]+s\n\nOK\n', (extracted / 'input-lock.log').read_text())
    write_json('actual-python-test-index.json', python_cases)

    worker = []
    for seed in [7, 19, 43]:
        prefix = 'worker-contention/seed-' + str(seed) + '/'
        summary = json.loads((extracted / (prefix + 'summary.json')).read_bytes())
        held = json.loads((extracted / (prefix + 'held-before.json')).read_bytes())
        cells = []
        for phase in ['quiet', 'held']:
            rows = json.loads((extracted / (prefix + phase + '-requests.json')).read_bytes())
            assert len(rows) == 64 and all(isinstance(r, dict) for r in rows)
            counts = collections.Counter(r['concurrency'] for r in rows)
            assert counts == {1: 32, 4: 32}
            cells.append({'phase': phase, 'rows': len(rows), 'concurrency_counts': dict(counts),
                          'all_have_stable_source_hit': all(any(h.get('file_path') == 'stable.rs' for h in r['hits']) for r in rows),
                          'raw_path': prefix + phase + '-requests.json'})
        worker.append({'seed': seed, 'cells': cells, 'held_provider': held['provider'], 'held_queue': held['queue'],
                       'old_held_input_publications': summary['old_held_input_publications'],
                       'queue_final': summary['queue_final'], 'resource_attribution_gate': summary['resource_attribution_gate'],
                       'performance_delta_gate': summary['performance_delta_gate'], 'full_p7_015_complete': summary['full_p7_015_complete']})
    write_json('worker-observation-index.json', worker)

    joblog = (ROOT / 'engineering-job.log').read_text()
    plain = re.sub(r'^\d{4}-\d\d-\d\dT\S+Z ', '', joblog, flags=re.M)
    plain = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', plain)
    command_blocks = []
    job_lines = plain.splitlines()
    for pos, line in enumerate(job_lines):
        if line.startswith(('##[group]Run mkdir -p "$RUNNER_TEMP/p7-engineering"',
                            '##[group]Run set -o pipefail', "##[group]Run python3 - <<'PY'")):
            end = pos + 1
            while end < len(job_lines) and not job_lines[end].startswith('shell:'):
                end += 1
            assert end < len(job_lines)
            command_blocks.append({'job_log_start_line': pos + 1, 'job_log_shell_line': end + 1,
                                   'script': '\n'.join(job_lines[pos + 1:end]), 'shell': job_lines[end]})
    assert len(command_blocks) == 8
    write_json('executed-command-index.json', command_blocks)
    deadline_observations, target = [], None
    for line_no, line in enumerate((extracted / 'deadline-cache.log').read_text().splitlines(), 1):
        running = re.match(r'\s*Running (\S+) ', line)
        if running:
            target = running[1]
        if target != 'tests/p7_query_deadline_public.rs' or not line.startswith('PUBLIC_JSON '):
            continue
        observation = json.loads(line[len('PUBLIC_JSON '):])
        calls = []
        for call in observation['calls']:
            if call.get('tool') not in ('search', 'context'):
                continue
            value = call['result']
            semantic = next(x for x in value['evidence_summary']['retrieval']['lanes'] if x['lane_id'] == 'semantic')
            calls.append({'tool': call['tool'], 'semantic_status': semantic['status'],
                          'candidate_count': semantic['candidate_count'], 'semantic_elapsed_us': semantic.get('elapsed_us'),
                          'truncation_reason': semantic.get('truncation_reason'),
                          'has_current_needle_source': any(s.get('file_path') == 'needle.py' for s in value['spans'])})
        expected_statuses = ['timeout', 'complete', 'complete'] if observation['test'].startswith('deadline-') else ['timeout', 'complete']
        assert [c['semantic_status'] for c in calls] == expected_statuses
        assert all(c['has_current_needle_source'] for c in calls)
        assert all(c['candidate_count'] > 0 for c in calls if c['semantic_status'] == 'complete')
        assert observation['only_loopback'] is True and observation['real_credentials'] is False
        deadline_observations.append({'scenario': observation['test'], 'raw_log': 'deadline-cache.log', 'raw_log_line': line_no,
                                      'raw_json_line_sha256': sha(line.encode()), 'calls': calls,
                                      'only_loopback': True, 'real_credentials': False, 'query_calls': observation['query_calls'],
                                      'peer_closed': observation['peer_closed'],
                                      'peer_close_events': [e for e in observation['http'] if e['kind'] == 'peer_closed']})
    assert {d['scenario'] for d in deadline_observations} == {
        'deadline-search-body-false', 'deadline-context-body-false', 'deadline-search-body-true',
        'deadline-context-body-true', 'cancel-search', 'cancel-context'}
    write_json('public-deadline-observation-index.json', deadline_observations)
    workflow = blob(HEAD, '.github/workflows/p7-engineering.yml')
    (ROOT / 'p7-engineering-at-head.yml').write_bytes(workflow)
    assert '--test-threads' not in plain and 'RUST_TEST_THREADS' not in workflow.decode()
    assert 'RUST_TEST_THREADS' not in plain
    assert 'rustc 1.95.0 (59807616e 2026-04-14)' in plain
    assert 'assert not target.exists()' in plain
    assert 'CARGO_TARGET_DIR' in plain and 'CARGO_BUILD_BUILD_DIR' in plain
    assert 'Swatinem/rust-cache' not in workflow.decode()
    assert before['source_commit'] in plain
    cases = [t for groups in suites.values() for g in groups for t in g['test_cases']]
    totals = collections.Counter(t['result'] for t in cases)
    assert totals == {'ok': 95, 'ignored': 1}
    groups_summary = {name: {'targets': len(groups), **{k: sum(g['summary'][k] for g in groups) for k in ['passed', 'failed', 'ignored', 'filtered_out']}}
                      for name, groups in suites.items()}
    jobs = json.loads((ROOT / 'jobs-final.json').read_bytes())['structuredContent']['jobs']
    assert len(jobs) == 1 and jobs[0]['id'] == JOB_ID and jobs[0]['run_id'] == RUN_ID and jobs[0]['conclusion'] == 'success'
    assert len(jobs[0]['steps']) == 14 and all(s['conclusion'] == 'success' for s in jobs[0]['steps'])
    summary = {
        'schema_version': 1,
        'reviewer': '/root/ci_evidence',
        'review_method_origin': {'author': '/root/pr_audit', 'original_script_sha256': TEMPLATE_SHA256, 'changes': 'Explicit immutable run/head/job/artifact/event parameters, current checkout identity validation, and actual result-derived summaries. Original evidence belongs to its named earlier reviewer; these two runs are independently parsed here.'},
        'run_event': EVENT, 'run_attempt': run_metadata['run_attempt'],
        'decision': 'accepted_scoped_ci_execution_evidence',
        'review_mode': 'Read-only GitHub terminal API/log/artifact review and independent fixed-Git-blob comparisons; no tests, products or workflows rerun, no repository changes.',
        'repository': 'jyqj/codecortex', 'pull_request': 144, 'run_id': RUN_ID, 'job_id': JOB_ID,
        'run_url': f'https://github.com/jyqj/codecortex/actions/runs/{RUN_ID}',
        'job_conclusion': 'success', 'successful_job_steps': 14,
        'source': {'audited_head': HEAD, 'actual_checkout_commit': before['source_commit'], 'actual_checkout_is_pr_merge': EVENT == 'pull_request',
                   'actual_tree': before['source_tree'], 'audited_head_tree': TREE, 'full_tree_equal': True,
                   'source_before_after_byte_identical': True, 'complete_inputs_match_fixed_audited_head': True,
                   'input_count': 776, 'manifest_sha256': before['manifest_sha256'],
                   'independent_method': 'Direct git ls-tree and cat-file --batch at immutable audited head, SHA256 each complete crate/Cargo blob, compare every uploaded path/hash and canonical manifest.',
                   'original58_test_source_files': 13, 'original58_test_sources_byte_identical_to_5af': True,
                   'source_before_sha256': sha(before_raw), 'source_after_sha256': sha(after_raw)},
        'execution': {'owner': 'GitHub Actions engineering job, not the independent reviewer',
                      'observed_rustc': 'rustc 1.95.0 (59807616e 2026-04-14)',
                      'target_directory': '/home/runner/work/_temp/p7-engineering-cargo',
                      'target_and_build_dir': 'both explicitly set to the same newly absent private job-owned directory; no cache restore in workflow',
                      'cargo_build_jobs': 2, 'incremental': 0, 'dev_debug': 0, 'test_debug': 0, 'rustflags': '-D warnings',
                      'original58_parallelism': 'original libtest scheduling: no --test-threads override in executed command logs, no workflow RUST_TEST_THREADS override; actual numeric scheduler width not separately measured',
                      'reviewed_job_only': True, 'original58_retries_within_reviewed_job': 0, 'original58_ignored': 0,
                      'original58_missing_or_duplicate_cases': 0, 'executed_command_index': 'executed-command-index.json',
                      'new_binary_sha256_receipt': None},
        'original58': {'passed': 58, 'failed': 0, 'ignored': 0,
                       'integration_targets': 11, 'integration_functions': 40, 'encoding_unit_functions': 13, 'execution_unit_functions': 5,
                       'expected_identity_source': receipt_path, 'actual_case_index': 'actual-rust-test-index.json',
                       'coverage_layer': 'Mixed unit/service/public-product-stdio cases; not all 58 are stdio or live provider tests.',
                       'previous_failure_now_passed': 'cc-server/tests/p7_query_deadline_public.rs::real_http_deadline_closes_transport_and_auto_keeps_local_search_and_context',
                       'public_deadline_raw_scenarios': 6, 'public_deadline_observation_index': 'public-deadline-observation-index.json',
                       'prior_evidence_policy': 'Historical source5af normal-schedule 57/1 and same-source serial diagnostic2/0 remain original evidence. This is a distinct fresh-job combined-source run; no claim that the old failure root cause was proven.'},
        'rust_groups': groups_summary,
        'python_input_lock': {'passed': 15, 'failed': 0, 'ignored': 0, 'raw_log': 'input-lock.log', 'scope': 'Synthetic/offline lock mechanism controls; no live provider, release approval or new canonical benchmark execution.'},
        'overall_selected_functions': {'passed': totals['ok'] + len(python_cases), 'failed': totals['FAILED'], 'ignored': totals['ignored'], 'rust_passed': totals['ok'], 'python_passed': len(python_cases),
                                       'not_double_counted_as_tests': '384 worker request rows and six lifecycle scenario receipts are observations inside three test functions.'},
        'ignored': [t for t in cases if t['result'] == 'ignored'],
        'worker_observations': {'seeds': [7, 19, 43], 'phases': ['quiet', 'held'], 'concurrency': [1, 4], 'requests_per_cell': 32, 'total_request_rows': 384,
                                'old_held_input_publications': [x['old_held_input_publications'] for x in worker],
                                'resource_attribution_gate': 'unknown', 'full_task_accepted': False,
                                'scope': 'Actual CodeIndex/post-index worker and local QueryHandle with synthetic provider vectors in the test process; no certified performance or attributed resource result.'},
        'artifact': {'id': meta['id'], 'name': meta['name'], 'zip_file': ARTIFACT.name, 'zip_sha256': sha(ARTIFACT.read_bytes()),
                     'zip_bytes': meta['size_in_bytes'], 'api_digest_matches_download': True, 'members': len(members),
                     'uncompressed_bytes': sum(m['size_bytes'] for m in members.values()), 'member_index': 'artifact-members.json'},
        'limitations': ['No full G7/live quality, real-provider gain/cost, dense-only counterfactual, P7-015 tail/resource acceptance, P7-017 isolated gate acceptance, or P8 release acceptance follows from this scope.',
                        'The ignored actual-stdio mechanism smoke was not executed in this workflow; the separate three-policy actual runner target is not selected.',
                        'Workflow logs preserve executed commands and source snapshots; there is no new standalone compiler-product SHA256 receipt or binary archive to claim.',
                        'Python inputs are covered by the checked-out immutable full tree and reviewed workflow, not by the crate/Cargo-only 776-path hash map.',
                        'No task statuses were changed by this audit.'],
        'raw_files': {name: {'size_bytes': (ROOT / name).stat().st_size, 'sha256': sha((ROOT / name).read_bytes())} for name in ['run-metadata-api.json', 'run-metadata.json', 'jobs-final.json', 'job-log-api.json', 'engineering-job.log', 'artifacts-api.json', 'artifact-download-receipt.json']},
    }
    digest = write_json('independent-review.json', summary)
    print(json.dumps({'review_sha256': digest, 'source': summary['source'], 'original58': summary['original58'],
                      'rust_groups': groups_summary, 'overall_selected_functions': summary['overall_selected_functions'],
                      'artifact': summary['artifact']}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()

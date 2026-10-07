#!/usr/bin/env python3
"""Index saved GitHub CI logs and fixed Git sources; no test/product execution."""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import re
import subprocess


ANSI = re.compile(r'\x1b\[[0-9;]*m')
STAMP = re.compile(r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+Z ')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def write(root, name, value):
    raw = (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
    (root / name).write_bytes(raw)
    return {'path': name, 'bytes': len(raw), 'sha256': sha(raw)}


def api(raw, name):
    value = json.loads((raw / name).read_bytes())
    assert not value.get('isError', False)
    content = value.get('structuredContent', value)
    return json.loads(content['content']) if isinstance(content.get('content'), str) else content


def plain(raw):
    return [ANSI.sub('', STAMP.sub('', line)) for line in raw.decode().splitlines()]


def source(repo, ref):
    def git(*args, data=None):
        return subprocess.check_output(['git', *args], cwd=repo, input=data)
    records = {}
    for entry in git('ls-tree', '-r', '-z', ref, '--', 'crates', 'Cargo.toml', 'Cargo.lock').split(b'\0'):
        if not entry:
            continue
        meta, name = entry.split(b'\t', 1)
        mode, kind, oid = meta.decode().split()
        assert mode in ('100644', '100755') and kind == 'blob'
        records[name.decode()] = oid
    ids = list(dict.fromkeys(records.values()))
    raw = git('cat-file', '--batch', data=('\n'.join(ids) + '\n').encode())
    offset, hashes = 0, {}
    for expected in ids:
        end = raw.index(b'\n', offset)
        oid, kind, size = raw[offset:end].decode().split()
        assert oid == expected and kind == 'blob'
        size, start = int(size), end + 1
        hashes[oid] = sha(raw[start:start + size])
        offset = start + size
        assert raw[offset:offset + 1] == b'\n'
        offset += 1
    assert offset == len(raw)
    inputs = {path: hashes[oid] for path, oid in sorted(records.items())}
    map_raw = (json.dumps(inputs, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
    workflow = git('show', ref + ':.github/workflows/ci.yml')
    tasks = git('show', ref + ':docs/roadmap/code-index-v2/tasks.json')
    return {'source_commit': ref, 'source_tree': git('rev-parse', ref + '^{tree}').decode().strip(),
            'inventory_scope': 'reconstructed_from_fixed_Git_blobs_not_a_downloaded_runner_manifest',
            'input_count': len(inputs), 'manifest_sha256': sha(map_raw), 'inputs': inputs,
            'workflow_sha256': sha(workflow), 'task_sha256': sha(tasks)}, workflow.decode()


def workflow_run_steps(workflow):
    steps = []
    current = None
    block = False
    for line in workflow.splitlines():
        if line.startswith('  msrv:'):
            break
        match = re.match(r'      - name: (.+)', line)
        if match:
            current = {'name': match[1], 'commands': []}
            steps.append(current)
            block = False
        elif current and line.startswith('        run: '):
            value = line[len('        run: '):]
            block = value == '|'
            if not block:
                current['commands'].append(value)
        elif block and line.startswith('          '):
            current['commands'].append(line[10:])
        elif block and line.strip():
            block = False
    return steps


def parse_check(raw, job, workflow):
    lines = plain(raw)
    first = next(i for i, line in enumerate(lines) if line == '##[group]Run cargo fmt --all -- --check')
    starts = [i for i in range(first, len(lines)) if lines[i].startswith('##[group]Run ')]
    declared = workflow_run_steps(workflow)
    assert len(starts) == len(declared) == 28, (len(starts), len(declared))
    api_steps = [step for step in job['steps'] if 5 <= step['number'] <= 32]
    assert len(api_steps) == len(declared)
    steps, groups, python_suites, observations = [], [], [], []
    for offset, (start, definition, actual) in enumerate(zip(starts, declared, api_steps)):
        end = starts[offset + 1] if offset + 1 < len(starts) else len(lines)
        assert definition['name'] == actual['name']
        assert lines[start] == '##[group]Run ' + definition['commands'][0]
        shell = next(i for i in range(start + 1, end) if lines[i].startswith('shell: '))
        displayed_commands = lines[start + 1:shell]
        assert displayed_commands == definition['commands'], (definition['name'], displayed_commands, definition['commands'])
        step = dict(actual, log_start_line=start + 1, log_end_line=end,
                    commands=displayed_commands, raw_log='check-job.log')
        steps.append(step)
        headers, blocks, block = [], [], None
        listing = False
        for i in range(shell + 1, end):
            line = lines[i]
            if line.startswith('COMMAND '):
                listing = '--list' in json.loads(line[len('COMMAND '):])
            match = re.match(r'^\s*Running (.+?) \(([^)]+)\)$', line)
            doc = re.match(r'^\s*Doc-tests (.+)$', line)
            if match or doc:
                header = {'step_number': actual['number'], 'step_name': actual['name'],
                          'target': match[1] if match else 'Doc-tests ' + doc[1],
                          'runner_path': match[2] if match else None,
                          'start_line': i + 1, 'raw_log': 'check-job.log',
                          'listed_not_executed': listing}
                headers.append(header)
            count = re.match(r'^running (\d+) tests?$', line)
            if count:
                assert block is None
                block = {'stdout_start_line': i + 1, 'declared_count': int(count[1]), 'cases': []}
                blocks.append(block)
            case = re.match(r'^test (.+?) \.\.\. (ok|FAILED|ignored)(?:, (.*))?$', line)
            if case:
                assert block is not None
                block['cases'].append({'name': case[1], 'result': case[2],
                                        'ignored_reason': case[3], 'line': i + 1})
            else:
                prefix = re.match(r'^test (.+?) \.\.\. ?(.*)$', line)
                if prefix:
                    assert block is not None and 'pending_case' not in block
                    block['pending_case'] = {'name': prefix[1], 'line': i + 1}
                suffix = re.match(r'^(ok|FAILED|ignored)(?:, (.*))?$', line)
                if suffix and block is not None and 'pending_case' in block:
                    item = block.pop('pending_case')
                    item.update(result=suffix[1], ignored_reason=suffix[2], result_line=i + 1)
                    block['cases'].append(item)
            result = re.match(r'^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in (.+)$', line)
            if result:
                assert block is not None and 'summary' not in block
                block['summary'] = dict(zip(['passed', 'failed', 'ignored', 'measured', 'filtered_out'],
                                            map(int, result.groups()[1:6])),
                                         status=result[1], elapsed=result[7], line=i + 1)
                counted = collections.Counter(case['result'] for case in block['cases'])
                assert 'pending_case' not in block
                assert [counted['ok'], counted['FAILED'], counted['ignored']] == [int(result[n]) for n in (2, 3, 4)], block
                assert len(block['cases']) == block['declared_count'], block
                block = None
            suite = re.match(r'^Ran (\d+) tests in (.+)$', line)
            if suite:
                following = next(value for value in lines[i + 1:end] if value.strip())
                python_suites.append({'step_number': actual['number'], 'step_name': actual['name'],
                                      'count': int(suite[1]), 'elapsed': suite[2],
                                      'result': following, 'line': i + 1})
            if line.startswith('{') and line.endswith('}'):
                try:
                    value = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if isinstance(value, dict) and any(key in value for key in ['verifier_version', 'source_version', 'binary', 'task_count']):
                    observations.append({'step_number': actual['number'], 'line': i + 1, 'value': value})
        assert block is None
        executed_headers = [header for header in headers if not header['listed_not_executed']]
        assert len(executed_headers) == len(blocks), (actual['name'], len(executed_headers), len(blocks))
        for header, stdout in zip(executed_headers, blocks):
            header.update(stdout)
        groups.extend(headers)
    for step in steps:
        executed = [g for g in groups if g['step_number'] == step['number'] and 'summary' in g]
        step['rust_results'] = {name: sum(g['summary'][name] for g in executed)
                                for name in ['passed', 'failed', 'ignored', 'measured', 'filtered_out']}
        step['rust_target_executions'] = len(executed)
    return {'steps': steps, 'rust_groups': groups, 'python_suites': python_suites,
            'structured_log_observations': observations,
            'count_scope': 'executions; repeated commands remain repeated and are not unique tests',
            'step_mapping': 'Exact ordered 28 workflow command blocks and full printed commands; API timestamps alone are ambiguous within one second.',
            'target_mapping': 'Cargo stderr target headers and complete libtest stdout blocks are paired FIFO within each step; intervening later Running headers never steal a previous stdout block. Explicit COMMAND --list headers are excluded from executed pairing. Each block named count equals declared count and summary.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--raw-dir', type=Path, required=True)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    run = api(args.raw_dir, 'run-api.json')
    jobs = api(args.raw_dir, 'jobs-full-api.json')
    artifact_data = api(args.raw_dir, 'artifacts-api.json')
    checkout_data = api(args.raw_dir, 'checkout-commit-api.json')
    assert run['status'] == 'completed' and run['conclusion'] == 'success'
    assert jobs['total_count'] == len(jobs['jobs']) == 3
    assert all(job['status'] == 'completed' and job['conclusion'] == 'success' for job in jobs['jobs'])
    identity, workflow = source(args.repo, run['head_sha'])
    assert identity['source_tree'] == checkout_data['tree']['sha'] == run['head_commit']['tree_id']
    parsed, checkouts = None, []
    for job in jobs['jobs']:
        raw = json.loads((args.raw_dir / (job['name'] + '-job-log-api.json')).read_bytes())['structuredContent']['content'].encode()
        (args.raw_dir / (job['name'] + '-job.log')).write_bytes(raw)
        lines = plain(raw)
        indexes = [i for i, line in enumerate(lines) if line == '[command]/usr/bin/git log -1 --format=%H']
        assert len(indexes) == 1
        checkout = lines[indexes[0] + 1]
        assert re.fullmatch('[0-9a-f]{40}', checkout) and checkout == checkout_data['sha']
        checkouts.append({'job_id': job['id'], 'name': job['name'], 'checkout': checkout,
                          'line': indexes[0] + 2, 'raw_log': job['name'] + '-job.log',
                          'log_bytes': len(raw), 'log_sha256': sha(raw)})
        if job['name'] == 'check':
            parsed = parse_check(raw, job, workflow)
    identity.update({'observed_checkout': checkout_data['sha'], 'checkout_tree': checkout_data['tree']['sha'],
                     'checkout_parents': [p['sha'] for p in checkout_data['parents']], 'checkout_logs': checkouts})
    write(args.output_dir, 'git-source-identity.json', identity)
    write(args.output_dir, 'step-and-test-index.json', parsed)
    write(args.output_dir, 'run-summary.json', {
        'run_id': run['id'], 'run_attempt': run['run_attempt'], 'event': run['event'],
        'source_commit': run['head_sha'], 'url': run['html_url'],
        'status': run['status'], 'conclusion': run['conclusion'],
        'started_at': run['run_started_at'], 'completed_updated_at': run['updated_at'],
        'jobs': jobs['jobs'], 'artifacts_count': artifact_data['total_count'],
        'binary_or_full_build_receipt_artifacts_available': False,
        'scope': 'This actual run only. No current-task, complete P7-017, G7, live or release acceptance.'})
    print(json.dumps({'run': run['id'], 'source': run['head_sha'], 'checkout': checkout_data['sha'],
                      'steps': len(parsed['steps']), 'rust_target_executions': sum('summary' in g for g in parsed['rust_groups']),
                      'python_suites': parsed['python_suites'], 'source_inputs': identity['input_count']}, ensure_ascii=False))


if __name__ == '__main__':
    main()

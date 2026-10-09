#!/usr/bin/env python3
"""Inspect the original fixed-275e failure-gate artifact; no Cargo/native execution."""
from pathlib import Path, PurePosixPath
import ast
import hashlib
import json
import re
import stat
import subprocess
import sys
import zipfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parent / 'source'
SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
TREE = '5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46'
RUN, JOB, ARTIFACT = 37890756917, 113690916001, 11598517678
ZIP_BYTES = 33005224
ZIP_SHA = 'f2fddad2d95b53731862b87a80fbeb6dacca5da46ceb3e00f0ecebe04d1d173b'

def require(ok, message):
    if not ok:
        raise ValueError(message)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result

def reject_constant(value):
    raise ValueError('nonfinite JSON constant ' + value)

def read_json(data):
    return json.loads(data, object_pairs_hook=pairs, parse_constant=reject_constant)

def file_sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()

def main():
    run = read_json((HERE / 'run.json').read_bytes())
    jobs = read_json((HERE / 'jobs.json').read_bytes())
    artifacts = read_json((HERE / 'artifacts.json').read_bytes())
    require(run['id'] == RUN and run['run_attempt'] == 1 and run['head_sha'] == SOURCE
            and run['status'] == 'completed' and run['conclusion'] == 'success', 'run identity/status')
    require(jobs['total_count'] == len(jobs['jobs']) == 1, 'complete job population')
    job = jobs['jobs'][0]
    require(job['id'] == JOB and job['run_id'] == RUN and job['name'] == 'gate_controls'
            and job['status'] == 'completed' and job['conclusion'] == 'success', 'job identity/status')
    require(artifacts['total_count'] == len(artifacts['artifacts']) == 1, 'complete artifact population')
    artifact = artifacts['artifacts'][0]
    require(artifact['id'] == ARTIFACT and artifact['name'] == 'p8-gates-' + SOURCE
            and artifact['size_in_bytes'] == ZIP_BYTES and artifact['digest'] == 'sha256:' + ZIP_SHA
            and artifact['expired'] is False and artifact['workflow_run']['id'] == RUN
            and artifact['workflow_run']['head_sha'] == SOURCE, 'artifact metadata identity')
    job_log = (HERE / f'job-{JOB}.log').read_text()
    require('git checkout --progress --force ' + SOURCE in job_log
            and re.search(r'git log -1 --format=%H\n[^\n]*' + SOURCE, job_log)
            and 'Artifact ID ' + str(ARTIFACT) in job_log, 'original checkout/upload log')
    archive = HERE / f'artifact-{ARTIFACT}.zip'
    require(archive.stat().st_size == ZIP_BYTES and file_sha(archive) == ZIP_SHA, 'original ZIP bytes')
    # Verify the original read-only source snapshot helper before importing it.
    script = REPO / 'scripts/p7_build_identity.py'
    require(script.read_bytes() == subprocess.check_output(
        ['git', '-C', str(REPO), 'show', SOURCE + ':scripts/p7_build_identity.py']), 'fixed snapshot helper')
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(REPO / 'scripts'))
    from p7_build_identity import source_snapshot
    source = source_snapshot(REPO)
    require(source['source_commit'] == SOURCE and source['source_tree'] == TREE
            and source['input_count'] == len(source['inputs']) == 1089, 'complete fixed source')
    observer = (REPO / 'scripts/p8_gate_controls.py').read_bytes()
    require(observer == subprocess.check_output(
        ['git', '-C', str(REPO), 'show', SOURCE + ':scripts/p8_gate_controls.py']), 'fixed gate observer')
    case_nodes = [node.value for node in ast.parse(observer).body if isinstance(node, ast.Assign)
                  and any(isinstance(t, ast.Name) and t.id == 'CASES' for t in node.targets)]
    require(len(case_nodes) == 1, 'original case declaration')
    cases = ast.literal_eval(case_nodes[0])
    inventory, summaries = {}, []
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            name = info.filename
            path = PurePosixPath(name)
            require(name and not info.is_dir() and not path.is_absolute()
                    and path.as_posix() == name and '\\' not in name and '\0' not in name
                    and '..' not in path.parts and name not in inventory
                    and not (info.flag_bits & 1), 'unsafe ZIP member')
            require(stat.S_IFMT(info.external_attr >> 16) in (0, stat.S_IFREG), 'nonregular ZIP member')
            h, count = hashlib.sha256(), 0
            with z.open(info) as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    h.update(block)
                    count += len(block)
            require(count == info.file_size, 'ZIP size/CRC')
            inventory[name] = dict(bytes=count, sha256=h.hexdigest(), crc32=f'{info.CRC:08x}')
        read = lambda name: read_json(z.read(name))
        report = read('report.json')
        require(report['source'] == source and report['status'] == 'passed_original_failure_gates'
                and report['exit_code'] == 0 and report['task_complete'] is False
                and report['release_approval'] is False, 'original terminal report')
        require(report['files'] == {name: row['sha256'] for name, row in inventory.items()
                                    if name != 'report.json'}, 'complete retained file manifest')
        require(len(report['cases']) == len(cases) == 6, 'six original Rust controls')
        cli = report['cli']
        require(cli['executable_sha256'] == inventory['cc-eval']['sha256'], 'retained original CLI')
        for index, (case, name) in enumerate(zip(report['cases'], cases)):
            prefix = f'case-{index:02}/'
            require(case == read(prefix + 'test-receipt.json') and case['source'] == source
                    and case['test_name'] == name and case['status'] == 'passed'
                    and case['package'] == 'cc-eval' and case['target'] == 'benchmark_cli', 'bound case receipt')
            require(case['executable_sha256'] == inventory[prefix + 'fault-test']['sha256'], 'retained test executable')
            for phase, timeout in [('build', 1800), ('execution', 300)]:
                receipt = read(prefix + phase + '/command.json')
                require(receipt == case[phase] and receipt['exit_code'] == 0 and receipt['status'] == 'passed'
                        and receipt['timeout_seconds'] == timeout, 'original phase receipt')
                require(receipt['logs_sha256'] == {file: inventory[prefix + phase + '/' + file]['sha256']
                                                  for file in ('stdout.log', 'stderr.log')}, 'original phase logs')
            require(case['build']['command'] == ['/home/runner/.cargo/bin/cargo', 'test', '-p', 'cc-eval',
                    '--locked', '--offline', '--no-run', '--message-format=json-render-diagnostics',
                    '--test', 'benchmark_cli'], 'original Cargo argv')
            require(case['execution']['command'] == [f'/home/runner/work/_temp/p8-gates/case-{index:02}/fault-test',
                    '--exact', name, '--nocapture', '--test-threads=1'], 'original test argv')
            output = z.read(prefix + 'execution/stdout.log').decode()
            require('test ' + name + ' ... ok' in output
                    and re.search(r'test result: ok\. 1 passed; 0 failed; 0 ignored;', output), 'one actual nonignored test')
            cargo = [read_json(line) for line in z.read(prefix + 'build/stdout.log').splitlines() if line]
            require(cargo[-1] == {'reason': 'build-finished', 'success': True}, 'Cargo completion')
            selected = [row for row in cargo if row.get('reason') == 'compiler-artifact'
                        and row.get('target', {}).get('name') == 'benchmark_cli'
                        and row.get('profile', {}).get('test') is True and row.get('executable')]
            require(selected == [case['cargo_artifact']], 'unique actual Cargo test artifact')
            selected_cli = [row for row in cargo if row.get('reason') == 'compiler-artifact'
                            and row.get('target', {}).get('name') == 'cc-eval'
                            and row.get('target', {}).get('kind') == ['bin'] and row.get('executable')]
            require(len(selected_cli) == 1 and selected_cli[0]['executable'] == cli['path'], 'actual CLI Cargo path')
            if index == 0:
                require(selected_cli[0] == cli['cargo_artifact'], 'original CLI Cargo receipt')
            summaries.append(dict(test=name, actual_passed=1, ignored=0, build_exit=0, test_exit=0,
                                  original_test_executable_sha256=case['executable_sha256']))
        comparisons = {name: read(name) for name in inventory if name.endswith('comparison.json')}
        require(len(comparisons) == 5, 'five preserved comparison outputs')
        output_checks = [('case-02/', 'quality-comparison.json', 'failed', 1, 'Top-1 regression'),
                         ('case-02/', 'latency-comparison.json', 'failed', 1, 'p95 regression'),
                         ('case-03/', 'comparison.json', 'inconclusive', 1, 'insufficient latency samples'),
                         ('case-04/', 'comparison.json', 'invalid_measurement', 2, 'raw response drift'),
                         ('case-05/', 'comparison.json', 'invalid_measurement', 2, 'JSON:')]
        comparison_summary = []
        for prefix, suffix, status, code, reason in output_checks:
            matches = [(name, row) for name, row in comparisons.items() if name.startswith(prefix) and name.endswith('/' + suffix)]
            require(len(matches) == 1, 'unique original comparison')
            name, row = matches[0]
            require(row['status'] == status and row['exit_code'] == code
                    and any(reason in value for value in row.get('reasons', []) + row.get('inconclusive_reasons', [])),
                    'original deliberately failing comparison')
            comparison_summary.append(dict(path=name, status=status, original_report_exit_code=code))
        drift = [name for name in inventory if name.startswith('case-04/') and name.endswith('/candidate/raw/000000.json')]
        bad_policy = [name for name in inventory if name.startswith('case-05/') and name.endswith('/policy.json')]
        require(len(drift) == len(bad_policy) == 1 and z.read(drift[0]) == b'changed raw evidence\n'
                and z.read(bad_policy[0]) == b'broken JSON', 'unrepaired invalid raw/policy preserved')
    require(source_snapshot(REPO) == source, 'source unchanged through offline reception')
    result = dict(decision='accepted_scoped_original_275e_failure_gate_evidence', source_commit=SOURCE,
                  source_tree=TREE, source_inputs=1089, source_manifest_sha256=source['manifest_sha256'],
                  run_id=RUN, run_attempt=1, job_id=JOB, artifact_id=ARTIFACT, artifact_bytes=ZIP_BYTES,
                  artifact_sha256=ZIP_SHA, member_count=len(inventory),
                  member_bytes=sum(row['bytes'] for row in inventory.values()), all_member_CRC_SHA_verified=True,
                  original_manifest_complete=True, original_tests=summaries, original_comparisons=comparison_summary,
                  cli_sha256=cli['executable_sha256'], actual_original_cli_invocations_asserted_by_tests=6,
                  original_distinct_comparison_outputs=5, original_broken_raw_and_policy_preserved=True,
                  job_log_sha256=file_sha(HERE / f'job-{JOB}.log'),
                  limitations=['Offline reading and original source snapshot only; no Cargo or native code executed by this receiver.',
                               'Six original Rust tests assert actual CLI process exits in CI. Five comparison outputs survive; bad-policy output refusal invokes that CLI twice and preserves its first output.',
                               'These are synthetic deterministic failure-gate fixtures, not retrieval-quality or performance measurements.',
                               'Dependency tasks remain open. This scoped evidence does not close P8-013 or grant release approval.'],
                  TODO_closed=0, TODO_remaining=29)
    for name, value in [('member-hashes.json', inventory), ('inspection.json', result)]:
        (HERE / name).write_text(json.dumps(value, sort_keys=True, indent=2) + '\n')
    print(json.dumps(dict(decision=result['decision'], original_tests=6, comparison_outputs=5,
                          inspection_sha256=file_sha(HERE / 'inspection.json'), TODO_closed=0, TODO_remaining=29)))

if __name__ == '__main__':
    main()

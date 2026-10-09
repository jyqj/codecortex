#!/usr/bin/env python3
"""Append the round-10 P8-017 evidence after the actual G5 CLI has passed.

Usage: python3 -B prepare-round10-docs.py R5_FULL_SHA G5_FULL_SHA
This script has no dry-run mode: the root agent must review it before execution.
It changes only tasks.json and the five original plan output documents, leaves
HEAD/index/source/guards alone, and preserves original command failures in scratch.
"""

import argparse
import collections
import copy
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


ROOT = Path('/workspace/scratch/50c364fd60b1/codecortex')
ROUND = Path('/workspace/scratch/50c364fd60b1/validation/lane-ownership-round10')
OUT = ROUND / 'round10-docs'
P5 = 'fa6562ad70c6a6f2a7a1e65594df23f807e62f18'
P5_TREE = '0e6fdcfd2ac7a727184daf5a0e290cf00c55abbc'
TASK_PATH = 'docs/roadmap/code-index-v2/tasks.json'
TASK_SHA = 'e75c68981943d85e31f8830331670f27401d3ab5ca13ae339d2134a693f23063'
PLAN_PATH = 'scripts/code_index_plan.py'
PLAN_CHECK_PATH = 'docs/roadmap/code-index-v2/PLAN-CHECK.json'
R5_PREFIX = 'artifacts/checkpoints/p8-lane-registry-20261009-50c'
POST_PREFIX = 'artifacts/checkpoints/p8-lane-registry-binding-20261009-50c'
EXPECTED_PATHS = {
    'README.md', TASK_PATH, PLAN_CHECK_PATH,
    'docs/roadmap/code-index-v2/05-TODO.md',
    'docs/roadmap/code-index-v2/08-HANDOFF.md',
    'docs/roadmap/code-index-v2/README.md',
}
ENV = {**os.environ, 'GIT_NO_LAZY_FETCH': '1', 'GIT_OPTIONAL_LOCKS': '0',
       'PYTHONDONTWRITEBYTECODE': '1'}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def blob(raw):
    return hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()


def encode_json(value):
    # This is the existing tasks.json byte format; do not sort its keys.
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def save_json(path, value):
    path.write_bytes(encode_json(value))


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, env=ENV)


def git_text(*args):
    return git(*args).decode('utf-8').strip()


def changed_paths():
    return sorted(p.decode('utf-8') for p in
                  git('diff', '--name-only', '--no-renames', '-z', 'HEAD', '--').split(b'\0') if p)


def guard_identity():
    return {
        'head': git_text('rev-parse', 'HEAD'),
        'tree': git_text('rev-parse', 'HEAD^{tree}'),
        'registry_sha256': sha((ROOT / 'scripts/reviewed-source-registry-v15.json').read_bytes()),
        'verifier_sha256': sha((ROOT / 'scripts/verify_reviewed_source_v15.py').read_bytes()),
    }


def read_json(path):
    return json.loads(path.read_bytes())


def preflight(r5, g5):
    require(not OUT.exists(), 'round10-docs already exists; preserve that attempt and review it first')
    require(git_text('rev-parse', 'HEAD') == g5, 'HEAD must be the supplied fixed G5')
    require(git('status', '--porcelain=v1', '-z', '--untracked-files=no') == b'',
            'tracked worktree and index must be clean before the append')
    require(git_text('rev-parse', r5 + '^{commit}') == r5, 'R5 must be an available commit')
    require(git_text('rev-parse', g5 + '^{commit}') == g5, 'G5 must be an available commit')

    cli_path = ROUND / 'G5-v15-cli/receipt.json'
    cli = read_json(cli_path)
    require(cli.get('source') == g5 and cli.get('product_source') == P5
            and cli.get('review_source') == r5, 'G5 CLI receipt source identities differ')
    require(cli.get('status') == 'completed' and cli.get('exit_code') == 0
            and cli.get('passed') is True and cli.get('source_unchanged') is True
            and bool(cli.get('completed_at_utc')), 'actual G5 CLI has not passed terminally')
    require(cli.get('source_before') == cli.get('source_after'), 'G5 CLI before/after identities differ')
    expected_cli = ['-B', 'scripts/verify_reviewed_source_v15.py',
                    '--source-version', 'p8-completion-source-20261009-v15']
    require(isinstance(cli.get('command'), list) and cli['command'][1:] == expected_cli
            and cli.get('timeout_seconds') == 3600, 'G5 CLI was not the original bounded invocation')
    current = guard_identity()
    require(cli['source_after'] == {**current, 'tracked_changes_from_G5': []},
            'current G5 source does not equal the actual CLI terminal source')
    cli_log = ROUND / 'G5-v15-cli/v15-cli.log'
    require(sha(cli_log.read_bytes()) == cli.get('log_sha256'), 'G5 CLI log digest differs')

    eng_path = ROUND / 'engineering-checks/receipt.json'
    eng = read_json(eng_path)
    require(eng.get('expected_product_source') == P5 and eng.get('all_selected_commands_passed') is True
            and eng.get('source_inputs_equal') is True and bool(eng.get('completed_at_utc')),
            'P5 engineering receipt is not terminally passed')
    require(eng.get('source_before') == eng.get('source_after'), 'P5 engineering inputs changed')
    identity = eng['source_before']
    require(identity.get('source_commit') == P5 and identity.get('source_tree') == P5_TREE
            and identity.get('input_count') == 1092 and len(identity.get('inputs', {})) == 1092,
            'P5 engineering input identity differs')
    names = ['format', 'clippy', 'search-lib', 'mechanism-anchor', 'mechanism-prepare']
    commands = eng.get('commands', [])
    require([row.get('name') for row in commands] == names, 'expected exactly the five P5 commands')
    expected_argv = [
        ['cargo', 'fmt', '--all', '--', '--check'],
        ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'],
        ['cargo', 'test', '-p', 'cc-search', '--lib', '--locked', '--offline'],
        ['cargo', 'test', '-p', 'cc-eval', '--lib', '--locked', '--offline',
         'fixed_controls_match_current_product_source_once'],
    ]
    logs = {}
    for index, row in enumerate(commands):
        require(row.get('exit_code') == 0 and row.get('head_before') == P5 and row.get('head_after') == P5,
                'P5 engineering command did not pass on the fixed source: ' + row['name'])
        require(row.get('log') == row['name'] + '.log', 'unexpected engineering log location')
        raw = (ROUND / 'engineering-checks' / row['log']).read_bytes()
        require(sha(raw) == row.get('log_sha256'), 'engineering original log digest differs: ' + row['name'])
        logs[row['name']] = raw.decode('utf-8')
        if index < 4:
            require(row.get('command') == expected_argv[index], 'engineering argv differs: ' + row['name'])
    require('test result: ok. 303 passed; 0 failed; 0 ignored;' in logs['search-lib'],
            'the actual search log does not establish 303/0/0')
    require('test result: ok. 1 passed; 0 failed; 0 ignored;' in logs['mechanism-anchor'],
            'the actual anchor log does not establish 1/0/0')
    prepare_argv = commands[4]['command']
    require(prepare_argv[1:] == ['-B', 'scripts/p7_mechanism_build.py', '--source', str(ROOT),
                                '--expected-head', P5, '--output', str(ROUND / 'mechanism-prepare'),
                                '--prepare-only'], 'the original mechanism command was not prepare-only on P5')
    prepared_path = ROUND / 'mechanism-prepare/prepared-snapshots.json'
    prepared = read_json(prepared_path)
    require(prepared.get('compiled') is False and prepared.get('build', {}).get('source_commit') == P5,
            'mechanism preparation is not the fixed uncompiled P5 record')
    variants = prepared['build'].get('variants', [])
    require(len(variants) == 4 and {v.get('id') for v in variants} == {'none', 'local', 'dense_only', 'hybrid'},
            'expected four original prepare-only variants')

    raw = git('show', P5 + ':' + TASK_PATH)
    require(sha(raw) == TASK_SHA and (ROOT / TASK_PATH).read_bytes() == raw,
            'tasks.json must remain exactly the original P5 bytes before appending')
    data = json.loads(raw)
    require(encode_json(data) == raw, 'original tasks.json formatting is not the expected indent-2/LF format')
    require(len(data['tasks']) == data['task_count'] == 192, 'original task count differs')
    counts = dict(collections.Counter(t['status'] for t in data['tasks']))
    require(counts == {'done': 163, 'in_progress': 16, 'todo': 12, 'blocked': 1}, 'original status counts differ')
    target = [t for t in data['tasks'] if t['id'] == 'P8-017']
    require(len(target) == 1 and isinstance(target[0]['implementation_notes'], str)
            and isinstance(target[0]['evidence'], list), 'P8-017 append fields differ')
    require(target[0]['status'] == 'in_progress' and target[0]['depends_on'] == ['P8-016'],
            'original P8-017 status or hard dependency differs')
    require((ROOT / PLAN_PATH).read_bytes() == git('show', P5 + ':' + PLAN_PATH),
            'plan script differs from the original P5 generator')
    return raw, data, current, {
        'G5_cli': {'receipt_sha256': sha(cli_path.read_bytes()), 'log_sha256': cli['log_sha256'],
                   'completed_at_utc': cli['completed_at_utc'], 'source': g5, 'review_source': r5},
        'P5_engineering': {'receipt_sha256': sha(eng_path.read_bytes()), 'source': P5,
                           'completed_at_utc': eng['completed_at_utc'], 'five_commands_passed': True},
        'mechanism_preparation': {'sha256': sha(prepared_path.read_bytes()), 'source': P5,
                                  'variant_count': 4, 'compiled': False},
        'original_plan_script_sha256': sha((ROOT / PLAN_PATH).read_bytes()),
    }


def append_record(data, r5, g5):
    changed = copy.deepcopy(data)
    target = next(t for t in changed['tasks'] if t['id'] == 'P8-017')
    evidence = {
        'date': '2026-10-09', 'round': 10, 'target_sha': P5, 'review_sha': r5, 'guard_sha': g5,
        'status': 'scoped_engineering_accepted_full_task_open',
        'artifact_paths': [
            R5_PREFIX + '/README.md', R5_PREFIX + '/independent-source-review.json',
            R5_PREFIX + '/semantic-independent-review-v2.json',
            R5_PREFIX + '/engineering-checks/receipt.json',
            R5_PREFIX + '/engineering-results-independent-review.json',
            R5_PREFIX + '/mechanism-prepare/prepared-snapshots.json',
            POST_PREFIX + '/README.md', POST_PREFIX + '/G5-v15-cli/receipt.json',
            POST_PREFIX + '/round10-docs/',
        ],
        'scope': 'Three-file default-lane ownership cleanup on P5; actual bounded engineering, four uncompiled mechanism snapshots, and the original v15 CLI on G5. Original P8-016 dependency and complete V18/V21 acceptance remain open.',
        'review': 'Independent non-author source review in R5 and separately recorded actual P5 engineering evidence; G5 CLI keeps its own guard-source identity.',
        'rollback_status': 'Revert the three scoped product/control-fixture changes together if needed; retain fixed P5/R5/G5 records and all original failure identities.',
        'summary': 'One borrowed default-lane registry serves execution and QueryPolicy; five-lane order, intent roles, budgets, semantic append, wire fields and policy fingerprints are preserved. Local ablation clears only the execution Vec. P5 fmt and workspace/all-target Clippy passed; cc-search lib 303/0/0 and the existing exact mechanism-anchor test 1/0/0 passed. Four prepare-only variants remain compiled=false. The original bounded G5 v15 CLI passed. Full workspace tests and mechanism builds/studies are not claimed; ledger remains 163/192 done, 29 remaining, zero newly closed original tasks.',
    }
    suffix = (
        '\n\n2026-10-09 第10轮，产品 ' + P5 + '、独立源码审查 ' + r5 + '、原 v15 守卫 ' + g5 +
        '：完成默认 lane 目录所有权收口。lanes.rs 的单一静态借用 registry 同时供执行 wrapper 与 QueryPolicy 使用；'
        '保留五个 lane 的顺序、intent 角色、timeout/budget、semantic 追加、wire 标签和 policy fingerprint。'
        '共改 lanes.rs、query_policy.rs 及既有 mechanism_controls.json 三个文件；local ablation 仍只清空执行 Vec，保留 registry/policy 义务，semantic control 不变。'
        'P5 实跑 fmt、全 workspace/all-target 严格 Clippy、cc-search --lib 303 passed/0 failed/0 ignored 和原 exact mechanism-anchor 测试 1 passed；'
        '原 mechanism builder 仅 prepare-only，四种快照全部 compiled=false，不声称完成编译或机制实验。'
        'G5 原 v15 CLI 按原 source-version 和 3600 秒边界实际 exit 0，前后输入一致；该结果保留 G5 身份，未重标为 P5 Rust 执行。'
        '原 P8-016 硬依赖与完整 V18/V21 验收仍开放，P8-017 保持 in_progress；全账本仍 163/192 done、剩余 29、本轮完全关闭原 TODO 0。'
        '证据见 ' + R5_PREFIX + '/README.md 与 ' + POST_PREFIX + '/README.md。'
    )
    target['evidence'].append(evidence)
    target['implementation_notes'] += suffix
    return changed, evidence, suffix


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('r5', help='fixed review commit, exactly 40 lowercase hexadecimal characters')
    parser.add_argument('g5', help='fixed guard commit, exactly 40 lowercase hexadecimal characters')
    args = parser.parse_args()
    require(all(re.fullmatch('[0-9a-f]{40}', value) for value in (args.r5, args.g5)), 'full R5 and G5 SHAs required')
    original, data, identity, inputs = preflight(args.r5, args.g5)
    changed, evidence, suffix = append_record(data, args.r5, args.g5)
    candidate = encode_json(changed)
    undone = copy.deepcopy(changed)
    undo_target = next(t for t in undone['tasks'] if t['id'] == 'P8-017')
    require(undo_target['evidence'].pop() == evidence and undo_target['implementation_notes'].endswith(suffix),
            'cannot reverse the two exact appends')
    undo_target['implementation_notes'] = undo_target['implementation_notes'][:-len(suffix)]
    require(encode_json(undone) == original, 'reverse append did not reconstruct the whole original JSON bytes')
    changed_ids = [old['id'] for old, new in zip(data['tasks'], changed['tasks']) if old != new]
    require(changed_ids == ['P8-017'], 'unexpected task changes')
    for old, new in zip(data['tasks'], changed['tasks']):
        require({k: v for k, v in old.items() if k not in {'evidence', 'implementation_notes'}} ==
                {k: v for k, v in new.items() if k not in {'evidence', 'implementation_notes'}},
                'task definitions/status/dependencies changed')
    require({k: v for k, v in data.items() if k != 'tasks'} ==
            {k: v for k, v in changed.items() if k != 'tasks'}, 'top-level ledger fields changed')

    OUT.mkdir()
    checks = OUT / 'plan-checks'
    checks.mkdir()
    receipt = {
        'started_at_utc': now(), 'status': 'running', 'product_source': P5,
        'review_source': args.r5, 'guard_source': args.g5, 'source_before': identity,
        'execution_identity': 'G5 HEAD with prospective six-document changes; not an execution on a future publication commit.',
        'input_receipts': inputs, 'commands': [], 'counts': {'total': 192, 'done': 163, 'remaining': 29, 'newly_closed': 0},
    }
    receipt_path = OUT / 'execution-receipt.json'
    save_json(receipt_path, receipt)
    try:
        require(guard_identity() == identity and git('status', '--porcelain=v1', '-z', '--untracked-files=no') == b'',
                'source or tracked worktree changed during preflight')
        (ROOT / TASK_PATH).write_bytes(candidate)
        for name, argv in [('plan-write', [sys.executable, '-B', PLAN_PATH, '--write']),
                           ('plan-check', [sys.executable, '-B', PLAN_PATH])]:
            row = {'name': name, 'command': argv, 'cwd': str(ROOT), 'timeout_seconds': 300,
                   'started_at_utc': now(), 'head_before': git_text('rev-parse', 'HEAD'), 'status': 'running',
                   'stdout_path': 'plan-checks/' + name + '.stdout.log',
                   'stderr_path': 'plan-checks/' + name + '.stderr.log'}
            receipt['commands'].append(row)
            save_json(receipt_path, receipt)
            started = time.monotonic()
            stdout_path, stderr_path = OUT / row['stdout_path'], OUT / row['stderr_path']
            with stdout_path.open('xb') as stdout, stderr_path.open('xb') as stderr:
                try:
                    result = subprocess.run(argv, cwd=ROOT, env=ENV, stdout=stdout, stderr=stderr, timeout=300)
                    row.update(status='completed', exit_code=result.returncode)
                except subprocess.TimeoutExpired:
                    row.update(status='timeout', exit_code=None)
            row.update(completed_at_utc=now(), elapsed_seconds=time.monotonic() - started,
                       head_after=git_text('rev-parse', 'HEAD'), stdout_sha256=sha(stdout_path.read_bytes()),
                       stderr_sha256=sha(stderr_path.read_bytes()))
            save_json(checks / (name + '.receipt.json'), row)
            save_json(receipt_path, receipt)
            require(row['status'] == 'completed' and row['exit_code'] == 0,
                    name + ' failed; original output is preserved, with no retry or repair')
            require(row['head_before'] == row['head_after'] == args.g5, 'HEAD moved during original plan invocation')
            report = read_json(stdout_path)
            require(report.get('status') == 'passed' and report.get('task_count') == 192
                    and report.get('task_sha256') == sha(candidate) and report.get('views') == 4
                    and report.get('states') == {'done': 163, 'in_progress': 16, 'todo': 12, 'blocked': 1},
                    'original plan stdout does not describe the expected unchanged ledger state')
            if name == 'plan-check':
                (ROOT / PLAN_CHECK_PATH).write_bytes(stdout_path.read_bytes())

        require((ROOT / TASK_PATH).read_bytes() == candidate, 'generator changed the appended task bytes')
        require(guard_identity() == identity, 'HEAD/tree/guard changed during document preparation')
        require(git('diff', '--cached', '--name-only', '-z') == b'', 'index changed during document preparation')
        paths = changed_paths()
        require(set(paths) == EXPECTED_PATHS, 'changed paths are not exactly the six authorized documents: ' + repr(paths))
        inventory = []
        for path in paths:
            before = git('show', args.g5 + ':' + path)
            after = (ROOT / path).read_bytes()
            inventory.append({'path': path, 'before': {'git_blob': blob(before), 'size': len(before), 'sha256': sha(before)},
                              'after': {'git_blob': blob(after), 'size': len(after), 'sha256': sha(after)}})
        original_target = next(t for t in data['tasks'] if t['id'] == 'P8-017')
        proof = {
            'schema_version': 1, 'prepared_at_utc': now(), 'product_source': P5, 'review_source': args.r5,
            'guard_source': args.g5, 'actual_execution_head': args.g5,
            'scope': 'Exactly one evidence append and one string suffix on P8-017; generated progress views only. Original task acceptance remains open.',
            'task_path': TASK_PATH, 'before': {'sha256': sha(original), 'git_blob': blob(original), 'size': len(original)},
            'after': {'sha256': sha(candidate), 'git_blob': blob(candidate), 'size': len(candidate)},
            'changed_task_ids': changed_ids, 'unchanged_other_task_count': 191,
            'appended_evidence_count': 1, 'appended_evidence': evidence,
            'previous_evidence_count': len(original_target['evidence']),
            'notes_suffix': suffix, 'notes_suffix_sha256': sha(suffix.encode('utf-8')),
            'previous_notes_sha256': sha(original_target['implementation_notes'].encode('utf-8')),
            'whole_original_json_reconstructed_byte_for_byte': True,
            'definitions_status_dependencies_and_top_fields_unchanged': True,
            'counts': receipt['counts'], 'changed_paths': inventory,
            'publication': {'review_prefix': R5_PREFIX, 'post_prefix': POST_PREFIX,
                            'post_artifacts': 'To be published by root with the original G5 CLI and these document receipts; not written by this script.'},
        }
        save_json(OUT / 'task-append-proof.json', proof)
        save_json(OUT / 'changed-files.json', {'actual_execution_head': args.g5, 'changed_paths': inventory})
        receipt.update(status='completed', passed=True, completed_at_utc=now(), source_after=guard_identity(),
                       tracked_changed_paths=paths, task_append_proof_sha256=sha((OUT / 'task-append-proof.json').read_bytes()))
        save_json(receipt_path, receipt)
        print(json.dumps({'status': 'passed', 'actual_execution_head': args.g5, 'task_sha256': sha(candidate),
                          'counts': receipt['counts'], 'changed_paths': paths, 'output': str(OUT)}, ensure_ascii=False))
    except BaseException as error:
        receipt.update(status='failed', passed=False, completed_at_utc=now(),
                       error={'type': type(error).__name__, 'message': str(error)})
        save_json(receipt_path, receipt)
        raise


if __name__ == '__main__':
    main()

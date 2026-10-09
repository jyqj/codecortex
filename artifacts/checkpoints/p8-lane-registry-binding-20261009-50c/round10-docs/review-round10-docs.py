#!/usr/bin/env python3
"""Read-only review of exact Round10 task append, views and existing receipts."""
from collections import Counter
from copy import deepcopy
from datetime import datetime, timezone
from pathlib import Path
import hashlib
import json
import os
import subprocess

ROOT = Path('/workspace/scratch/50c364fd60b1')
REPO = ROOT / 'codecortex'
V = ROOT / 'validation/lane-ownership-round10'
D = V / 'round10-docs'
G5 = '27d6e8294e4b7ae337ee978ffada5b2dbeb43f31'
R5 = '365e2cc503a0119b2fba3cf98ce38e5aa65cfcd4'
P5 = 'fa6562ad70c6a6f2a7a1e65594df23f807e62f18'
BEFORE_SHA = 'e75c68981943d85e31f8830331670f27401d3ab5ca13ae339d2134a693f23063'
AFTER_SHA = 'd51f61a920d887d375e95ca529509d0750233ea22477e40f7d43b3caa985c495'
R_PREFIX = 'artifacts/checkpoints/p8-lane-registry-20261009-50c'
POST_PREFIX = 'artifacts/checkpoints/p8-lane-registry-binding-20261009-50c'
TASKS = 'docs/roadmap/code-index-v2/tasks.json'
SIX = ['README.md', 'docs/roadmap/code-index-v2/05-TODO.md', 'docs/roadmap/code-index-v2/08-HANDOFF.md', 'docs/roadmap/code-index-v2/PLAN-CHECK.json', 'docs/roadmap/code-index-v2/README.md', TASKS]
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0', GIT_TERMINAL_PROMPT='0')

def sha(b):
    return hashlib.sha256(b).hexdigest()

def digest(b):
    return dict(size=len(b), sha256=sha(b), git_blob=hashlib.sha1(b'blob ' + str(len(b)).encode() + b'\0' + b).hexdigest())

def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args], env=ENV)

def raw(path, source=G5):
    return git('show', source + ':' + path)

def record(path):
    b = path.read_bytes()
    return dict(path=str(path.relative_to(ROOT)), bytes=len(b), sha256=sha(b))

def load(path):
    return json.loads(path.read_bytes())

proof = load(D / 'task-append-proof.json')
changes = load(D / 'changed-files.json')
execution = load(D / 'execution-receipt.json')
assert changes['changed_paths'] == proof['changed_paths']
assert proof['product_source'] == P5 and proof['review_source'] == R5 and proof['guard_source'] == G5
assert proof['actual_execution_head'] == changes['actual_execution_head'] == G5
assert sorted(x['path'] for x in changes['changed_paths']) == sorted(SIX)
current_diff = git('diff', '--no-renames', '--name-only', G5, '--').decode().splitlines()
assert sorted(current_diff) == sorted(SIX)
old = {p: raw(p) for p in SIX}
new = {p: (REPO / p).read_bytes() for p in SIX}
for item in changes['changed_paths']:
    p = item['path']
    assert digest(old[p]) == item['before']
    assert digest(new[p]) == item['after']
assert old[TASKS] == raw(TASKS, P5)
assert sha(old[TASKS]) == BEFORE_SHA and sha(new[TASKS]) == AFTER_SHA
a, b = json.loads(old[TASKS]), json.loads(new[TASKS])
assert set(a) == set(b)
assert {k: v for k, v in a.items() if k != 'tasks'} == {k: v for k, v in b.items() if k != 'tasks'}
assert [t['id'] for t in a['tasks']] == [t['id'] for t in b['tasks']]
assert len(a['tasks']) == len(b['tasks']) == 192
changed_tasks = [x['id'] for x, y in zip(a['tasks'], b['tasks']) if x != y]
assert changed_tasks == ['P8-017']
aa = next(t for t in a['tasks'] if t['id'] == 'P8-017')
bb = next(t for t in b['tasks'] if t['id'] == 'P8-017')
assert set(aa) == set(bb)
assert {k: v for k, v in aa.items() if k not in ('evidence', 'implementation_notes')} == {k: v for k, v in bb.items() if k not in ('evidence', 'implementation_notes')}
assert bb['evidence'] == aa['evidence'] + [proof['appended_evidence']]
assert len(bb['evidence']) == len(aa['evidence']) + 1
assert bb['implementation_notes'] == aa['implementation_notes'] + proof['notes_suffix']
assert sha(aa['implementation_notes'].encode()) == proof['previous_notes_sha256']
assert sha(proof['notes_suffix'].encode()) == proof['notes_suffix_sha256']
inverse = deepcopy(b)
back = next(t for t in inverse['tasks'] if t['id'] == 'P8-017')
back['evidence'].pop()
back['implementation_notes'] = back['implementation_notes'][:-len(proof['notes_suffix'])]
assert inverse == a
inverse_bytes = (json.dumps(inverse, ensure_ascii=False, indent=2) + '\n').encode()
assert inverse_bytes == old[TASKS]
counts = Counter(t['status'] for t in b['tasks'])
assert counts == Counter(t['status'] for t in a['tasks'])
assert counts['done'] == 163 and len(b['tasks']) - counts['done'] == 29
assert bb['status'] == 'in_progress' and 'P8-016' in bb['depends_on']

# Independently reconstruct only the permitted textual changes, without running
# or importing the original generator or a copy of its rendering functions.
views = {}
for p in ['README.md', 'docs/roadmap/code-index-v2/README.md', 'docs/roadmap/code-index-v2/08-HANDOFF.md']:
    assert old[p].count(BEFORE_SHA.encode()) == 1
    assert old[p].replace(BEFORE_SHA.encode(), AFTER_SHA.encode(), 1) == new[p]
    views[p] = dict(change='Only the source task SHA-256 inside the generated progress block changes.', all_other_bytes_unchanged=True)
todo = 'docs/roadmap/code-index-v2/05-TODO.md'
expected = old[todo].decode()
replacements = [(BEFORE_SHA, AFTER_SHA), ('证据：' + json.dumps(aa['evidence'], ensure_ascii=False), '证据：' + json.dumps(bb['evidence'], ensure_ascii=False)), ('实施备注：' + aa['implementation_notes'], '实施备注：' + bb['implementation_notes'])]
for before, after in replacements:
    assert expected.count(before) == 1
    expected = expected.replace(before, after, 1)
assert expected.encode() == new[todo]
views[todo] = dict(change='Only task source hash, the P8-017 evidence line and the P8-017 notes suffix change.', all_other_bytes_unchanged=True)

assert execution['passed'] is True and execution['status'] == 'completed'
assert execution['source_before'] == execution['source_after']
g_tree = git('show', '-s', '--format=%T', G5).decode().strip()
assert execution['source_before']['head'] == G5 and execution['source_before']['tree'] == g_tree
assert execution['task_append_proof_sha256'] == record(D / 'task-append-proof.json')['sha256']
assert sorted(execution['tracked_changed_paths']) == sorted(SIX)
assert [c['name'] for c in execution['commands']] == ['plan-write', 'plan-check']
original_plan = raw('scripts/code_index_plan.py')
assert sha(original_plan) == execution['input_receipts']['original_plan_script_sha256']
assert (REPO / 'scripts/code_index_plan.py').read_bytes() == original_plan
plan_records = []
for c in execution['commands']:
    assert load(D / 'plan-checks' / (c['name'] + '.receipt.json')) == c
    assert c['exit_code'] == 0 and c['head_before'] == c['head_after'] == G5
    assert c['status'] == 'completed' and c['timeout_seconds'] == 300
    expected_args = ['-B', 'scripts/code_index_plan.py'] + (['--write'] if c['name'] == 'plan-write' else [])
    assert c['command'][1:] == expected_args
    out, err = (D / c['stdout_path']).read_bytes(), (D / c['stderr_path']).read_bytes()
    assert sha(out) == c['stdout_sha256'] and sha(err) == c['stderr_sha256'] and err == b''
    assert out == new['docs/roadmap/code-index-v2/PLAN-CHECK.json']
    value = json.loads(out)
    assert value == dict(status='passed', task_count=192, states=dict(counts), task_sha256=AFTER_SHA, views=4)
    plan_records.append(c)

cli_path = V / 'G5-v15-cli/receipt.json'
cli = load(cli_path)
cli_log_path = V / 'G5-v15-cli/v15-cli.log'
cli_log = cli_log_path.read_bytes()
assert sha(cli_path.read_bytes()) == execution['input_receipts']['G5_cli']['receipt_sha256']
assert sha(cli_log) == cli['log_sha256'] == execution['input_receipts']['G5_cli']['log_sha256']
assert cli['source'] == G5 and cli['product_source'] == P5 and cli['review_source'] == R5
assert cli['passed'] is True and cli['exit_code'] == 0 and cli['timeout_seconds'] == 3600
assert cli['source_before'] == cli['source_after'] and cli['source_unchanged'] is True
assert cli['source_before']['head'] == G5 and cli['source_before']['tree'] == g_tree
assert cli['command'][1:] == ['-B', 'scripts/verify_reviewed_source_v15.py', '--source-version', 'p8-completion-source-20261009-v15']
assert datetime.fromisoformat(cli['completed_at_utc']) < datetime.fromisoformat(execution['started_at_utc'])
for name, key in [('scripts/reviewed-source-registry-v15.json', 'registry_sha256'), ('scripts/verify_reviewed_source_v15.py', 'verifier_sha256')]:
    body = raw(name)
    assert (REPO / name).read_bytes() == body
    assert sha(body) == execution['source_before'][key] == cli['source_before'][key]

engineering = load(V / 'engineering-checks/receipt.json')
assert record(V / 'engineering-checks/receipt.json')['sha256'] == execution['input_receipts']['P5_engineering']['receipt_sha256']
assert engineering['expected_product_source'] == P5 and engineering['all_selected_commands_passed'] is True
assert len(engineering['commands']) == 5 and all(c['exit_code'] == 0 for c in engineering['commands'])
prepared_path = V / 'mechanism-prepare/prepared-snapshots.json'
prepared = load(prepared_path)
assert record(prepared_path)['sha256'] == execution['input_receipts']['mechanism_preparation']['sha256']
assert prepared['compiled'] is False and prepared['build']['source_commit'] == P5 and len(prepared['build']['variants']) == 4
entry = proof['appended_evidence']
assert entry['target_sha'] == P5 and entry['review_sha'] == R5 and entry['guard_sha'] == G5
assert entry['status'] == 'scoped_engineering_accepted_full_task_open'
linked_R5 = []
pending_post = []
for p in entry['artifact_paths']:
    if p.startswith(R_PREFIX + '/'):
        body = raw(p)
        assert body == raw(p, R5)
        linked_R5.append(dict(path=p, bytes=len(body), sha256=sha(body)))
    else:
        assert p.startswith(POST_PREFIX + '/')
        pending_post.append(p)
assert len(linked_R5) == 6 and len(pending_post) == 3
assert json.loads(raw(R_PREFIX + '/independent-source-review.json'))['source'] == P5
assert sha(raw(R_PREFIX + '/independent-source-review.json')) == 'c74e75d90f9e94d7b4572f0e75f02ab172a01eb6dda28e3ec47aacd94e511dd0'
assert raw(R_PREFIX + '/engineering-checks/receipt.json') == (V / 'engineering-checks/receipt.json').read_bytes()
assert raw(R_PREFIX + '/mechanism-prepare/prepared-snapshots.json') == prepared_path.read_bytes()
assert all((REPO / p).read_bytes() == new[p] for p in SIX)

report = dict(
    schema_version=1, reviewer='/root/todo_audit', reviewed_at_utc=datetime.now(timezone.utc).isoformat(), verdict='accepted_scoped_docs_and_append', unresolved_blockers=[],
    scope='Independent read-only comparison of fixed G5 against exactly six prospective document files and the already completed original plan/engineering/CLI receipts. No new project execution.',
    source_identity=dict(product=P5, review=R5, actual_plan_execution_head=G5, committed_G5_tree=g_tree, prospective_workingtree_changed_documents=SIX, future_A5_execution_claimed=False),
    original_evidence=[record(D / 'task-append-proof.json'), record(D / 'changed-files.json'), record(D / 'execution-receipt.json'), record(cli_path), record(cli_log_path), record(V / 'G5-cli-independent-review.json'), record(V / 'engineering-checks/receipt.json'), record(prepared_path), record(V / 'prepare-round10-docs.py')],
    task_review=dict(path=TASKS, before=digest(old[TASKS]), after=digest(new[TASKS]), changed_task_ids=changed_tasks, unchanged_other_tasks=191, top_level_fields_and_task_order_unchanged=True, all_definitions_statuses_dependencies_and_unknown_fields_unchanged=True, old_evidence_prefix_and_notes_prefix_preserved=True, exactly_one_evidence_append_and_one_notes_suffix=True, whole_original_json_reconstructed_byte_for_byte=True, inverse_sha256=sha(inverse_bytes), old_evidence_count=len(aa['evidence']), new_evidence_count=len(bb['evidence']), notes_suffix_sha256=sha(proof['notes_suffix'].encode()), appended_evidence_sha256=sha(json.dumps(entry, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()), P8_017_status=bb['status'], P8_017_dependencies=bb['depends_on'], counts=dict(total=192, done=163, remaining=29, newly_closed=0), states=dict(counts)),
    derived_views=views,
    six_file_frozen_hashes=changes['changed_paths'],
    plan_execution=dict(identity='Original --write and no-argument plan ran on G5 HEAD with prospective document changes. The committed G5 tree remains the recorded tree; no future publication-commit execution is claimed.', commands=plan_records, original_generator_sha256=sha(original_plan), PLAN_CHECK_equals_both_original_stdout_byte_for_byte=True, PLAN_CHECK_sha256=sha(new['docs/roadmap/code-index-v2/PLAN-CHECK.json']), reviewer_executed_generator=False),
    linked_execution_scope=dict(G5_original_CLI_exit=0, G5_original_CLI_timeout_seconds=3600, G5_original_CLI_elapsed_seconds=cli['elapsed_seconds'], G5_completed_before_plan_started=True, G5_source_and_guard_hashes_before_after_equal=True, P5_actual_five_commands_passed=True, P5_search_lib_passed=303, P5_mechanism_anchor_passed=1, prepared_variants=4, compiled_counterfactual_variants=False, native_suite_or_unfiltered_workspace_result_claimed=False, former_P2_failure_identity_preserved=True),
    artifact_link_review=dict(existing_R5_six_exact_original_blobs=linked_R5, postbinding_publication_paths=pending_post, postbinding_path_status='Explicit intended paths for the next publication checkpoint, not represented as existing G5 blobs; final A5 archive/tree inclusion remains root publication scope.'),
    review_execution=dict(reviewer_ran_project_plan_Cargo_guard_or_native=False, reviewer_changed_shared_repository_HEAD_index_ref_or_files=False, reviewer_fetched=False, six_working_document_bytes_stable_through_review=True, reproduction=record(Path(__file__))),
)
out = D / 'independent-docs-review.json'
out.write_text(json.dumps(report, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
print(json.dumps(record(out) | dict(verdict=report['verdict'], remaining=29, newly_closed=0)))

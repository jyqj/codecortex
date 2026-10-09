#!/usr/bin/env python3
"""Build a compact P5 canonical using fixed Git reads and typed R4 inheritance.

No project module is imported or executed. No Git object, index, ref or checkout
is written. Full required bodies are independently read, SHA-1 checked and hashed.
"""
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
import ast
import hashlib
import json
import os
import re
import subprocess

ROOT = Path('/workspace/scratch/50c364fd60b1')
REPO = ROOT / 'codecortex'
DIR = ROOT / 'validation/lane-ownership-round10'
OUT = DIR / 'source-review'
PREFIX = 'artifacts/checkpoints/p8-lane-registry-20261009-50c'
P5 = 'fa6562ad70c6a6f2a7a1e65594df23f807e62f18'
P5_TREE = '0e6fdcfd2ac7a727184daf5a0e290cf00c55abbc'
M = 'b9412406e11422d7cf914458a8bfbbd58cf94eaa'
P4 = '31a42daeb12da6936695abb08eb3912d4f3c6064'
R4 = '20efd9664f1be2ca4705bec757505bcb530e7f48'
G4 = '2b60ae7bff2b4b2bf52257bb5cde600fc61c3858'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
PRIOR_PATH = 'artifacts/checkpoints/p8-empty-input-main-55-integration-20261009-50c/independent-source-review.json'
PRIOR_SHA = '79b3830e7ddb7a23074a25ef9e6c267621d0314bb12632c9dc5c2bcff050e11e'
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0', GIT_TERMINAL_PROMPT='0')
SOURCE_ROOTS = ['crates', 'Cargo.toml', 'Cargo.lock']
VALIDATION_ROOTS = ['scripts', '.github/workflows', 'tests/source_integrity']
EXCLUDED = ['.github/workflows/ci.yml', 'scripts/reviewed-source-registry-v15.json', 'scripts/verify_reviewed_source_v15.py']
THREE = ['crates/cc-search/src/lanes.rs', 'crates/cc-search/src/query_policy.rs', 'crates/cc-eval/src/benchmark/ablation/mechanism_controls.json']
started = datetime.now(timezone.utc).isoformat()
read_operations = []

def sha(data):
    return hashlib.sha256(data).hexdigest()

def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()

def git(*args):
    data = subprocess.check_output(['git', '-C', str(REPO), *args], env=ENV)
    read_operations.append(dict(argv=['git', '-C', str(REPO), *args], output_bytes=len(data), output_sha256=sha(data)))
    return data

def metadata(source):
    lines = git('show', '-s', '--format=%T%n%P', source).decode().splitlines()
    return dict(commit=source, tree=lines[0], parents=lines[1].split())

def tree(source):
    out = {}
    for row in git('ls-tree', '-r', '-z', source).split(b'\0'):
        if row:
            meta, path = row.split(b'\t', 1)
            mode, kind, blob = meta.decode().split()
            out[path.decode()] = dict(mode=mode, type=kind, blob=blob)
    return out

def under(path, roots):
    return any(path == root or path.startswith(root + '/') for root in roots)

def selected(t, validation=False):
    roots = VALIDATION_ROOTS if validation else SOURCE_ROOTS
    result = {}
    for p, m in t.items():
        if not under(p, roots):
            continue
        if validation and (p in EXCLUDED or ('__pycache__' in Path(p).parts and p.endswith('.pyc'))):
            continue
        assert m['type'] == 'blob' and m['mode'] in ('100644', '100755'), (p, m)
        result[p] = m
    return result

def local_record(path, repository_name=None):
    b = path.read_bytes()
    return dict(path_at_review=str(path.relative_to(ROOT)), repository_path=(PREFIX + '/' + repository_name) if repository_name else None, bytes=len(b), sha256=sha(b))

def pointer(path):
    return path.replace('~', '~0').replace('/', '~1')

identities = {s: metadata(s) for s in [P5, M, P4, R4, G4, BASE]}
assert identities[P5] == dict(commit=P5, tree=P5_TREE, parents=[M])
trees = {s: tree(s) for s in [P5, M, P4, R4, G4, BASE]}
products = {s: selected(t) for s, t in trees.items()}
validations = {s: selected(t, True) for s, t in trees.items()}
actual_three = sorted(p for p in trees[P5].keys() | trees[M].keys() if trees[P5].get(p) != trees[M].get(p))
assert actual_three == sorted(THREE)
assert products[M] == products[P4] == products[R4] == products[G4]
assert validations[P5] == validations[M] == validations[P4] == validations[R4] == validations[G4]
assert set(products[P5]) == set(products[P4])
assert sorted(p for p in products[P5] if products[P5][p] != products[P4][p]) == sorted(THREE)
assert len(products[P5]) == 1092 and len(validations[P5]) == 139
assert trees[R4][PRIOR_PATH] == trees[M][PRIOR_PATH] == trees[P5][PRIOR_PATH]

# Read every P5 required body, every BASE source body, the three prior bodies,
# fixed R4 canonical, ledger and excluded trust-root inputs in one no-fetch batch.
needed = set()
for group in [products[P5], products[P4], products[BASE], validations[P5]]:
    needed.update(m['blob'] for m in group.values())
needed.add(trees[R4][PRIOR_PATH]['blob'])
LEDGER = 'docs/roadmap/code-index-v2/tasks.json'
needed.add(trees[P5][LEDGER]['blob'])
needed.update(trees[P5][p]['blob'] for p in EXCLUDED)
oids = sorted(needed)
raw = subprocess.run(['git', '-C', str(REPO), 'cat-file', '--batch'], input=('\n'.join(oids) + '\n').encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=ENV, check=True)
read_operations.append(dict(argv=['git', '-C', str(REPO), 'cat-file', '--batch'], requested_unique_oids=len(oids), request_sha256=sha(('\n'.join(oids) + '\n').encode()), output_bytes=len(raw.stdout), output_sha256=sha(raw.stdout), stderr_bytes=len(raw.stderr)))
assert not raw.stderr
bodies = {}
pos = 0
for expected_oid in oids:
    end = raw.stdout.index(b'\n', pos)
    header = raw.stdout[pos:end].decode().split()
    assert len(header) == 3 and header[0] == expected_oid and header[1] == 'blob', header
    size = int(header[2]); pos = end + 1
    body = raw.stdout[pos:pos + size]
    assert len(body) == size and raw.stdout[pos + size:pos + size + 1] == b'\n'
    assert hashlib.sha1(b'blob ' + str(size).encode() + b'\0' + body).hexdigest() == expected_oid
    bodies[expected_oid] = body; pos += size + 1
assert pos == len(raw.stdout)

def blob(source, path):
    return bodies[trees[source][path]['blob']]

def digest_map(entries):
    return {p: sha(bodies[m['blob']]) for p, m in entries.items()}

complete = digest_map(products[P5])
validation = digest_map(validations[P5])
base_inputs = digest_map(products[BASE])
previous_inputs = digest_map(products[P4])
prior_raw = blob(R4, PRIOR_PATH)
assert sha(prior_raw) == PRIOR_SHA
prior = json.loads(prior_raw)
assert prior['source'] == P4 and prior['base'] == BASE and prior['verdict'] == 'accepted_scoped' and prior['unresolved_blockers'] == []
assert prior['complete_inputs'] == previous_inputs
assert prior['validation_inputs'] == validation
assert prior['actual_input_mode_blob_inventory']['source'] == products[P4]
assert prior['actual_input_mode_blob_inventory']['validation'] == validations[P4]
assert prior['source_manifest_sha256'] == sha(encoded(previous_inputs))

changed = sorted(p for p in products[BASE].keys() | products[P5].keys() if products[BASE].get(p) != products[P5].get(p))
assert set(products[BASE]) <= set(products[P5])
git_changed = git('diff', '--no-renames', '--name-only', BASE, P5, '--', *SOURCE_ROOTS).decode().splitlines()
assert changed == sorted(git_changed)
delta = {p: dict(before_sha256=base_inputs.get(p), sha256=complete[p]) for p in changed}
reconstructed = dict(base_inputs)
for p, d in delta.items():
    reconstructed[p] = d['sha256']
assert reconstructed == complete
assert set(prior['paths']) <= set(delta)
for p in prior['paths']:
    assert prior['paths'][p] == delta[p]
assert set(delta) - set(prior['paths']) == set(THREE)
for p in complete:
    assert p in THREE or complete[p] == previous_inputs[p]

sem_path = DIR / 'reviewer/semantic-independent-review-v2.json'
sem_raw = sem_path.read_bytes(); sem = json.loads(sem_raw)
assert sha(sem_raw) == 'fb9e5401c7f85ad3d788ad6f1c2c4125a47fd0da0eb6efc70028481cddeb3f60'
assert sem['source'] == P5 and sem['verdict'] == 'accepted_scoped' and not sem['unresolved_blockers']
for row in sem['changed_files']:
    p = row['path']; assert row['after']['sha256'] == complete[p] and row['after']['blob'] == products[P5][p]['blob']

guard_path = 'scripts/verify_reviewed_source_v15.py'
guard = blob(P5, guard_path)
assignments = {}
for node in ast.parse(guard).body:
    if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
        name = node.targets[0].id
        if name in ('VERSION', 'BASE', 'PRODUCT', 'REVIEW', 'REVIEW_PATH', 'REGISTRY_SHA256', 'FROZEN', 'VALIDATION_ROOTS'):
            assignments[name] = ast.literal_eval(node.value)
assert assignments['BASE'] == BASE and assignments['PRODUCT'] == P4 and assignments['REVIEW'] == R4
assert assignments['REVIEW_PATH'] == PRIOR_PATH
assert list(assignments['VALIDATION_ROOTS']) == VALIDATION_ROOTS
assert len(assignments['FROZEN']) == 6
for p in assignments['FROZEN']:
    assert trees[P5][p] == trees[BASE][p]
for p in EXCLUDED:
    assert trees[P5][p] == trees[M][p] == trees[G4][p]
assert sha(blob(P5, 'scripts/reviewed-source-registry-v15.json')) == assignments['REGISTRY_SHA256']

tasks_raw = blob(P5, LEDGER); tasks_doc = json.loads(tasks_raw)
tasks = tasks_doc['tasks']; assert isinstance(tasks, list)
counts = Counter(t['status'] for t in tasks)
assert len(tasks) == 192 and counts['done'] == 163 and len(tasks) - counts['done'] == 29
t017 = next(t for t in tasks if t['id'] == 'P8-017')
assert t017['status'] != 'done' and 'P8-016' in t017['depends_on']
assert trees[P5][LEDGER] == trees[M][LEDGER]

# Inspect the already completed root-owned receipt/logs without invoking commands.
engineering_dir = DIR / 'engineering-checks'
engineering_raw = (engineering_dir / 'receipt.json').read_bytes()
engineering = json.loads(engineering_raw)
assert engineering['expected_product_source'] == P5 and engineering['all_selected_commands_passed'] is True
assert engineering['source_before'] == engineering['source_after']
for key in ('source_before', 'source_after'):
    entry = engineering[key]
    assert entry['source_commit'] == P5 and entry['source_tree'] == P5_TREE
    assert entry['inputs'] == complete and entry['input_count'] == len(complete)
    assert entry['manifest_sha256'] == sha(encoded(complete))
assert [c['name'] for c in engineering['commands']] == ['format', 'clippy', 'search-lib', 'mechanism-anchor', 'mechanism-prepare']
engineering_commands = []
for c in engineering['commands']:
    assert c['exit_code'] == 0 and c['head_before'] == c['head_after'] == P5
    b = (engineering_dir / c['log']).read_bytes(); s = b.decode()
    assert sha(b) == c['log_sha256']
    row = dict(c)
    row['log_bytes'] = len(b)
    row['repository_log'] = PREFIX + '/engineering-checks/' + c['log']
    row['parsed_test_summaries'] = re.findall(r'test result: .*', s)
    if c['name'] == 'search-lib':
        assert row['parsed_test_summaries'] == ['test result: ok. 303 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.00s']
        wanted = sem['meaningful_test_review']['new_test_names'] + sem['meaningful_test_review']['retained_test_names'] + ['default_lanes_registry_keeps_fusion_order']
        assert all(re.search(r'^test [^\n]*::' + re.escape(n) + r' \.\.\. ok$', s, re.M) for n in wanted)
        row['reviewed_named_controls_present'] = wanted
    if c['name'] == 'mechanism-anchor':
        assert row['parsed_test_summaries'] == ['test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 66 filtered out; finished in 0.00s']
        assert 'fixed_controls_match_current_product_source_once ... ok' in s
    engineering_commands.append(row)
prepared_path = DIR / 'mechanism-prepare/prepared-snapshots.json'
prepared = json.loads(prepared_path.read_text())
assert prepared['compiled'] is False and prepared['build']['source_commit'] == P5
assert [x['id'] for x in prepared['build']['variants']] == ['none', 'local', 'dense_only', 'hybrid']
assert prepared['build']['controls'] == json.loads(blob(P5, THREE[2]))
execution_evidence = dict(
    kind='read_only_inspection_of_separately_executed_root_receipt', execution_owner='/root', execution_source=P5, execution_tree=P5_TREE,
    receipt=local_record(engineering_dir / 'receipt.json', 'engineering-checks/receipt.json'),
    execution_script=local_record(DIR / 'run-P5-engineering.py', 'run-P5-engineering.py'),
    started_at_utc=engineering['started_at_utc'], completed_at_utc=engineering['completed_at_utc'], commands=engineering_commands,
    all_selected_exit_codes_zero=True, full_product_inputs_before_after_equal_actual_fixed_P5=True,
    prepared_snapshot_summary=local_record(prepared_path, 'mechanism-prepare/prepared-snapshots.json'), prepared_variants=4, compiled_counterfactual_variants=False,
    workspace_full_suite=engineering['workspace_full_suite'], mechanism_scope=engineering['mechanism_scope'], reviewer_executed_these_commands=False,
)

prior_ref = dict(id='R4-canonical', type='immutable_prior_non_author_semantic_review', review_commit=R4, review_path=PRIOR_PATH, review_git_blob=trees[R4][PRIOR_PATH]['blob'], review_bytes=len(prior_raw), review_sha256=PRIOR_SHA, original_product_source=P4, original_product_tree=identities[P4]['tree'], original_independent_reviewers=prior['independent_reviewers'], original_verdict=prior['verdict'], unchanged_source_paths=len(complete) - len(THREE), unchanged_validation_paths=len(validation), inherited_author_recusals_preserved=True, execution_results_not_relabelled=True)
source_reviewers = {}
for p in changed:
    if p in THREE:
        source_reviewers[p] = dict(type='current_non_author_semantic_review', reviewer='/root/todo_audit', author='/root/pr_audit', source=P5, sha256=complete[p], before_sha256=delta[p]['before_sha256'], review_path=PREFIX + '/semantic-independent-review-v2.json', review_sha256=sha(sem_raw), review_pointer='/changed_files', previous_body_source=M)
    else:
        assert p in prior['source_path_reviewers']
        source_reviewers[p] = dict(type='inherited_exact_unchanged_semantic_review', basis='R4-canonical', original_record_pointer='/source_path_reviewers/' + pointer(p), current_identity_reviewer='/root/todo_audit', source=P5, sha256=complete[p], original_reviewer=prior['source_path_reviewers'][p].get('reviewer'), old_author_and_recusal_chain_not_reassigned=True)
validation_reviewers = {p: dict(type='inherited_exact_unchanged_semantic_review', basis='R4-canonical', original_record_pointer='/validation_path_reviewers/' + pointer(p), sha256=validation[p]) for p in validation}
canonical = dict(
    schema_version=1, source=P5, tree=P5_TREE, source_tree=P5_TREE, source_parent=M, source_parents=[M], base=BASE,
    intended_repository_review_path=PREFIX + '/independent-source-review.json', reviewed_at_utc=datetime.now(timezone.utc).isoformat(),
    scope='independently_reviewed_source_and_validation_inputs', verdict='accepted_scoped', unresolved_blockers=[], independent_reviewers=['/root/todo_audit'],
    changed_source_input_count=len(delta), complete_input_count=len(complete), validation_input_count=len(validation),
    paths=delta, complete_inputs=complete, validation_inputs=validation,
    source_manifest_digest_encoding='UTF-8 JSON, sorted keys, separators=(comma,colon), ensure_ascii=False, no trailing newline', source_manifest_sha256=sha(encoded(complete)),
    actual_input_mode_blob_inventory=dict(source=products[P5], validation=validations[P5]),
    source_path_reviewers=source_reviewers, validation_path_reviewers=validation_reviewers,
    review_basis=dict(
        current_semantic_review=local_record(sem_path, 'semantic-independent-review-v2.json'),
        current_reader=local_record(Path(__file__), 'read-P5-source.py'),
        inherited_reviews=[prior_ref],
        complete_inventory_review='All actual required P5 source/validation bodies and all BASE source bodies were independently read through no-fetch Git cat-file, each Git SHA-1 rechecked, and SHA-256 calculated. No product module, validator or previous review script was executed.',
        semantic_coverage='Current non-author approval covers the three changed files and interacting original contracts. All other 1089 product and 139 validation bodies match the immutable R4 canonical by complete path/mode/type/blob and recomputed SHA-256. Their semantic approval and original author/recusal chain are inherited by typed pointers; hashing is not labelled a fresh semantic reread of every unchanged historical component.',
        original_product_roots=SOURCE_ROOTS, original_validation_roots=VALIDATION_ROOTS, original_validation_exclusions=EXCLUDED,
        only_generated_validation_exclusion='A path segment named __pycache__ together with a .pyc suffix; other files inside that directory remain inputs.',
        base_reconstruction=dict(base_source=BASE, base_input_count=len(base_inputs), base_inventory_sha256=sha(encoded(base_inputs)), all_before_after_hashes_recomputed=True, complete_BASE_plus_actual_deltas_equals_P5=True, new_product_paths=sorted(set(complete) - set(base_inputs)), removed_product_paths=[], inherited_BASE_delta_count=len(prior['paths']), new_BASE_delta_paths=sorted(set(delta) - set(prior['paths']))),
        body_read_totals=dict(unique_Git_blobs=len(bodies), body_bytes=sum(len(b) for b in bodies.values())),
    ),
    inheritance_evidence=dict(
        fixed_identities=identities, exactly_three_P5_changes_from_M=actual_three,
        M_P4_R4_G4_product_inputs_equal_by_complete_path_mode_type_blob=True,
        P5_M_P4_R4_G4_validation_inputs_equal_by_complete_path_mode_type_blob=True,
        all_other_M_tracked_paths_including_archives_ledger_and_guards_unchanged=True,
        immutable_R4_review_original_bytes_preserved=True,
        current_prebinding_guard_constants=assignments,
        current_prebinding_trust_root_inputs={p: trees[P5][p] | dict(sha256=sha(blob(P5, p))) for p in EXCLUDED},
        all_six_frozen_v14_entries_equal_original_BASE={p: trees[P5][p] for p in assignments['FROZEN']},
        later_binding_boundary='The guard in P5 deliberately retains P4/R4 bindings. This canonical does not borrow their admission for P5. A subsequent R5 archive and G5 four-constant/registry binding must retain original validation policy, exclusions, BASE, VERSION, historical proof and CI selector; actual source admission remains a separate execution.',
    ),
    semantic_findings=sem['semantic_findings'], meaningful_test_review=sem['meaningful_test_review'],
    author_independence=dict(current_candidate_author='/root/pr_audit', current_composition_author='/root', current_reviewer='/root/todo_audit', current_reviewer_authored_none_of_three_product_files=True, current_reviewer_wrote_no_repository_source_guard_index_ref_or_worktree=True, prior_semantic_reviewer_roles_and_recusals='Retained only for the exact unchanged R4-era bytes. The current candidate author is recused from semantic approval of the new three-file change; their earlier non-author review of different, unchanged source is retained with its original P4 identity.'),
    review_execution=dict(Cargo_rustfmt_product_or_guard_execution_by_reviewer=False, network_fetch_or_Git_object_index_ref_worktree_write_by_reviewer=False, mutable_HEAD_used_as_identity=False, all_required_bodies_SHA1_and_SHA256_recomputed=True, writes='Only this scratch reader, canonical and source-read receipt.'),
    external_execution_evidence=execution_evidence,
    historical_execution_identity=dict(typed_prior_evidence='R4-canonical#/historical_execution_identity retains all exact historical sources, logs, failures and evidence-strength limits. No inherited command result is relabelled as P5.', original_P2_unfiltered_workspace='Retained exit 101, including the repeated sampler live_child failure; not green.', original_P3_P4_and_G_admission='Keep original product, guard, checkout and execution scope; the selected P5 command receipts above are separate.'),
    task_ledger=dict(path=LEDGER, source=P5, sha256=sha(tasks_raw), blob=trees[P5][LEDGER]['blob'], total=192, statuses=dict(counts), remaining=29, newly_closed=0, P8_017_status=t017['status'], P8_017_dependencies=t017['depends_on'], bytes_equal_M=True),
    TODO_done=163, TODO_remaining=29, TODO_closed=0,
    acceptance_boundary=[
        'Accepted for the fixed P5 product/validation inputs with explicit inherited semantic coverage. Source inventory, static review and selected engineering do not close P8-017 or its P8-016 dependency.',
        'No original TODO definition, scope, dependency, workload, gate, quality/performance/scale criterion or source-admission algorithm changed.',
        'The five selected root commands passed at actual P5: formatting, strict workspace/all-target Clippy, cc-search library, one cc-eval mechanism-anchor test and original prepare-only. This is not an unfiltered workspace test, live-provider run, compiled ablation, runtime/scale study, platform/release certification or new G5 admission.',
        'default_lane_registry avoids an intermediate registry allocation and performs no lane execution; QueryPolicy retains its original obligations Vec allocation. No measured speedup is claimed.',
    ],
)
OUT.mkdir(parents=True, exist_ok=True)
canonical_path = OUT / 'independent-source-review.json'
canonical_path.write_text(json.dumps(canonical, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
receipt = dict(schema_version=1, reviewer='/root/todo_audit', started_at_utc=started, completed_at_utc=datetime.now(timezone.utc).isoformat(), source=P5, tree=P5_TREE, verdict='accepted_scoped', canonical=local_record(canonical_path, 'independent-source-review.json'), reader=local_record(Path(__file__), 'read-P5-source.py'), complete_source_inputs=len(complete), validation_inputs=len(validation), actual_BASE_delta_count=len(delta), unique_blobs_read=len(bodies), blob_bytes_read=sum(len(b) for b in bodies.values()), fixed_object_read_operations=read_operations, no_project_or_guard_execution=True, no_network_fetch_or_repository_mutation=True, external_execution_receipt_read=local_record(engineering_dir / 'receipt.json', 'engineering-checks/receipt.json'))
receipt_path = OUT / 'source-read-receipt.json'
receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
print(json.dumps(dict(canonical=local_record(canonical_path), read_receipt=local_record(receipt_path), reader=local_record(Path(__file__), counts=[len(complete), len(validation), len(delta)], source=P5, source_manifest_sha256=sha(encoded(complete)))))

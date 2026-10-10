import ast
import datetime
import hashlib
import json
from pathlib import Path

ROOT = Path('/workspace/scratch/bfccb8494ba0')
OUT = ROOT / 'round40-failed-cohort-and-next-work/final'
PREFIX = 'artifacts/checkpoints/p8-round40-complete-bfcc-20261010/next-work/'


def read(path):
    return json.loads((ROOT / path).read_text())


def identity(path):
    p = ROOT / path
    b = p.read_bytes()
    return dict(path=str(p), bytes=len(b), sha256=hashlib.sha256(b).hexdigest(),
                git_blob=hashlib.sha1(b'blob ' + str(len(b)).encode() + b'\0' + b).hexdigest())


def write(name, content):
    p = OUT / name
    assert not p.exists(), str(p)
    p.write_text(json.dumps(content, ensure_ascii=False, indent=2) + '\n')
    return identity(str(p.relative_to(ROOT)))


tasks_path = 'round36-006-and-dependency-scope-review/fixed-main-source/docs/roadmap/code-index-v2/tasks.json'
old_plan_path = 'round37-nine-task-application-revision/nine-task-dependent-update-input.applyfalse.v2.json'
ledger_before = identity(tasks_path)
plan_before = identity(old_plan_path)
ledger = read(tasks_path)
tasks = {t['id']: t for t in ledger['tasks']}
incomplete = [t for t in ledger['tasks'] if t['status'] != 'done']
assert len(incomplete) == 28 and len(ledger['tasks']) == 192
jobs_path = 'round39-0355-dual-study-observation/fullcohort-jobs.original.json'
artifacts_path = 'round39-0355-dual-study-observation/fullcohort-artifacts.original.json'
observation_path = 'round39-0355-dual-study-observation/finite-observation-review.json'
jobs = read(jobs_path)['jobs']
artifacts = read(artifacts_path)
obs = read(observation_path)
failed = next(j for j in jobs if j['id'] == 114100438875)
aggregate = next(j for j in jobs if j['id'] == 114125450702)
measure = next(j for j in jobs if j['name'] == 'measure')
assert failed['conclusion'] == aggregate['conclusion'] == 'failure'
assert measure['conclusion'] == 'skipped' and artifacts['total_count'] == 11
assert not any(a['name'].startswith('p8-scale-wide-dirty-shard-100000-') for a in artifacts['artifacts'])
matrix_path = 'round40-full-cohort-terminal-independent-review/original-members/matrix.json'
matrix = read(matrix_path)
assert matrix['passed'] is False and matrix['status'] == 'failed'
missing = ast.literal_eval(matrix['error'].split(': ', 1)[1])
assert len(missing) == 146 and (100000, 0) in missing
log_failure_path = 'round40-full-cohort-terminal-original-logs/114100438875-original-log-request-result.json'
log_failure = read(log_failure_path)
assert 'BlobNotFound' in json.dumps(log_failure) and '404' in json.dumps(log_failure)
now = datetime.datetime.now(datetime.timezone.utc).isoformat()
notice = write('formal-cohort-failure-notice.applyfalse.json', {
    'schema': 'p8_formal_cohort_failed_append_only_notice_v1', 'recorded_at_utc': now,
    'author': '/root/todo_audit', 'apply_now': False, 'task_credit': 0,
    'append_to_preserved_input': plan_before,
    'source': '9cc6bf49f6dd81e4069a8004eed49addba0ab79b', 'run_id': 38013753078, 'attempt': 1,
    'registered_profile': 'scale_wide_dirty_v1', 'registered_shards': 150, 'registered_samples': 1500,
    'observed_at': obs['request_end_utc'],
    'official_failed_job': {k: failed.get(k) for k in ['id', 'name', 'status', 'conclusion', 'started_at', 'completed_at', 'steps']},
    'official_aggregate_job': {k: aggregate.get(k) for k in ['id', 'name', 'status', 'conclusion', 'started_at', 'completed_at']},
    'official_measure_matrix': {'job_id': measure['id'], 'conclusion': measure['conclusion'],
                                'registered_slots_not_started': 145,
                                'boundary': 'API returns the matrix placeholder job named measure as skipped; not 145 individually materialized job records.'},
    'artifact_index': {'total_count': 11, 'hundred_k_measurement_artifact_present': False,
                       'capacity_is_not_a_measurement': True,
                       'aggregate_artifact_id': 11658738280},
    'original_aggregate_result': matrix,
    'aggregate_missing_count': len(missing),
    'log_retrieval': {'attempts_observed': 1, 'result': '404 BlobNotFound',
                      'receipt': identity(log_failure_path), 'retry_or_download_by_this_review': False},
    'unknowns': ['native exit', 'native stopping stage', 'completed 100k samples', 'resource cause',
                 'deadline cause', 'runner-loss cause', 'whether original stream bytes remained on the host'],
    'important_mixed_status': 'Job completed/failure is genuine while returned measurement step remains in_progress/null and upload step remains pending/null. Neither level is rewritten.',
    'prior_accepted_evidence_retained': {'shards': 4, 'samples': 41, 'scales': [1000, 5000, 10000, 50000],
                                        'fifth_prespecified_rep0': 'failed_without_accepted_original_shard'},
    'effect_on_future_application': {'original_five_rep0_route_complete': False, 'actual_006_receipt': None,
                                      'nine_updates_may_apply': False,
                                      'old_pending_template_preserved': True,
                                      'diagnostic_run_38012409915_cohort_credit': 0,
                                      'four_successes_do_not_replace_the_fifth': True,
                                      'old_run_and_registered_150_1500_not_relabelled': True},
    'source_references': [identity(p) for p in [jobs_path, artifacts_path, observation_path, matrix_path]],
    'current_counts': {'done': 164, 'remaining': 28, 'session_completed': 1},
    'operations': {'new_network_requests': 0, 'Actions_downloads': 0, 'ZIP_reads_or_CRC': 0,
                   'validators': 0, 'tests': 0, 'product_runs': 0, 'task_writes': 0, 'ref_mutations': 0}
})


def path_to(task_id, ancestor):
    pending = [(task_id, [task_id])]
    seen = set()
    while pending:
        current, path = pending.pop()
        if current == ancestor:
            return path
        if current in seen:
            continue
        seen.add(current)
        pending.extend((d, path + [d]) for d in tasks[current]['depends_on'] if d not in seen)
    return None


text = (ROOT / tasks_path).read_text()
clause_keys = ['id', 'title', 'status', 'depends_on', 'steps', 'acceptance', 'validations', 'conditional', 'conditional_dependencies']
clauses = []
for t in incomplete:
    c = {k: t.get(k) for k in clause_keys}
    c['original_start_line'] = text[:text.index('"id": "' + t['id'] + '"')].count('\n') + 1
    c['hard_path_to_006'] = path_to(t['id'], 'P8-006')
    c['currently_unsatisfied_hard_dependencies'] = [d for d in t['depends_on'] if tasks[d]['status'] != 'done']
    clauses.append(c)
assert [c['id'] for c in clauses if c['hard_path_to_006'] is None] == ['P7-018']

view = 'round32-capture-context-admission/guard-view/'
source_files = ['.github/workflows/p8-scale-wide-dirty.yml', 'scripts/p8_scale_matrix.py',
                'crates/cc-eval/src/benchmark/oracle/streaming.rs',
                'crates/cc-eval/tests/benchmark_oracle_streaming.rs']
findings = write('remaining-original-work-and-critical-path.json', {
    'schema': 'p8_remaining_original_tasks_and_bounded_engineering_selection_v1',
    'recorded_at_utc': now, 'reviewer': '/root/todo_audit', 'apply_now': False,
    'fixed_source': {'main': 'b9b089bb4eae072affe9326681d4980eae15fd84',
                     'equal_tree_G': '9cc6bf49f6dd81e4069a8004eed49addba0ab79b',
                     'tree': '6461938788665701cfa4fa095a064a3a404ec492', 'ledger': ledger_before},
    'failure_notice': notice,
    'original_dependency_semantics': ledger['dependency_semantics'],
    'remaining_tasks_exact_clauses': clauses,
    'decision': {
        'immediately_closable_independent_original_todos_identified': [],
        'reason': '27 of 28 incomplete tasks are P8-006 or have a hard dependency path to it. The remaining P7-018 has a live authorization/budget condition and no accepted real-provider evidence. Dependency bypass, optional deferred, and preparation are not done.',
        'highest_value_action': 'Implement the bounded sorted-cursor text borrowing candidate in a separate ordinary-development change, preserving all existing dual-spool and exact comparison contracts; verify correctness with existing controls before any performance claim. In parallel, prepare first-failure stream retention for a future full-cohort preflight using the already reviewed capture mechanism. Neither is a task completion or a proven cure for the unknown present failure.',
        'no_outcome_based_route_change': 'The pre-existing five complete same-run rep0 descriptive route is not complete. Four accepted shards, missing100k, an aggregate failure, or a different diagnostic run cannot be substituted. Any future revised-source task evidence must be prospectively fixed with all original five-scale/stage/parity/fanout coverage; no retroactive merging or reduction.',
        'registered_studies': 'Original150/1500N30 populations and failures remain unchanged. The earlier independently accepted task/release distinction remains, but does not waive the failed fifth rep0.'
    },
    'engineering_candidates': [
        {'priority': 1, 'name': 'Borrow already-sorted canonical row text instead of materializing each as String',
         'status': 'unimplemented_unmeasured_design_ready',
         'source': 'crates/cc-eval/src/benchmark/oracle/streaming.rs:228-232,358-374',
         'concrete_work': 'Replace the next_row owned String adapter and immediate as_deref loop with two scoped row/text borrows. Keep A step and text conversion before B step/text conversion; compare/hash and own at most the existing bounded examples before either next(). Avoid unsafe lifetime extension.',
         'verified_source_work': 'Existing accepted100k cold/no_op row counts correspond to13,307,986 row String materializations and10,302,684,584 canonical payload bytes across both sides. This is source/count arithmetic, not a measured saved time or current failure diagnosis.',
         'preserve': ['complete A then complete B spool and their simultaneous original capacity', 'all15tables, physical columns, multiplicity, ordinal and BINARY sort', 'Value equality including signed zero and nonidentical serialized strings', 'A-before-B conversion/error order and existing type/UTF8/SQLite error mapping', 'full EOF, both digests, counts, bounded examples and exact report fields', 'original row/byte/scratch/cache budgets and no new witness shortcut'],
         'minimum_relevant_existing_engineering_checks': ['cargo fmt --all -- --check', 'cargo test --locked -p cc-eval --test benchmark_oracle_streaming', 'affected cc-eval library/Clippy coverage in normal CI'],
         'necessary_narrow_control_if_not_already_exercised': 'A borrowed cursor type/UTF8/error-order compatibility control with a malformed B and failing A, plus exact-report equality against the retained owned implementation. No new business threshold or cohort acceptance predicate.',
         'why_useful_now': 'A concrete allocation/copy removal on each successful large-table comparison; ordinary implementation and semantic validation do not require waiting for a study or knowing the missing100k failure cause.',
         'acceptance_boundary': 'No speedup, five-hour success, taskdone, or old-run repair until actual evidence; no mandatory new150 matrix merely for source review.'},
        {'priority': 2, 'name': 'Retain in-flight prefixes for future100k full-cohort preflight',
         'status': 'implementation_design_only_not_applied',
         'source': '.github/workflows/p8-scale-wide-dirty.yml:100-115; scripts/p8_scale_matrix.py:384-447; existing capture wrapper',
         'observed_gap': 'This completed/failure job has no original100k shard in the complete artifact index. The workflow already uses if:always() on final upload, so adding another always condition would not repair the observable gap.',
         'concrete_work': 'For a future source, reuse the existing reviewed supervisor/prefix capture around the unchanged original driver invocation for100k preflight; supply the same originalplan/limits, independently named progress artifacts, offsets and upload ACKs. Keep final whole original shard upload and original validator untouched. Limit scope to the necessary future100k preflight, not145 extra jobs or native algorithm.',
         'minimum_relevant_existing_engineering_checks': 'Reuse capture fake-subprocess controls; add only exact argv/profile pass-through and correct full-cohort artifact namespace/custody smoke if the adapter changes. Never count prefixes as accepted measurements.',
         'acceptance_boundary': 'Addresses observability/custody risk only. Host death may still lose the unacknowledged tail. Does not establish why this job failed, improve runtime, or complete006.'},
        {'priority': 3, 'name': 'Filesystem continuation reuse / scratch layout changes',
         'status': 'not_recommended_as_immediate_patch',
         'reason': 'Existing file-state/token/catalog caches and64/8/1scratch batching are already implemented. Changing default scan semantics, readlease, cache limits, oraclelayout or fullcontrol count lacks a safe minimal demonstrated fix here; do not repeat old optimizations or remove required validation.'}
    ],
    'other_original_tasks': {
        'P7-018': 'tasks10962: real authorized provider calls and usage/partial behavior are the missing work; excluded live calls cannot be replaced with fake or blocked/deferred marked done.',
        'P8-014': 'tasks12872: optional authorized/budgeted LLM judge, frozen model/prompt and disputed-answer review. Paid commercial/live provider is not mandatory in its own clause: a specifically approved fixed local model over publicDEV cases is a possible implementation path. Current18offline fixture controls and prepare/reconcile do not authenticate a real judge; P8-013 remains hard prerequisite. This is not a shortcut to current task credit.',
        'P8-015': 'tasks12923: expressly requires authorized live holdout, cost/error distribution and modelrevision plus P7-018/P8-013; existing24paper controls remain preparation only. Cannot be satisfied by ordinary local code, cold or fake results.',
        'P8-017_to_019': 'Existing ownscope accepted and round39baseline completed0+3historicalDRIFT; no fresh implementation gap. Original016→017→018→019 remains, so retain post-nine applyfalse rather than invent tests.',
        'P8-020': 'tasks13287 plus06-VALIDATION58: selectedreleaseblockers0 and G8release-profile evidence; M4-local may omit conditional015 but cannot omit006 or the other originalharddependencies or relabel failed/incomplete release evidence. Existing19controls are review-tool tests, not releaseapproval.',
        'P9': 'tasks13364 onward: ANN/LSP/rerank roots each dependP8-020 and are conditional benefit decisions. Permitted deferred is explicitly notdone. No reason to broaden scope into unused implementation for count; no model or holdout work proposed.'},
    'existing_evidence_reused_without_execution': [identity(p) for p in [
        'round39-oracle-bounded-algorithm-review/bounded-oracle-algorithm-review.json',
        'round32-batch-resume-readonly-audit/bounded-readonly-audit.json',
        'round37-P8-014-scope-review/independent-P8-014-scope-review.json',
        'round39-post-nine-addendum/post-nine-baseline-addendum.applyfalse.json',
        'round38-P8-006-regression-applicability/independent-V07-and-old-regression-applicability.json']],
    'bounded_source_file_identities': [dict(repository_path=p, **identity(view+p)) for p in source_files],
    'no_work_performed_claim': {'code_changes': 0, 'tests': 0, 'experiments': 0, 'Actions_downloads': 0,
                               'old_validators_or_CRC': 0, 'task_mutations': 0, 'ref_or_PR_mutations': 0},
    'actual_future_006_receipt': None, 'current_counts': {'done': 164, 'remaining': 28, 'session_completed': 1}
})
assert identity(tasks_path) == ledger_before
assert identity(old_plan_path) == plan_before
sel = write('archive-selection.json', {'schema': 'selected-evidence-v1', 'apply_now': False,
    'files': [notice, findings, identity(str((OUT/'prepare-readonly-notice.py').relative_to(ROOT)))],
    'old_inputs_preserved': [ledger_before, plan_before], 'no_large_original_reembedded': True})
files = [notice, findings, identity(str((OUT/'prepare-readonly-notice.py').relative_to(ROOT))), sel]
for f in files:
    f.update(repository_path=PREFIX+Path(f['path']).name, mode='100644', type='blob')
write('upload-manifest.computed.json', {'actual_publication_commit': None, 'server_upload_pending': True, 'files': files})
print(json.dumps({'files': [{k:f[k] for k in ['path','bytes','sha256','git_blob']} for f in files],
                  'old_tasks_and_future_input_unchanged': True, 'product_execution': False}, ensure_ascii=False))

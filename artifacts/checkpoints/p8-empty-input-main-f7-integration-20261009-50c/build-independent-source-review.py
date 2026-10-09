#!/usr/bin/env python3
"""Read fixed Git objects and seal this reviewer's P3 source inventory.

This is an audit reader. It imports no repository Python, runs no project test
or validator, and writes only its own scratch report and receipt.
"""
from pathlib import Path
from collections import Counter
import ast
import datetime
import hashlib
import json
import os
import subprocess

D = Path('/workspace/scratch/50c364fd60b1/validation/main-f7-integration')
REPO = D.parents[1] / 'codecortex'
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0')
P = '034982202bdccc90b4938c6a6d6a56a36d4678af'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
F = 'f7a003636b4902f569404f4e493e1fff7630093e'
M = '3d571fad90f0c10030417d29faabcfe7c841972f'
MAIN900 = '900539f25ed9f9f78cf15895257f82f78810831e'
P2 = 'b4fef72211e5967f4fba729d25d0ca2958094fd5'
F_PRODUCT = '3f1268f005b61f1bcaaa2af4228401503ba49777'
F_REVIEW = 'cfdea50755bb392509f70609cc5dcca08b343f7b'
P2_REVIEW = '98fe910f22eb92f8d9f9c8a8043492ba45dc997c'
REVIEW_PATH = 'artifacts/checkpoints/p8-empty-input-main-f7-integration-20261009-50c/independent-source-review.json'
F_REVIEW_PATH = 'artifacts/checkpoints/p8-pr179-stack-integration-20261009/independent-source-review.json'
P2_PREFIX = 'artifacts/checkpoints/p8-empty-input-preparation-20261009-50c/'
P2_REVIEW_PATH = P2_PREFIX + 'independent-source-review.json'
OURS = (
    'crates/cc-index/src/dispatch_synthesis/interface_dispatch.rs',
    'crates/cc-index/src/dispatch_synthesis/interface_dispatch_tests.rs',
    'crates/cc-index/src/indexer_phases/snapshot.rs',
    'crates/cc-index/tests/full_snapshot_empty_config.rs',
)
F_RUST = (
    'crates/cc-eval/src/benchmark/statistics.rs',
    'crates/cc-eval/src/bin/p8-runtime-statistics.rs',
    'crates/cc-index/src/resolver/catalog.rs',
    'crates/cc-index/src/resolver/helpers.rs',
)
F_PYTHON = ('scripts/p8_runtime.py', 'scripts/tests/test_p8_runtime.py')
DOCS = (
    'README.md', 'docs/roadmap/code-index-v2/tasks.json',
    'docs/roadmap/code-index-v2/05-TODO.md',
    'docs/roadmap/code-index-v2/08-HANDOFF.md',
    'docs/roadmap/code-index-v2/README.md',
    'docs/roadmap/code-index-v2/PLAN-CHECK.json',
)
COMMANDS = []


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical(raw):
    return json.dumps(raw, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def git(*args, input=None):
    completed = subprocess.run(['git', *args], cwd=REPO, env=ENV, input=input,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    COMMANDS.append({'argv': ['git', *args], 'exit_code': completed.returncode})
    if completed.returncode:
        raise RuntimeError(completed.stderr.decode(errors='replace'))
    return completed.stdout


def tree(ref):
    result = {}
    for row in git('ls-tree', '-r', '-z', ref).split(b'\0'):
        if not row:
            continue
        metadata, path = row.split(b'\t', 1)
        mode, kind, oid = metadata.decode().split()
        result[path.decode()] = {'mode': mode, 'type': kind, 'blob': oid}
    return result


def product(rows):
    return {p: r for p, r in rows.items() if p.startswith('crates/') or p in ('Cargo.toml', 'Cargo.lock')}


start_head = git('rev-parse', 'HEAD').decode().strip()
refs = {'P3': P, 'BASE': BASE, 'f7': F, 'M900': M, 'main900': MAIN900,
        'P2': P2, 'f7_original_product': F_PRODUCT, 'f7_original_review': F_REVIEW}
trees = {label: tree(ref) for label, ref in refs.items()}
guard_raw = git('show', P + ':scripts/verify_reviewed_source_v15.py')
guard_ast = ast.parse(guard_raw)
constants = {}
for node in guard_ast.body:
    if not isinstance(node, ast.Assign) or len(node.targets) != 1 or not isinstance(node.targets[0], ast.Name):
        continue
    name = node.targets[0].id
    if name in ('BASE', 'VERSION', 'PRODUCT', 'REVIEW', 'REVIEW_PATH', 'REGISTRY_SHA256', 'FROZEN', 'VALIDATION_ROOTS'):
        constants[name] = ast.literal_eval(node.value)
    if name == 'VALIDATION_EXCLUSIONS':
        assert isinstance(node.value, ast.Call) and isinstance(node.value.func, ast.Name) and node.value.func.id == 'frozenset'
        constants[name] = frozenset(ast.literal_eval(node.value.args[0]))
assert constants['BASE'] == BASE
assert constants['PRODUCT'] == F_PRODUCT and constants['REVIEW'] == F_REVIEW
assert constants['REVIEW_PATH'] == F_REVIEW_PATH


def validation(rows):
    return {p: r for p, r in rows.items()
            if p.startswith(tuple(x + '/' for x in constants['VALIDATION_ROOTS']))
            and p not in constants['VALIDATION_EXCLUSIONS']
            and not ('__pycache__' in Path(p).parts and p.endswith('.pyc'))}


products = {k: product(v) for k, v in trees.items()}
validations = {k: validation(v) for k, v in trees.items()}
base = products['BASE']
current = products['P3']
vals = validations['P3']
assert set(base) <= set(current)
for rows in [*products.values(), *validations.values()]:
    assert all(r['type'] == 'blob' and r['mode'] in ('100644', '100755') for r in rows.values())
changed = sorted(p for p in set(base) | set(current) if base.get(p) != current.get(p))
assert changed == sorted(git('diff', '--name-only', BASE, P, '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines())
needed = {r['blob'] for label in ('P3', 'BASE', 'f7', 'P2')
          for rows in (products[label], validations[label]) for r in rows.values()}
extra_paths = {
    'scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json',
    '.github/workflows/ci.yml', 'scripts/verify_current_source.py',
    'docs/roadmap/code-index-v2/tasks.json', F_REVIEW_PATH, P2_REVIEW_PATH,
}
extra_paths.update(P2_PREFIX + x for x in (
    'final-product-checks/receipt.json',
    'publication/g181-ci-review/independent-review.json',
    'publication/native-source-checks/outcome-review.json',
))
needed.update(trees['P3'][p]['blob'] for p in extra_paths)
ordered = sorted(needed)
raw_batch = git('cat-file', '--batch', input=''.join(oid + '\n' for oid in ordered).encode())
bodies = {}
offset = 0
for expected_oid in ordered:
    end = raw_batch.index(b'\n', offset)
    header = raw_batch[offset:end].decode().split()
    assert len(header) == 3 and header[0] == expected_oid and header[1] == 'blob', header
    size = int(header[2])
    raw = raw_batch[end + 1:end + 1 + size]
    assert len(raw) == size and raw_batch[end + 1 + size:end + 2 + size] == b'\n'
    assert hashlib.sha1(b'blob ' + str(size).encode() + b'\0' + raw).hexdigest() == expected_oid
    bodies[expected_oid] = raw
    offset = end + 2 + size
assert offset == len(raw_batch)


def digest_map(rows):
    return {p: sha(bodies[r['blob']]) for p, r in sorted(rows.items())}


complete = digest_map(current)
validation_inputs = digest_map(vals)
delta = {p: {'before_sha256': sha(bodies[base[p]['blob']]) if p in base else None,
             'sha256': complete[p]} for p in changed}
rebuilt = {p: bodies[r['blob']] for p, r in base.items()}
rebuilt.update({p: bodies[current[p]['blob']] for p in changed})
assert set(rebuilt) == set(current) and all(raw == bodies[current[p]['blob']] for p, raw in rebuilt.items())

f_raw = bodies[trees['P3'][F_REVIEW_PATH]['blob']]
p2_raw = bodies[trees['P3'][P2_REVIEW_PATH]['blob']]
upstream = json.loads(f_raw)
p2_review = json.loads(p2_raw)
registry_raw = bodies[trees['P3']['scripts/reviewed-source-registry-v15.json']['blob']]
registry = json.loads(registry_raw)
assert sha(f_raw) == 'c4c5ea8741b9f5e2b86336be14b3050b14ca57ee1d723f3a775b278d3951b317'
assert f_raw == (D / 'guard-current-f7-independent-source-review.json').read_bytes()
assert p2_raw == (D.parent / 'p8-empty-preparation-corrected-independent-source-review.json').read_bytes()
assert trees['P3'][F_REVIEW_PATH] == trees['f7'][F_REVIEW_PATH] == trees['f7_original_review'][F_REVIEW_PATH]
assert git('rev-parse', P2_REVIEW + ':' + P2_REVIEW_PATH).decode().strip() == trees['P3'][P2_REVIEW_PATH]['blob']
assert upstream['source'] == F_PRODUCT and upstream['base'] == BASE and upstream['verdict'] == 'accepted_scoped'
assert p2_review['source'] == P2 and p2_review['base'] == BASE and p2_review['verdict'] == 'accepted_scoped'
assert products['f7'] == products['f7_original_product']
assert validations['f7'] == validations['f7_original_product'] == vals
assert upstream['complete_inputs'] == registry['complete_inputs'] == digest_map(products['f7'])
assert upstream['validation_inputs'] == registry['validation_inputs'] == validation_inputs
f_delta = {p: delta[p] for p in upstream['paths']}
assert upstream['paths'] == registry['delta'] == f_delta
assert sha(registry_raw) == constants['REGISTRY_SHA256']
assert set(changed) == set(upstream['paths']) | set(OURS)
assert not (set(upstream['paths']) & set(OURS))
assert {p for p in set(current) | set(products['f7']) if current.get(p) != products['f7'].get(p)} == set(OURS)
assert all(current[p] == products['P2'][p] == products['M900'][p] for p in OURS)
assert {p: complete[p] for p in OURS} == {p: p2_review['complete_inputs'][p] for p in OURS}
assert {p: delta[p] for p in OURS} == {p: p2_review['paths'][p] for p in OURS}
assert all(current[p] == products['f7'][p] for p in current if p not in OURS)
assert [len(complete), len(delta), len(validation_inputs)] == [1091, 46, 139]

main_delta = sorted(p for p in set(trees['main900']) | set(trees['f7'])
                    if trees['main900'].get(p) != trees['f7'].get(p))
assert len(main_delta) == 34 and all(p in trees['f7'] for p in main_delta)
expected_tree = dict(trees['M900'])
expected_tree.update({p: trees['f7'][p] for p in main_delta})
assert expected_tree == trees['P3']
assert [p for p in main_delta if not p.startswith('artifacts/')] == sorted([
    *F_RUST, *F_PYTHON, 'docs/P8_RUNTIME_EVIDENCE.md',
    'scripts/reviewed-source-registry-v15.json', 'scripts/verify_reviewed_source_v15.py',
])
assert all(trees['P3'][p] == trees['M900'][p] for p in DOCS)
artifacts = {k: {p: r for p, r in v.items() if p.startswith('artifacts/')} for k, v in trees.items()}
overlap = set(artifacts['M900']) & set(artifacts['f7'])
assert all(artifacts['M900'][p] == artifacts['f7'][p] for p in overlap)
assert artifacts['P3'] == dict(artifacts['M900'], **artifacts['f7'])
assert all(trees['P3'][p] == trees['BASE'][p] for p in constants['FROZEN'])
assert all(trees['P3'][p] == trees['f7'][p] for p in (
    '.github/workflows/ci.yml', 'scripts/verify_reviewed_source_v15.py',
    'scripts/reviewed-source-registry-v15.json', 'scripts/v15_historical_context.py',
    'tests/source_integrity/v15_historical_test_adapter.py',
))

commit = git('show', '-s', '--format=%H%n%T%n%P', P).decode().splitlines()
assert commit[0] == P and commit[1] == '7e5ea7972eef08fe2a1874ed8d2b9af977d26358'
assert commit[2].split() == [M, F]
tasks_raw = bodies[trees['P3']['docs/roadmap/code-index-v2/tasks.json']['blob']]
tasks_document = json.loads(tasks_raw)
tasks = tasks_document['tasks']
status_counts = dict(Counter(t['status'] for t in tasks))
assert len(tasks) == 192 and status_counts['done'] == 163
assert sha(tasks_raw) == '8e4742e00ee114fac549c86d560c6776f3384b4f0ad30179365b5a6ebbe678c3'

findings = [
    {'id': 'CONFIG_EMPTY', 'paths': [OURS[2]], 'status': 'accepted',
     'finding': 'The actual P3 body retains P2 exactly. Full builds still compute the config signature before the fallible scan and filter typed project-model configs before the new empty-token branch. Only pure symbol/file collection and resolver lookup preparation is skipped. With no raw tokens the original resolver produced no links, and its snapshot wrapper returned before per-config filesystem reads. Common payload timestamp, signature, algorithm, empty token-cache serialization, route/hierarchy/config writes and graph-signature baseline remain unchanged across the three shared adapters. Nonempty tokens use the original builder with equal inputs.'},
    {'id': 'CONFIG_CONTROLS', 'paths': [OURS[3]], 'status': 'accepted_with_original_scope',
     'finding': 'The two actual real-index controls preserve linked-to-empty full replacement, metadata/empty-cache persistence, a later incremental source change and relinking; the second verifies typed TS alias resolution while heuristic config tokens are empty. These exercise the staged full-build route. No new execution of legacy temp-db/DirectWriter routes or of every nonempty token channel is claimed by this static review.'},
    {'id': 'INTERFACE_PREREQUISITES', 'paths': [OURS[0]], 'status': 'accepted',
     'finding': 'The actual P3 body retains P2 exactly. Disabled behavior remains no read/no cleanup. Enabled behavior always owns deletion of old interface_dispatch edges. Typed CALL rows and prior in-round CALL overlays are read first, no-call return precedes symbols, and every selected symbol is typed-decoded before checking interface presence. The persisted UNIQUE symbol_uid constraint and omission of empty UIDs make direct row-based interface membership equivalent to the former uid-map projection. Implements is still unread without calls/interfaces and otherwise is fully typed-decoded before its empty return. Only unused full symbol/container maps move; no SQL read reduction or changed error order is claimed. Normal mapping insertion rules, existing unordered-map behavior, prior CALL overlay, fanout cap, edge fields and edge-id deduplication remain the original ones.'},
    {'id': 'INTERFACE_CONTROLS', 'paths': [OURS[1]], 'status': 'accepted',
     'finding': 'Eight real SQLite controls cover stale-edge deletion ownership while retaining real calls, original no-call/no-interface read boundaries, empty interface UIDs, disabled behavior, typed malformed CALL/symbol/implements error ordering, prior in-round CALL replacement, interface/trait positives and 0/1 fanout limits. The exact three P2 expected-result expect_err corrections are present; actual graph-result assertions are unchanged. The controls reject malformed rows and add no lint allowance or validation bypass.'},
    {'id': 'RESOLVER_EXPORT_KEYS', 'paths': [F_RUST[2]], 'status': 'accepted',
     'finding': 'The inherited f7 body was reread against main900. At most three lowercase export keys are deduplicated directly: name is always inserted, alias only when distinct from name, and default only when distinct from both. Per-key candidate append order and bucket identity remain unchanged. The retained legacy-oracle fixtures include empty names/aliases, case/default collisions, Unicode lowercase, repeated batches and tombstone removal/reinsertion. This changes allocation preparation, not catalog ownership, exported identities or downstream lookup contracts.'},
    {'id': 'RESOLVER_CANDIDATE_ORDER', 'paths': [F_RUST[3]], 'status': 'accepted',
     'finding': 'The inherited f7 body was reread against main900. Common-path scoring zips the same stripped-extension path components directly, preserving empty, dotted, repeated-separator, Unicode and backslash cases. Candidate selection keeps all ties at the maximum score in encounter order, clears earlier candidates only for a strictly larger score, then calls the original stable_candidates deduplicator. Retained old-algorithm comparisons cover the path cross-product and repeated/tied candidate sequences. Lookup and dispatch/config consumers still receive the same candidate identity/order contract.'},
    {'id': 'RUST_NANOSECOND_OWNER', 'paths': [F_RUST[0], F_RUST[1]], 'status': 'accepted',
     'finding': 'The inherited f7 bodies were reread against main900. legacy_latency_ns uses the existing nearest-rank helper on sorted original u64 nanoseconds, returning null quantiles for an empty group and preserving all terminal outcomes. replay calls validate first; validate rejects finished_ns below offered_ns, so subsequent subtraction is safe. The added legacy_ns fields leave existing microsecond summaries, status/failure denominators, acceptance exits, confidence-interval restrictions and tail-stability disclaimer unchanged. Controls cover sub-microsecond values, repeated values, rank boundaries, u64 limits, failure/rejection/cancel outcomes, operation filters and row permutation.'},
    {'id': 'PYTHON_SUMMARY_COPY', 'paths': list(F_PYTHON), 'status': 'accepted',
     'finding': 'The inherited f7 Python delta removes the duplicate latency quantile implementation. After the original Rust replay, Python rereads statistics.json, checks its SHA against the replay seal and copies the Rust-owned legacy_ns fields. It neither rounds microseconds back into nanoseconds nor drops rejected/failed terminal rows. The existing real-owner quantile assertion moves to Rust; the Python protocol fixture uses explicit synthetic sentinel ns values to test copying, and a changed-seal negative control must fail with exit 2. No guard algorithm, runtime acceptance denominator or old raw execution is rewritten.'},
    {'id': 'COMPOSITION', 'paths': [*OURS, *F_RUST, *F_PYTHON], 'status': 'accepted_scoped',
     'finding': 'The actual two-parent P3 combines disjoint product paths. Resolver candidate/export contracts are preserved before the unchanged resolution persistence and postprocess stages; the empty-config and interface changes do not change those contracts, schemas, mutation ownership or transaction boundaries. Runtime statistics owns evidence summarization in cc-eval and its Python driver, and does not change index construction. All sixteen peer-listed supporting dependencies retain exact P2/f7/P3 mode/type/blob identity. Actual full inventories, complete BASE reconstruction and all inherited evidence are checked below. This conclusion is static source admission only; fresh P3 engineering and final G source-chain execution remain separately required.'},
]
finding_by_path = {p: [f['id'] for f in findings if p in f['paths']] for p in [*OURS, *F_RUST, *F_PYTHON]}
source_reviews = {}
for p in changed:
    row = {
        'reviewer': '/root/pr_audit',
        'current_source': P,
        'current_mode_type_blob': current[p],
        'sha256': complete[p],
        'base_source': BASE,
        'base_mode_type_blob': base.get(p),
        'before_sha256': delta[p]['before_sha256'],
    }
    if p in OURS:
        row.update({
            'origin_source': P2, 'composition_parent': M,
            'origin_review_source': P2_REVIEW, 'origin_review_path': P2_REVIEW_PATH,
            'origin_review_sha256': sha(p2_raw),
            'semantic_review': 'Current-session non-author reread of the complete actual P3 file, its main-baseline delta and supporting contracts; exact P2 origin and original non-author evidence retained.',
            'current_findings': finding_by_path[p],
        })
    else:
        row.update({
            'origin_source': F_PRODUCT, 'composition_parent': F,
            'origin_review_source': F_REVIEW, 'origin_review_path': F_REVIEW_PATH,
            'origin_review_sha256': sha(f_raw),
            'inherited_original_review_record': upstream['source_path_reviewers'][p],
            'semantic_review': (
                'Current-session non-author review of the f7/main900 production/test delta and its interaction with the P2 changes; exact full body, original BASE before/after hashes and original non-author source review retained.'
                if p in F_RUST else
                'Inherited semantic approval from the exact fixed f7 canonical and its named original non-author/recusal chain. This session independently read and hashed the actual body and verified complete path/mode/type/blob inheritance, but does not claim a new semantic reread of this entire unchanged historical component.'),
            'current_findings': finding_by_path.get(p, []),
        })
    source_reviews[p] = row
validation_reviews = {}
for p, digest in validation_inputs.items():
    validation_reviews[p] = {
        'current_source': P, 'origin_source': F_PRODUCT, 'composition_parent': F,
        'mode_type_blob': vals[p], 'sha256': digest,
        'origin_review_source': F_REVIEW, 'origin_review_path': F_REVIEW_PATH,
        'inherited_original_review_record': upstream['validation_path_reviewers'][p],
        'reviewer': '/root/pr_audit',
        'semantic_review': (
            'Current non-author review of the runtime summary-copy/control delta; exact inherited f7 validation body and original acceptance restrictions preserved.'
            if p in F_PYTHON else
            'Exact full-inventory and actual-byte inheritance from the fixed f7 validation approval; no assertion of fresh execution or semantic rereading of every unchanged validator.'),
        'current_findings': finding_by_path.get(p, []),
    }

peer_raw = (D / 'semantic-composition-review.json').read_bytes()
peer = json.loads(peer_raw)
assert peer['reviewer'] == '/root/todo_audit' and peer['sources']['combined_P3'] == P
dependencies = {}
for row in peer['unchanged_composition_dependencies']:
    p = row['path']
    assert current[p] == products['P2'][p] == products['f7'][p]
    assert complete[p] == row['sha256']
    dependencies[p] = dict(current[p], sha256=complete[p], bytes=len(bodies[current[p]['blob']]))
assert len(dependencies) == 16
peer_reports = []
for name in ('semantic-composition-review.json', 'P3-composition-object-binding.json', 'guard-binding-plan.json'):
    raw = (D / name).read_bytes()
    peer_reports.append({'file_name': name, 'bytes': len(raw), 'sha256': sha(raw),
                         'location_kind': 'scratch supporting report; publication is separately owned by root'})

historical_evidence = []
for p in sorted(extra_paths):
    if p.startswith(P2_PREFIX) and p != P2_REVIEW_PATH:
        raw = bodies[trees['P3'][p]['blob']]
        historical_evidence.append({'path': p, 'mode_type_blob': trees['P3'][p],
                                    'bytes': len(raw), 'sha256': sha(raw),
                                    'scope': 'Original execution identity preserved; not a P3 execution'})

review = {
    'schema_version': 1,
    'reviewed_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'source': P, 'source_tree': commit[1], 'tree': commit[1],
    'source_parent': M, 'source_parents': [M, F], 'base': BASE,
    'scope': 'independently_reviewed_source_and_validation_inputs',
    'verdict': 'accepted_scoped',
    'independent_reviewers': ['/root/pr_audit'], 'unresolved_blockers': [],
    'complete_input_count': len(complete), 'changed_source_input_count': len(delta),
    'validation_input_count': len(validation_inputs),
    'source_manifest_sha256': sha(canonical(complete)),
    'source_manifest_digest_encoding': 'UTF-8 JSON, sorted keys, separators=(comma,colon), ensure_ascii=False, no trailing newline',
    'paths': delta, 'complete_inputs': complete, 'validation_inputs': validation_inputs,
    'actual_input_mode_blob_inventory': {'source': current, 'validation': vals},
    'source_path_reviewers': source_reviews, 'validation_path_reviewers': validation_reviews,
    'review_basis': {
        'current_semantic_review': 'The current /root/pr_audit reread the actual four P2 files, the four f7 Rust deltas, the two runtime Python deltas, relevant original baselines and supporting contracts. All sixteen peer-listed composition dependencies were checked for exact unchanged identity. The review does not claim semantic rereading of all 1091 product files or 139 validation files.',
        'complete_inventory_review': 'Actual immutable Git trees were independently enumerated in the unchanged original product and validation domains. Every distinct required local Git blob was read through git cat-file --batch, its Git blob SHA-1 and content SHA-256 recomputed, and full path/mode/type/blob inventories compared. GIT_NO_LAZY_FETCH=1 prevented implicit network hydration. No digest map was copied as a substitute for reading actual source bodies.',
        'base_reconstruction': {'base_input_count': len(base), 'all_inherited_paths_retained': True,
                                'actual_BASE_inventory_sha256': sha(canonical(base)),
                                'all_delta_before_and_after_hashes_recomputed': True,
                                'complete_BASE_plus_46_delta_reconstruction_equals_actual_P3': True,
                                'removed_product_paths': [],
                                'new_product_paths': sorted(set(current) - set(base))},
        'read_blob_count': len(bodies), 'read_blob_bytes': sum(map(len, bodies.values())),
        'read_blob_method': 'Existing local immutable Git object cache only; no fetch, source execution, source import or repository mutation',
        'original_product_roots': ['crates', 'Cargo.toml', 'Cargo.lock'],
        'original_validation_roots': list(constants['VALIDATION_ROOTS']),
        'original_validation_exclusions': sorted(constants['VALIDATION_EXCLUSIONS']),
        'only_generated_validation_exclusion': '__pycache__ path segment together with .pyc suffix',
        'supporting_dependencies': dependencies, 'peer_reports': peer_reports,
    },
    'inheritance_evidence': {
        'fixed_main': F, 'fixed_main_tree': git('rev-parse', F + '^{tree}').decode().strip(),
        'inherited_product': F_PRODUCT, 'inherited_review': F_REVIEW,
        'inherited_review_path': F_REVIEW_PATH,
        'inherited_review_sha256': sha(f_raw), 'inherited_review_blob': trees['P3'][F_REVIEW_PATH]['blob'],
        'inherited_review_bytes': len(f_raw), 'inherited_registry_sha256': sha(registry_raw),
        'inherited_independent_reviewers': upstream['independent_reviewers'],
        'inherited_author_boundaries_verbatim_record': upstream['author_independence'],
        'main_and_original_product_domains_equal_by_path_mode_type_blob': True,
        'inherited_complete_inputs_and_validation_maps_recomputed_and_exact': True,
        'inherited_all_42_BASE_delta_rows_recomputed_and_exact': True,
        'P3_all_1087_other_main_product_paths_unchanged': True,
        'P3_validation_139_equals_f7': True,
        'validation_changes_since_P2': {p: {'before_sha256': sha(bodies[validations['P2'][p]['blob']]), 'sha256': validation_inputs[p]} for p in vals if vals[p] != validations['P2'].get(p)},
        'P2_four_path_origin': {'product': P2, 'review': P2_REVIEW,
                               'review_path': P2_REVIEW_PATH, 'review_sha256': sha(p2_raw),
                               'paths': list(OURS), 'exact_actual_bodies_equal_P2_and_M900': True},
        'all_six_frozen_v14_paths_equal_original_BASE': {p: dict(trees['P3'][p], sha256=validation_inputs[p]) for p in constants['FROZEN']},
        'current_v15_guard_registry_and_CI_selected_from_f7_unchanged': True,
        'current_guard_constants': {k: v for k, v in constants.items() if k not in ('FROZEN', 'VALIDATION_ROOTS', 'VALIDATION_EXCLUSIONS')},
        'current_P3_guard_binding_status': 'P3 deliberately retains f7 trust metadata. The new P3 review/R/G bindings must be installed before running the final unchanged source-admission chain.',
    },
    'complete_root_composition': {
        'ordered_parents': [M, F], 'parent_M900_tree': git('rev-parse', M + '^{tree}').decode().strip(),
        'main900_source': MAIN900, 'main900_to_f7_exact_overlay_path_count': len(main_delta),
        'main900_to_f7_overlay': {p: trees['f7'][p] for p in main_delta},
        'full_P3_leaf_inventory_equals_M900_plus_exact_f7_overlay': True,
        'root_leaf_count': len(trees['P3']), 'root_leaf_inventory_sha256': sha(canonical(trees['P3'])),
        'unchanged_M900_six_documents': {p: trees['P3'][p] for p in DOCS},
        'artifact_union': {'M900_count': len(artifacts['M900']), 'f7_count': len(artifacts['f7']),
                           'common_count': len(overlap), 'conflicting_common_paths': [],
                           'M900_only_count': len(set(artifacts['M900']) - set(artifacts['f7'])),
                           'f7_only_count': len(set(artifacts['f7']) - set(artifacts['M900'])),
                           'P3_count': len(artifacts['P3']), 'P3_is_exact_union': True,
                           'evidence_method': 'Full path/mode/type/Gitblob equality; no claim that this review re-executed all archived proofs or semantically reread every historical artifact'}
    },
    'semantic_findings': findings,
    'author_independence': {
        'current_session': 'This /root/pr_audit did not author the eight product implementation/test files, the two changed Python files, or the P3 composition. It made no source, validation, guard, index, ref or shared-worktree edit in this review.',
        'current_composition_author': '/root',
        'historical_names': 'Repeated /root/pr_audit or other agent names inside inherited external-session records retain their original session and role. They are not attributed as current-session authorship, independent review or execution.',
        'inherited_recusals': 'The existing non-author and recusal chain is preserved verbatim through the fixed inherited canonical; current byte identity does not convert an author into an independent reviewer.'
    },
    'review_execution': {
        'actual_immutable_Git_reads': True, 'all_required_blob_SHA1_and_SHA256_recomputed': True,
        'Cargo_or_rustfmt_or_tests_or_product_execution_by_this_reviewer': False,
        'native_or_historical_or_source_guard_executed_by_this_reviewer': False,
        'network_fetch_or_Git_object_write_by_this_reviewer': False,
        'repository_source_index_ref_or_worktree_mutation_by_this_reviewer': False,
        'writes': 'Only this scratch audit reader, canonical review and audit receipt',
        'new_P3_engineering_results': 'Owned separately by root; no pass/fail result is granted or relabeled by this source review',
    },
    'historical_execution_records_retained': historical_evidence,
    'acceptance_boundary': [
        'Accepted only for fixed P3 product-source and validation-input admission. This source review certifies no new engineering, raw-study, quality, V20, scale, runtime, release or TODO completion result.',
        'Earlier P2 local workspace exit 101 and the repeated sampler live_child failure remain original failures. The later original G50d9 CI/default-features results and G source-proof pass retain their own actual checkout, commands and environment. None is a P3 execution or a waiver of the earlier failure.',
        'All main-f7, PR175, PR179 and G275 original results preserve their exact source/run/artifact identities. The new same-source P3 engineering results and final G source-integrity/CI checks must be assessed separately.',
        'The statistics changes preserve original failure denominators and ns precision. Allocation changes are not measured RSS/latency or SQL-read-reduction evidence.',
        'A later R may add this review and bounded evidence outside product/validation domains. Later G may update only the matching registry and PRODUCT, REVIEW, REVIEW_PATH and REGISTRY_SHA256 constants. BASE, VERSION, exclusions, six frozen v14 files, CI selector, validation algorithms and original historical-proof logic stay unchanged.',
        'The original historical/source-integrity chain and ordinary required CI must still execute on the final exact G. This JSON is not a substitute for those original checks.',
        'The six M900 documents and all previous artifacts are preserved; the task ledger remains 192 total, 163 done, 29 remaining and zero newly closed. Source admission does not satisfy an unfinished task dependency.'
    ],
    'intended_repository_review_path': REVIEW_PATH,
    'TODO_closed': 0, 'TODO_done': 163, 'TODO_remaining': 29,
    'task_ledger': {'path': 'docs/roadmap/code-index-v2/tasks.json', 'sha256': sha(tasks_raw),
                    'git_blob': trees['P3']['docs/roadmap/code-index-v2/tasks.json']['blob'],
                    'bytes': len(tasks_raw), 'status_counts': status_counts, 'unchanged_from_M900': True},
}

end_head = git('rev-parse', 'HEAD').decode().strip()
assert start_head == end_head == P
raw_review = (json.dumps(review, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
target = D / 'independent-source-review.json'
assert not target.exists(), 'Do not silently overwrite a fixed review'
target.write_bytes(raw_review)
receipt = {
    'schema': 'P3-independent-source-review-read-receipt-v1',
    'reviewer': '/root/pr_audit', 'source': P, 'source_tree': commit[1],
    'HEAD_before': start_head, 'HEAD_after': end_head,
    'review_file': target.name, 'review_bytes': len(raw_review),
    'review_sha256': sha(raw_review),
    'review_git_blob': hashlib.sha1(b'blob ' + str(len(raw_review)).encode() + b'\0' + raw_review).hexdigest(),
    'reader_file': Path(__file__).name, 'reader_sha256': sha(Path(__file__).read_bytes()),
    'read_unique_blob_count': len(bodies), 'read_unique_blob_bytes': sum(map(len, bodies.values())),
    'complete_input_count': len(complete), 'changed_source_input_count': len(delta),
    'validation_input_count': len(validation_inputs),
    'commands': COMMANDS,
    'prohibited_actions_performed': [],
    'all_assertions_passed': True,
}
(D / 'independent-source-review-read-receipt.json').write_bytes((json.dumps(receipt, sort_keys=True, indent=2) + '\n').encode())
print(json.dumps({k: receipt[k] for k in ('source', 'source_tree', 'review_bytes', 'review_sha256', 'review_git_blob', 'read_unique_blob_count', 'read_unique_blob_bytes', 'complete_input_count', 'changed_source_input_count', 'validation_input_count', 'all_assertions_passed')}))
